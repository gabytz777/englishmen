/* Native `host` module for the C bootstrap VM.
 *
 * The module is an ordinary Englishmen object: each member is a native value
 * created with emv_native().  I/O failures and bad arguments are reported as
 * ordinary values of the shape
 *
 *     {__hostError: true, error: "<message>"}
 *
 * rather than raising a VM error or aborting the process.  `exit()` is the one
 * deliberately non-returning primitive.
 *
 * Program arguments are supplied by em_host_register(): argv is the process
 * argv of `emrun <image.emb> [program args...]`, so host.args() returns
 * argv[2..argc].  State is kept per VM (the CLI has a single VM, but tests may
 * create several), without touching the parent-owned headers.
 */

#define _POSIX_C_SOURCE 200809L

#include "host.h"

/* vm.h includes emb.h, whose EmFunctionProto typedef conflicts with the
 * forward declaration in value.h.  Declare the only VM entry point used here
 * with exactly the signature from vm.h instead of editing parent-owned
 * headers. */
int em_vm_register_import(EmVm *vm, const char *name, EmValue *module);
EmValue *em_vm_lookup_import(EmVm *vm, const char *name);

#include <errno.h>
#include <limits.h>
#include <math.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct HostState {
    EmVm *vm;
    EmValue *module; /* borrowed: the `host` import this state belongs to */
    char **args;
    size_t argc;
    struct HostState *next;
} HostState;

static HostState *host_states;

static char *host_strdup(const char *text)
{
    size_t len = strlen(text);
    char *copy = malloc(len + 1);
    if (copy == NULL) {
        return NULL;
    }
    memcpy(copy, text, len + 1);
    return copy;
}

static void host_set_err(char **err, const char *message)
{
    if (err != NULL) {
        *err = host_strdup(message);
    }
}

static HostState *host_state_find(EmVm *vm)
{
    HostState *state;
    if (vm == NULL) {
        return NULL;
    }
    for (state = host_states; state != NULL; state = state->next) {
        if (state->vm == vm) {
            return state;
        }
    }
    return NULL;
}

static HostState *host_state_get(EmVm *vm, bool create)
{
    HostState *state = host_state_find(vm);
    if (state != NULL || !create || vm == NULL) {
        return state;
    }
    state = calloc(1, sizeof(*state));
    if (state == NULL) {
        return NULL;
    }
    state->vm = vm;
    state->next = host_states;
    host_states = state;
    return state;
}

static void host_args_clear(HostState *state)
{
    size_t i;
    for (i = 0; i < state->argc; i++) {
        free(state->args[i]);
    }
    free(state->args);
    state->args = NULL;
    state->argc = 0;
}

/* argv[0] is emrun itself and argv[1] is the image path. */
static bool host_args_set(HostState *state, int argc, char **argv)
{
    size_t count = 0;
    char **copy = NULL;
    size_t i;

    if (argc > 2 && argv != NULL) {
        count = (size_t)(argc - 2);
    }
    if (count > 0) {
        copy = calloc(count, sizeof(*copy));
        if (copy == NULL) {
            return false;
        }
        for (i = 0; i < count; i++) {
            const char *arg = argv[i + 2] != NULL ? argv[i + 2] : "";
            copy[i] = host_strdup(arg);
            if (copy[i] == NULL) {
                for (i = 0; i < count; i++) {
                    free(copy[i]);
                }
                free(copy);
                return false;
            }
        }
    }

    host_args_clear(state);
    state->args = copy;
    state->argc = count;
    return true;
}

static EmValue *host_error(const char *message)
{
    EmValue *error = emv_object();
    EmValue *flag;
    EmValue *text;

    if (error == NULL) {
        return NULL;
    }
    flag = emv_bool(true);
    text = emv_string(message);
    if (flag == NULL || text == NULL) {
        emv_release(flag);
        emv_release(text);
        emv_release(error);
        return NULL;
    }
    emv_object_set(error, "__hostError", flag);
    emv_object_set(error, "error", text);
    emv_release(flag);
    emv_release(text);
    return error;
}

static EmValue *host_error_fmt(const char *format, ...)
{
    char buffer[512];
    va_list ap;

    va_start(ap, format);
    vsnprintf(buffer, sizeof(buffer), format, ap);
    va_end(ap);
    return host_error(buffer);
}

static EmValue *host_errno_error(const char *what, const char *path, int code)
{
    const char *reason = strerror(code != 0 ? code : EIO);
    if (path == NULL || path[0] == '\0') {
        return host_error_fmt("%s: %s", what, reason);
    }
    return host_error_fmt("%s '%s': %s", what, path, reason);
}

/* Fetch a required string argument; *error receives a host error value. */
static bool host_string_arg(EmValue **args, size_t argc, size_t index,
                            const char *function, const char *description,
                            const char **out, EmValue **error)
{
    if (index >= argc || !emv_is_string(args[index])) {
        *error = host_error_fmt("%s expects argument %zu (%s) as a string",
                                function, index + 1, description);
        return false;
    }
    *out = emv_as_cstr(args[index]);
    return true;
}

/* Read a whole file into a fresh byte buffer. */
static bool host_read_file(const char *path, unsigned char **out,
                           size_t *out_len, int *out_errno)
{
    FILE *file = fopen(path, "rb");
    unsigned char *buffer;
    size_t length = 0;
    size_t capacity = 4096;

    if (file == NULL) {
        *out_errno = errno;
        return false;
    }
    buffer = malloc(capacity);
    if (buffer == NULL) {
        *out_errno = ENOMEM;
        fclose(file);
        return false;
    }

    for (;;) {
        size_t got;
        if (length == capacity) {
            size_t next;
            unsigned char *grown;
            if (capacity > SIZE_MAX / 2) {
                *out_errno = ENOMEM;
                free(buffer);
                fclose(file);
                return false;
            }
            next = capacity * 2;
            grown = realloc(buffer, next);
            if (grown == NULL) {
                *out_errno = ENOMEM;
                free(buffer);
                fclose(file);
                return false;
            }
            buffer = grown;
            capacity = next;
        }
        got = fread(buffer + length, 1, capacity - length, file);
        length += got;
        if (got == 0) {
            if (ferror(file)) {
                *out_errno = errno != 0 ? errno : EIO;
                free(buffer);
                fclose(file);
                return false;
            }
            break; /* EOF */
        }
    }

    if (fclose(file) != 0) {
        *out_errno = errno != 0 ? errno : EIO;
        free(buffer);
        return false;
    }
    *out = buffer;
    *out_len = length;
    return true;
}

/* Write a validated byte buffer, creating or truncating the file. */
static bool host_write_file(const char *path, const unsigned char *bytes,
                            size_t length, int *out_errno)
{
    FILE *file = fopen(path, "wb");
    if (file == NULL) {
        *out_errno = errno;
        return false;
    }
    if (length > 0 && fwrite(bytes, 1, length, file) != length) {
        *out_errno = errno != 0 ? errno : EIO;
        fclose(file);
        return false;
    }
    if (fclose(file) != 0) {
        *out_errno = errno != 0 ? errno : EIO;
        return false;
    }
    return true;
}

static EmValue *em_host_args(EmVm *vm, EmValue **args, size_t argc,
                             char **err)
{
    HostState *state = host_state_get(vm, false);
    EmValue *array = emv_array();
    size_t i;

    (void)args;
    (void)argc;
    (void)err;
    if (array == NULL) {
        return NULL;
    }
    /* A VM pointer can be reused after em_vm_free(), so only use arguments
     * belonging to the host module actually registered on this VM. */
    if (state != NULL && state->module != NULL &&
        em_vm_lookup_import(vm, "host") == state->module) {
        for (i = 0; i < state->argc; i++) {
            EmValue *item = emv_string(state->args[i]);
            if (item == NULL) {
                emv_release(array);
                return NULL;
            }
            emv_array_push(array, item);
            emv_release(item);
        }
    }
    return array;
}

static EmValue *em_host_env(EmVm *vm, EmValue **args, size_t argc,
                            char **err)
{
    const char *name;
    EmValue *error = NULL;
    const char *value;

    (void)vm;
    (void)err;
    if (!host_string_arg(args, argc, 0, "host.env", "variable name", &name,
                         &error)) {
        return error;
    }
    value = getenv(name);
    if (value != NULL) {
        return emv_string(value);
    }
    if (argc > 1) {
        emv_retain(args[1]); /* the fallback is returned as a strong reference */
        return args[1];
    }
    return emv_null();
}

static EmValue *host_do_read_file(const char *path, bool as_bytes)
{
    unsigned char *bytes = NULL;
    size_t length = 0;
    int code = 0;

    if (!host_read_file(path, &bytes, &length, &code)) {
        return host_errno_error("Could not read", path, code);
    }
    if (as_bytes) {
        EmValue *array = emv_array();
        size_t i;
        if (array == NULL) {
            free(bytes);
            return NULL;
        }
        for (i = 0; i < length; i++) {
            EmValue *item = emv_number((double)bytes[i]);
            if (item == NULL) {
                emv_release(array);
                free(bytes);
                return NULL;
            }
            emv_array_push(array, item);
            emv_release(item);
        }
        free(bytes);
        return array;
    }
    {
        EmValue *text = emv_string_len((const char *)bytes, length);
        free(bytes);
        return text;
    }
}

static EmValue *em_host_read_file(EmVm *vm, EmValue **args, size_t argc,
                                  char **err)
{
    const char *path;
    EmValue *error = NULL;

    (void)vm;
    (void)err;
    if (!host_string_arg(args, argc, 0, "host.readFile", "path", &path,
                         &error)) {
        return error;
    }
    return host_do_read_file(path, false);
}

static EmValue *em_host_read_bytes(EmVm *vm, EmValue **args, size_t argc,
                                   char **err)
{
    const char *path;
    EmValue *error = NULL;

    (void)vm;
    (void)err;
    if (!host_string_arg(args, argc, 0, "host.readBytes", "path", &path,
                         &error)) {
        return error;
    }
    return host_do_read_file(path, true);
}

static EmValue *em_host_write_file(EmVm *vm, EmValue **args, size_t argc,
                                   char **err)
{
    const char *path;
    const char *text;
    size_t length;
    EmValue *error = NULL;
    int code = 0;

    (void)vm;
    (void)err;
    if (!host_string_arg(args, argc, 0, "host.writeFile", "path", &path,
                         &error)) {
        return error;
    }
    if (argc < 2 || !emv_is_string(args[1])) {
        return host_error("host.writeFile expects argument 2 (text) as a string");
    }
    text = emv_as_cstr(args[1]);
    length = emv_strlen(args[1]);
    if (!host_write_file(path, (const unsigned char *)text, length, &code)) {
        return host_errno_error("Could not write", path, code);
    }
    return emv_bool(true);
}

static EmValue *em_host_write_bytes(EmVm *vm, EmValue **args, size_t argc,
                                    char **err)
{
    const char *path;
    EmValue *error = NULL;
    size_t length;
    size_t i;
    unsigned char *bytes;
    int code = 0;

    (void)vm;
    (void)err;
    if (!host_string_arg(args, argc, 0, "host.writeBytes", "path", &path,
                         &error)) {
        return error;
    }
    if (argc < 2 || !emv_is_array(args[1])) {
        return host_error("host.writeBytes expects argument 2 (bytes) as an array");
    }

    /* Validate everything before touching the file: bad input must not leave
     * a truncated file behind. */
    length = emv_array_len(args[1]);
    bytes = malloc(length > 0 ? length : 1);
    if (bytes == NULL) {
        return host_error("host.writeBytes could not allocate the byte buffer");
    }
    for (i = 0; i < length; i++) {
        EmValue *item = emv_array_get(args[1], i);
        double number;
        if (item == NULL || !emv_is_number(item)) {
            free(bytes);
            return host_error_fmt("host.writeBytes expects a whole number "
                                  "from 0 to 255 at index %zu, found %s",
                                  i, item == NULL ? "null" : emv_debug(item));
        }
        number = emv_as_number(item);
        if (!isfinite(number) || number < 0.0 || number > 255.0 ||
            number != (double)(unsigned char)number) {
            free(bytes);
            return host_error_fmt("Expected a whole number from 0 to 255 at "
                                  "index %zu for host.writeBytes, found %.17g",
                                  i, number);
        }
        bytes[i] = (unsigned char)number;
    }

    if (!host_write_file(path, bytes, length, &code)) {
        free(bytes);
        return host_errno_error("Could not write", path, code);
    }
    free(bytes);
    return emv_bool(true);
}

static EmValue *host_write_stream(EmValue **args, size_t argc,
                                  const char *function, FILE *stream)
{
    const char *text;
    size_t length;
    EmValue *error = NULL;

    if (!host_string_arg(args, argc, 0, function, "text", &text, &error)) {
        return error;
    }
    length = emv_strlen(args[0]);
    if (length > 0 && fwrite(text, 1, length, stream) != length) {
        return host_error_fmt("Could not write to %s: %s", function + 5,
                              strerror(errno != 0 ? errno : EIO));
    }
    if (fflush(stream) != 0) {
        return host_error_fmt("Could not flush %s: %s", function + 5,
                              strerror(errno != 0 ? errno : EIO));
    }
    return emv_bool(true);
}

static EmValue *em_host_stdout(EmVm *vm, EmValue **args, size_t argc,
                               char **err)
{
    (void)vm;
    (void)err;
    return host_write_stream(args, argc, "host.stdout", stdout);
}

static EmValue *em_host_stderr(EmVm *vm, EmValue **args, size_t argc,
                               char **err)
{
    (void)vm;
    (void)err;
    return host_write_stream(args, argc, "host.stderr", stderr);
}

static EmValue *em_host_exit(EmVm *vm, EmValue **args, size_t argc,
                             char **err)
{
    double code;
    int status = 0;

    (void)vm;
    (void)err;
    if (argc < 1 || !emv_is_number(args[0])) {
        return host_error("host.exit expects argument 1 (code) as a number");
    }
    code = emv_as_number(args[0]);
    if (isfinite(code)) {
        if (code > (double)INT_MAX) {
            status = INT_MAX;
        } else if (code < (double)INT_MIN) {
            status = INT_MIN;
        } else {
            status = (int)code;
        }
    }
    fflush(NULL);
    exit(status);
}

static EmValue *em_host_cwd(EmVm *vm, EmValue **args, size_t argc,
                            char **err)
{
    size_t capacity = 256;
    (void)vm;
    (void)args;
    (void)argc;
    (void)err;

    for (;;) {
        char *buffer = malloc(capacity);
        int code;
        EmValue *result;
        if (buffer == NULL) {
            return host_error("host.cwd could not allocate a buffer");
        }
        if (getcwd(buffer, capacity) != NULL) {
            result = emv_string(buffer);
            free(buffer);
            return result;
        }
        code = errno;
        free(buffer);
        if (code == ERANGE && capacity <= SIZE_MAX / 2) {
            capacity *= 2;
            continue;
        }
        return host_errno_error("Could not determine the current directory",
                                "", code);
    }
}

static EmValue *em_host_temp_dir(EmVm *vm, EmValue **args, size_t argc,
                                 char **err)
{
    const char *directory;
    (void)vm;
    (void)args;
    (void)argc;
    (void)err;

    directory = getenv("TMPDIR");
    if (directory != NULL && directory[0] != '\0') {
        return emv_string(directory);
    }
    return emv_string("/tmp");
}

static EmValue *em_host_pid(EmVm *vm, EmValue **args, size_t argc,
                            char **err)
{
    (void)vm;
    (void)args;
    (void)argc;
    (void)err;
    return emv_number((double)getpid());
}

int em_host_register(EmVm *vm, int argc, char **argv, char **err)
{
    HostState *state;
    EmValue *module;
    int result;

    if (err != NULL) {
        *err = NULL;
    }
    if (vm == NULL) {
        host_set_err(err, "em_host_register requires a VM");
        return -1;
    }
    state = host_state_get(vm, true);
    if (state == NULL) {
        host_set_err(err, "em_host_register could not allocate host state");
        return -1;
    }
    if (!host_args_set(state, argc, argv)) {
        host_set_err(err, "em_host_register could not copy program arguments");
        return -1;
    }
    module = em_host_module(vm);
    if (module == NULL) {
        host_set_err(err, "em_host_register could not create the host module");
        return -1;
    }
    result = em_vm_register_import(vm, "host", module);
    emv_release(module); /* the VM retains registered imports */
    if (result != 0) {
        host_set_err(err, "em_host_register could not register the host module");
        return -1;
    }
    state->module = em_vm_lookup_import(vm, "host");
    return 0;
}

EmValue *em_host_module(EmVm *vm)
{
    static const struct {
        const char *key;
        const char *native_name;
        EmNativeFn fn;
    } members[] = {
        {"args", "host.args", em_host_args},
        {"env", "host.env", em_host_env},
        {"readFile", "host.readFile", em_host_read_file},
        {"writeFile", "host.writeFile", em_host_write_file},
        {"readBytes", "host.readBytes", em_host_read_bytes},
        {"writeBytes", "host.writeBytes", em_host_write_bytes},
        {"stdout", "host.stdout", em_host_stdout},
        {"stderr", "host.stderr", em_host_stderr},
        {"exit", "host.exit", em_host_exit},
        {"cwd", "host.cwd", em_host_cwd},
        {"tempDir", "host.tempDir", em_host_temp_dir},
        {"pid", "host.pid", em_host_pid}
    };
    EmValue *module = emv_object();
    size_t i;

    (void)vm;
    if (module == NULL) {
        return NULL;
    }
    for (i = 0; i < sizeof(members) / sizeof(members[0]); i++) {
        EmValue *native = emv_native(members[i].native_name, members[i].fn);
        if (native == NULL) {
            emv_release(module);
            return NULL;
        }
        emv_object_set(module, members[i].key, native);
        emv_release(native);
    }
    return module;
}

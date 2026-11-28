/*
 * VM core for the independent C `emrun` host.
 *
 * This file owns the top-level interpreter loop: scopes, constants, stack
 * operations, arithmetic, control flow, arrays/objects, Say and Import.
 * Function calls are dispatched synchronously through the vm_call.c entry
 * points, which run callee bytecode on their own stack; therefore this loop
 * never pushes a frame for a call. Return just finishes the root chunk.
 */

#include "vm.h"
#include "vm_call_internal.h"

#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *name;
    EmValue *module;
} EmImportEntry;

struct EmFrame {
    const EmChunk *chunk;
    EmEnv *env; /* retained through em_call_env_retain */
    size_t ip;
};

struct EmVm {
    EmEnv *globals;
    EmValue **stack;
    size_t stack_len;
    size_t stack_cap;
    EmFrame *frames;
    size_t nframes;
    size_t frames_cap;
    char *error_message;
    EmImportEntry *imports;
    size_t nimports;
    size_t imports_cap;
};

void *em_alloc(size_t size)
{
    void *p = malloc(size == 0 ? 1 : size);
    if (!p) {
        fputs("out of memory\n", stderr);
        abort();
    }
    return p;
}

static void *em_realloc(void *ptr, size_t size)
{
    void *p = realloc(ptr, size == 0 ? 1 : size);
    if (!p) {
        fputs("out of memory\n", stderr);
        abort();
    }
    return p;
}

char *em_strdup(const char *s)
{
    size_t n = strlen(s) + 1;
    char *copy = em_alloc(n);
    memcpy(copy, s, n);
    return copy;
}

static char *em_vformat(const char *fmt, va_list ap)
{
    va_list probe;
    va_copy(probe, ap);
    int needed = vsnprintf(NULL, 0, fmt, probe);
    va_end(probe);
    if (needed < 0) {
        fputs("failed to format an error message\n", stderr);
        abort();
    }
    char *out = em_alloc((size_t)needed + 1);
    vsnprintf(out, (size_t)needed + 1, fmt, ap);
    return out;
}

char *em_format(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    char *out = em_vformat(fmt, ap);
    va_end(ap);
    return out;
}

static const char *em_type_name(EmValueType type)
{
    switch (type) {
    case EMV_NUMBER: return "number";
    case EMV_STRING: return "string";
    case EMV_BOOL:   return "boolean";
    case EMV_NULL:   return "null";
    case EMV_ARRAY:  return "array";
    case EMV_OBJECT: return "object";
    case EMV_CLOSURE:
    case EMV_NATIVE: return "function";
    }
    return "unknown";
}

/* ------------------------------------------------------------------------- */
/* VM lifecycle, errors and registers                                        */
/* ------------------------------------------------------------------------- */

static void vm_release_stack(EmVm *vm)
{
    while (vm->stack_len > 0) emv_release(vm->stack[--vm->stack_len]);
}

static void vm_release_frames(EmVm *vm)
{
    while (vm->nframes > 0) em_call_env_release(vm->frames[--vm->nframes].env);
}

EmVm *em_vm_new(void)
{
    EmVm *vm = em_alloc(sizeof *vm);
    memset(vm, 0, sizeof *vm);
    vm->globals = em_call_env_new(NULL);
    return vm;
}

void em_vm_free(EmVm *vm)
{
    if (!vm) return;
    vm_release_stack(vm);
    vm_release_frames(vm);
    free(vm->stack);
    free(vm->frames);
    for (size_t i = 0; i < vm->nimports; i++) {
        free(vm->imports[i].name);
        emv_release(vm->imports[i].module);
    }
    free(vm->imports);
    em_call_env_release(vm->globals);
    free(vm->error_message);
    free(vm);
}

void em_vm_error(EmVm *vm, uint32_t line, uint32_t col, const char *message)
{
    if (!vm) return;
    char *formatted = em_format("line %u, col %u: %s", line, col,
                                message ? message : "unknown error");
    free(vm->error_message);
    vm->error_message = formatted;
}

static void em_vm_errorf(EmVm *vm, uint32_t line, uint32_t col, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    char *body = em_vformat(fmt, ap);
    va_end(ap);
    em_vm_error(vm, line, col, body);
    free(body);
}

char *em_vm_error_message(EmVm *vm)
{
    if (!vm || !vm->error_message) return NULL;
    return em_strdup(vm->error_message);
}

EmEnv *em_vm_global_env(EmVm *vm)
{
    return vm ? vm->globals : NULL;
}

int em_vm_register_native(EmVm *vm, const char *name, EmNativeFn fn)
{
    if (!vm || !name || !fn) return -1;
    EmValue *native = emv_native(name, fn);
    em_call_env_define(vm->globals, name, native);
    emv_release(native);
    return 0;
}

int em_vm_register_import(EmVm *vm, const char *name, EmValue *module)
{
    if (!vm || !name || !module) return -1;
    for (size_t i = 0; i < vm->nimports; i++) {
        if (strcmp(vm->imports[i].name, name) == 0) {
            emv_retain(module);
            EmValue *old = vm->imports[i].module;
            vm->imports[i].module = module;
            emv_release(old);
            return 0;
        }
    }
    if (vm->nimports == vm->imports_cap) {
        size_t cap = vm->imports_cap == 0 ? 8 : vm->imports_cap * 2;
        vm->imports = em_realloc(vm->imports, cap * sizeof *vm->imports);
        vm->imports_cap = cap;
    }
    emv_retain(module);
    vm->imports[vm->nimports].name = em_strdup(name);
    vm->imports[vm->nimports].module = module;
    vm->nimports++;
    return 0;
}

EmValue *em_vm_lookup_import(EmVm *vm, const char *name)
{
    if (!vm || !name) return NULL;
    for (size_t i = 0; i < vm->nimports; i++) {
        if (strcmp(vm->imports[i].name, name) == 0) return vm->imports[i].module;
    }
    return NULL;
}

/* ------------------------------------------------------------------------- */
/* Stack and the single root frame                                           */
/* ------------------------------------------------------------------------- */

static void stack_push(EmVm *vm, EmValue *value)
{
    if (vm->stack_len == vm->stack_cap) {
        size_t cap = vm->stack_cap == 0 ? 256 : vm->stack_cap * 2;
        vm->stack = em_realloc(vm->stack, cap * sizeof *vm->stack);
        vm->stack_cap = cap;
    }
    vm->stack[vm->stack_len++] = value;
}

static EmValue *stack_pop(EmVm *vm)
{
    if (vm->stack_len == 0) return NULL;
    return vm->stack[--vm->stack_len];
}

static EmValue *stack_peek(EmVm *vm)
{
    if (vm->stack_len == 0) return NULL;
    return vm->stack[vm->stack_len - 1];
}

static int push_root_frame(EmVm *vm, const EmChunk *chunk)
{
    if (vm->frames_cap == 0) {
        vm->frames = em_alloc(sizeof *vm->frames);
        vm->frames_cap = 1;
    }
    EmFrame *frame = &vm->frames[vm->nframes++];
    frame->chunk = chunk;
    frame->env = vm->globals;
    em_call_env_retain(frame->env);
    frame->ip = 0;
    return 0;
}

/* ------------------------------------------------------------------------- */
/* Operation helpers                                                         */
/* ------------------------------------------------------------------------- */

static const char STACK_UNDERFLOW[] = "Corrupt bytecode: the value stack ran empty";
static const char CORRUPT_STACK[] = "Corrupt bytecode: unexpected value on the stack";

static const char *chunk_name(const EmChunk *chunk, uint32_t idx, char **err)
{
    if (idx >= chunk->nconstants || chunk->constants[idx].tag != EMC_STRING ||
        !chunk->constants[idx].as.string) {
        *err = em_strdup("Corrupt bytecode: expected a name constant");
        return NULL;
    }
    return chunk->constants[idx].as.string;
}

static EmValue *const_to_value(const EmConst *constant)
{
    switch (constant->tag) {
    case EMC_NUMBER: return emv_number(constant->as.number);
    case EMC_STRING: return emv_string(constant->as.string);
    case EMC_BOOL:   return emv_bool(constant->as.boolean);
    case EMC_NULL:   return emv_null();
    }
    return emv_null();
}

static int64_t double_to_i64(double n)
{
    const double limit = 9223372036854775808.0; /* 2^63 */
    if (isnan(n)) return 0;
    if (n >= limit) return INT64_MAX;
    if (n <= -limit) return INT64_MIN;
    return (int64_t)n;
}

static bool expect_number(EmValue *value, const char *context, double *out, char **err)
{
    if (!emv_is_number(value)) {
        *err = em_format("Type mismatch: expected a number %s, found %s",
                         context, em_type_name(emv_type(value)));
        return false;
    }
    *out = emv_as_number(value);
    return true;
}

static size_t repeat_count(double n)
{
    if (!isfinite(n) || n <= 0.0) return 0;
    if (n >= 1000000.0) return 1000000;
    return (size_t)n;
}

static EmValue *make_repeated_string(const char *s, size_t len, size_t count)
{
    if (len == 0 || count == 0) return emv_string("");
    if (len > SIZE_MAX / count) return emv_string("");
    size_t total = len * count;
    char *buf = em_alloc(total + 1);
    for (size_t i = 0; i < count; i++) memcpy(buf + i * len, s, len);
    buf[total] = '\0';
    EmValue *out = emv_string_len(buf, total);
    free(buf);
    return out;
}

static EmValue *binary_op(uint8_t op, EmValue *left, EmValue *right, char **err)
{
    if (op == OP_EQ) return emv_bool(emv_equal(left, right));
    if (op == OP_NOT_EQ) return emv_bool(!emv_equal(left, right));

    if (op == OP_ADD) {
        bool lstr = emv_is_string(left);
        bool rstr = emv_is_string(right);
        if (lstr || rstr) {
            char *ltext = lstr ? em_strdup(emv_as_cstr(left)) : emv_display(left);
            char *rtext = rstr ? em_strdup(emv_as_cstr(right)) : emv_display(right);
            size_t ln = strlen(ltext ? ltext : "");
            size_t rn = strlen(rtext ? rtext : "");
            char *joined = em_alloc(ln + rn + 1);
            memcpy(joined, ltext ? ltext : "", ln);
            memcpy(joined + ln, rtext ? rtext : "", rn + 1);
            free(ltext);
            free(rtext);
            EmValue *out = emv_string_len(joined, ln + rn);
            free(joined);
            return out;
        }
    }

    if (op == OP_MUL) {
        if (emv_is_string(left) && emv_is_number(right)) {
            const char *s = emv_as_cstr(left);
            return make_repeated_string(s, strlen(s ? s : ""),
                                        repeat_count(emv_as_number(right)));
        }
        if (emv_is_number(left) && emv_is_string(right)) {
            const char *s = emv_as_cstr(right);
            return make_repeated_string(s, strlen(s ? s : ""),
                                        repeat_count(emv_as_number(left)));
        }
    }

    bool comparison = op == OP_GT || op == OP_LT || op == OP_GT_EQ || op == OP_LT_EQ;
    const char *left_ctx = comparison ? "on the left of the comparison"
                                      : "on the left of the operator";
    const char *right_ctx = comparison ? "on the right of the comparison"
                                       : "on the right of the operator";
    double a = 0.0;
    double b = 0.0;
    if (!expect_number(left, left_ctx, &a, err)) return NULL;
    if (!expect_number(right, right_ctx, &b, err)) return NULL;

    switch ((EmOp)op) {
    case OP_ADD:      return emv_number(a + b);
    case OP_SUB:      return emv_number(a - b);
    case OP_MUL:      return emv_number(a * b);
    case OP_DIV:
        if (b == 0.0) { *err = em_strdup("Division by zero"); return NULL; }
        return emv_number(a / b);
    case OP_MOD:
        if (b == 0.0) { *err = em_strdup("Division by zero"); return NULL; }
        return emv_number(fmod(a, b));
    case OP_FLOOR_DIV:
        if (b == 0.0) { *err = em_strdup("Division by zero"); return NULL; }
        return emv_number(floor(a / b));
    case OP_POW:      return emv_number(pow(a, b));
    case OP_BIT_AND:  return emv_number((double)(double_to_i64(a) & double_to_i64(b)));
    case OP_BIT_OR:   return emv_number((double)(double_to_i64(a) | double_to_i64(b)));
    case OP_BIT_XOR:  return emv_number((double)(double_to_i64(a) ^ double_to_i64(b)));
    case OP_SHL: {
        uint32_t shift = ((uint32_t)double_to_i64(b)) & 63u;
        uint64_t bits = ((uint64_t)double_to_i64(a)) << shift;
        int64_t value;
        memcpy(&value, &bits, sizeof value);
        return emv_number((double)value);
    }
    case OP_SHR: {
        uint32_t shift = ((uint32_t)double_to_i64(b)) & 63u;
        uint64_t bits = ((uint64_t)double_to_i64(a)) >> shift;
        int64_t value;
        memcpy(&value, &bits, sizeof value);
        return emv_number((double)value);
    }
    case OP_GT:       return emv_bool(a > b);
    case OP_LT:       return emv_bool(a < b);
    case OP_GT_EQ:    return emv_bool(a >= b);
    case OP_LT_EQ:    return emv_bool(a <= b);
    default:
        *err = em_strdup("Corrupt bytecode: unknown operator");
        return NULL;
    }
}

static void do_say(EmValue **args, size_t argc)
{
    for (size_t i = 0; i < argc; i++) {
        char *text = emv_display(args[i]);
        if (i > 0) fputc(' ', stdout);
        fputs(text ? text : "", stdout);
        free(text);
    }
    fputc('\n', stdout);
    fflush(stdout);
}

static EmValue *get_property(EmValue *base, const char *name, char **err)
{
    if (emv_is_object(base)) {
        EmValue *value = emv_object_get(base, name);
        if (value) {
            emv_retain(value);
            return value;
        }
        *err = em_format("Unknown property '%s'", name);
        return NULL;
    }
    *err = em_format("Cannot access property '%s' on a %s",
                     name, em_type_name(emv_type(base)));
    return NULL;
}

static bool utf8_item(const char *s, size_t len, size_t index, size_t *start, size_t *item_len)
{
    size_t seen = 0;
    size_t pos = 0;
    size_t begin = 0;
    while (pos < len) {
        if ((unsigned char)s[pos] < 0x80 || ((unsigned char)s[pos] & 0xc0) != 0x80) {
            if (seen == index) {
                *start = begin;
                *item_len = pos - begin;
                return true;
            }
            begin = pos;
            seen++;
        }
        pos++;
    }
    if (seen == index) {
        *start = begin;
        *item_len = len - begin;
        return true;
    }
    return false;
}

static bool array_index(EmValue *index, size_t len, size_t *out, char **err, bool is_string)
{
    double n = 0.0;
    if (!expect_number(index, is_string ? "as a string index" : "as an array index", &n, err))
        return false;
    int64_t i = double_to_i64(n);
    if (i < 0 || (uint64_t)i >= (uint64_t)len) {
        *err = em_format(is_string ? "String index %lld is out of bounds (length %zu)"
                                   : "Array index %lld is out of bounds (length %zu)",
                         (long long)i, len);
        return false;
    }
    *out = (size_t)i;
    return true;
}

static EmValue *get_index(EmValue *base, EmValue *index, char **err)
{
    if (emv_is_array(base)) {
        size_t at = 0;
        if (!array_index(index, emv_array_len(base), &at, err, false)) return NULL;
        EmValue *item = emv_array_get(base, at);
        emv_retain(item);
        return item;
    }
    if (emv_is_string(base)) {
        const char *s = emv_as_cstr(base);
        size_t byte_len = emv_strlen(base);
        size_t chars = 0;
        for (size_t i = 0; i < byte_len; i++) {
            unsigned char c = (unsigned char)s[i];
            if (c < 0x80 || (c & 0xc0) != 0x80) chars++;
        }
        size_t at = 0;
        if (!array_index(index, chars, &at, err, true)) return NULL;
        size_t start = 0;
        size_t item_len = 0;
        if (!utf8_item(s, byte_len, at, &start, &item_len)) {
            *err = em_format("String index %zu is out of bounds (length %zu)", at, chars);
            return NULL;
        }
        return emv_string_len(s + start, item_len);
    }
    *err = em_format("Cannot index into a %s", em_type_name(emv_type(base)));
    return NULL;
}

static bool set_index(EmValue *base, EmValue *index, EmValue *value, char **err)
{
    if (!emv_is_array(base)) {
        *err = em_format("Cannot index into a %s", em_type_name(emv_type(base)));
        return false;
    }
    size_t at = 0;
    if (!array_index(index, emv_array_len(base), &at, err, false)) return false;
    emv_array_set(base, at, value);
    return true;
}

/* ------------------------------------------------------------------------- */
/* Top-level interpreter loop                                                */
/* ------------------------------------------------------------------------- */

#define FAILS(text) do { em_vm_error(vm, ins.line, ins.col, (text)); goto fail; } while (0)
#define FAILF(...) do { em_vm_errorf(vm, ins.line, ins.col, __VA_ARGS__); goto fail; } while (0)
#define GET_NAME(chunk, idx, out)                                                   \
    do {                                                                            \
        char *name_err = NULL;                                                      \
        (out) = chunk_name((chunk), (idx), &name_err);                              \
        if (!(out)) { em_vm_error(vm, ins.line, ins.col, name_err);                 \
                      free(name_err); goto fail; }                                  \
    } while (0)

static int execute(EmVm *vm)
{
    for (;;) {
        EmFrame *frame = &vm->frames[vm->nframes - 1];
        const EmChunk *chunk = frame->chunk;
        if (frame->ip >= chunk->ncode) break; /* falling off the end returns null */

        EmInstr ins = chunk->code[frame->ip++];

        switch ((EmOp)ins.tag) {
        case OP_LOAD_CONST: {
            if (ins.a >= chunk->nconstants) FAILS("Corrupt bytecode: invalid constant index");
            stack_push(vm, const_to_value(&chunk->constants[ins.a]));
            break;
        }
        case OP_POP: {
            EmValue *value = stack_pop(vm);
            if (!value) FAILS(STACK_UNDERFLOW);
            emv_release(value);
            break;
        }
        case OP_GET_VAR: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            EmValue *value = em_call_env_get(frame->env, name);
            if (!value) FAILF("Unknown variable '%s'", name);
            emv_retain(value);
            stack_push(vm, value);
            break;
        }
        case OP_SET_VAR: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            EmValue *value = stack_pop(vm);
            if (!value) FAILS(STACK_UNDERFLOW);
            em_call_env_assign(frame->env, name, value);
            emv_release(value);
            break;
        }
        case OP_DEFINE_VAR: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            EmValue *value = stack_pop(vm);
            if (!value) FAILS(STACK_UNDERFLOW);
            em_call_env_define(frame->env, name, value);
            emv_release(value);
            break;
        }
        case OP_GET_PROP: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            EmValue *base = stack_pop(vm);
            if (!base) FAILS(STACK_UNDERFLOW);
            char *err = NULL;
            EmValue *value = get_property(base, name, &err);
            emv_release(base);
            if (!value) {
                em_vm_error(vm, ins.line, ins.col, err);
                free(err);
                goto fail;
            }
            stack_push(vm, value);
            break;
        }
        case OP_SET_PROP: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            EmValue *base = stack_pop(vm);
            EmValue *value = stack_pop(vm);
            if (!base || !value) {
                emv_release(base);
                emv_release(value);
                FAILS(STACK_UNDERFLOW);
            }
            if (!emv_is_object(base)) {
                const char *type = em_type_name(emv_type(base));
                emv_release(base);
                emv_release(value);
                FAILF("Cannot set property '%s' on a %s", name, type);
            }
            emv_object_set(base, name, value);
            emv_release(base);
            emv_release(value);
            break;
        }
        case OP_GET_VAR_OR_OBJECT: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            if (!em_call_env_has(frame->env, name)) {
                EmValue *object = emv_object();
                em_call_env_define(frame->env, name, object);
                emv_release(object);
            }
            EmValue *value = em_call_env_get(frame->env, name);
            emv_retain(value);
            stack_push(vm, value);
            break;
        }
        case OP_GET_PROP_OR_OBJECT: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            EmValue *base = stack_pop(vm);
            if (!base) FAILS(STACK_UNDERFLOW);
            if (emv_is_object(base) && !emv_object_has(base, name)) {
                EmValue *object = emv_object();
                emv_object_set(base, name, object);
                emv_release(object);
            }
            char *err = NULL;
            EmValue *value = get_property(base, name, &err);
            emv_release(base);
            if (!value) {
                em_vm_error(vm, ins.line, ins.col, err);
                free(err);
                goto fail;
            }
            stack_push(vm, value);
            break;
        }
        case OP_GET_INDEX:
        case OP_GET_INDEX_KEEP: {
            size_t len = vm->stack_len;
            if (len < 2) FAILS(STACK_UNDERFLOW);
            EmValue *base = vm->stack[len - 2];
            EmValue *index = vm->stack[len - 1];
            char *err = NULL;
            EmValue *value = get_index(base, index, &err);
            if (!value) {
                em_vm_error(vm, ins.line, ins.col, err);
                free(err);
                goto fail;
            }
            if ((EmOp)ins.tag == OP_GET_INDEX) {
                emv_release(stack_pop(vm));
                emv_release(stack_pop(vm));
            }
            stack_push(vm, value);
            break;
        }
        case OP_SET_INDEX: {
            if (vm->stack_len < 3) FAILS(STACK_UNDERFLOW);
            /* Stack layout is [value, base, index]: the index is on top. */
            EmValue *value = vm->stack[vm->stack_len - 3];
            EmValue *base = vm->stack[vm->stack_len - 2];
            EmValue *index = vm->stack[vm->stack_len - 1];
            char *err = NULL;
            if (!set_index(base, index, value, &err)) {
                em_vm_error(vm, ins.line, ins.col, err);
                free(err);
                goto fail;
            }
            for (int i = 0; i < 3; i++) emv_release(stack_pop(vm));
            break;
        }
        case OP_MAKE_ARRAY: {
            size_t count = ins.a;
            if (count > vm->stack_len) FAILS(STACK_UNDERFLOW);
            EmValue *array = emv_array();
            for (size_t i = vm->stack_len - count; i < vm->stack_len; i++) {
                emv_array_push(array, vm->stack[i]);
            }
            for (size_t i = 0; i < count; i++) emv_release(stack_pop(vm));
            stack_push(vm, array);
            break;
        }
        case OP_MAKE_OBJECT: {
            size_t pairs = ins.a;
            if (pairs > SIZE_MAX / 2 || pairs * 2 > vm->stack_len) FAILS(STACK_UNDERFLOW);
            size_t total = pairs * 2;
            EmValue **flat = em_alloc((total == 0 ? 1 : total) * sizeof *flat);
            for (size_t i = total; i > 0; i--) flat[i - 1] = stack_pop(vm);
            EmValue *object = emv_object();
            for (size_t i = 0; i < total; i += 2) {
                char *key = emv_display(flat[i]);
                emv_object_set(object, key ? key : "", flat[i + 1]);
                free(key);
                emv_release(flat[i]);
                emv_release(flat[i + 1]);
            }
            free(flat);
            stack_push(vm, object);
            break;
        }
        case OP_MAKE_CLOSURE: {
            if (ins.a >= chunk->nfunctions) FAILS("Corrupt bytecode: invalid function index");
            /* Top-level closures capture the globals env, which outlives them;
             * emv_closure avoids adding a retain that value.c cannot release. */
            EmValue *closure = emv_closure(&chunk->functions[ins.a], frame->env);
            if (!closure) FAILS("Could not create a closure");
            stack_push(vm, closure);
            break;
        }
        case OP_CALL:
        case OP_METHOD_CALL: {
            size_t argc = (EmOp)ins.tag == OP_CALL ? ins.a : ins.b;
            size_t name_idx = (EmOp)ins.tag == OP_CALL ? ins.b : ins.a;
            const char *name;
            GET_NAME(chunk, name_idx, name);
            if (argc > vm->stack_len) FAILS(STACK_UNDERFLOW);
            EmValue **args = em_alloc((argc == 0 ? 1 : argc) * sizeof *args);
            for (size_t i = argc; i > 0; i--) args[i - 1] = stack_pop(vm);
            EmValue *target = stack_pop(vm);
            if (!target) {
                for (size_t i = 0; i < argc; i++) emv_release(args[i]);
                free(args);
                FAILS(STACK_UNDERFLOW);
            }
            EmValue *result = NULL;
            char *call_err = NULL;
            int status;
            if ((EmOp)ins.tag == OP_CALL) {
                /* vm_call.c executes the callee synchronously and never
                 * touches this VM's frame or value stack. */
                status = em_vm_call_value_named(vm, target, args, argc, name,
                                                &result, &call_err);
            } else {
                status = em_vm_call_method(vm, target, name, args, argc,
                                           &result, &call_err);
            }
            for (size_t i = 0; i < argc; i++) emv_release(args[i]);
            free(args);
            emv_release(target);
            if (status != 0) {
                if (call_err) {
                    em_vm_error(vm, ins.line, ins.col, call_err);
                    free(call_err);
                } else {
                    em_vm_error(vm, ins.line, ins.col, "Call failed");
                }
                emv_release(result);
                goto fail;
            }
            free(call_err);
            stack_push(vm, result ? result : emv_null());
            break;
        }
        case OP_SAY: {
            size_t argc = ins.a;
            if (argc > vm->stack_len) FAILS(STACK_UNDERFLOW);
            EmValue **args = em_alloc((argc == 0 ? 1 : argc) * sizeof *args);
            for (size_t i = argc; i > 0; i--) args[i - 1] = stack_pop(vm);
            do_say(args, argc);
            for (size_t i = 0; i < argc; i++) emv_release(args[i]);
            free(args);
            stack_push(vm, emv_null());
            break;
        }
        case OP_ADD:
        case OP_SUB:
        case OP_MUL:
        case OP_DIV:
        case OP_MOD:
        case OP_FLOOR_DIV:
        case OP_POW:
        case OP_BIT_AND:
        case OP_BIT_OR:
        case OP_BIT_XOR:
        case OP_SHL:
        case OP_SHR:
        case OP_EQ:
        case OP_NOT_EQ:
        case OP_GT:
        case OP_LT:
        case OP_GT_EQ:
        case OP_LT_EQ: {
            EmValue *right = stack_pop(vm);
            EmValue *left = stack_pop(vm);
            if (!right || !left) {
                emv_release(right);
                emv_release(left);
                FAILS(STACK_UNDERFLOW);
            }
            char *err = NULL;
            EmValue *value = binary_op(ins.tag, left, right, &err);
            emv_release(left);
            emv_release(right);
            if (!value) {
                em_vm_error(vm, ins.line, ins.col, err);
                free(err);
                goto fail;
            }
            stack_push(vm, value);
            break;
        }
        case OP_NEG:
        case OP_BIT_NOT:
        case OP_NOT:
        case OP_TO_BOOL: {
            EmValue *value = stack_pop(vm);
            if (!value) FAILS(STACK_UNDERFLOW);
            EmValue *out = NULL;
            if ((EmOp)ins.tag == OP_NOT || (EmOp)ins.tag == OP_TO_BOOL) {
                out = emv_bool(emv_truthy(value));
            } else {
                double n = 0.0;
                char *err = NULL;
                const char *context = (EmOp)ins.tag == OP_NEG ? "for unary '-'"
                                                              : "for bitwise '~'";
                if (!expect_number(value, context, &n, &err)) {
                    em_vm_error(vm, ins.line, ins.col, err);
                    free(err);
                    emv_release(value);
                    goto fail;
                }
                out = (EmOp)ins.tag == OP_NEG
                          ? emv_number(-n)
                          : emv_number((double)~double_to_i64(n));
            }
            emv_release(value);
            stack_push(vm, out);
            break;
        }
        case OP_DUP: {
            EmValue *top = stack_peek(vm);
            if (!top) FAILS(STACK_UNDERFLOW);
            emv_retain(top);
            stack_push(vm, top);
            break;
        }
        case OP_ROTATE: {
            size_t count = (size_t)ins.a + 1;
            if (count > vm->stack_len) FAILS(STACK_UNDERFLOW);
            size_t split = vm->stack_len - count;
            EmValue *top = vm->stack[vm->stack_len - 1];
            if (count > 1) {
                memmove(&vm->stack[split + 1], &vm->stack[split],
                        (count - 1) * sizeof *vm->stack);
            }
            vm->stack[split] = top;
            break;
        }
        case OP_GET_PROP_KEEP: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            EmValue *base = stack_peek(vm);
            if (!base) FAILS(STACK_UNDERFLOW);
            char *err = NULL;
            EmValue *value = get_property(base, name, &err);
            if (!value) {
                em_vm_error(vm, ins.line, ins.col, err);
                free(err);
                goto fail;
            }
            stack_push(vm, value);
            break;
        }
        case OP_JUMP:
            frame->ip = ins.a;
            break;
        case OP_JUMP_IF_FALSE: {
            EmValue *value = stack_pop(vm);
            if (!value) FAILS(STACK_UNDERFLOW);
            bool truthy = emv_truthy(value);
            emv_release(value);
            if (!truthy) frame->ip = ins.a;
            break;
        }
        case OP_JUMP_IF_FALSE_PEEK: {
            EmValue *value = stack_peek(vm);
            if (!value) FAILS(STACK_UNDERFLOW);
            if (!emv_truthy(value)) frame->ip = ins.a;
            break;
        }
        case OP_JUMP_IF_TRUE_PEEK: {
            EmValue *value = stack_peek(vm);
            if (!value) FAILS(STACK_UNDERFLOW);
            if (emv_truthy(value)) frame->ip = ins.a;
            break;
        }
        case OP_JUMP_IF_NULL_PEEK: {
            EmValue *value = stack_peek(vm);
            if (!value) FAILS(STACK_UNDERFLOW);
            if (emv_is_null(value)) frame->ip = ins.a;
            break;
        }
        case OP_REPEAT_SETUP: {
            EmValue *count = stack_pop(vm);
            if (!count) FAILS(STACK_UNDERFLOW);
            double n = 0.0;
            char *err = NULL;
            if (!expect_number(count, "as a repeat count", &n, &err)) {
                em_vm_error(vm, ins.line, ins.col, err);
                free(err);
                emv_release(count);
                goto fail;
            }
            emv_release(count);
            if (n < 0.0) FAILS("repeat count must not be negative");
            stack_push(vm, emv_number((double)double_to_i64(n)));
            break;
        }
        case OP_REPEAT_NEXT: {
            EmValue *counter = stack_peek(vm);
            if (!counter || !emv_is_number(counter)) FAILS(CORRUPT_STACK);
            double remaining = emv_as_number(counter);
            if (remaining <= 0.0) {
                emv_release(stack_pop(vm));
                frame->ip = ins.a;
            } else {
                emv_release(vm->stack[vm->stack_len - 1]);
                vm->stack[vm->stack_len - 1] = emv_number(remaining - 1.0);
            }
            break;
        }
        case OP_ITER_SETUP: {
            EmValue *iterable = stack_pop(vm);
            if (!iterable) FAILS(STACK_UNDERFLOW);
            if (!emv_is_array(iterable)) {
                char *msg = em_format("Cannot iterate over a %s",
                                      em_type_name(emv_type(iterable)));
                emv_release(iterable);
                em_vm_error(vm, ins.line, ins.col, msg);
                free(msg);
                goto fail;
            }
            EmValue *items = emv_array();
            size_t n = emv_array_len(iterable);
            for (size_t i = 0; i < n; i++) emv_array_push(items, emv_array_get(iterable, i));
            emv_release(iterable);
            stack_push(vm, items);
            stack_push(vm, emv_number(0.0));
            break;
        }
        case OP_ITER_NEXT: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            if (vm->stack_len < 2) FAILS(STACK_UNDERFLOW);
            EmValue *cursor = vm->stack[vm->stack_len - 1];
            EmValue *source = vm->stack[vm->stack_len - 2];
            if (!emv_is_number(cursor) || !emv_is_array(source)) FAILS(CORRUPT_STACK);
            size_t at = (size_t)double_to_i64(emv_as_number(cursor));
            if (at < emv_array_len(source)) {
                em_call_env_define(frame->env, name, emv_array_get(source, at));
                emv_release(vm->stack[vm->stack_len - 1]);
                vm->stack[vm->stack_len - 1] = emv_number((double)(at + 1));
            } else {
                emv_release(stack_pop(vm));
                emv_release(stack_pop(vm));
                frame->ip = ins.b;
            }
            break;
        }
        case OP_RETURN: {
            EmValue *value = stack_pop(vm);
            if (!value) FAILS(STACK_UNDERFLOW);
            emv_release(value);
            goto done; /* Return from the root chunk ends the program. */
        }
        case OP_RETURN_NULL:
            goto done;
        case OP_IMPORT: {
            const char *name;
            GET_NAME(chunk, ins.a, name);
            EmValue *module = em_vm_lookup_import(vm, name);
            if (!module) FAILF("Unknown module '%s'", name);
            em_call_env_define(frame->env, name, module);
            break;
        }
        default:
            FAILF("Corrupt bytecode: unknown opcode %u", (unsigned)ins.tag);
        }
    }

done:
    return 0;

fail:
    vm_release_stack(vm);
    vm_release_frames(vm);
    return -1;
}

int em_vm_run(EmVm *vm, const EmChunk *chunk, char **err)
{
    if (err) *err = NULL;
    if (!vm || !chunk) {
        if (vm) em_vm_error(vm, 0, 0, "No bytecode to run");
        if (err) *err = vm ? em_vm_error_message(vm) : em_strdup("No bytecode to run");
        return -1;
    }
    free(vm->error_message);
    vm->error_message = NULL;
    vm_release_stack(vm);
    vm_release_frames(vm);
    push_root_frame(vm, chunk);

    int status = execute(vm);

    vm_release_stack(vm);
    vm_release_frames(vm);
    if (status != 0) {
        if (!vm->error_message) em_vm_error(vm, 0, 0, "Execution failed");
        if (err) *err = em_vm_error_message(vm);
        return -1;
    }
    return 0;
}

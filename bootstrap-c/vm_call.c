/*
 * Call machinery for the C bootstrap VM (round 4).
 *
 * em_vm_call_value is deliberately self-contained: the shared headers expose
 * values and chunks but not the VM core's frame layout. Calls therefore run a
 * small interpreter for the callee's chunk here. The VM core can dispatch its
 * Call / MakeClosure / Return opcodes through the public vm.h API and let this
 * file own argument binding, capture, recursion and return propagation.
 */

#include "vm_call_internal.h"

#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define EM_CALL_MAX_DEPTH 512
#define EM_CALL_MAX_REPEAT 1000000

struct EmEnv {
    EmEnv *parent;
    EmValue *vars;
    size_t refs;
};

typedef struct {
    EmVm *vm;
    const EmChunk *chunk;
    EmEnv *env;
    EmValue **stack;
    size_t count;
    size_t cap;
    size_t ip;
    int depth;
    EmValue *result;
} Exec;

/* ------------------------------------------------------------------ errors */

static char *format_message(const char *fmt, ...)
{
    va_list ap;
    va_list copy;
    int n;
    char *out;

    va_start(ap, fmt);
    va_copy(copy, ap);
    n = vsnprintf(NULL, 0, fmt, ap);
    va_end(ap);
    if (n < 0) {
        va_end(copy);
        return NULL;
    }
    out = malloc((size_t)n + 1);
    if (out) {
        vsnprintf(out, (size_t)n + 1, fmt, copy);
    }
    va_end(copy);
    return out;
}

static char *duplicate_string(const char *text)
{
    size_t len = strlen(text);
    char *copy = malloc(len + 1);
    if (copy) {
        memcpy(copy, text, len + 1);
    }
    return copy;
}

static void set_error(char **err, char *message)
{
    if (!message) {
        return;
    }
    if (err && !*err) {
        *err = message;
    } else {
        free(message);
    }
}

static const char *value_type_name(const EmValue *value)
{
    if (!value || emv_is_null(value)) {
        return "null";
    }
    if (emv_is_number(value)) {
        return "number";
    }
    if (emv_is_string(value)) {
        return "string";
    }
    if (emv_is_array(value)) {
        return "array";
    }
    if (emv_is_object(value)) {
        return "object";
    }
    if (emv_is_closure(value) || emv_is_native(value)) {
        return "function";
    }
    return "boolean";
}

/* -------------------------------------------------------------- environment */

EmEnv *em_call_env_new(EmEnv *parent)
{
    EmEnv *env = calloc(1, sizeof(*env));
    if (!env) {
        return NULL;
    }
    env->vars = emv_object();
    if (!env->vars) {
        free(env);
        return NULL;
    }
    env->parent = parent;
    env->refs = 1;
    return env;
}

void em_call_env_retain(EmEnv *env)
{
    if (env) {
        env->refs++;
    }
}

void em_call_env_release(EmEnv *env)
{
    if (!env) {
        return;
    }
    if (env->refs > 0 && --env->refs == 0) {
        emv_release(env->vars);
        free(env);
    }
}

bool em_call_env_has(const EmEnv *env, const char *name)
{
    for (; env; env = env->parent) {
        if (emv_object_has(env->vars, name)) {
            return true;
        }
    }
    return false;
}

EmValue *em_call_env_get(const EmEnv *env, const char *name)
{
    for (; env; env = env->parent) {
        EmValue *found = emv_object_get(env->vars, name);
        if (found) {
            return found;
        }
    }
    return NULL;
}

void em_call_env_define(EmEnv *env, const char *name, EmValue *value)
{
    if (env && name) {
        emv_object_set(env->vars, name, value);
    }
}

void em_call_env_assign(EmEnv *env, const char *name, EmValue *value)
{
    EmEnv *target = env;
    for (; env; env = env->parent) {
        if (emv_object_has(env->vars, name)) {
            emv_object_set(env->vars, name, value);
            return;
        }
    }
    /* A first assignment declares the name in the current scope. The loop
     * leaves `env` NULL on miss, so define into the saved starting scope. */
    em_call_env_define(target, name, value);
}

char **em_call_env_names(const EmEnv *env, size_t *count)
{
    size_t total = 0;
    size_t used = 0;
    size_t i;
    const EmEnv *cursor;
    char **names;

    for (cursor = env; cursor; cursor = cursor->parent) {
        size_t in_scope = 0;
        char **here = emv_object_keys(cursor->vars, &in_scope);
        total += in_scope;
        for (i = 0; i < in_scope; i++) {
            free(here[i]);
        }
        free(here);
    }
    names = calloc(total ? total : 1, sizeof(*names));
    if (!names) {
        if (count) {
            *count = 0;
        }
        return NULL;
    }
    for (; env; env = env->parent) {
        size_t in_scope = 0;
        char **here = emv_object_keys(env->vars, &in_scope);
        for (i = 0; i < in_scope && used < total; i++) {
            names[used++] = here[i];
        }
        for (; i < in_scope; i++) {
            free(here[i]);
        }
        free(here);
    }
    if (count) {
        *count = used;
    }
    return names;
}

/* ------------------------------------------------------------------- stack */

static int stack_push(Exec *exec, EmValue *value, char **err)
{
    if (exec->count == exec->cap) {
        size_t cap = exec->cap ? exec->cap * 2 : 16;
        EmValue **grown = realloc(exec->stack, cap * sizeof(*grown));
        if (!grown) {
            set_error(err, format_message("Out of memory while calling a function"));
            return -1;
        }
        exec->stack = grown;
        exec->cap = cap;
    }
    exec->stack[exec->count++] = value;
    emv_retain(value);
    return 0;
}

/* Popped values transfer one strong reference to the caller. */
static EmValue *stack_pop(Exec *exec, char **err)
{
    if (exec->count == 0) {
        set_error(err, format_message("Corrupt bytecode: the value stack ran empty"));
        return NULL;
    }
    return exec->stack[--exec->count];
}

static EmValue *stack_peek(Exec *exec, char **err)
{
    if (exec->count == 0) {
        set_error(err, format_message("Corrupt bytecode: the value stack ran empty"));
        return NULL;
    }
    return exec->stack[exec->count - 1];
}

/* Drained values transfer one strong reference each to the caller. */
static int stack_drain(Exec *exec, size_t count, EmValue ***out, char **err)
{
    EmValue **items;
    size_t i;
    size_t start;

    if (count > exec->count) {
        set_error(err, format_message("Corrupt bytecode: the value stack ran empty"));
        return -1;
    }
    items = calloc(count ? count : 1, sizeof(*items));
    if (!items) {
        set_error(err, format_message("Out of memory while calling a function"));
        return -1;
    }
    start = exec->count - count;
    for (i = 0; i < count; i++) {
        items[i] = exec->stack[start + i];
    }
    exec->count = start;
    *out = items;
    return 0;
}

static void release_values(EmValue **values, size_t count)
{
    size_t i;
    for (i = 0; i < count; i++) {
        emv_release(values[i]);
    }
    free(values);
}

/* ---------------------------------------------------------------- constants */

static const char *chunk_name(const EmChunk *chunk, uint32_t index, char **err)
{
    if (!chunk || index >= chunk->nconstants ||
        chunk->constants[index].tag != EMC_STRING ||
        !chunk->constants[index].as.string) {
        set_error(err, format_message("Corrupt bytecode: name index %u is out of range",
                                      (unsigned)index));
        return NULL;
    }
    return chunk->constants[index].as.string;
}

static int constant_value(const EmChunk *chunk, uint32_t index, EmValue **out, char **err)
{
    EmConst constant;

    if (!chunk || index >= chunk->nconstants) {
        set_error(err, format_message("Corrupt bytecode: constant index %u is out of range",
                                      (unsigned)index));
        return -1;
    }
    constant = chunk->constants[index];
    switch (constant.tag) {
    case EMC_NUMBER:
        *out = emv_number(constant.as.number);
        break;
    case EMC_STRING:
        *out = emv_string(constant.as.string ? constant.as.string : "");
        break;
    case EMC_BOOL:
        *out = emv_bool(constant.as.boolean);
        break;
    case EMC_NULL:
        *out = emv_null();
        break;
    default:
        set_error(err, format_message("Corrupt bytecode: unknown constant tag %u",
                                      (unsigned)constant.tag));
        return -1;
    }
    if (!*out) {
        set_error(err, format_message("Out of memory while loading a constant"));
        return -1;
    }
    return 0;
}

/* ---------------------------------------------------------------- utilities */

static int expect_number(EmValue *value, const char *context, double *out, char **err)
{
    if (!emv_is_number(value)) {
        set_error(err, format_message("Type mismatch: expected a number %s, found %s",
                                      context, value_type_name(value)));
        return -1;
    }
    *out = emv_as_number(value);
    return 0;
}

static size_t repeat_count(double n)
{
    if (!isfinite(n) || n <= 0.0) {
        return 0;
    }
    if (n >= (double)EM_CALL_MAX_REPEAT) {
        return EM_CALL_MAX_REPEAT;
    }
    return (size_t)n;
}

static EmValue *repeat_string(EmValue *value, double n)
{
    const char *text = emv_as_cstr(value);
    size_t len = emv_strlen(value);
    size_t times = repeat_count(n);
    size_t total;
    char *buffer;
    EmValue *result;
    size_t i;

    if (!text) {
        text = "";
        len = 0;
    }
    if (times == 0 || len == 0) {
        return emv_string("");
    }
    if (len > SIZE_MAX / times) {
        return emv_string("");
    }
    total = len * times;
    buffer = malloc(total + 1);
    if (!buffer) {
        return emv_string("");
    }
    for (i = 0; i < times; i++) {
        memcpy(buffer + i * len, text, len);
    }
    buffer[total] = '\0';
    result = emv_string_len(buffer, total);
    free(buffer);
    return result;
}

static size_t utf8_char_count(const char *text, size_t len)
{
    size_t count = 0;
    size_t i = 0;
    while (i < len) {
        unsigned char c = (unsigned char)text[i];
        size_t width = 1;
        if (c >= 0xf0) {
            width = 4;
        } else if (c >= 0xe0) {
            width = 3;
        } else if (c >= 0xc0) {
            width = 2;
        }
        i += width < (len - i) ? width : (len - i);
        count++;
    }
    return count;
}

static int utf8_char_at(const char *text, size_t len, size_t index,
                        size_t *offset, size_t *width)
{
    size_t seen = 0;
    size_t i = 0;
    while (i < len) {
        unsigned char c = (unsigned char)text[i];
        size_t step = 1;
        if (c >= 0xf0) {
            step = 4;
        } else if (c >= 0xe0) {
            step = 3;
        } else if (c >= 0xc0) {
            step = 2;
        }
        if (step > len - i) {
            step = len - i;
        }
        if (seen == index) {
            *offset = i;
            *width = step;
            return 0;
        }
        seen++;
        i += step;
    }
    return -1;
}

/* -------------------------------------------------------------- binary ops */

static int apply_binary(Exec *exec, uint8_t op, EmValue *left, EmValue *right,
                        EmValue **out, char **err)
{
    double a = 0.0;
    double b = 0.0;

    (void)exec;
    if (op == OP_EQ) {
        *out = emv_bool(emv_equal(left, right));
        return 0;
    }
    if (op == OP_NOT_EQ) {
        *out = emv_bool(!emv_equal(left, right));
        return 0;
    }
    if (op == OP_ADD) {
        if (emv_is_number(left) && emv_is_number(right)) {
            *out = emv_number(emv_as_number(left) + emv_as_number(right));
            return 0;
        }
        if (emv_is_string(left) || emv_is_string(right)) {
            char *l = emv_display(left);
            char *r = emv_display(right);
            char *joined;
            size_t llen;
            size_t rlen;
            l = l ? l : duplicate_string("");
            r = r ? r : duplicate_string("");
            if (!l || !r) {
                free(l);
                free(r);
                set_error(err, format_message("Out of memory while concatenating strings"));
                return -1;
            }
            llen = strlen(l);
            rlen = strlen(r);
            joined = malloc(llen + rlen + 1);
            if (joined) {
                memcpy(joined, l, llen);
                memcpy(joined + llen, r, rlen);
                joined[llen + rlen] = '\0';
                *out = emv_string(joined);
                free(joined);
            }
            free(l);
            free(r);
            if (!*out) {
                set_error(err, format_message("Out of memory while concatenating strings"));
                return -1;
            }
            return 0;
        }
        set_error(err, format_message("Cannot add %s and %s",
                                      value_type_name(left), value_type_name(right)));
        return -1;
    }
    if (op == OP_MUL && (emv_is_string(left) || emv_is_string(right))) {
        double n;
        EmValue *text = emv_is_string(left) ? left : right;
        EmValue *count = emv_is_string(left) ? right : left;
        if (expect_number(count, "as a repeat count", &n, err) != 0) {
            return -1;
        }
        *out = repeat_string(text, n);
        return *out ? 0 : -1;
    }

    {
        bool comparison = op == OP_GT || op == OP_LT || op == OP_GT_EQ || op == OP_LT_EQ;
        const char *left_context = comparison ? "on the left of the comparison"
                                              : "on the left of the operator";
        const char *right_context = comparison ? "on the right of the comparison"
                                               : "on the right of the operator";
        if (expect_number(left, left_context, &a, err) != 0) {
            return -1;
        }
        if (expect_number(right, right_context, &b, err) != 0) {
            return -1;
        }
    }

    switch (op) {
    case OP_SUB:
        *out = emv_number(a - b);
        break;
    case OP_MUL:
        *out = emv_number(a * b);
        break;
    case OP_DIV:
        if (b == 0.0) {
            set_error(err, format_message("Division by zero"));
            return -1;
        }
        *out = emv_number(a / b);
        break;
    case OP_MOD:
        if (b == 0.0) {
            set_error(err, format_message("Division by zero"));
            return -1;
        }
        *out = emv_number(fmod(a, b));
        break;
    case OP_FLOOR_DIV:
        if (b == 0.0) {
            set_error(err, format_message("Division by zero"));
            return -1;
        }
        *out = emv_number(floor(a / b));
        break;
    case OP_POW:
        *out = emv_number(pow(a, b));
        break;
    case OP_BIT_AND:
        *out = emv_number((double)((int64_t)a & (int64_t)b));
        break;
    case OP_BIT_OR:
        *out = emv_number((double)((int64_t)a | (int64_t)b));
        break;
    case OP_BIT_XOR:
        *out = emv_number((double)((int64_t)a ^ (int64_t)b));
        break;
    case OP_SHL:
        *out = emv_number((double)((int64_t)a << (uint32_t)(int64_t)b));
        break;
    case OP_SHR:
        *out = emv_number((double)((int64_t)a >> (uint32_t)(int64_t)b));
        break;
    case OP_GT:
        *out = emv_bool(a > b);
        break;
    case OP_LT:
        *out = emv_bool(a < b);
        break;
    case OP_GT_EQ:
        *out = emv_bool(a >= b);
        break;
    case OP_LT_EQ:
        *out = emv_bool(a <= b);
        break;
    default:
        set_error(err, format_message("Corrupt bytecode: unknown binary operator %u",
                                      (unsigned)op));
        return -1;
    }
    return *out ? 0 : -1;
}

/* -------------------------------------------------------- property / index */

static EmValue *get_property(EmValue *base, const char *name, char **err)
{
    EmValue *value;
    if (!emv_is_object(base)) {
        set_error(err, format_message("Cannot access property '%s' on a %s",
                                      name, value_type_name(base)));
        return NULL;
    }
    value = emv_object_get(base, name);
    if (!value) {
        set_error(err, format_message("Unknown property '%s'", name));
        return NULL;
    }
    return value;
}

static int set_property(EmValue *base, const char *name, EmValue *value, char **err)
{
    if (!emv_is_object(base)) {
        set_error(err, format_message("Cannot set property '%s' on a %s",
                                      name, value_type_name(base)));
        return -1;
    }
    emv_object_set(base, name, value);
    return 0;
}

static int get_index(EmValue *base, EmValue *index, EmValue **out, char **err)
{
    double n = 0.0;
    int64_t at;

    if (emv_is_array(base)) {
        if (expect_number(index, "as an array index", &n, err) != 0) {
            return -1;
        }
        at = (int64_t)n;
        if (at < 0 || (uint64_t)at >= emv_array_len(base)) {
            set_error(err, format_message("Array index %lld is out of bounds (length %zu)",
                                          (long long)at, emv_array_len(base)));
            return -1;
        }
        *out = emv_array_get(base, (size_t)at);
        return 0;
    }
    if (emv_is_string(base)) {
        const char *text = emv_as_cstr(base);
        size_t len = emv_strlen(base);
        size_t chars;
        size_t offset = 0;
        size_t width = 0;
        if (expect_number(index, "as a string index", &n, err) != 0) {
            return -1;
        }
        chars = utf8_char_count(text ? text : "", len);
        at = (int64_t)n;
        if (at < 0 || (uint64_t)at >= chars ||
            utf8_char_at(text ? text : "", len, (size_t)at, &offset, &width) != 0) {
            set_error(err, format_message("String index %lld is out of bounds (length %zu)",
                                          (long long)at, chars));
            return -1;
        }
        *out = emv_string_len(text + offset, width);
        return 0;
    }
    set_error(err, format_message("Cannot index into a %s", value_type_name(base)));
    return -1;
}

static int set_index(EmValue *base, EmValue *index, EmValue *value, char **err)
{
    double n = 0.0;
    int64_t at;

    if (!emv_is_array(base)) {
        set_error(err, format_message("Cannot index into a %s", value_type_name(base)));
        return -1;
    }
    if (expect_number(index, "as an array index", &n, err) != 0) {
        return -1;
    }
    at = (int64_t)n;
    if (at < 0 || (uint64_t)at >= emv_array_len(base)) {
        set_error(err, format_message("Array index %lld is out of bounds (length %zu)",
                                      (long long)at, emv_array_len(base)));
        return -1;
    }
    emv_array_set(base, (size_t)at, value);
    return 0;
}

/* -------------------------------------------------------------------- calls */

static size_t native_fixed_arity(const char *name)
{
    static const struct {
        const char *name;
        size_t arity;
    } table[] = {
        {"args", 0},      {"env", 2},        {"readFile", 1},  {"writeFile", 2},
        {"readBytes", 1}, {"writeBytes", 2}, {"stdout", 1},    {"stderr", 1},
        {"exit", 1},      {"cwd", 0},        {"tempDir", 0},   {"pid", 0}
    };
    const char *dot;
    size_t i;

    if (!name) {
        return 0;
    }
    dot = strrchr(name, '.');
    if (dot) {
        name = dot + 1;
    }
    for (i = 0; i < sizeof(table) / sizeof(table[0]); i++) {
        if (strcmp(table[i].name, name) == 0) {
            return table[i].arity;
        }
    }
    return 0;
}

static int pad_arguments(EmValue **args, size_t argc, size_t target,
                         EmValue ***out, bool *allocated, char **err)
{
    EmValue **padded;
    size_t i;

    *allocated = false;
    if (argc >= target) {
        *out = args;
        return 0;
    }
    padded = calloc(target ? target : 1, sizeof(*padded));
    if (!padded) {
        set_error(err, format_message("Out of memory while calling a function"));
        return -1;
    }
    for (i = 0; i < argc; i++) {
        padded[i] = args[i];
    }
    for (; i < target; i++) {
        padded[i] = emv_null();
        if (!padded[i]) {
            release_values(padded, i);
            set_error(err, format_message("Out of memory while padding call arguments"));
            return -1;
        }
    }
    *out = padded;
    *allocated = true;
    return 0;
}

static int call_named(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                      const char *name, size_t fixed_arity, int depth,
                      EmValue **out, char **err);

/* Run one compiled chunk to its Return (or to the end of its code). */
static int exec_chunk(EmVm *vm, const EmChunk *chunk, EmEnv *env, int depth,
                      EmValue **out, char **err)
{
    Exec exec;
    int rc = -1;

    memset(&exec, 0, sizeof(exec));
    exec.vm = vm;
    exec.chunk = chunk;
    exec.env = env;
    exec.depth = depth;

    while (exec.ip < chunk->ncode) {
        const EmInstr *ins = &chunk->code[exec.ip++];
        EmValue *value = NULL;
        EmValue *left = NULL;
        EmValue *right = NULL;
        EmValue **items = NULL;
        size_t item_count = 0;
        const char *name;
        double number = 0.0;

        switch (ins->tag) {
        case OP_LOAD_CONST:
            if (constant_value(chunk, ins->a, &value, err) != 0) {
                goto done;
            }
            if (stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_POP:
            value = stack_pop(&exec, err);
            if (!value) {
                goto done;
            }
            emv_release(value);
            break;

        case OP_GET_VAR:
            name = chunk_name(chunk, ins->a, err);
            if (!name) {
                goto done;
            }
            value = em_call_env_get(env, name);
            if (!value) {
                set_error(err, format_message("Unknown variable '%s'", name));
                goto done;
            }
            if (stack_push(&exec, value, err) != 0) {
                goto done;
            }
            break;

        case OP_SET_VAR:
        case OP_DEFINE_VAR:
            name = chunk_name(chunk, ins->a, err);
            if (!name) {
                goto done;
            }
            value = stack_pop(&exec, err);
            if (!value) {
                goto done;
            }
            if (ins->tag == OP_DEFINE_VAR) {
                em_call_env_define(env, name, value);
            } else {
                em_call_env_assign(env, name, value);
            }
            emv_release(value);
            break;

        case OP_GET_PROP:
            right = stack_pop(&exec, err); /* base */
            if (!right) {
                goto done;
            }
            name = chunk_name(chunk, ins->a, err);
            if (!name) {
                emv_release(right);
                goto done;
            }
            value = get_property(right, name, err);
            if (!value) {
                emv_release(right);
                goto done;
            }
            if (stack_push(&exec, value, err) != 0) {
                emv_release(right);
                goto done;
            }
            emv_release(right);
            break;

        case OP_SET_PROP:
            right = stack_pop(&exec, err); /* base */
            left = stack_pop(&exec, err);  /* value */
            if (!right || !left) {
                emv_release(right);
                emv_release(left);
                goto done;
            }
            name = chunk_name(chunk, ins->a, err);
            if (!name || set_property(right, name, left, err) != 0) {
                emv_release(right);
                emv_release(left);
                goto done;
            }
            emv_release(right);
            emv_release(left);
            break;

        case OP_GET_VAR_OR_OBJECT:
            name = chunk_name(chunk, ins->a, err);
            if (!name) {
                goto done;
            }
            if (!em_call_env_has(env, name)) {
                value = emv_object();
                em_call_env_define(env, name, value);
                emv_release(value);
            }
            value = em_call_env_get(env, name);
            if (!value || stack_push(&exec, value, err) != 0) {
                goto done;
            }
            break;

        case OP_GET_PROP_OR_OBJECT:
            right = stack_pop(&exec, err);
            if (!right) {
                goto done;
            }
            name = chunk_name(chunk, ins->a, err);
            if (!name) {
                emv_release(right);
                goto done;
            }
            if (emv_is_object(right)) {
                if (!emv_object_get(right, name)) {
                    value = emv_object();
                    emv_object_set(right, name, value);
                    emv_release(value);
                }
                value = emv_object_get(right, name);
            } else {
                value = get_property(right, name, err);
            }
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(right);
                goto done;
            }
            emv_release(right);
            break;

        case OP_GET_PROP_KEEP:
            right = stack_peek(&exec, err);
            if (!right) {
                goto done;
            }
            name = chunk_name(chunk, ins->a, err);
            if (!name) {
                goto done;
            }
            value = get_property(right, name, err);
            if (!value || stack_push(&exec, value, err) != 0) {
                goto done;
            }
            break;

        case OP_GET_INDEX:
            right = stack_pop(&exec, err); /* index */
            left = stack_pop(&exec, err);  /* base */
            if (!right || !left) {
                emv_release(right);
                emv_release(left);
                goto done;
            }
            if (get_index(left, right, &value, err) != 0 ||
                stack_push(&exec, value, err) != 0) {
                emv_release(right);
                emv_release(left);
                goto done;
            }
            emv_release(right);
            emv_release(left);
            break;

        case OP_GET_INDEX_KEEP:
            if (exec.count < 2) {
                set_error(err, format_message("Corrupt bytecode: the value stack ran empty"));
                goto done;
            }
            left = exec.stack[exec.count - 2];
            right = exec.stack[exec.count - 1];
            if (get_index(left, right, &value, err) != 0 ||
                stack_push(&exec, value, err) != 0) {
                goto done;
            }
            break;

        case OP_SET_INDEX:
            right = stack_pop(&exec, err); /* index */
            left = stack_pop(&exec, err);  /* base */
            value = stack_pop(&exec, err); /* value */
            if (!right || !left || !value) {
                emv_release(right);
                emv_release(left);
                emv_release(value);
                goto done;
            }
            if (set_index(left, right, value, err) != 0) {
                emv_release(right);
                emv_release(left);
                emv_release(value);
                goto done;
            }
            emv_release(right);
            emv_release(left);
            emv_release(value);
            break;

        case OP_MAKE_ARRAY:
            if (stack_drain(&exec, ins->a, &items, err) != 0) {
                goto done;
            }
            item_count = ins->a;
            value = emv_array();
            for (size_t i = 0; i < item_count; i++) {
                emv_array_push(value, items[i]);
            }
            release_values(items, item_count);
            items = NULL;
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_MAKE_OBJECT:
            if (stack_drain(&exec, (size_t)ins->a * 2, &items, err) != 0) {
                goto done;
            }
            item_count = (size_t)ins->a * 2;
            value = emv_object();
            for (size_t i = 0; i + 1 < item_count; i += 2) {
                char *key = emv_display(items[i]);
                emv_object_set(value, key ? key : "", items[i + 1]);
                free(key);
            }
            release_values(items, item_count);
            items = NULL;
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_MAKE_CLOSURE:
            if (!chunk->functions || ins->a >= chunk->nfunctions) {
                set_error(err, format_message("Corrupt bytecode: function index %u is out of range",
                                              (unsigned)ins->a));
                goto done;
            }
            value = em_vm_call_make_closure(&chunk->functions[ins->a], env);
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_CALL:
            name = chunk_name(chunk, ins->b, err);
            if (!name || stack_drain(&exec, ins->a, &items, err) != 0) {
                goto done;
            }
            item_count = ins->a;
            value = stack_pop(&exec, err);
            if (!value) {
                release_values(items, item_count);
                goto done;
            }
            {
                EmValue *result = NULL;
                int call_rc = call_named(vm, value, items, item_count, name, 0,
                                         depth + 1, &result, err);
                emv_release(value);
                release_values(items, item_count);
                items = NULL;
                if (call_rc != 0) {
                    emv_release(result);
                    goto done;
                }
                if (stack_push(&exec, result, err) != 0) {
                    emv_release(result);
                    goto done;
                }
                emv_release(result);
            }
            break;

        case OP_METHOD_CALL:
            name = chunk_name(chunk, ins->a, err);
            if (!name || stack_drain(&exec, ins->b, &items, err) != 0) {
                goto done;
            }
            item_count = ins->b;
            value = stack_pop(&exec, err); /* receiver */
            if (!value) {
                release_values(items, item_count);
                goto done;
            }
            {
                EmValue *result = NULL;
                int call_rc = em_vm_call_method(vm, value, name, items, item_count,
                                                &result, err);
                emv_release(value);
                release_values(items, item_count);
                items = NULL;
                if (call_rc != 0) {
                    emv_release(result);
                    goto done;
                }
                if (!result) {
                    result = emv_null();
                }
                if (stack_push(&exec, result, err) != 0) {
                    emv_release(result);
                    goto done;
                }
                emv_release(result);
            }
            break;

        case OP_SAY:
            if (stack_drain(&exec, ins->a, &items, err) != 0) {
                goto done;
            }
            item_count = ins->a;
            {
                char *line = duplicate_string("");
                size_t i;
                if (!line) {
                    release_values(items, item_count);
                    set_error(err, format_message("Out of memory while printing"));
                    goto done;
                }
                for (i = 0; i < item_count; i++) {
                    char *part = emv_display(items[i]);
                    size_t old_len = strlen(line);
                    size_t part_len = part ? strlen(part) : 0;
                    char *grown = realloc(line, old_len + part_len + 2);
                    if (!grown) {
                        free(part);
                        free(line);
                        release_values(items, item_count);
                        set_error(err, format_message("Out of memory while printing"));
                        goto done;
                    }
                    line = grown;
                    if (i > 0) {
                        line[old_len++] = ' ';
                    }
                    if (part_len) {
                        memcpy(line + old_len, part, part_len);
                        old_len += part_len;
                    }
                    line[old_len] = '\0';
                    free(part);
                }
                fputs(line, stdout);
                fputc('\n', stdout);
                fflush(stdout);
                free(line);
            }
            release_values(items, item_count);
            items = NULL;
            value = emv_null();
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

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
        case OP_LT_EQ:
            right = stack_pop(&exec, err);
            left = stack_pop(&exec, err);
            if (!right || !left) {
                emv_release(right);
                emv_release(left);
                goto done;
            }
            if (apply_binary(&exec, ins->tag, left, right, &value, err) != 0 ||
                stack_push(&exec, value, err) != 0) {
                emv_release(right);
                emv_release(left);
                goto done;
            }
            emv_release(right);
            emv_release(left);
            break;

        case OP_NEG:
            right = stack_pop(&exec, err);
            if (!right || expect_number(right, "for unary '-'", &number, err) != 0) {
                emv_release(right);
                goto done;
            }
            emv_release(right);
            value = emv_number(-number);
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_NOT:
            right = stack_pop(&exec, err);
            if (!right) {
                goto done;
            }
            value = emv_bool(!emv_truthy(right));
            emv_release(right);
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_BIT_NOT:
            right = stack_pop(&exec, err);
            if (!right || expect_number(right, "for bitwise '~'", &number, err) != 0) {
                emv_release(right);
                goto done;
            }
            emv_release(right);
            value = emv_number((double)(~(int64_t)number));
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_TO_BOOL:
            right = stack_pop(&exec, err);
            if (!right) {
                goto done;
            }
            value = emv_bool(emv_truthy(right));
            emv_release(right);
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_DUP:
            right = stack_peek(&exec, err);
            if (!right || stack_push(&exec, right, err) != 0) {
                goto done;
            }
            break;

        case OP_ROTATE:
            {
                size_t count = (size_t)ins->a + 1;
                size_t split;
                if (exec.count < count) {
                    set_error(err, format_message("Corrupt bytecode: the value stack ran empty"));
                    goto done;
                }
                split = exec.count - count;
                value = stack_pop(&exec, err);
                if (!value) {
                    goto done;
                }
                memmove(exec.stack + split + 1, exec.stack + split,
                        (count - 1) * sizeof(*exec.stack));
                exec.stack[split] = value;
            }
            break;

        case OP_JUMP:
            exec.ip = ins->a;
            break;

        case OP_JUMP_IF_FALSE:
            right = stack_pop(&exec, err);
            if (!right) {
                goto done;
            }
            if (!emv_truthy(right)) {
                exec.ip = ins->a;
            }
            emv_release(right);
            break;

        case OP_JUMP_IF_FALSE_PEEK:
        case OP_JUMP_IF_TRUE_PEEK:
        case OP_JUMP_IF_NULL_PEEK:
            right = stack_peek(&exec, err);
            if (!right) {
                goto done;
            }
            if ((ins->tag == OP_JUMP_IF_FALSE_PEEK && !emv_truthy(right)) ||
                (ins->tag == OP_JUMP_IF_TRUE_PEEK && emv_truthy(right)) ||
                (ins->tag == OP_JUMP_IF_NULL_PEEK && emv_is_null(right))) {
                exec.ip = ins->a;
            }
            break;

        case OP_REPEAT_SETUP:
            right = stack_pop(&exec, err);
            if (!right || expect_number(right, "as a repeat count", &number, err) != 0) {
                emv_release(right);
                goto done;
            }
            emv_release(right);
            if (number < 0.0) {
                set_error(err, format_message("repeat count must not be negative"));
                goto done;
            }
            value = emv_number((double)((int64_t)number));
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_REPEAT_NEXT:
            right = stack_peek(&exec, err);
            if (!right || expect_number(right, "as a repeat count", &number, err) != 0) {
                goto done;
            }
            if (number <= 0.0) {
                EmValue *spent = stack_pop(&exec, err);
                if (!spent) {
                    goto done;
                }
                emv_release(spent);
                exec.ip = ins->a;
            } else {
                value = emv_number(number - 1.0);
                emv_release(exec.stack[exec.count - 1]);
                exec.stack[exec.count - 1] = value;
            }
            break;

        case OP_ITER_SETUP:
            right = stack_pop(&exec, err);
            if (!right) {
                goto done;
            }
            if (!emv_is_array(right)) {
                set_error(err, format_message("Cannot iterate over a %s",
                                              value_type_name(right)));
                emv_release(right);
                goto done;
            }
            value = emv_array();
            for (size_t i = 0; i < emv_array_len(right); i++) {
                emv_array_push(value, emv_array_get(right, i));
            }
            emv_release(right);
            right = NULL;
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            value = emv_number(0.0);
            if (!value || stack_push(&exec, value, err) != 0) {
                emv_release(value);
                goto done;
            }
            emv_release(value);
            break;

        case OP_ITER_NEXT:
            if (exec.count < 2) {
                set_error(err, format_message("Corrupt bytecode: the value stack ran empty"));
                goto done;
            }
            right = exec.stack[exec.count - 1]; /* cursor */
            left = exec.stack[exec.count - 2];  /* snapshot */
            if (!emv_is_number(right) || !emv_is_array(left)) {
                set_error(err, format_message("Corrupt bytecode: unexpected value on the stack"));
                goto done;
            }
            name = chunk_name(chunk, ins->a, err);
            if (!name) {
                goto done;
            }
            number = emv_as_number(right);
            if (number >= 0.0 && (uint64_t)number < emv_array_len(left)) {
                em_call_env_define(env, name, emv_array_get(left, (size_t)number));
                value = emv_number(number + 1.0);
                emv_release(exec.stack[exec.count - 1]);
                exec.stack[exec.count - 1] = value;
            } else {
                EmValue *cursor = stack_pop(&exec, err);
                EmValue *snapshot = stack_pop(&exec, err);
                if (!cursor || !snapshot) {
                    emv_release(cursor);
                    emv_release(snapshot);
                    goto done;
                }
                emv_release(cursor);
                emv_release(snapshot);
                exec.ip = ins->b;
            }
            break;

        case OP_RETURN:
            exec.result = stack_pop(&exec, err);
            if (!exec.result) {
                goto done;
            }
            rc = 0;
            goto done;

        case OP_RETURN_NULL:
            exec.result = emv_null();
            if (!exec.result) {
                set_error(err, format_message("Out of memory while returning"));
                goto done;
            }
            rc = 0;
            goto done;

        case OP_IMPORT:
            name = chunk_name(chunk, ins->a, err);
            if (!name) {
                goto done;
            }
            value = em_vm_lookup_import(vm, name);
            if (!value) {
                set_error(err, format_message("Unknown module '%s'", name));
                goto done;
            }
            em_call_env_define(env, name, value);
            break;

        default:
            set_error(err, format_message("Corrupt bytecode: unknown instruction %u",
                                          (unsigned)ins->tag));
            goto done;
        }
    }

    /* Falling off the end of a chunk returns null, as in the Rust VM. */
    if (!exec.result) {
        exec.result = emv_null();
    }
    rc = exec.result ? 0 : -1;

done:
    release_values(exec.stack, exec.count);
    if (rc != 0) {
        emv_release(exec.result);
        exec.result = NULL;
    }
    if (out) {
        *out = exec.result; /* ownership transfers to the caller */
    } else {
        emv_release(exec.result);
    }
    return rc;
}

static int call_named(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                      const char *name, size_t fixed_arity, int depth,
                      EmValue **out, char **err)
{
    EmValue **padded = NULL;
    bool padded_allocated = false;
    EmValue *result = NULL;
    size_t target = 0;
    int rc = -1;

    if (out) {
        *out = NULL;
    }
    if (!vm) {
        set_error(err, format_message("Cannot call a function without a VM"));
        return -1;
    }
    if (argc && !args) {
        set_error(err, format_message("Corrupt call: %zu arguments without an argument array",
                                      argc));
        return -1;
    }
    if (!callee) {
        set_error(err, format_message("'%s' is not callable (found null)",
                                      name ? name : "callee"));
        return -1;
    }

    if (emv_is_native(callee)) {
        EmNativeFn fn = emv_native_fn(callee);
        size_t target = fixed_arity;
        const char *native_name = emv_native_name(callee);
        const char *hint = name ? name : (native_name ? native_name : "native");
        if (!fn) {
            set_error(err, format_message("'%s' is not callable (found function)", hint));
            return -1;
        }
        if (target == 0) {
            target = native_fixed_arity(native_name);
        }
        if (pad_arguments(args, argc, target, &padded, &padded_allocated, err) != 0) {
            goto done;
        }
        {
            char *native_err = NULL;
            result = fn(vm, padded, argc < target ? target : argc, &native_err);
            if (native_err) {
                set_error(err, native_err);
                emv_release(result);
                result = NULL;
                goto done;
            }
        }
        if (!result) {
            result = emv_null();
        }
        if (!result) {
            set_error(err, format_message("Out of memory while calling '%s'", hint));
            goto done;
        }
        rc = 0;
        goto done;
    }

    if (emv_is_closure(callee)) {
        EmFunctionProto *proto = emv_closure_proto(callee);
        EmEnv *captured;
        EmEnv *call_env;
        size_t nparams;
        size_t target;
        size_t i;

        if (!proto || !proto->chunk) {
            set_error(err, format_message("Cannot call a function without a body"));
            goto done;
        }
        nparams = proto->nparams;
        if (argc > nparams) {
            char *params = duplicate_string("");
            char *joined = NULL;
            for (i = 0; i < nparams; i++) {
                const char *param = proto->params[i];
                size_t old_len = params ? strlen(params) : 0;
                size_t add_len = param ? strlen(param) : 1;
                char *grown = realloc(params, old_len + add_len + 2);
                if (!grown) {
                    break;
                }
                params = grown;
                if (i > 0) {
                    params[old_len++] = ',';
                    params[old_len++] = ' ';
                } else if (old_len > 0) {
                    params[old_len++] = ' ';
                }
                if (param) {
                    memcpy(params + old_len, param, add_len);
                } else {
                    params[old_len] = '?';
                }
                params[old_len + add_len] = '\0';
            }
            {
                const char *hint = nparams == 0
                                       ? "This function takes no arguments."
                                       : nparams == 1
                                           ? "Pass one argument."
                                           : "Pass the arguments.";
                joined = format_message(
                    "Function '%s(%s)' expects %zu argument(s) but got %zu. Hint: %s",
                    proto->name ? proto->name : "?", params ? params : "",
                    nparams, argc, hint);
            }
            free(params);
            set_error(err, joined);
            goto done;
        }
        if (depth >= EM_CALL_MAX_DEPTH) {
            set_error(err, format_message("Too many nested calls while calling '%s'",
                                          proto->name ? proto->name : "?"));
            goto done;
        }
        target = nparams > fixed_arity ? nparams : fixed_arity;
        if (pad_arguments(args, argc, target, &padded, &padded_allocated, err) != 0) {
            goto done;
        }
        captured = emv_closure_env(callee);
        if (!captured) {
            captured = em_vm_global_env(vm);
        }
        call_env = em_call_env_new(captured);
        if (!call_env) {
            set_error(err, format_message("Out of memory while calling '%s'",
                                          proto->name ? proto->name : "?"));
            goto done;
        }
        for (i = 0; i < nparams; i++) {
            em_call_env_define(call_env,
                               proto->params[i] ? proto->params[i] : "?", padded[i]);
        }
        rc = exec_chunk(vm, proto->chunk, call_env, depth, &result, err);
        em_call_env_release(call_env);
        goto done;
    }

    set_error(err, format_message("'%s' is not callable (found %s)",
                                  name ? name : "callee", value_type_name(callee)));
    return -1;

done:
    if (padded_allocated) {
        release_values(padded, argc < target ? target : argc);
    }
    if (rc == 0 && out) {
        *out = result; /* ownership transfers to the caller */
        result = NULL;
    }
    emv_release(result);
    return rc;
}

/* ------------------------------------------------------------- public API */

int em_vm_call_value_named(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                           const char *name, EmValue **out, char **err)
{
    if (err) {
        *err = NULL;
    }
    return call_named(vm, callee, args, argc, name, 0, 0, out, err);
}

int em_vm_call_value_fixed(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                           size_t fixed_arity, EmValue **out, char **err)
{
    const char *hint = NULL;
    if (callee && emv_is_native(callee)) {
        hint = emv_native_name(callee);
    }
    if (err) {
        *err = NULL;
    }
    return call_named(vm, callee, args, argc, hint, fixed_arity, 0, out, err);
}

int em_vm_call_value(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                     EmValue **out, char **err)
{
    const char *hint = NULL;
    if (callee && emv_is_native(callee)) {
        hint = emv_native_name(callee);
    }
    return em_vm_call_value_named(vm, callee, args, argc, hint, out, err);
}

EmValue *em_vm_call_make_closure(EmFunctionProto *proto, EmEnv *env)
{
    if (!proto) {
        return NULL;
    }
    em_call_env_retain(env);
    return emv_closure(proto, env);
}

int em_vm_call_return(EmVm *vm, EmValue *value, EmValue **out, char **err)
{
    (void)vm;
    if (err) {
        *err = NULL;
    }
    if (!out) {
        set_error(err, format_message("Return needs a result slot"));
        return -1;
    }
    if (value) {
        *out = value;
        emv_retain(*out);
    } else {
        *out = emv_null();
    }
    return 0;
}

int em_vm_call_return_null(EmVm *vm, EmValue **out, char **err)
{
    return em_vm_call_return(vm, NULL, out, err);
}

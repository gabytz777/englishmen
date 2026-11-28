/*
 * Global builtins for the C bootstrap VM.
 *
 * This file is independent of the Rust crate: every function below mirrors the
 * behaviour (including the error text) of src/builtins.rs and src/runtime.rs,
 * which is the oracle used by the self-hosted compiler. JSON support is
 * implemented here rather than delegated through em_vm_lookup_import(): the
 * bootstrap VM promises only the "host" module, so relying on an import table
 * entry named "json" would make jsonEncode/jsonDecode order-dependent. The
 * one exception is `args`, which is delegated to the host module because only
 * the host program knows the command line (see em_builtin_args).
 */

#define _POSIX_C_SOURCE 200809L

#include "builtins.h"

/* bootstrap-c/vm.h pulls in emb.h, whose anonymous EmFunctionProto typedef
 * conflicts with the tagged forward declaration in the parent-owned value.h.
 * Only three VM entry points are needed here; declaring them locally keeps
 * this translation unit buildable without touching headers owned by parents.
 * These prototypes exactly match bootstrap-c/vm.h. */
int em_vm_register_native(EmVm *vm, const char *name, EmNativeFn fn);
EmValue *em_vm_lookup_import(EmVm *vm, const char *name);
int em_vm_call_value(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                     EmValue **out, char **err);

#include <dirent.h>
#include <errno.h>
#include <math.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

/* ------------------------------------------------------------------ */
/* Small utilities                                                     */
/* ------------------------------------------------------------------ */

static void *xmalloc(size_t n)
{
    void *p = malloc(n ? n : 1);
    if (!p) {
        fputs("builtins: out of memory\n", stderr);
        exit(1);
    }
    return p;
}

static char *dup_cstr(const char *s)
{
    size_t n = strlen(s) + 1;
    char *out = xmalloc(n);
    memcpy(out, s, n);
    return out;
}

static char *dup_range(const char *s, size_t n)
{
    char *out = xmalloc(n + 1);
    memcpy(out, s, n);
    out[n] = '\0';
    return out;
}

static char *em_fmt(const char *format, ...)
{
    va_list ap;
    va_start(ap, format);
    va_list copy;
    va_copy(copy, ap);
    int needed = vsnprintf(NULL, 0, format, copy);
    va_end(copy);
    if (needed < 0) {
        va_end(ap);
        return dup_cstr("");
    }
    char *out = xmalloc((size_t)needed + 1);
    vsnprintf(out, (size_t)needed + 1, format, ap);
    va_end(ap);
    return out;
}

static EmValue *fail(char **err, const char *format, ...)
{
    va_list ap;
    va_start(ap, format);
    va_list copy;
    va_copy(copy, ap);
    int needed = vsnprintf(NULL, 0, format, copy);
    va_end(copy);
    if (needed >= 0 && err) {
        char *out = xmalloc((size_t)needed + 1);
        vsnprintf(out, (size_t)needed + 1, format, ap);
        *err = out;
    }
    va_end(ap);
    return NULL;
}

static const char *type_name(EmValueType type)
{
    switch (type) {
    case EMV_NUMBER: return "number";
    case EMV_STRING: return "string";
    case EMV_BOOL: return "boolean";
    case EMV_NULL: return "null";
    case EMV_ARRAY: return "array";
    case EMV_OBJECT: return "object";
    case EMV_CLOSURE:
    case EMV_NATIVE: return "function";
    }
    return "null";
}

static const char *value_type_name(const EmValue *v)
{
    return v ? type_name(emv_type(v)) : "null";
}

static char *value_display(EmValue *v)
{
    char *text = v ? emv_display(v) : NULL;
    return text ? text : dup_cstr("null");
}

/* ------------------------------------------------------------------ */
/* Argument validation. Messages match src/builtins.rs exactly.         */
/* ------------------------------------------------------------------ */

static bool exact_args(size_t argc, size_t expected, const char *name, char **err)
{
    if (argc == expected)
        return true;
    fail(err, "%s expects %zu argument%s, found %zu", name,
         expected, expected == 1 ? "" : "s", argc);
    return false;
}

static bool at_least_args(size_t argc, size_t expected, const char *name, char **err)
{
    if (argc >= expected)
        return true;
    fail(err, "%s expects at least %zu argument%s, found %zu", name,
         expected, expected == 1 ? "" : "s", argc);
    return false;
}

static bool no_extra_args(size_t argc, size_t limit, const char *name, char **err)
{
    if (argc <= limit)
        return true;
    fail(err, "%s expects at most %zu argument%s, found %zu", name,
         limit, limit == 1 ? "" : "s", argc);
    return false;
}

static bool expect_number(EmValue *v, const char *ctx, double *out, char **err)
{
    if (v && emv_is_number(v)) {
        *out = emv_as_number(v);
        return true;
    }
    fail(err, "Type mismatch: expected a number %s, found %s", ctx, value_type_name(v));
    return false;
}

static bool expect_string(EmValue *v, const char *ctx, const char **out,
                          size_t *len, char **err)
{
    if (v && emv_is_string(v)) {
        *out = emv_as_cstr(v);
        *len = emv_strlen(v);
        return true;
    }
    fail(err, "Type mismatch: expected a string %s, found %s", ctx, value_type_name(v));
    return false;
}

static bool expect_array(EmValue *v, const char *ctx, EmValue **out, char **err)
{
    if (v && emv_is_array(v)) {
        *out = v;
        return true;
    }
    fail(err, "Type mismatch: expected an array %s, found %s", ctx, value_type_name(v));
    return false;
}

static bool expect_count(EmValue *v, const char *name, size_t *out, char **err)
{
    double n;
    char *ctx = em_fmt("as the count for %s", name);
    bool ok = expect_number(v, ctx, &n, err);
    free(ctx);
    if (!ok)
        return false;
    if (!isfinite(n) || trunc(n) != n || n < 0.0) {
        fail(err, "%s expects a non-negative integer count", name);
        return false;
    }
    if (n >= (double)SIZE_MAX) {
        fail(err, "%s count is too large", name);
        return false;
    }
    *out = (size_t)n;
    return true;
}

static bool slice_index(double raw, size_t len, size_t *out, char **err)
{
    if (!isfinite(raw) || trunc(raw) != raw) {
        fail(err, "slice expects integer slice indexes");
        return false;
    }
    double index = raw < 0.0 ? (double)len + raw : raw;
    if (index < 0.0)
        index = 0.0;
    if (index > (double)len)
        index = (double)len;
    *out = (size_t)index;
    return true;
}

/* ------------------------------------------------------------------ */
/* Number formatting                                                    */
/* ------------------------------------------------------------------ */

/* Rust's f64 Display and format_number_json use the shortest round-tripping
 * decimal without exponent notation. */
static char *format_double(double n)
{
    if (isfinite(n) && trunc(n) == n && fabs(n) < 1e15)
        return em_fmt("%lld", (long long)n);

    char shortest[64];
    int precision;
    for (precision = 1; precision <= 17; precision++) {
        snprintf(shortest, sizeof(shortest), "%.*g", precision, n);
        if (strtod(shortest, NULL) == n)
            break;
    }
    if (precision > 17)
        snprintf(shortest, sizeof(shortest), "%.17g", n);

    if (!strchr(shortest, 'e') && !strchr(shortest, 'E'))
        return dup_cstr(shortest);

    /* Expand scientific notation into the plain decimal form Display uses. */
    char mantissa[64];
    char *e = strchr(shortest, 'e');
    if (!e)
        e = strchr(shortest, 'E');
    size_t mlen = (size_t)(e - shortest);
    memcpy(mantissa, shortest, mlen);
    mantissa[mlen] = '\0';
    int exponent = atoi(e + 1);

    bool negative = mantissa[0] == '-';
    const char *digits = mantissa + (negative ? 1 : 0);
    char digits_only[64];
    size_t nd = 0;
    int decimal_pos = 0;
    bool seen_dot = false;
    for (size_t i = 0; digits[i]; i++) {
        if (digits[i] == '.') {
            seen_dot = true;
            continue;
        }
        digits_only[nd++] = digits[i];
        if (!seen_dot)
            decimal_pos++;
    }
    decimal_pos += exponent;

    char out[800];
    size_t o = 0;
    if (negative)
        out[o++] = '-';
    if (decimal_pos <= 0) {
        out[o++] = '0';
        out[o++] = '.';
        for (int z = 0; z < -decimal_pos; z++)
            out[o++] = '0';
        memcpy(out + o, digits_only, nd);
        o += nd;
    } else if (decimal_pos >= (int)nd) {
        memcpy(out + o, digits_only, nd);
        o += nd;
        for (int z = 0; z < decimal_pos - (int)nd; z++)
            out[o++] = '0';
    } else {
        memcpy(out + o, digits_only, (size_t)decimal_pos);
        o += (size_t)decimal_pos;
        out[o++] = '.';
        memcpy(out + o, digits_only + decimal_pos, nd - (size_t)decimal_pos);
        o += nd - (size_t)decimal_pos;
    }
    out[o] = '\0';
    return dup_cstr(out);
}

/* ------------------------------------------------------------------ */
/* UTF-8 helpers. Builtin strings count and slice Unicode characters,   */
/* matching Rust's chars().                                             */
/* ------------------------------------------------------------------ */

static uint32_t utf8_decode(const char *s, size_t len, size_t *pos)
{
    const unsigned char *bytes = (const unsigned char *)s;
    size_t at = *pos;
    unsigned char first = bytes[at];
    if (first < 0x80) {
        *pos = at + 1;
        return first;
    }
    size_t need;
    uint32_t cp;
    uint32_t minimum;
    if (first >= 0xC2 && first <= 0xDF) {
        need = 2; cp = first & 0x1Fu; minimum = 0x80;
    } else if (first >= 0xE0 && first <= 0xEF) {
        need = 3; cp = first & 0x0Fu; minimum = 0x800;
    } else if (first >= 0xF0 && first <= 0xF4) {
        need = 4; cp = first & 0x07u; minimum = 0x10000;
    } else {
        *pos = at + 1;
        return first;
    }
    if (at + need > len) {
        *pos = at + 1;
        return first;
    }
    for (size_t i = 1; i < need; i++) {
        unsigned char next = bytes[at + i];
        if ((next & 0xC0) != 0x80) {
            *pos = at + 1;
            return first;
        }
        cp = (cp << 6) | (uint32_t)(next & 0x3F);
    }
    if (cp < minimum || cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) {
        *pos = at + 1;
        return first;
    }
    *pos = at + need;
    return cp;
}

static size_t utf8_encode(uint32_t cp, char *out)
{
    if (cp < 0x80) {
        out[0] = (char)cp;
        return 1;
    }
    if (cp < 0x800) {
        out[0] = (char)(0xC0 | (cp >> 6));
        out[1] = (char)(0x80 | (cp & 0x3F));
        return 2;
    }
    if (cp < 0x10000) {
        out[0] = (char)(0xE0 | (cp >> 12));
        out[1] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[2] = (char)(0x80 | (cp & 0x3F));
        return 3;
    }
    out[0] = (char)(0xF0 | (cp >> 18));
    out[1] = (char)(0x80 | ((cp >> 12) & 0x3F));
    out[2] = (char)(0x80 | ((cp >> 6) & 0x3F));
    out[3] = (char)(0x80 | (cp & 0x3F));
    return 4;
}

static size_t utf8_count(const char *s, size_t len)
{
    size_t pos = 0, count = 0;
    while (pos < len) {
        utf8_decode(s, len, &pos);
        count++;
    }
    return count;
}

static uint32_t *utf8_codepoints(const char *s, size_t len, size_t *count)
{
    *count = utf8_count(s, len);
    uint32_t *cps = xmalloc((*count + 1) * sizeof(uint32_t));
    size_t pos = 0, i = 0;
    while (pos < len)
        cps[i++] = utf8_decode(s, len, &pos);
    return cps;
}

static char *utf8_slice(const char *s, size_t len, size_t start, size_t end)
{
    size_t pos = 0, index = 0;
    while (pos < len && index < start) {
        utf8_decode(s, len, &pos);
        index++;
    }
    size_t begin = pos;
    while (pos < len && index < end) {
        utf8_decode(s, len, &pos);
        index++;
    }
    return dup_range(s + begin, pos - begin);
}

static bool utf8_char_at(const char *s, size_t len, size_t index, uint32_t *out)
{
    size_t pos = 0, i = 0;
    while (pos < len) {
        uint32_t cp = utf8_decode(s, len, &pos);
        if (i == index) {
            *out = cp;
            return true;
        }
        i++;
    }
    return false;
}

static bool is_whitespace_cp(uint32_t cp)
{
    return (cp >= 0x09 && cp <= 0x0D) || cp == 0x20 || cp == 0x85 ||
           cp == 0xA0 || cp == 0x1680 || (cp >= 0x2000 && cp <= 0x200A) ||
           cp == 0x2028 || cp == 0x2029 || cp == 0x202F || cp == 0x205F ||
           cp == 0x3000;
}

static char *trim_text(const char *s, size_t len)
{
    size_t count;
    uint32_t *cps = utf8_codepoints(s, len, &count);
    size_t start = 0, end = count;
    while (start < end && is_whitespace_cp(cps[start]))
        start++;
    while (end > start && is_whitespace_cp(cps[end - 1]))
        end--;
    size_t pos = 0, index = 0, begin = 0, finish = 0;
    while (pos < len) {
        if (index == start)
            begin = pos;
        utf8_decode(s, len, &pos);
        index++;
        if (index == end)
            finish = pos;
    }
    if (start == end) {
        free(cps);
        return dup_cstr("");
    }
    char *out = dup_range(s + begin, finish - begin);
    free(cps);
    return out;
}

/* ------------------------------------------------------------------ */
/* Growable string builder                                              */
/* ------------------------------------------------------------------ */

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} StrBuf;

static void sb_init(StrBuf *sb)
{
    sb->cap = 64;
    sb->len = 0;
    sb->data = xmalloc(sb->cap);
    sb->data[0] = '\0';
}

static void sb_reserve(StrBuf *sb, size_t extra)
{
    if (sb->len + extra + 1 <= sb->cap)
        return;
    size_t cap = sb->cap;
    while (cap < sb->len + extra + 1)
        cap *= 2;
    char *data = xmalloc(cap);
    memcpy(data, sb->data, sb->len + 1);
    free(sb->data);
    sb->data = data;
    sb->cap = cap;
}

static void sb_push_n(StrBuf *sb, const char *s, size_t n)
{
    sb_reserve(sb, n);
    memcpy(sb->data + sb->len, s, n);
    sb->len += n;
    sb->data[sb->len] = '\0';
}

static void sb_push(StrBuf *sb, const char *s)
{
    sb_push_n(sb, s, strlen(s));
}

static void sb_push_char(StrBuf *sb, char c)
{
    sb_push_n(sb, &c, 1);
}

static void sb_push_cp(StrBuf *sb, uint32_t cp)
{
    char encoded[4];
    size_t n = utf8_encode(cp, encoded);
    sb_push_n(sb, encoded, n);
}

static void sb_push_display(StrBuf *sb, EmValue *v)
{
    char *text = value_display(v);
    sb_push(sb, text);
    free(text);
}

static char *sb_take(StrBuf *sb)
{
    char *out = sb->data;
    sb->data = NULL;
    sb->len = sb->cap = 0;
    return out;
}

static void sb_free(StrBuf *sb)
{
    free(sb->data);
    sb->data = NULL;
}

/* Case mapping for the ranges used by Englishmen programs. Full Unicode case
 * conversion is supplied by Rust; this covers ASCII, Latin-1, Greek and
 * Cyrillic, which is what the compiler and its tests exercise. */
static void append_upper(StrBuf *sb, uint32_t cp)
{
    if (cp >= 'a' && cp <= 'z') {
        sb_push_cp(sb, cp - 32);
    } else if ((cp >= 0xE0 && cp <= 0xF6) || (cp >= 0xF8 && cp <= 0xFE)) {
        sb_push_cp(sb, cp - 32);
    } else if (cp == 0xFF) {
        sb_push(sb, "Ÿ");
    } else if (cp == 0xDF) {
        sb_push(sb, "SS");
    } else if (cp == 0x3C2) {
        sb_push_cp(sb, 0x3A3);
    } else if (cp >= 0x3B1 && cp <= 0x3C9) {
        sb_push_cp(sb, cp - 32);
    } else if (cp >= 0x430 && cp <= 0x44F) {
        sb_push_cp(sb, cp - 32);
    } else if (cp >= 0x450 && cp <= 0x45F) {
        sb_push_cp(sb, cp - 80);
    } else {
        sb_push_cp(sb, cp);
    }
}

static void append_lower(StrBuf *sb, uint32_t cp)
{
    if (cp >= 'A' && cp <= 'Z') {
        sb_push_cp(sb, cp + 32);
    } else if ((cp >= 0xC0 && cp <= 0xD6) || (cp >= 0xD8 && cp <= 0xDE)) {
        sb_push_cp(sb, cp + 32);
    } else if (cp == 0x178) {
        sb_push_cp(sb, 0xFF);
    } else if (cp == 0x3A3) {
        sb_push_cp(sb, 0x3C2);
    } else if (cp >= 0x391 && cp <= 0x3A9) {
        sb_push_cp(sb, cp + 32);
    } else if (cp >= 0x410 && cp <= 0x42F) {
        sb_push_cp(sb, cp + 32);
    } else if (cp >= 0x400 && cp <= 0x40F) {
        sb_push_cp(sb, cp + 80);
    } else {
        sb_push_cp(sb, cp);
    }
}

static char *change_case(const char *s, size_t len, bool upper)
{
    StrBuf sb;
    sb_init(&sb);
    size_t pos = 0;
    while (pos < len) {
        uint32_t cp = utf8_decode(s, len, &pos);
        if (upper)
            append_upper(&sb, cp);
        else
            append_lower(&sb, cp);
    }
    return sb_take(&sb);
}

/* ------------------------------------------------------------------ */
/* Random state: xorshift64*, identical to src/builtins.rs.             */
/* ------------------------------------------------------------------ */

static uint64_t rng_state = 0;

static uint64_t next_rand_u64(void)
{
    uint64_t x = rng_state;
    if (x == 0) {
        struct timespec ts = {0, 0};
        if (timespec_get(&ts, TIME_UTC) == TIME_UTC)
            x = (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
        else
            x = (uint64_t)time(NULL);
        x ^= 0xA0761D6478BD642FULL;
    }
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    rng_state = x;
    return x * 0x2545F4914F6CDD1DULL;
}

static double next_rand(void)
{
    return (double)(next_rand_u64() >> 11) / (double)(1ull << 53);
}

static int64_t double_to_i64(double n)
{
    if (n >= 9223372036854775808.0)
        return INT64_MAX;
    if (n <= -9223372036854775808.0)
        return INT64_MIN;
    return (int64_t)n;
}

/* ------------------------------------------------------------------ */
/* Output                                                              */
/* ------------------------------------------------------------------ */

static EmValue *print_values(EmValue **args, size_t argc, char **err)
{
    StrBuf sb;
    sb_init(&sb);
    for (size_t i = 0; i < argc; i++) {
        if (i)
            sb_push_char(&sb, ' ');
        sb_push_display(&sb, args[i]);
    }
    sb_push_char(&sb, '\n');
    if (fwrite(sb.data, 1, sb.len, stdout) != sb.len) {
        sb_free(&sb);
        return fail(err, "could not write to stdout");
    }
    sb_free(&sb);
    fflush(stdout);
    return emv_null();
}

static EmValue *em_builtin_say(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    return print_values(args, argc, err);
}

static EmValue *em_builtin_print(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    return print_values(args, argc, err);
}

/* ------------------------------------------------------------------ */
/* Conversions and predicates                                           */
/* ------------------------------------------------------------------ */

static EmValue *em_builtin_len(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "len", err))
        return NULL;
    EmValue *v = args[0];
    if (emv_is_string(v))
        return emv_number((double)utf8_count(emv_as_cstr(v), emv_strlen(v)));
    if (emv_is_array(v))
        return emv_number((double)emv_array_len(v));
    if (emv_is_object(v)) {
        size_t count = 0;
        char **keys = emv_object_keys(v, &count);
        for (size_t i = 0; i < count; i++)
            free(keys[i]);
        free(keys);
        return emv_number((double)count);
    }
    return fail(err,
                "Type mismatch: len expects a string, array, or object, found %s",
                value_type_name(v));
}

static EmValue *em_builtin_type(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "type", err))
        return NULL;
    return emv_string(value_type_name(args[0]));
}

static EmValue *em_builtin_to_string(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "toString", err))
        return NULL;
    char *text = value_display(args[0]);
    EmValue *out = emv_string_len(text, strlen(text));
    free(text);
    return out;
}

/* C strtod accepts hexadecimal floats; Rust's f64 parser does not. */
static bool is_hex_float(const char *s)
{
    if (*s == '+' || *s == '-')
        s++;
    return s[0] == '0' && (s[1] == 'x' || s[1] == 'X');
}

static EmValue *em_builtin_to_number(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "toNumber", err))
        return NULL;
    EmValue *v = args[0];
    if (emv_is_number(v))
        return emv_number(emv_as_number(v));
    if (emv_is_bool(v))
        return emv_number(emv_as_bool(v) ? 1.0 : 0.0);
    if (emv_is_null(v))
        return emv_number(0.0);
    if (emv_is_string(v)) {
        char *trimmed = trim_text(emv_as_cstr(v), emv_strlen(v));
        char *end = NULL;
        double n = strtod(trimmed, &end);
        bool valid = !is_hex_float(trimmed) && end != trimmed &&
                     *end == '\0' && isfinite(n);
        if (valid) {
            free(trimmed);
            return emv_number(n);
        }
        char *message = em_fmt("toNumber could not convert string \"%s\" to a number",
                               emv_as_cstr(v));
        free(trimmed);
        *err = message;
        return NULL;
    }
    return fail(err,
                "Type mismatch: toNumber expects a number, string, boolean, or null, found %s",
                value_type_name(v));
}

static EmValue *em_builtin_to_bool(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "toBool", err))
        return NULL;
    return emv_bool(emv_truthy(args[0]));
}

#define PREDICATE_BUILTIN(NAME, TEST)                                        \
    static EmValue *em_builtin_##NAME(EmVm *vm, EmValue **args, size_t argc, \
                                      char **err)                            \
    {                                                                        \
        (void)vm;                                                            \
        if (!exact_args(argc, 1, #NAME, err))                                \
            return NULL;                                                     \
        return emv_bool(TEST(args[0]));                                      \
    }

PREDICATE_BUILTIN(isNumber, emv_is_number)
PREDICATE_BUILTIN(isString, emv_is_string)
PREDICATE_BUILTIN(isArray, emv_is_array)
PREDICATE_BUILTIN(isObject, emv_is_object)
PREDICATE_BUILTIN(isBool, emv_is_bool)
PREDICATE_BUILTIN(isNull, emv_is_null)
#undef PREDICATE_BUILTIN

static EmValue *em_builtin_is_function(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "isFunction", err))
        return NULL;
    EmValueType type = emv_type(args[0]);
    return emv_bool(type == EMV_CLOSURE || type == EMV_NATIVE);
}

static EmValue *em_builtin_is_nan(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "isNaN", err))
        return NULL;
    return emv_bool(emv_is_number(args[0]) && isnan(emv_as_number(args[0])));
}

/* ------------------------------------------------------------------ */
/* Parsing                                                              */
/* ------------------------------------------------------------------ */

static int digit_value(unsigned char c)
{
    if (c >= '0' && c <= '9')
        return c - '0';
    if (c >= 'a' && c <= 'z')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'Z')
        return c - 'A' + 10;
    return -1;
}

static EmValue *em_builtin_parse_int(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!at_least_args(argc, 1, "parseInt", err))
        return NULL;
    if (!no_extra_args(argc, 2, "parseInt", err))
        return NULL;
    const char *src;
    size_t len;
    if (!expect_string(args[0], "for parseInt", &src, &len, err))
        return NULL;
    double radix_value = 10.0;
    if (argc == 2 && !expect_number(args[1], "as the radix for parseInt", &radix_value, err))
        return NULL;
    if (!isfinite(radix_value) || trunc(radix_value) != radix_value ||
        radix_value < 2.0 || radix_value > 36.0)
        return fail(err, "parseInt radix must be an integer between 2 and 36");

    char *trimmed = trim_text(src, len);
    size_t at = 0;
    bool negative = false;
    if (trimmed[0] == '-') {
        negative = true;
        at = 1;
    } else if (trimmed[0] == '+') {
        at = 1;
    }
    uint64_t result = 0;
    unsigned radix = (unsigned)radix_value;
    bool valid = trimmed[at] != '\0';
    bool invalid_digit = false;
    for (size_t i = at; valid && trimmed[i]; i++) {
        int digit = digit_value((unsigned char)trimmed[i]);
        if (digit < 0 || (unsigned)digit >= radix) {
            valid = false;
            invalid_digit = true;
            break;
        }
        if (result > (UINT64_MAX - (uint64_t)digit) / radix) {
            free(trimmed);
            return fail(err, "parseInt number is too large: \"%s\"", src);
        }
        result = result * radix + (uint64_t)digit;
    }
    free(trimmed);
    if (!valid) {
        if (invalid_digit)
            return fail(err, "parseInt could not parse \"%s\" as base %u", src, radix);
        return fail(err, "parseInt could not parse \"%s\"", src);
    }
    double number = (double)result;
    if (negative)
        number = -number;
    return emv_number(number);
}

static EmValue *em_builtin_parse_float(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "parseFloat", err))
        return NULL;
    const char *src;
    size_t len;
    if (!expect_string(args[0], "for parseFloat", &src, &len, err))
        return NULL;
    char *trimmed = trim_text(src, len);
    char *end = NULL;
    double number = strtod(trimmed, &end);
    bool valid = !is_hex_float(trimmed) && end != trimmed && *end == '\0';
    free(trimmed);
    if (!valid)
        return fail(err, "parseFloat could not parse \"%s\"", src);
    return emv_number(number);
}

/* ------------------------------------------------------------------ */
/* Collections                                                          */
/* ------------------------------------------------------------------ */

static EmValue *em_builtin_range(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!at_least_args(argc, 1, "range", err))
        return NULL;
    if (!no_extra_args(argc, 3, "range", err))
        return NULL;
    double start = 0.0, end = 0.0, step = 1.0;
    if (argc == 1) {
        if (!expect_number(args[0], "as the end for range", &end, err))
            return NULL;
    } else {
        if (!expect_number(args[0], "as the start for range", &start, err) ||
            !expect_number(args[1], "as the end for range", &end, err))
            return NULL;
        if (argc == 3 &&
            !expect_number(args[2], "as the step for range", &step, err))
            return NULL;
    }
    if (step == 0.0 || !isfinite(step))
        return fail(err, "range step must be a non-zero finite number");
    if (!isfinite(start) || !isfinite(end))
        return fail(err, "range start and end must be finite numbers");

    EmValue *out = emv_array();
    double cur = start;
    /* The iteration cap keeps malformed bytecode or pathological inputs from
     * spinning forever while preserving the native half-open interval. */
    size_t guard = 0;
    if (step > 0.0) {
        while (cur < end && guard++ < 10000000) {
            emv_array_push(out, emv_number(cur));
            cur += step;
        }
    } else {
        while (cur > end && guard++ < 10000000) {
            emv_array_push(out, emv_number(cur));
            cur += step;
        }
    }
    return out;
}

static bool numeric_sum(EmValue *array, const char *name, double *total, char **err)
{
    size_t len = emv_array_len(array);
    for (size_t i = 0; i < len; i++) {
        EmValue *item = emv_array_get(array, i);
        if (!item || !emv_is_number(item)) {
            fail(err,
                 "Type mismatch: %s expects an array of numbers, found %s at index %zu",
                 name, value_type_name(item), i);
            return false;
        }
        *total += emv_as_number(item);
    }
    return true;
}

static EmValue *em_builtin_sum(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "sum", err))
        return NULL;
    EmValue *array;
    if (!expect_array(args[0], "for sum", &array, err))
        return NULL;
    double total = 0.0;
    if (!numeric_sum(array, "sum", &total, err))
        return NULL;
    return emv_number(total);
}

static EmValue *em_builtin_average(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "average", err))
        return NULL;
    EmValue *array;
    if (!expect_array(args[0], "for average", &array, err))
        return NULL;
    size_t len = emv_array_len(array);
    if (len == 0)
        return fail(err, "average cannot compute the average of an empty array");
    double total = 0.0;
    if (!numeric_sum(array, "average", &total, err))
        return NULL;
    return emv_number(total / (double)len);
}

static EmValue *extreme(EmVm *vm, EmValue **args, size_t argc,
                        const char *name, bool maximum, char **err)
{
    (void)vm;
    if (!at_least_args(argc, 1, name, err))
        return NULL;
    EmValue *array = NULL;
    size_t count;
    if (argc == 1) {
        char *ctx = em_fmt("for %s", name);
        bool ok = expect_array(args[0], ctx, &array, err);
        free(ctx);
        if (!ok)
            return NULL;
        count = emv_array_len(array);
        if (count == 0)
            return fail(err, "%s expects at least one value", name);
    } else {
        count = argc;
    }
    char *ctx = em_fmt("for %s", name);
    double best;
    bool ok = expect_number(argc == 1 ? emv_array_get(array, 0) : args[0], ctx, &best, err);
    if (!ok) {
        free(ctx);
        return NULL;
    }
    for (size_t i = 1; i < count; i++) {
        EmValue *item = argc == 1 ? emv_array_get(array, i) : args[i];
        double n;
        ok = expect_number(item, ctx, &n, err);
        if (!ok) {
            free(ctx);
            return NULL;
        }
        if ((maximum && n > best) || (!maximum && n < best))
            best = n;
    }
    free(ctx);
    return emv_number(best);
}

static EmValue *em_builtin_min(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    return extreme(vm, args, argc, "min", false, err);
}

static EmValue *em_builtin_max(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    return extreme(vm, args, argc, "max", true, err);
}

static EmValue *em_builtin_abs(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "abs", err))
        return NULL;
    double n;
    if (!expect_number(args[0], "for abs", &n, err))
        return NULL;
    return emv_number(fabs(n));
}

static EmValue *em_builtin_clamp(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 3, "clamp", err))
        return NULL;
    double value, lo, hi;
    if (!expect_number(args[0], "as the value for clamp", &value, err) ||
        !expect_number(args[1], "as the minimum for clamp", &lo, err) ||
        !expect_number(args[2], "as the maximum for clamp", &hi, err))
        return NULL;
    if (lo > hi)
        return fail(err, "clamp expects min <= max");
    return emv_number(fmax(fmin(value, hi), lo));
}

static int compare_numbers(const void *a, const void *b)
{
    double x = *(const double *)a;
    double y = *(const double *)b;
    if (x < y)
        return -1;
    if (x > y)
        return 1;
    return 0; /* NaN and equal numbers compare as Equal in Rust. */
}

static int compare_strings(const void *a, const void *b)
{
    const char *x = *(const char *const *)a;
    const char *y = *(const char *const *)b;
    return strcmp(x, y);
}

static EmValue *em_builtin_sorted(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "sorted", err))
        return NULL;
    EmValue *array;
    if (!expect_array(args[0], "for sorted", &array, err))
        return NULL;
    size_t len = emv_array_len(array);
    EmValue *out = emv_array();
    if (len == 0)
        return out;
    bool all_numbers = true, all_strings = true;
    for (size_t i = 0; i < len; i++) {
        EmValueType type = emv_type(emv_array_get(array, i));
        all_numbers = all_numbers && type == EMV_NUMBER;
        all_strings = all_strings && type == EMV_STRING;
    }
    if (!all_numbers && !all_strings)
        return fail(err, "sorted expects an array of all numbers or all strings");
    if (all_numbers) {
        double *numbers = xmalloc(len * sizeof(double));
        for (size_t i = 0; i < len; i++)
            numbers[i] = emv_as_number(emv_array_get(array, i));
        qsort(numbers, len, sizeof(double), compare_numbers);
        for (size_t i = 0; i < len; i++)
            emv_array_push(out, emv_number(numbers[i]));
        free(numbers);
    } else {
        char **strings = xmalloc(len * sizeof(char *));
        for (size_t i = 0; i < len; i++)
            strings[i] = dup_cstr(emv_as_cstr(emv_array_get(array, i)));
        qsort(strings, len, sizeof(char *), compare_strings);
        for (size_t i = 0; i < len; i++) {
            emv_array_push(out, emv_string(strings[i]));
            free(strings[i]);
        }
        free(strings);
    }
    return out;
}

static EmValue *em_builtin_reversed(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "reversed", err))
        return NULL;
    EmValue *v = args[0];
    if (emv_is_array(v)) {
        size_t len = emv_array_len(v);
        EmValue *out = emv_array();
        for (size_t i = len; i > 0; i--)
            emv_array_push(out, emv_array_get(v, i - 1));
        return out;
    }
    if (emv_is_string(v)) {
        const char *text = emv_as_cstr(v);
        size_t len = emv_strlen(v);
        size_t count = utf8_count(text, len);
        StrBuf sb;
        sb_init(&sb);
        for (size_t i = count; i > 0; i--) {
            size_t start = i - 1;
            char *part = utf8_slice(text, len, start, i);
            sb_push(&sb, part);
            free(part);
        }
        char *out = sb_take(&sb);
        EmValue *result = emv_string_len(out, strlen(out));
        free(out);
        return result;
    }
    return fail(err, "Type mismatch: reversed expects an array or string, found %s",
                value_type_name(v));
}

static EmValue *em_builtin_zip(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "zip", err))
        return NULL;
    EmValue *a, *b;
    if (!expect_array(args[0], "as the first array for zip", &a, err) ||
        !expect_array(args[1], "as the second array for zip", &b, err))
        return NULL;
    size_t len = emv_array_len(a);
    if (emv_array_len(b) < len)
        len = emv_array_len(b);
    EmValue *out = emv_array();
    for (size_t i = 0; i < len; i++) {
        EmValue *pair = emv_array();
        emv_array_push(pair, emv_array_get(a, i));
        emv_array_push(pair, emv_array_get(b, i));
        emv_array_push(out, pair);
    }
    return out;
}

static EmValue *take_or_drop(EmValue *source, size_t count, bool drop,
                             const char *name, char **err)
{
    if (emv_is_array(source)) {
        size_t len = emv_array_len(source);
        size_t n = count < len ? count : len;
        EmValue *out = emv_array();
        if (drop) {
            for (size_t i = n; i < len; i++)
                emv_array_push(out, emv_array_get(source, i));
        } else {
            for (size_t i = 0; i < n; i++)
                emv_array_push(out, emv_array_get(source, i));
        }
        return out;
    }
    if (emv_is_string(source)) {
        const char *text = emv_as_cstr(source);
        size_t len = utf8_count(text, emv_strlen(source));
        size_t n = count < len ? count : len;
        char *part = drop ? utf8_slice(text, emv_strlen(source), n, len)
                          : utf8_slice(text, emv_strlen(source), 0, n);
        EmValue *out = emv_string_len(part, strlen(part));
        free(part);
        return out;
    }
    return fail(err, "Type mismatch: %s expects an array or string, found %s",
                name, value_type_name(source));
}

static EmValue *em_builtin_take(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "take", err))
        return NULL;
    size_t count;
    if (!expect_count(args[1], "take", &count, err))
        return NULL;
    return take_or_drop(args[0], count, false, "take", err);
}

static EmValue *em_builtin_drop(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "drop", err))
        return NULL;
    size_t count;
    if (!expect_count(args[1], "drop", &count, err))
        return NULL;
    return take_or_drop(args[0], count, true, "drop", err);
}

/* ------------------------------------------------------------------ */
/* Object builtins                                                      */
/* ------------------------------------------------------------------ */

static char **sorted_object_keys(EmValue *object, size_t *count)
{
    char **keys = emv_object_keys(object, count);
    qsort(keys, *count, sizeof(char *), compare_strings);
    return keys;
}

static EmValue *em_builtin_keys(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "keys", err))
        return NULL;
    if (!emv_is_object(args[0]))
        return fail(err, "Type mismatch: keys expects an object, found %s",
                    value_type_name(args[0]));
    size_t count;
    char **keys = sorted_object_keys(args[0], &count);
    EmValue *out = emv_array();
    for (size_t i = 0; i < count; i++) {
        emv_array_push(out, emv_string(keys[i]));
        free(keys[i]);
    }
    free(keys);
    return out;
}

static EmValue *em_builtin_values(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "values", err))
        return NULL;
    if (!emv_is_object(args[0]))
        return fail(err, "Type mismatch: values expects an object, found %s",
                    value_type_name(args[0]));
    size_t count;
    char **keys = sorted_object_keys(args[0], &count);
    EmValue *out = emv_array();
    for (size_t i = 0; i < count; i++) {
        EmValue *value = emv_object_get(args[0], keys[i]);
        emv_array_push(out, value ? value : emv_null());
        free(keys[i]);
    }
    free(keys);
    return out;
}

static EmValue *em_builtin_entries(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "entries", err))
        return NULL;
    if (!emv_is_object(args[0]))
        return fail(err, "Type mismatch: entries expects an object, found %s",
                    value_type_name(args[0]));
    size_t count;
    char **keys = sorted_object_keys(args[0], &count);
    EmValue *out = emv_array();
    for (size_t i = 0; i < count; i++) {
        EmValue *pair = emv_array();
        emv_array_push(pair, emv_string(keys[i]));
        EmValue *value = emv_object_get(args[0], keys[i]);
        emv_array_push(pair, value ? value : emv_null());
        emv_array_push(out, pair);
        free(keys[i]);
    }
    free(keys);
    return out;
}

static EmValue *em_builtin_from_entries(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "fromEntries", err))
        return NULL;
    EmValue *entries;
    if (!expect_array(args[0], "for fromEntries", &entries, err))
        return NULL;
    size_t count = emv_array_len(entries);
    EmValue *out = emv_object();
    for (size_t i = 0; i < count; i++) {
        EmValue *entry = emv_array_get(entries, i);
        if (!entry || !emv_is_array(entry))
            return fail(err,
                        "fromEntries expects an array of [key, value] pairs, found %s at index %zu",
                        value_type_name(entry), i);
        if (emv_array_len(entry) != 2)
            return fail(err,
                        "fromEntries expects exactly 2 values per entry, found %zu at index %zu",
                        emv_array_len(entry), i);
        EmValue *key = emv_array_get(entry, 0);
        if (!key || !emv_is_string(key))
            return fail(err, "fromEntries expects string keys, found %s at index %zu",
                        value_type_name(key), i);
        const char *key_text = emv_as_cstr(key);
        if (emv_object_has(out, key_text))
            return fail(err, "fromEntries found duplicate key \"%s\"", key_text);
        emv_object_set(out, key_text, emv_array_get(entry, 1));
    }
    return out;
}

static EmValue *em_builtin_first(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "first", err))
        return NULL;
    EmValue *v = args[0];
    if (emv_is_array(v)) {
        if (emv_array_len(v) == 0)
            return emv_null();
        emv_retain(emv_array_get(v, 0));
        return emv_array_get(v, 0);
    }
    if (emv_is_string(v)) {
        uint32_t cp;
        if (!utf8_char_at(emv_as_cstr(v), emv_strlen(v), 0, &cp))
            return emv_null();
        char encoded[4];
        size_t n = utf8_encode(cp, encoded);
        return emv_string_len(encoded, n);
    }
    return fail(err, "Type mismatch: first expects an array or string, found %s",
                value_type_name(v));
}

static EmValue *em_builtin_last(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "last", err))
        return NULL;
    EmValue *v = args[0];
    if (emv_is_array(v)) {
        size_t len = emv_array_len(v);
        if (len == 0)
            return emv_null();
        EmValue *item = emv_array_get(v, len - 1);
        emv_retain(item);
        return item;
    }
    if (emv_is_string(v)) {
        size_t count = utf8_count(emv_as_cstr(v), emv_strlen(v));
        if (count == 0)
            return emv_null();
        uint32_t cp;
        utf8_char_at(emv_as_cstr(v), emv_strlen(v), count - 1, &cp);
        char encoded[4];
        size_t n = utf8_encode(cp, encoded);
        return emv_string_len(encoded, n);
    }
    return fail(err, "Type mismatch: last expects an array or string, found %s",
                value_type_name(v));
}

static long long byte_index_of(const char *haystack, size_t haylen,
                               const char *needle, size_t needlelen)
{
    if (needlelen == 0)
        return 0;
    if (needlelen > haylen)
        return -1;
    for (size_t i = 0; i + needlelen <= haylen; i++) {
        if (memcmp(haystack + i, needle, needlelen) == 0)
            return (long long)i;
    }
    return -1;
}

static long long index_of_value(EmValue *haystack, EmValue *needle)
{
    if (!haystack || !needle)
        return -1;
    if (emv_is_string(haystack) && emv_is_string(needle)) {
        long long byte = byte_index_of(emv_as_cstr(haystack), emv_strlen(haystack),
                                       emv_as_cstr(needle), emv_strlen(needle));
        if (byte < 0)
            return -1;
        return (long long)utf8_count(emv_as_cstr(haystack), (size_t)byte);
    }
    if (emv_is_array(haystack)) {
        size_t len = emv_array_len(haystack);
        for (size_t i = 0; i < len; i++) {
            if (emv_equal(emv_array_get(haystack, i), needle))
                return (long long)i;
        }
        return -1;
    }
    if (emv_is_object(haystack) && emv_is_string(needle)) {
        size_t count;
        char **keys = sorted_object_keys(haystack, &count);
        long long result = -1;
        for (size_t i = 0; i < count; i++) {
            if (result < 0 && strcmp(keys[i], emv_as_cstr(needle)) == 0)
                result = (long long)i;
            free(keys[i]);
        }
        free(keys);
        return result;
    }
    return -1;
}

static EmValue *em_builtin_index_of(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "indexOf", err))
        return NULL;
    return emv_number((double)index_of_value(args[0], args[1]));
}

static EmValue *em_builtin_contains(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "contains", err))
        return NULL;
    return emv_bool(index_of_value(args[0], args[1]) >= 0);
}

static EmValue *em_builtin_slice(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!at_least_args(argc, 2, "slice", err))
        return NULL;
    if (!no_extra_args(argc, 3, "slice", err))
        return NULL;
    EmValue *source = args[0];
    size_t len;
    if (emv_is_array(source)) {
        len = emv_array_len(source);
    } else if (emv_is_string(source)) {
        len = utf8_count(emv_as_cstr(source), emv_strlen(source));
    } else {
        return fail(err, "Type mismatch: slice expects an array or string, found %s",
                    value_type_name(source));
    }
    double start_raw, end_raw;
    if (!expect_number(args[1], "as the start for slice", &start_raw, err))
        return NULL;
    size_t start, end;
    if (!slice_index(start_raw, len, &start, err))
        return NULL;
    if (argc == 3) {
        if (!expect_number(args[2], "as the end for slice", &end_raw, err))
            return NULL;
        if (!slice_index(end_raw, len, &end, err))
            return NULL;
    } else {
        end = len;
    }
    if (end < start)
        return fail(err, "slice expects end >= start");
    if (emv_is_array(source)) {
        EmValue *out = emv_array();
        for (size_t i = start; i < end; i++)
            emv_array_push(out, emv_array_get(source, i));
        return out;
    }
    char *part = utf8_slice(emv_as_cstr(source), emv_strlen(source), start, end);
    EmValue *out = emv_string_len(part, strlen(part));
    free(part);
    return out;
}

/* ------------------------------------------------------------------ */
/* Math                                                                 */
/* ------------------------------------------------------------------ */

#define UNARY_MATH_BUILTIN(NAME, EXPR, CTX)                                  \
    static EmValue *em_builtin_##NAME(EmVm *vm, EmValue **args, size_t argc, \
                                      char **err)                            \
    {                                                                        \
        (void)vm;                                                            \
        if (!exact_args(argc, 1, #NAME, err))                                \
            return NULL;                                                     \
        double n;                                                            \
        if (!expect_number(args[0], CTX, &n, err))                           \
            return NULL;                                                     \
        return emv_number(EXPR);                                             \
    }

UNARY_MATH_BUILTIN(floor, floor(n), "for floor")
UNARY_MATH_BUILTIN(ceil, ceil(n), "for ceil")
UNARY_MATH_BUILTIN(round, round(n), "for round")
#undef UNARY_MATH_BUILTIN

static EmValue *em_builtin_pow(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "pow", err))
        return NULL;
    double base, exponent;
    if (!expect_number(args[0], "as the base for pow", &base, err) ||
        !expect_number(args[1], "as the exponent for pow", &exponent, err))
        return NULL;
    return emv_number(pow(base, exponent));
}

static EmValue *em_builtin_sqrt(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "sqrt", err))
        return NULL;
    double n;
    if (!expect_number(args[0], "for sqrt", &n, err))
        return NULL;
    if (n < 0.0)
        return fail(err, "sqrt cannot take the square root of a negative number");
    return emv_number(sqrt(n));
}

static EmValue *em_builtin_sign(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "sign", err))
        return NULL;
    double n;
    if (!expect_number(args[0], "for sign", &n, err))
        return NULL;
    return emv_number(n > 0.0 ? 1.0 : (n < 0.0 ? -1.0 : 0.0));
}

static EmValue *em_builtin_median(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "median", err))
        return NULL;
    EmValue *array;
    if (!expect_array(args[0], "for median", &array, err))
        return NULL;
    size_t len = emv_array_len(array);
    if (len == 0)
        return fail(err, "median expects a non-empty array");
    double *numbers = xmalloc(len * sizeof(double));
    for (size_t i = 0; i < len; i++) {
        char *ctx = em_fmt("at index %zu for median", i);
        bool ok = expect_number(emv_array_get(array, i), ctx, &numbers[i], err);
        free(ctx);
        if (!ok) {
            free(numbers);
            return NULL;
        }
        if (!isfinite(numbers[i])) {
            char *shown = format_double(numbers[i]);
            fail(err, "median expects finite numbers, found %s at index %zu", shown, i);
            free(shown);
            free(numbers);
            return NULL;
        }
    }
    qsort(numbers, len, sizeof(double), compare_numbers);
    size_t mid = len / 2;
    double result = len % 2 == 1 ? numbers[mid] : numbers[mid - 1] / 2.0 + numbers[mid] / 2.0;
    free(numbers);
    return emv_number(result);
}

#define POSITIVE_LOG_BUILTIN(NAME, FN)                                       \
    static EmValue *em_builtin_##NAME(EmVm *vm, EmValue **args, size_t argc,  \
                                      char **err)                             \
    {                                                                        \
        (void)vm;                                                            \
        if (!exact_args(argc, 1, #NAME, err))                                \
            return NULL;                                                     \
        double n;                                                            \
        if (!expect_number(args[0], "for " #NAME, &n, err))                  \
            return NULL;                                                     \
        if (!isfinite(n) || n <= 0.0)                                        \
            return fail(err, #NAME " expects a finite positive number");     \
        return emv_number(FN(n));                                            \
    }

POSITIVE_LOG_BUILTIN(log2, log2)
POSITIVE_LOG_BUILTIN(log10, log10)
#undef POSITIVE_LOG_BUILTIN

static double degrees(double n)
{
    return n * 180.0 / (acos(-1.0));
}

static double radians(double n)
{
    return n * (acos(-1.0) / 180.0);
}

#define FINITE_UNARY_MATH(NAME, FN)                                          \
    static EmValue *em_builtin_##NAME(EmVm *vm, EmValue **args, size_t argc,  \
                                      char **err)                             \
    {                                                                        \
        (void)vm;                                                            \
        if (!exact_args(argc, 1, #NAME, err))                                \
            return NULL;                                                     \
        double n;                                                            \
        if (!expect_number(args[0], "for " #NAME, &n, err))                  \
            return NULL;                                                     \
        if (!isfinite(n))                                                    \
            return fail(err, #NAME " expects a finite number");              \
        return emv_number(FN(n));                                            \
    }

FINITE_UNARY_MATH(toDegrees, degrees)
FINITE_UNARY_MATH(toRadians, radians)
#undef FINITE_UNARY_MATH

/* ------------------------------------------------------------------ */
/* Random                                                              */
/* ------------------------------------------------------------------ */

static EmValue *em_builtin_random(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    (void)args;
    if (!exact_args(argc, 0, "random", err))
        return NULL;
    return emv_number(next_rand());
}

static EmValue *em_builtin_random_int(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!at_least_args(argc, 1, "randomInt", err))
        return NULL;
    if (!no_extra_args(argc, 2, "randomInt", err))
        return NULL;
    double min = 0.0, max;
    if (argc == 2 &&
        !expect_number(args[0], "as the min for randomInt", &min, err))
        return NULL;
    if (!expect_number(args[argc - 1], "as the max for randomInt", &max, err))
        return NULL;
    if (!isfinite(min) || !isfinite(max) || trunc(min) != min || trunc(max) != max)
        return fail(err, "randomInt expects finite integer arguments");
    int64_t min_i = double_to_i64(min);
    int64_t max_i = double_to_i64(max);
    if (min_i > max_i)
        return fail(err, "randomInt expects min <= max");
    uint64_t span = (uint64_t)max_i - (uint64_t)min_i + 1ull;
    uint64_t bits = (uint64_t)min_i + (next_rand_u64() % span);
    int64_t result;
    memcpy(&result, &bits, sizeof(result));
    return emv_number((double)result);
}

static EmValue *em_builtin_seed(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "seed", err))
        return NULL;
    double n;
    if (!expect_number(args[0], "for seed", &n, err))
        return NULL;
    if (!isfinite(n))
        return fail(err, "seed expects a finite number");
    uint64_t bits;
    memcpy(&bits, &n, sizeof(bits));
    rng_state = bits ^ 0xA0761D6478BD642FULL;
    if (rng_state == 0)
        rng_state = 0x9E3779B97F4A7C15ULL;
    return emv_null();
}

static EmValue *em_builtin_random_choice(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "randomChoice", err))
        return NULL;
    EmValue *array;
    if (!expect_array(args[0], "for randomChoice", &array, err))
        return NULL;
    size_t len = emv_array_len(array);
    if (len == 0)
        return fail(err, "randomChoice expects a non-empty array");
    EmValue *item = emv_array_get(array, (size_t)(next_rand_u64() % (uint64_t)len));
    emv_retain(item);
    return item;
}

static EmValue *em_builtin_shuffle(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "shuffle", err))
        return NULL;
    EmValue *array;
    if (!expect_array(args[0], "for shuffle", &array, err))
        return NULL;
    size_t len = emv_array_len(array);
    EmValue *out = emv_array();
    EmValue **items = xmalloc((len + 1) * sizeof(EmValue *));
    for (size_t i = 0; i < len; i++)
        items[i] = emv_array_get(array, i);
    for (size_t i = len; i > 1; i--) {
        size_t j = (size_t)(next_rand_u64() % (uint64_t)i);
        EmValue *tmp = items[i - 1];
        items[i - 1] = items[j];
        items[j] = tmp;
    }
    for (size_t i = 0; i < len; i++)
        emv_array_push(out, items[i]);
    free(items);
    return out;
}

/* ------------------------------------------------------------------ */
/* Text                                                                 */
/* ------------------------------------------------------------------ */

static EmValue *em_builtin_trim(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "trim", err))
        return NULL;
    const char *text;
    size_t len;
    if (!expect_string(args[0], "for trim", &text, &len, err))
        return NULL;
    char *out = trim_text(text, len);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

static EmValue *em_builtin_upper(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "upper", err))
        return NULL;
    const char *text;
    size_t len;
    if (!expect_string(args[0], "for upper", &text, &len, err))
        return NULL;
    char *out = change_case(text, len, true);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

static EmValue *em_builtin_lower(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "lower", err))
        return NULL;
    const char *text;
    size_t len;
    if (!expect_string(args[0], "for lower", &text, &len, err))
        return NULL;
    char *out = change_case(text, len, false);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

static EmValue *em_builtin_split(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "split", err))
        return NULL;
    const char *text, *separator;
    size_t len, seplen;
    if (!expect_string(args[0], "for split", &text, &len, err) ||
        !expect_string(args[1], "as the separator for split", &separator, &seplen, err))
        return NULL;
    EmValue *out = emv_array();
    if (seplen == 0) {
        size_t pos = 0;
        while (pos < len) {
            uint32_t cp = utf8_decode(text, len, &pos);
            char encoded[4];
            size_t n = utf8_encode(cp, encoded);
            emv_array_push(out, emv_string_len(encoded, n));
        }
        return out;
    }
    size_t start = 0, at = 0;
    while (at + seplen <= len) {
        if (memcmp(text + at, separator, seplen) == 0) {
            char *piece = dup_range(text + start, at - start);
            emv_array_push(out, emv_string_len(piece, at - start));
            free(piece);
            at += seplen;
            start = at;
        } else {
            at++;
        }
    }
    char *piece = dup_range(text + start, len - start);
    emv_array_push(out, emv_string_len(piece, len - start));
    free(piece);
    return out;
}

static EmValue *em_builtin_join(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!at_least_args(argc, 1, "join", err))
        return NULL;
    if (!no_extra_args(argc, 2, "join", err))
        return NULL;
    EmValue *array;
    if (!expect_array(args[0], "for join", &array, err))
        return NULL;
    const char *separator = "";
    size_t seplen = 0;
    if (argc == 2 &&
        !expect_string(args[1], "as the separator for join", &separator, &seplen, err))
        return NULL;
    StrBuf sb;
    sb_init(&sb);
    size_t len = emv_array_len(array);
    for (size_t i = 0; i < len; i++) {
        if (i)
            sb_push_n(&sb, separator, seplen);
        sb_push_display(&sb, emv_array_get(array, i));
    }
    char *out = sb_take(&sb);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

static EmValue *em_builtin_replace(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 3, "replace", err))
        return NULL;
    const char *text, *from, *to;
    size_t len, fromlen, tolen;
    if (!expect_string(args[0], "for replace", &text, &len, err) ||
        !expect_string(args[1], "as the text to replace for replace", &from, &fromlen, err) ||
        !expect_string(args[2], "as the replacement for replace", &to, &tolen, err))
        return NULL;
    if (fromlen == 0)
        return emv_string_len(text, len);
    StrBuf sb;
    sb_init(&sb);
    size_t start = 0, at = 0;
    while (at + fromlen <= len) {
        if (memcmp(text + at, from, fromlen) == 0) {
            sb_push_n(&sb, text + start, at - start);
            sb_push_n(&sb, to, tolen);
            at += fromlen;
            start = at;
        } else {
            at++;
        }
    }
    sb_push_n(&sb, text + start, len - start);
    char *out = sb_take(&sb);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

#define PREFIX_BUILTIN(NAME, CTX, WORD, TEST)                                \
    static EmValue *em_builtin_##NAME(EmVm *vm, EmValue **args, size_t argc, \
                                      char **err)                             \
    {                                                                        \
        (void)vm;                                                            \
        if (!exact_args(argc, 2, #NAME, err))                                \
            return NULL;                                                     \
        const char *text, *part;                                             \
        size_t len, partlen;                                                 \
        if (!expect_string(args[0], CTX, &text, &len, err) ||                 \
            !expect_string(args[1], WORD, &part, &partlen, err))             \
            return NULL;                                                     \
        return emv_bool(TEST);                                               \
    }

PREFIX_BUILTIN(startsWith, "for startsWith", "as the prefix for startsWith",
               partlen <= len && memcmp(text, part, partlen) == 0)
PREFIX_BUILTIN(endsWith, "for endsWith", "as the suffix for endsWith",
               partlen <= len && memcmp(text + len - partlen, part, partlen) == 0)
#undef PREFIX_BUILTIN

static EmValue *em_builtin_repeat(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "repeat", err))
        return NULL;
    const char *text;
    size_t len;
    double count_value;
    if (!expect_string(args[0], "for repeat", &text, &len, err) ||
        !expect_number(args[1], "as the count for repeat", &count_value, err))
        return NULL;
    if (!isfinite(count_value) || trunc(count_value) != count_value || count_value < 0.0)
        return fail(err, "repeat expects a non-negative integer count");
    if (count_value >= (double)SIZE_MAX)
        return fail(err, "repeat count is too large");
    size_t times = (size_t)count_value;
    if (len != 0 && times > 1000000 / len)
        return fail(err, "repeat output would exceed 1000000 bytes");
    StrBuf sb;
    sb_init(&sb);
    for (size_t i = 0; i < times; i++)
        sb_push_n(&sb, text, len);
    char *out = sb_take(&sb);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

static EmValue *pad_string(const char *text, size_t len, size_t target,
                           const char *fill, size_t filllen, bool at_start)
{
    if (target > 100000)
        target = 100000;
    size_t chars = utf8_count(text, len);
    if (target <= chars)
        return emv_string_len(text, len);
    size_t missing = target - chars;
    if (filllen == 0) {
        fill = " ";
        filllen = 1;
    }
    size_t filler_count = 0;
    uint32_t *filler = utf8_codepoints(fill, filllen, &filler_count);
    StrBuf sb;
    sb_init(&sb);
    if (at_start) {
        for (size_t i = 0; i < missing; i++)
            sb_push_cp(&sb, filler[i % filler_count]);
        sb_push_n(&sb, text, len);
    } else {
        sb_push_n(&sb, text, len);
        for (size_t i = 0; i < missing; i++)
            sb_push_cp(&sb, filler[i % filler_count]);
    }
    free(filler);
    char *out = sb_take(&sb);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

static EmValue *padding_builtin(EmVm *vm, EmValue **args, size_t argc,
                                const char *name, bool at_start, char **err)
{
    (void)vm;
    if (!at_least_args(argc, 2, name, err))
        return NULL;
    if (!no_extra_args(argc, 3, name, err))
        return NULL;
    const char *text, *fill = " ";
    size_t len, filllen = 1;
    char *ctx = em_fmt("for %s", name);
    bool ok = expect_string(args[0], ctx, &text, &len, err);
    free(ctx);
    if (!ok)
        return NULL;
    size_t target;
    if (!expect_count(args[1], name, &target, err))
        return NULL;
    if (argc == 3) {
        char *ctx = em_fmt("as the fill for %s", name);
        bool ok = expect_string(args[2], ctx, &fill, &filllen, err);
        free(ctx);
        if (!ok)
            return NULL;
    }
    return pad_string(text, len, target, fill, filllen, at_start);
}

static EmValue *em_builtin_pad_start(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    return padding_builtin(vm, args, argc, "padStart", true, err);
}

static EmValue *em_builtin_pad_end(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    return padding_builtin(vm, args, argc, "padEnd", false, err);
}

static EmValue *em_builtin_char_code(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "charCode", err))
        return NULL;
    const char *text;
    size_t len;
    if (!expect_string(args[0], "for charCode", &text, &len, err))
        return NULL;
    uint32_t cp;
    if (!utf8_char_at(text, len, 0, &cp))
        return emv_null();
    return emv_number((double)cp);
}

static EmValue *em_builtin_from_char_code(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "fromCharCode", err))
        return NULL;
    double n;
    if (!expect_number(args[0], "for fromCharCode", &n, err))
        return NULL;
    if (!isfinite(n) || trunc(n) != n || n < 0.0 || n > 0x10FFFF)
        return fail(err, "fromCharCode expects a Unicode code point from 0 to 1114111");
    uint32_t cp = (uint32_t)n;
    if (cp >= 0xD800 && cp <= 0xDFFF)
        return fail(err, "fromCharCode: %u is not a valid Unicode code point", cp);
    char encoded[4];
    size_t len = utf8_encode(cp, encoded);
    return emv_string_len(encoded, len);
}

static EmValue *em_builtin_format(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!at_least_args(argc, 1, "format", err))
        return NULL;
    const char *template;
    size_t len;
    if (!expect_string(args[0], "as the format template", &template, &len, err))
        return NULL;
    StrBuf sb;
    sb_init(&sb);
    size_t auto_index = 0;
    for (size_t i = 0; i < len; i++) {
        char c = template[i];
        if (c == '{') {
            if (i + 1 < len && template[i + 1] == '{') {
                sb_push_char(&sb, '{');
                i++;
                continue;
            }
            size_t start = ++i;
            while (i < len && template[i] != '}')
                i++;
            if (i >= len) {
                sb_free(&sb);
                return fail(err, "format: unmatched '{' in template");
            }
            size_t width = i - start;
            size_t index;
            if (width == 0) {
                index = auto_index++;
                if (index >= argc - 1) {
                    sb_free(&sb);
                    return fail(err, "format: missing argument for placeholder at index %zu",
                                index);
                }
            } else {
                char *digits = dup_range(template + start, width);
                char *end = NULL;
                errno = 0;
                unsigned long long parsed = strtoull(digits, &end, 10);
                bool valid = errno == 0 && *end == '\0' && digits[0] != '\0';
                if (!valid) {
                    sb_free(&sb);
                    char *message = em_fmt("format: invalid placeholder '{%s}'", digits);
                    free(digits);
                    *err = message;
                    return NULL;
                }
                if (parsed >= (unsigned long long)(argc - 1)) {
                    sb_free(&sb);
                    char *message = em_fmt("format: missing argument for placeholder {%s}",
                                           digits);
                    free(digits);
                    *err = message;
                    return NULL;
                }
                free(digits);
                index = (size_t)parsed + 1;
            }
            sb_push_display(&sb, args[index]);
        } else if (c == '}') {
            if (i + 1 < len && template[i + 1] == '}') {
                sb_push_char(&sb, '}');
                i++;
                continue;
            }
            sb_free(&sb);
            return fail(err, "format: unmatched '}' in template");
        } else {
            sb_push_char(&sb, c);
        }
    }
    char *out = sb_take(&sb);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

static EmValue *em_builtin_input(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!no_extra_args(argc, 1, "input", err))
        return NULL;
    if (argc == 1) {
        const char *prompt;
        size_t len;
        if (!expect_string(args[0], "as the prompt for input", &prompt, &len, err))
            return NULL;
        if (fwrite(prompt, 1, len, stdout) != len ||
            fflush(stdout) != 0)
            return fail(err, "input could not flush stdout");
    }
    StrBuf sb;
    sb_init(&sb);
    int c;
    while ((c = getchar()) != EOF && c != '\n') {
        sb_push_char(&sb, (char)c);
    }
    if (ferror(stdin)) {
        sb_free(&sb);
        return fail(err, "input could not read from stdin");
    }
    while (sb.len > 0 && (sb.data[sb.len - 1] == '\n' || sb.data[sb.len - 1] == '\r'))
        sb.data[--sb.len] = '\0';
    char *out = sb_take(&sb);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

static EmValue *em_builtin_assert(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!at_least_args(argc, 1, "assert", err))
        return NULL;
    if (!no_extra_args(argc, 2, "assert", err))
        return NULL;
    if (emv_truthy(args[0]))
        return emv_null();
    const char *message = "assertion failed";
    if (argc == 2) {
        size_t len;
        if (!expect_string(args[1], "as the message for assert", &message, &len, err))
            return NULL;
    }
    return fail(err, "Assertion failed: %s", message);
}

/* `args` is the only builtin that needs host state. The C VM registers a
 * "host" import module (see bootstrap-c/host.h) whose args() returns the
 * program arguments; delegating keeps this file independent of main(). */
static EmValue *em_builtin_args(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)args;
    if (!exact_args(argc, 0, "args", err))
        return NULL;
    EmValue *host = em_vm_lookup_import(vm, "host");
    if (!host || !emv_is_object(host))
        return emv_array();
    EmValue *fn = emv_object_get(host, "args");
    if (!fn)
        return emv_array();
    if (emv_is_native(fn)) {
        EmNativeFn native = emv_native_fn(fn);
        if (!native)
            return emv_array();
        return native(vm, NULL, 0, err);
    }
    EmValue *result = NULL;
    if (em_vm_call_value(vm, fn, NULL, 0, &result, err) != 0)
        return NULL;
    return result ? result : emv_array();
}

/* ------------------------------------------------------------------ */
/* JSON. Implemented natively so the bootstrap VM does not depend on a
 * registered "json" import (only "host" is guaranteed by the ABI).       */
/* ------------------------------------------------------------------ */

static bool json_is_control(uint32_t cp)
{
    return cp < 0x20 || (cp >= 0x7F && cp <= 0x9F);
}

static void json_escape(StrBuf *sb, const char *s, size_t len)
{
    sb_push_char(sb, '"');
    size_t pos = 0;
    while (pos < len) {
        uint32_t cp = utf8_decode(s, len, &pos);
        switch (cp) {
        case '"': sb_push(sb, "\\\""); break;
        case '\\': sb_push(sb, "\\\\"); break;
        case '\n': sb_push(sb, "\\n"); break;
        case '\r': sb_push(sb, "\\r"); break;
        case '\t': sb_push(sb, "\\t"); break;
        default:
            if (json_is_control(cp))
                sb_push(sb, em_fmt("\\u%04x", cp));
            else
                sb_push_cp(sb, cp);
            break;
        }
    }
    sb_push_char(sb, '"');
}

static bool json_encode_value(EmValue *value, StrBuf *sb, char **err)
{
    EmValueType type = emv_type(value);
    switch (type) {
    case EMV_NULL:
        sb_push(sb, "null");
        return true;
    case EMV_BOOL:
        sb_push(sb, emv_as_bool(value) ? "true" : "false");
        return true;
    case EMV_NUMBER: {
        double n = emv_as_number(value);
        if (!isfinite(n)) {
            fail(err, "jsonEncode cannot encode non-finite numbers");
            return false;
        }
        char *text = format_double(n);
        sb_push(sb, text);
        free(text);
        return true;
    }
    case EMV_STRING: {
        const char *text = emv_as_cstr(value);
        json_escape(sb, text, emv_strlen(value));
        return true;
    }
    case EMV_ARRAY: {
        sb_push_char(sb, '[');
        size_t len = emv_array_len(value);
        for (size_t i = 0; i < len; i++) {
            if (i)
                sb_push_char(sb, ',');
            if (!json_encode_value(emv_array_get(value, i), sb, err))
                return false;
        }
        sb_push_char(sb, ']');
        return true;
    }
    case EMV_OBJECT: {
        size_t count = 0;
        char **keys = sorted_object_keys(value, &count);
        sb_push_char(sb, '{');
        for (size_t i = 0; i < count; i++) {
            if (i)
                sb_push_char(sb, ',');
            json_escape(sb, keys[i], strlen(keys[i]));
            sb_push_char(sb, ':');
            EmValue *item = emv_object_get(value, keys[i]);
            bool ok = json_encode_value(item ? item : emv_null(), sb, err);
            free(keys[i]);
            if (!ok) {
                for (size_t j = i + 1; j < count; j++)
                    free(keys[j]);
                free(keys);
                return false;
            }
        }
        free(keys);
        sb_push_char(sb, '}');
        return true;
    }
    case EMV_CLOSURE:
    case EMV_NATIVE:
        fail(err, "jsonEncode cannot encode functions");
        return false;
    }
    fail(err, "jsonEncode cannot encode functions");
    return false;
}

static EmValue *em_builtin_json_encode(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "jsonEncode", err))
        return NULL;
    StrBuf sb;
    sb_init(&sb);
    if (!json_encode_value(args[0], &sb, err)) {
        sb_free(&sb);
        return NULL;
    }
    char *out = sb_take(&sb);
    EmValue *value = emv_string_len(out, strlen(out));
    free(out);
    return value;
}

typedef struct {
    const char *text;
    size_t len;
    size_t pos;
    char **err;
} JsonParser;

static bool json_error(JsonParser *p, const char *format, ...)
{
    if (*p->err)
        return false;
    va_list ap;
    va_start(ap, format);
    va_list copy;
    va_copy(copy, ap);
    int needed = vsnprintf(NULL, 0, format, copy);
    va_end(copy);
    if (needed >= 0) {
        char *out = xmalloc((size_t)needed + 1);
        vsnprintf(out, (size_t)needed + 1, format, ap);
        *p->err = out;
    }
    va_end(ap);
    return false;
}

static char json_peek(JsonParser *p)
{
    return p->pos < p->len ? p->text[p->pos] : '\0';
}

static char json_next(JsonParser *p)
{
    if (p->pos >= p->len)
        return '\0';
    return p->text[p->pos++];
}

static void json_skip_ws(JsonParser *p)
{
    while (p->pos < p->len) {
        size_t pos = p->pos;
        uint32_t cp = utf8_decode(p->text, p->len, &pos);
        if (!is_whitespace_cp(cp))
            break;
        p->pos = pos;
    }
}

static bool json_parse_value(JsonParser *p, EmValue **out);

static bool json_consume_literal(JsonParser *p, const char *literal)
{
    for (const char *at = literal; *at; at++) {
        if (json_next(p) != *at)
            return json_error(p, "jsonDecode: expected '%s'", literal);
    }
    return true;
}

static bool json_parse_number(JsonParser *p, EmValue **out)
{
    size_t start = p->pos;
    if (json_peek(p) == '-')
        p->pos++;
    char first = json_peek(p);
    if (first == '0') {
        p->pos++;
    } else if (first >= '1' && first <= '9') {
        while (json_peek(p) >= '0' && json_peek(p) <= '9')
            p->pos++;
    } else {
        return json_error(p, "jsonDecode: invalid number");
    }
    if (json_peek(p) == '.') {
        p->pos++;
        if (json_peek(p) < '0' || json_peek(p) > '9')
            return json_error(p, "jsonDecode: invalid number");
        while (json_peek(p) >= '0' && json_peek(p) <= '9')
            p->pos++;
    }
    if (json_peek(p) == 'e' || json_peek(p) == 'E') {
        p->pos++;
        if (json_peek(p) == '+' || json_peek(p) == '-')
            p->pos++;
        if (json_peek(p) < '0' || json_peek(p) > '9')
            return json_error(p, "jsonDecode: invalid number");
        while (json_peek(p) >= '0' && json_peek(p) <= '9')
            p->pos++;
    }
    char *token = dup_range(p->text + start, p->pos - start);
    char *end = NULL;
    double number = strtod(token, &end);
    if (*end != '\0') {
        char *message = em_fmt("jsonDecode: invalid number '%s'", token);
        free(token);
        *p->err = message;
        return false;
    }
    free(token);
    *out = emv_number(number);
    return true;
}

static bool json_hex4(JsonParser *p, uint32_t *out)
{
    uint32_t code = 0;
    for (int i = 0; i < 4; i++) {
        if (p->pos >= p->len)
            return json_error(p, "jsonDecode: incomplete unicode escape");
        char c = json_next(p);
        int digit = digit_value((unsigned char)c);
        if (digit < 0 || digit > 15)
            return json_error(p, "jsonDecode: invalid unicode escape");
        code = code * 16 + (uint32_t)digit;
    }
    *out = code;
    return true;
}

static bool json_parse_string(JsonParser *p, EmValue **out)
{
    if (json_next(p) != '"')
        return json_error(p, "jsonDecode: expected string");
    StrBuf sb;
    sb_init(&sb);
    for (;;) {
        char c = json_next(p);
        if (c == '\0') {
            sb_free(&sb);
            return json_error(p, "jsonDecode: unterminated string");
        }
        if (c == '"') {
            size_t n = sb.len;
            char *text = sb_take(&sb);
            *out = emv_string_len(text, n);
            free(text);
            return true;
        }
        if ((unsigned char)c < 0x20) {
            sb_free(&sb);
            return json_error(p, "jsonDecode: control character in string");
        }
        if (c != '\\') {
            sb_push_char(&sb, c);
            continue;
        }
        if (p->pos >= p->len) {
            sb_free(&sb);
            return json_error(p, "jsonDecode: unterminated string escape");
        }
        char escape = json_next(p);
        switch (escape) {
        case '"': sb_push_char(&sb, '"'); break;
        case '\\': sb_push_char(&sb, '\\'); break;
        case '/': sb_push_char(&sb, '/'); break;
        case 'b': sb_push_char(&sb, '\b'); break;
        case 'f': sb_push_char(&sb, '\f'); break;
        case 'n': sb_push_char(&sb, '\n'); break;
        case 'r': sb_push_char(&sb, '\r'); break;
        case 't': sb_push_char(&sb, '\t'); break;
        case 'u': {
            uint32_t code;
            if (!json_hex4(p, &code)) {
                sb_free(&sb);
                return false;
            }
            if (code >= 0xD800 && code <= 0xDBFF) {
                if (json_next(p) != '\\' || json_next(p) != 'u') {
                    sb_free(&sb);
                    return json_error(p, "jsonDecode: invalid unicode surrogate pair");
                }
                uint32_t low;
                if (!json_hex4(p, &low)) {
                    sb_free(&sb);
                    return false;
                }
                if (low < 0xDC00 || low > 0xDFFF) {
                    sb_free(&sb);
                    return json_error(p, "jsonDecode: invalid unicode surrogate pair");
                }
                code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
            } else if (code >= 0xDC00 && code <= 0xDFFF) {
                sb_free(&sb);
                return json_error(p, "jsonDecode: invalid unicode codepoint");
            }
            sb_push_cp(&sb, code);
            break;
        }
        default:
            sb_free(&sb);
            return json_error(p, "jsonDecode: invalid escape '\\%c'", escape ? escape : '?');
        }
    }
}

static bool json_parse_array(JsonParser *p, EmValue **out)
{
    if (json_next(p) != '[')
        return json_error(p, "jsonDecode: expected array");
    EmValue *array = emv_array();
    json_skip_ws(p);
    if (json_peek(p) == ']') {
        p->pos++;
        *out = array;
        return true;
    }
    for (;;) {
        EmValue *item = NULL;
        if (!json_parse_value(p, &item)) {
            emv_release(array);
            return false;
        }
        emv_array_push(array, item);
        emv_release(item);
        json_skip_ws(p);
        char c = json_next(p);
        if (c == ']')
            break;
        if (c != ',') {
            emv_release(array);
            return json_error(p, "jsonDecode: expected ',' or ']' in array");
        }
    }
    *out = array;
    return true;
}

static bool json_parse_object(JsonParser *p, EmValue **out)
{
    if (json_next(p) != '{')
        return json_error(p, "jsonDecode: expected object");
    EmValue *object = emv_object();
    json_skip_ws(p);
    if (json_peek(p) == '}') {
        p->pos++;
        *out = object;
        return true;
    }
    for (;;) {
        json_skip_ws(p);
        EmValue *key_value = NULL;
        if (!json_parse_string(p, &key_value)) {
            emv_release(object);
            return false;
        }
        json_skip_ws(p);
        if (json_next(p) != ':') {
            emv_release(key_value);
            emv_release(object);
            return json_error(p, "jsonDecode: expected ':' in object");
        }
        EmValue *item = NULL;
        if (!json_parse_value(p, &item)) {
            emv_release(key_value);
            emv_release(object);
            return false;
        }
        emv_object_set(object, emv_as_cstr(key_value), item);
        emv_release(key_value);
        emv_release(item);
        json_skip_ws(p);
        char c = json_next(p);
        if (c == '}')
            break;
        if (c != ',') {
            emv_release(object);
            return json_error(p, "jsonDecode: expected ',' or '}' in object");
        }
    }
    *out = object;
    return true;
}

static bool json_parse_value(JsonParser *p, EmValue **out)
{
    json_skip_ws(p);
    switch (json_peek(p)) {
    case 'n':
        if (!json_consume_literal(p, "null"))
            return false;
        *out = emv_null();
        return true;
    case 't':
        if (!json_consume_literal(p, "true"))
            return false;
        *out = emv_bool(true);
        return true;
    case 'f':
        if (!json_consume_literal(p, "false"))
            return false;
        *out = emv_bool(false);
        return true;
    case '"': return json_parse_string(p, out);
    case '[': return json_parse_array(p, out);
    case '{': return json_parse_object(p, out);
    case '-':
    case '0': case '1': case '2': case '3': case '4':
    case '5': case '6': case '7': case '8': case '9':
        return json_parse_number(p, out);
    case '\0':
        return json_error(p, "jsonDecode: unexpected end of input");
    default:
        return json_error(p, "jsonDecode: unexpected character '%c'", json_peek(p));
    }
}

static EmValue *em_builtin_json_decode(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "jsonDecode", err))
        return NULL;
    const char *text;
    size_t len;
    if (!expect_string(args[0], "as the JSON string for jsonDecode", &text, &len, err))
        return NULL;
    JsonParser parser = {text, len, 0, err};
    EmValue *result = NULL;
    if (!json_parse_value(&parser, &result))
        return NULL;
    json_skip_ws(&parser);
    if (parser.pos != parser.len) {
        fail(err, "jsonDecode: trailing characters after JSON value");
        return NULL;
    }
    return result;
}

/* ------------------------------------------------------------------ */
/* Filesystem and system (mirrored from src/builtins.rs for programs that
 * call the globals directly; the host module remains the preferred API).  */
/* ------------------------------------------------------------------ */

static char *io_error(const char *prefix, const char *action,
                      const char *path, int code)
{
    return em_fmt("%s could not %s '%s': %s (os error %d)",
                  prefix, action, path, strerror(code), code);
}

static EmValue *em_builtin_read_file(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "readFile", err))
        return NULL;
    const char *path;
    size_t len;
    if (!expect_string(args[0], "as the path for readFile", &path, &len, err))
        return NULL;
    FILE *file = fopen(path, "rb");
    if (!file) {
        *err = io_error("readFile", "read", path, errno);
        return NULL;
    }
    StrBuf sb;
    sb_init(&sb);
    char buffer[4096];
    for (;;) {
        size_t n = fread(buffer, 1, sizeof(buffer), file);
        sb_reserve(&sb, n);
        memcpy(sb.data + sb.len, buffer, n);
        sb.len += n;
        sb.data[sb.len] = '\0';
        if (n < sizeof(buffer)) {
            if (ferror(file)) {
                int code = errno ? errno : EIO;
                fclose(file);
                sb_free(&sb);
                *err = io_error("readFile", "read", path, code);
                return NULL;
            }
            break;
        }
    }
    fclose(file);
    size_t n = sb.len;
    char *out = sb_take(&sb);
    EmValue *value = emv_string_len(out, n);
    free(out);
    return value;
}

static bool write_file(const char *path, const char *content, size_t len,
                       bool append, char **err)
{
    FILE *file = fopen(path, append ? "ab" : "wb");
    if (!file) {
        const char *name = append ? "appendFile" : "writeFile";
        *err = io_error(name, append ? "open" : "write", path, errno);
        return false;
    }
    if (fwrite(content, 1, len, file) != len) {
        int code = errno ? errno : EIO;
        fclose(file);
        const char *name = append ? "appendFile" : "writeFile";
        *err = io_error(name, "write", path, code);
        return false;
    }
    if (fclose(file) != 0) {
        const char *name = append ? "appendFile" : "writeFile";
        *err = io_error(name, "write", path, errno);
        return false;
    }
    return true;
}

static EmValue *em_builtin_write_file(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "writeFile", err))
        return NULL;
    const char *path, *content;
    size_t pathlen, len;
    if (!expect_string(args[0], "as the path for writeFile", &path, &pathlen, err) ||
        !expect_string(args[1], "as the content for writeFile", &content, &len, err))
        return NULL;
    if (!write_file(path, content, len, false, err))
        return NULL;
    return emv_null();
}

static EmValue *em_builtin_append_file(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 2, "appendFile", err))
        return NULL;
    const char *path, *content;
    size_t pathlen, len;
    if (!expect_string(args[0], "as the path for appendFile", &path, &pathlen, err) ||
        !expect_string(args[1], "as the content for appendFile", &content, &len, err))
        return NULL;
    if (!write_file(path, content, len, true, err))
        return NULL;
    return emv_null();
}

static EmValue *em_builtin_file_exists(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "fileExists", err))
        return NULL;
    const char *path;
    size_t len;
    if (!expect_string(args[0], "as the path for fileExists", &path, &len, err))
        return NULL;
    struct stat info;
    return emv_bool(stat(path, &info) == 0);
}

static bool delete_path(const char *path, char **err)
{
    struct stat info;
    if (lstat(path, &info) != 0) {
        *err = io_error("deleteFile", "delete", path, errno);
        return false;
    }
    if (!S_ISDIR(info.st_mode)) {
        if (unlink(path) != 0) {
            *err = io_error("deleteFile", "delete", path, errno);
            return false;
        }
        return true;
    }
    DIR *dir = opendir(path);
    if (!dir) {
        *err = io_error("deleteFile", "delete directory", path, errno);
        return false;
    }
    struct dirent *entry;
    while ((entry = readdir(dir)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0)
            continue;
        char *child = em_fmt("%s/%s", path, entry->d_name);
        bool ok = delete_path(child, err);
        free(child);
        if (!ok) {
            closedir(dir);
            return false;
        }
    }
    closedir(dir);
    if (rmdir(path) != 0) {
        *err = io_error("deleteFile", "delete directory", path, errno);
        return false;
    }
    return true;
}

static EmValue *em_builtin_delete_file(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "deleteFile", err))
        return NULL;
    const char *path;
    size_t len;
    if (!expect_string(args[0], "as the path for deleteFile", &path, &len, err))
        return NULL;
    if (!delete_path(path, err))
        return NULL;
    return emv_null();
}

static EmValue *em_builtin_list_files(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "listFiles", err))
        return NULL;
    const char *path;
    size_t len;
    if (!expect_string(args[0], "as the path for listFiles", &path, &len, err))
        return NULL;
    DIR *dir = opendir(path);
    if (!dir) {
        *err = io_error("listFiles", "read directory", path, errno);
        return NULL;
    }
    size_t cap = 16, count = 0;
    char **names = xmalloc(cap * sizeof(char *));
    for (;;) {
        errno = 0;
        struct dirent *entry = readdir(dir);
        if (!entry) {
            if (errno != 0) {
                int code = errno;
                closedir(dir);
                for (size_t i = 0; i < count; i++)
                    free(names[i]);
                free(names);
                *err = io_error("listFiles", "read entry in", path, code);
                return NULL;
            }
            break;
        }
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0)
            continue;
        if (count == cap) {
            cap *= 2;
            char **grown = xmalloc(cap * sizeof(char *));
            memcpy(grown, names, count * sizeof(char *));
            free(names);
            names = grown;
        }
        names[count++] = dup_cstr(entry->d_name);
    }
    closedir(dir);
    qsort(names, count, sizeof(char *), compare_strings);
    EmValue *out = emv_array();
    for (size_t i = 0; i < count; i++) {
        emv_array_push(out, emv_string(names[i]));
        free(names[i]);
    }
    free(names);
    return out;
}

static bool make_dir_all(const char *path, char **err)
{
    struct stat info;
    if (stat(path, &info) == 0)
        return S_ISDIR(info.st_mode);
    char *slash = strrchr(path, '/');
    if (slash && slash > path) {
        char *parent = dup_range(path, (size_t)(slash - path));
        bool ok = make_dir_all(parent, err);
        free(parent);
        if (!ok)
            return false;
    }
    if (mkdir(path, 0777) != 0) {
        if (errno == EEXIST)
            return true;
        *err = io_error("makeDir", "create", path, errno);
        return false;
    }
    return true;
}

static EmValue *em_builtin_make_dir(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "makeDir", err))
        return NULL;
    const char *path;
    size_t len;
    if (!expect_string(args[0], "as the path for makeDir", &path, &len, err))
        return NULL;
    if (!make_dir_all(path, err))
        return NULL;
    return emv_null();
}

static bool system_time(struct timespec *ts)
{
    return timespec_get(ts, TIME_UTC) == TIME_UTC;
}

static EmValue *em_builtin_clock(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    (void)args;
    if (!exact_args(argc, 0, "clock", err))
        return NULL;
    struct timespec ts;
    if (!system_time(&ts))
        return fail(err, "clock could not read system time: unknown error");
    return emv_number((double)ts.tv_sec + (double)ts.tv_nsec / 1e9);
}

static EmValue *em_builtin_timestamp(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    (void)args;
    if (!exact_args(argc, 0, "timestamp", err))
        return NULL;
    struct timespec ts;
    if (!system_time(&ts))
        return fail(err, "timestamp could not read system time: unknown error");
    return emv_number((double)ts.tv_sec * 1000.0 + (double)ts.tv_nsec / 1e6);
}

static EmValue *em_builtin_env(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "env", err))
        return NULL;
    const char *key;
    size_t len;
    if (!expect_string(args[0], "as the variable name for env", &key, &len, err))
        return NULL;
    const char *value = getenv(key);
    return value ? emv_string(value) : emv_null();
}

static EmValue *em_builtin_sleep(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    if (!exact_args(argc, 1, "sleep", err))
        return NULL;
    double ms;
    if (!expect_number(args[0], "as milliseconds for sleep", &ms, err))
        return NULL;
    if (!isfinite(ms) || ms < 0.0)
        return fail(err, "sleep expects a non-negative finite number of milliseconds");
    struct timespec request;
    request.tv_sec = (time_t)(ms / 1000.0);
    request.tv_nsec = (long)((ms - (double)request.tv_sec * 1000.0) * 1000000.0);
    while (nanosleep(&request, &request) != 0) {
        if (errno != EINTR)
            return fail(err, "sleep could not wait: %s", strerror(errno));
    }
    return emv_null();
}

/* ------------------------------------------------------------------ */
/* Registry                                                             */
/* ------------------------------------------------------------------ */

typedef struct {
    const char *name;
    EmNativeFn fn;
} BuiltinEntry;

static const BuiltinEntry kBuiltins[] = {
    {"say", em_builtin_say},
    {"print", em_builtin_print},
    {"len", em_builtin_len},
    {"type", em_builtin_type},
    {"toString", em_builtin_to_string},
    {"toNumber", em_builtin_to_number},
    {"toBool", em_builtin_to_bool},
    {"isNumber", em_builtin_isNumber},
    {"isString", em_builtin_isString},
    {"isArray", em_builtin_isArray},
    {"isObject", em_builtin_isObject},
    {"isBool", em_builtin_isBool},
    {"isNull", em_builtin_isNull},
    {"isFunction", em_builtin_is_function},
    {"isNaN", em_builtin_is_nan},
    {"parseInt", em_builtin_parse_int},
    {"parseFloat", em_builtin_parse_float},
    {"range", em_builtin_range},
    {"sum", em_builtin_sum},
    {"average", em_builtin_average},
    {"min", em_builtin_min},
    {"max", em_builtin_max},
    {"abs", em_builtin_abs},
    {"clamp", em_builtin_clamp},
    {"sorted", em_builtin_sorted},
    {"reversed", em_builtin_reversed},
    {"zip", em_builtin_zip},
    {"take", em_builtin_take},
    {"drop", em_builtin_drop},
    {"keys", em_builtin_keys},
    {"values", em_builtin_values},
    {"entries", em_builtin_entries},
    {"fromEntries", em_builtin_from_entries},
    {"first", em_builtin_first},
    {"last", em_builtin_last},
    {"indexOf", em_builtin_index_of},
    {"contains", em_builtin_contains},
    {"slice", em_builtin_slice},
    {"pow", em_builtin_pow},
    {"sqrt", em_builtin_sqrt},
    {"floor", em_builtin_floor},
    {"ceil", em_builtin_ceil},
    {"round", em_builtin_round},
    {"sign", em_builtin_sign},
    {"median", em_builtin_median},
    {"log2", em_builtin_log2},
    {"log10", em_builtin_log10},
    {"toDegrees", em_builtin_toDegrees},
    {"toRadians", em_builtin_toRadians},
    {"random", em_builtin_random},
    {"randomInt", em_builtin_random_int},
    {"seed", em_builtin_seed},
    {"randomChoice", em_builtin_random_choice},
    {"shuffle", em_builtin_shuffle},
    {"input", em_builtin_input},
    {"format", em_builtin_format},
    {"trim", em_builtin_trim},
    {"upper", em_builtin_upper},
    {"lower", em_builtin_lower},
    {"split", em_builtin_split},
    {"join", em_builtin_join},
    {"replace", em_builtin_replace},
    {"startsWith", em_builtin_startsWith},
    {"endsWith", em_builtin_endsWith},
    {"repeat", em_builtin_repeat},
    {"padStart", em_builtin_pad_start},
    {"padEnd", em_builtin_pad_end},
    {"charCode", em_builtin_char_code},
    {"fromCharCode", em_builtin_from_char_code},
    {"readFile", em_builtin_read_file},
    {"writeFile", em_builtin_write_file},
    {"appendFile", em_builtin_append_file},
    {"fileExists", em_builtin_file_exists},
    {"deleteFile", em_builtin_delete_file},
    {"listFiles", em_builtin_list_files},
    {"makeDir", em_builtin_make_dir},
    {"clock", em_builtin_clock},
    {"timestamp", em_builtin_timestamp},
    {"env", em_builtin_env},
    {"args", em_builtin_args},
    {"sleep", em_builtin_sleep},
    {"assert", em_builtin_assert},
    {"jsonEncode", em_builtin_json_encode},
    {"jsonDecode", em_builtin_json_decode},
};

static const size_t kBuiltinCount = sizeof(kBuiltins) / sizeof(kBuiltins[0]);

static const BuiltinEntry *find_builtin(const char *name)
{
    if (!name)
        return NULL;
    for (size_t i = 0; i < kBuiltinCount; i++) {
        if (strcmp(kBuiltins[i].name, name) == 0)
            return &kBuiltins[i];
    }
    return NULL;
}

int em_builtins_register(EmVm *vm, char **err)
{
    for (size_t i = 0; i < kBuiltinCount; i++) {
        if (em_vm_register_native(vm, kBuiltins[i].name, kBuiltins[i].fn) != 0) {
            if (err)
                *err = em_fmt("em_builtins_register: could not register '%s'",
                              kBuiltins[i].name);
            return -1;
        }
    }
    return 0;
}

bool em_builtin_exists(const char *name)
{
    return find_builtin(name) != NULL;
}

int em_builtin_call(EmVm *vm, const char *name, EmValue **args, size_t argc,
                    EmValue **out, char **err)
{
    const BuiltinEntry *entry = find_builtin(name);
    if (!entry) {
        fail(err, "unknown builtin '%s'", name ? name : "(null)");
        return -1;
    }
    char *local_err = NULL;
    char **target = err ? err : &local_err;
    EmValue *result = entry->fn(vm, args, argc, target);
    if (!result) {
        if (!*target)
            fail(target, "builtin '%s' failed", entry->name);
        if (!err)
            free(local_err);
        return -1;
    }
    if (out)
        *out = result;
    else
        emv_release(result);
    if (!err)
        free(local_err);
    return 0;
}







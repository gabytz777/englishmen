/*
 * Runtime method dispatch for the C bootstrap VM.
 *
 * The semantics here follow src/runtime.rs (the native Englishmen VM), which
 * is the oracle for the round-4 bootstrap: string indices are Unicode scalar
 * values, negative indices count from the end, slice-like methods clamp and
 * never mutate their receiver, and argument type errors are reported through
 * the `err` out-parameter.
 */

#include "vm.h"

#include <float.h>
#include <limits.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ------------------------------------------------------------------ */
/* Small utilities                                                     */
/* ------------------------------------------------------------------ */

static char *em_strdup(const char *text)
{
    size_t len = strlen(text) + 1;
    char *copy = (char *)malloc(len);
    if (copy != NULL) {
        memcpy(copy, text, len);
    }
    return copy;
}

static int em_fail(char **err, const char *message)
{
    if (err != NULL) {
        char *copy = em_strdup(message);
        if (copy != NULL) {
            free(*err);
            *err = copy;
        }
    }
    return -1;
}

static int em_fail_fmt(char **err, const char *format, ...)
{
    char message[512];
    va_list args;
    va_start(args, format);
    vsnprintf(message, sizeof(message), format, args);
    va_end(args);
    return em_fail(err, message);
}

static const char *em_type_name(EmValueType type)
{
    switch (type) {
    case EMV_NUMBER: return "number";
    case EMV_STRING: return "string";
    case EMV_BOOL: return "boolean";
    case EMV_NULL: return "null";
    case EMV_ARRAY: return "array";
    case EMV_OBJECT: return "object";
    case EMV_CLOSURE: return "function";
    case EMV_NATIVE: return "function";
    }
    return "unknown";
}

static int em_no_method(char **err, const char *kind, const char *method)
{
    return em_fail_fmt(err, "%s has no method '%s'", kind, method);
}

/* ------------------------------------------------------------------ */
/* Floating point helpers (no libm dependency)                         */
/* ------------------------------------------------------------------ */

static bool em_finite(double n)
{
    return n == n && n < DBL_MAX && n > -DBL_MAX;
}

static double em_floor(double n)
{
    if (!em_finite(n) || n >= 9007199254740992.0 || n <= -9007199254740992.0) {
        return n;
    }
    double truncated = (double)(long long)n;
    if (truncated > n) {
        truncated -= 1.0;
    }
    return truncated;
}

static double em_ceil(double n)
{
    return -em_floor(-n);
}

static double em_trunc(double n)
{
    if (!em_finite(n) || n >= 9007199254740992.0 || n <= -9007199254740992.0) {
        return n;
    }
    return (double)(long long)n;
}

static double em_abs(double n)
{
    return n < 0.0 ? -n : n;
}

static double em_round(double n)
{
    if (n >= 0.0) {
        return em_floor(n + 0.5);
    }
    return em_ceil(n - 0.5);
}

static double em_negative(double n)
{
    /* signbit() is a macro in math.h but we do not want to pull in libm. */
    return n == 0.0 && 1.0 / n < 0.0;
}

/* Rust's clamp_index(): negative values count from the end and out-of-range
 * values are clamped. Fractional values are floored, like an f64->usize cast. */
static size_t em_clamp_index(size_t len, double n)
{
    if (!em_finite(n)) {
        return em_negative(n) ? 0 : len;
    }
    if (n < 0.0) {
        double back = em_floor(-n);
        if (back >= (double)len) {
            return 0;
        }
        return len - (size_t)back;
    }
    double forward = em_floor(n);
    if (forward >= (double)len) {
        return len;
    }
    return (size_t)forward;
}

/* Rust's exact_index(): whole numbers only; negative values count from the
 * end; anything out of range addresses no element. */
static bool em_exact_index(size_t len, double n, size_t *out)
{
    if (!em_finite(n) || em_trunc(n) != n) {
        return false;
    }
    if (n < 0.0) {
        double back = -n;
        if (back > (double)len) {
            return false;
        }
        *out = len - (size_t)back;
        return true;
    }
    if (n >= (double)len) {
        return false;
    }
    *out = (size_t)n;
    return true;
}

static void em_slice_range(size_t len, double start, double end,
                           size_t *from, size_t *to)
{
    *from = em_clamp_index(len, start);
    *to = em_clamp_index(len, end);
    if (*to < *from) {
        *to = *from;
    }
}

/* ------------------------------------------------------------------ */
/* UTF-8 helpers                                                       */
/* ------------------------------------------------------------------ */

static uint32_t em_utf8_decode(const unsigned char *data, size_t len, size_t *pos)
{
    unsigned char first = data[*pos];
    if (first < 0x80) {
        (*pos)++;
        return first;
    }

    size_t extra;
    uint32_t cp;
    uint32_t minimum;
    if ((first & 0xE0) == 0xC0) {
        extra = 1;
        cp = first & 0x1Fu;
        minimum = 0x80;
    } else if ((first & 0xF0) == 0xE0) {
        extra = 2;
        cp = first & 0x0Fu;
        minimum = 0x800;
    } else if ((first & 0xF8) == 0xF0) {
        extra = 3;
        cp = first & 0x07u;
        minimum = 0x10000;
    } else {
        (*pos)++;
        return 0xFFFD;
    }
    if (*pos + extra >= len) {
        (*pos)++;
        return 0xFFFD;
    }
    for (size_t i = 1; i <= extra; i++) {
        unsigned char next = data[*pos + i];
        if ((next & 0xC0) != 0x80) {
            (*pos)++;
            return 0xFFFD;
        }
        cp = (cp << 6) | (uint32_t)(next & 0x3Fu);
    }
    if (cp < minimum || cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) {
        (*pos)++;
        return 0xFFFD;
    }
    *pos += extra + 1;
    return cp;
}

static size_t em_utf8_encode(uint32_t cp, char *out)
{
    if (cp > 0x10FFFF || (cp >= 0xD800 && cp <= 0xDFFF)) {
        cp = 0xFFFD;
    }
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

/* Offsets[i] is the byte position of character i; offsets[count] is the end. */
static size_t *em_utf8_offsets(const char *data, size_t len, size_t *count)
{
    size_t *offsets = (size_t *)malloc((len + 2) * sizeof(size_t));
    if (offsets == NULL) {
        return NULL;
    }
    size_t pos = 0;
    size_t items = 0;
    offsets[items++] = 0;
    while (pos < len) {
        em_utf8_decode((const unsigned char *)data, len, &pos);
        offsets[items++] = pos;
    }
    *count = items - 1;
    return offsets;
}

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} EmBuf;

static bool em_buf_reserve(EmBuf *buf, size_t extra)
{
    if (extra > (size_t)-1 - buf->len) {
        return false;
    }
    size_t needed = buf->len + extra;
    if (needed <= buf->cap) {
        return true;
    }
    size_t cap = buf->cap == 0 ? 32 : buf->cap;
    while (cap < needed) {
        if (cap > (size_t)-1 / 2) {
            cap = needed;
            break;
        }
        cap *= 2;
    }
    char *data = (char *)realloc(buf->data, cap);
    if (data == NULL) {
        return false;
    }
    buf->data = data;
    buf->cap = cap;
    return true;
}

static bool em_buf_append(EmBuf *buf, const char *text, size_t len)
{
    if (!em_buf_reserve(buf, len)) {
        return false;
    }
    if (len > 0) {
        memcpy(buf->data + buf->len, text, len);
        buf->len += len;
    }
    return true;
}

static bool em_buf_append_cp(EmBuf *buf, uint32_t cp)
{
    char encoded[4];
    size_t len = em_utf8_encode(cp, encoded);
    return em_buf_append(buf, encoded, len);
}

static EmValue *em_buf_string(EmBuf *buf)
{
    EmValue *value = emv_string_len(buf->data == NULL ? "" : buf->data, buf->len);
    free(buf->data);
    buf->data = NULL;
    buf->len = 0;
    buf->cap = 0;
    return value;
}

static void em_buf_clear(EmBuf *buf)
{
    free(buf->data);
    buf->data = NULL;
    buf->len = 0;
    buf->cap = 0;
}

static bool em_is_whitespace(uint32_t cp)
{
    switch (cp) {
    case 0x09: case 0x0A: case 0x0B: case 0x0C: case 0x0D: case 0x20:
    case 0x85: case 0xA0: case 0x1680:
    case 0x2028: case 0x2029: case 0x202F: case 0x205F: case 0x3000:
        return true;
    default:
        return cp >= 0x2000 && cp <= 0x200A;
    }
}

static bool em_is_digit(uint32_t cp)
{
    if (cp >= '0' && cp <= '9') {
        return true;
    }
    /* Common Unicode decimal-digit blocks (Rust's char::is_numeric covers
     * more, but identifiers and compiler text are overwhelmingly ASCII). */
    static const uint32_t blocks[] = {
        0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6,
        0x0B66, 0x0BE6, 0x0C66, 0x0CE6, 0x0D66, 0x0DE6, 0x0E50,
        0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0xFF10
    };
    for (size_t i = 0; i < sizeof(blocks) / sizeof(blocks[0]); i++) {
        if (cp >= blocks[i] && cp <= blocks[i] + 9) {
            return true;
        }
    }
    return false;
}

static bool em_is_alpha(uint32_t cp)
{
    if ((cp >= 'A' && cp <= 'Z') || (cp >= 'a' && cp <= 'z')) {
        return true;
    }
    if ((cp >= 0xC0 && cp <= 0xD6) || (cp >= 0xD8 && cp <= 0xF6) ||
        (cp >= 0xF8 && cp <= 0x2FF)) {
        return true;
    }
    if (cp >= 0x370 && cp <= 0x52F) {
        return true;
    }
    if (cp >= 0x531 && cp <= 0x58F) {
        return true;
    }
    if (cp >= 0x5D0 && cp <= 0x5EA) {
        return true;
    }
    if (cp >= 0x620 && cp <= 0x64A) {
        return true;
    }
    if (cp >= 0x900 && cp <= 0xD7F) {
        return true;
    }
    if (cp >= 0x1E00 && cp <= 0x1FFF) {
        return true;
    }
    if (cp >= 0x3041 && cp <= 0x30FF) {
        return true;
    }
    if (cp >= 0x4E00 && cp <= 0x9FFF) {
        return true;
    }
    if (cp >= 0xFF21 && cp <= 0xFF3A) {
        return true;
    }
    if (cp >= 0xFF41 && cp <= 0xFF5A) {
        return true;
    }
    return false;
}

static void em_append_upper(EmBuf *buf, uint32_t cp)
{
    if (cp >= 'a' && cp <= 'z') {
        cp -= 32;
    } else if (cp == 0xDF) {
        em_buf_append(buf, "SS", 2);
        return;
    } else if (cp >= 0xE0 && cp <= 0xFE && cp != 0xF7) {
        cp -= 32;
    } else if (cp == 0xFF) {
        cp = 0x178;
    } else if (cp >= 0x3B1 && cp <= 0x3CB && cp != 0x3C2) {
        cp -= 32;
    } else if (cp >= 0x430 && cp <= 0x44F) {
        cp -= 32;
    } else if (cp >= 0x450 && cp <= 0x45F) {
        cp -= 80;
    }
    em_buf_append_cp(buf, cp);
}

static void em_append_lower(EmBuf *buf, uint32_t cp)
{
    if (cp >= 'A' && cp <= 'Z') {
        cp += 32;
    } else if (cp >= 0xC0 && cp <= 0xDE && cp != 0xD7) {
        cp += 32;
    } else if (cp == 0x178) {
        cp = 0xFF;
    } else if (cp >= 0x391 && cp <= 0x3AB && cp != 0x3A2) {
        cp += 32;
    } else if (cp >= 0x410 && cp <= 0x42F) {
        cp += 32;
    } else if (cp >= 0x400 && cp <= 0x40F) {
        cp += 80;
    }
    em_buf_append_cp(buf, cp);
}

static const char *em_mem_find(const char *hay, size_t hay_len,
                               const char *needle, size_t needle_len)
{
    if (needle_len == 0) {
        return hay;
    }
    if (needle_len > hay_len) {
        return NULL;
    }
    for (size_t i = 0; i + needle_len <= hay_len; i++) {
        if (memcmp(hay + i, needle, needle_len) == 0) {
            return hay + i;
        }
    }
    return NULL;
}

static const char *em_mem_rfind(const char *hay, size_t hay_len,
                                const char *needle, size_t needle_len)
{
    if (needle_len == 0) {
        return hay + hay_len;
    }
    if (needle_len > hay_len) {
        return NULL;
    }
    for (size_t i = hay_len - needle_len + 1; i > 0; i--) {
        size_t at = i - 1;
        if (memcmp(hay + at, needle, needle_len) == 0) {
            return hay + at;
        }
    }
    return NULL;
}

/* Byte offset -> character offset, as Rust's str::find() reports char counts. */
static size_t em_char_offset(const char *data, size_t byte_offset)
{
    size_t pos = 0;
    size_t chars = 0;
    while (pos < byte_offset) {
        em_utf8_decode((const unsigned char *)data, byte_offset, &pos);
        chars++;
    }
    return chars;
}

/* ------------------------------------------------------------------ */
/* Argument helpers                                                    */
/* ------------------------------------------------------------------ */

static bool em_arg_number(EmValue **args, size_t argc, size_t index, double *out)
{
    if (index >= argc || !emv_is_number(args[index])) {
        return false;
    }
    *out = emv_as_number(args[index]);
    return true;
}

static bool em_arg_string(EmValue **args, size_t argc, size_t index,
                          const char **data, size_t *len)
{
    if (index >= argc || !emv_is_string(args[index])) {
        return false;
    }
    *data = emv_as_cstr(args[index]);
    *len = emv_strlen(args[index]);
    return true;
}

static int em_bad_argument(char **err, const char *method, const char *context,
                           const char *wanted, EmValue **args, size_t argc,
                           size_t index)
{
    if (index >= argc) {
        return em_fail_fmt(err, "%s() expects argument %lu (%s)",
                           method, (unsigned long)(index + 1), context);
    }
    return em_fail_fmt(err, "%s(): expected %s %s, found %s", method,
                       wanted, context, em_type_name(emv_type(args[index])));
}

static int em_number_arg(char **err, const char *method, EmValue **args,
                         size_t argc, size_t index, const char *context,
                         double *out)
{
    if (!em_arg_number(args, argc, index, out)) {
        return em_bad_argument(err, method, context, "a number", args, argc, index);
    }
    return 0;
}

static int em_string_arg(char **err, const char *method, EmValue **args,
                         size_t argc, size_t index, const char *context,
                         const char **data, size_t *len)
{
    if (!em_arg_string(args, argc, index, data, len)) {
        return em_bad_argument(err, method, context, "a string", args, argc, index);
    }
    return 0;
}

static int em_opt_number_arg(char **err, const char *method, EmValue **args,
                             size_t argc, size_t index, const char *context,
                             double fallback, double *out)
{
    if (index >= argc) {
        *out = fallback;
        return 0;
    }
    return em_number_arg(err, method, args, argc, index, context, out);
}

static int em_opt_string_arg(char **err, const char *method, EmValue **args,
                             size_t argc, size_t index, const char *context,
                             const char *fallback, const char **data,
                             size_t *len)
{
    if (index >= argc) {
        *data = fallback;
        *len = strlen(fallback);
        return 0;
    }
    return em_string_arg(err, method, args, argc, index, context, data, len);
}

/* ------------------------------------------------------------------ */
/* String methods                                                      */
/* ------------------------------------------------------------------ */

static EmValue *em_chars_array(EmValue *receiver)
{
    const char *data = emv_as_cstr(receiver);
    size_t byte_len = emv_strlen(receiver);
    size_t count = 0;
    size_t *offsets = em_utf8_offsets(data, byte_len, &count);
    EmValue *result = emv_array();
    if (offsets == NULL || result == NULL) {
        free(offsets);
        if (result != NULL) {
            emv_release(result);
        }
        return emv_array();
    }
    for (size_t i = 0; i < count; i++) {
        EmValue *item = emv_string_len(data + offsets[i], offsets[i + 1] - offsets[i]);
        emv_array_push(result, item);
        emv_release(item);
    }
    free(offsets);
    return result;
}

/* Append `chars` codepoints by cycling through the padding unit, then
 * truncating. This matches Rust's "repeat until long enough, take n" rule. */
static void em_append_padding(EmBuf *buf, const char *unit, size_t unit_len,
                              size_t chars)
{
    size_t unit_chars = 0;
    size_t *offsets = em_utf8_offsets(unit, unit_len, &unit_chars);
    if (offsets == NULL || unit_chars == 0) {
        free(offsets);
        return;
    }
    for (size_t i = 0; i < chars; i++) {
        size_t at = i % unit_chars;
        em_buf_append(buf, unit + offsets[at], offsets[at + 1] - offsets[at]);
    }
    free(offsets);
}

static EmValue *em_string_map_case(EmValue *receiver, bool upper)
{
    const char *data = emv_as_cstr(receiver);
    size_t len = emv_strlen(receiver);
    size_t pos = 0;
    EmBuf buf = {0};
    while (pos < len) {
        uint32_t cp = em_utf8_decode((const unsigned char *)data, len, &pos);
        if (upper) {
            em_append_upper(&buf, cp);
        } else {
            em_append_lower(&buf, cp);
        }
    }
    return em_buf_string(&buf);
}

static int em_string_method(EmVm *vm, EmValue *receiver, const char *method,
                            EmValue **args, size_t argc, EmValue **out,
                            char **err)
{
    (void)vm;
    const char *data = emv_as_cstr(receiver);
    size_t byte_len = emv_strlen(receiver);
    size_t count = 0;
    size_t *offsets = em_utf8_offsets(data, byte_len, &count);
    if (offsets == NULL) {
        return em_fail(err, "out of memory");
    }

    int status = 0;

    if (strcmp(method, "length") == 0) {
        *out = emv_number((double)count);
    } else if (strcmp(method, "upper") == 0) {
        *out = em_string_map_case(receiver, true);
    } else if (strcmp(method, "lower") == 0) {
        *out = em_string_map_case(receiver, false);
    } else if (strcmp(method, "trim") == 0) {
        size_t first = 0;
        size_t last = count;
        while (first < last) {
            size_t pos = offsets[first];
            uint32_t cp = em_utf8_decode((const unsigned char *)data, offsets[first + 1], &pos);
            if (!em_is_whitespace(cp)) {
                break;
            }
            first++;
        }
        while (last > first) {
            size_t pos = offsets[last - 1];
            uint32_t cp = em_utf8_decode((const unsigned char *)data, offsets[last], &pos);
            if (!em_is_whitespace(cp)) {
                break;
            }
            last--;
        }
        *out = emv_string_len(data + offsets[first], offsets[last] - offsets[first]);
    } else if (strcmp(method, "chars") == 0) {
        *out = em_chars_array(receiver);
    } else if (strcmp(method, "split") == 0) {
        const char *sep = "";
        size_t sep_len = 0;
        if (argc > 0 && !emv_is_string(args[0])) {
            /* Native runtime::call_string_method falls back to "" on a bad
             * separator, which splits into characters. */
            sep_len = 0;
        } else if (argc > 0) {
            sep = emv_as_cstr(args[0]);
            sep_len = emv_strlen(args[0]);
        }
        EmValue *result = emv_array();
        if (sep_len == 0) {
            for (size_t i = 0; i < count; i++) {
                EmValue *item = emv_string_len(data + offsets[i],
                                               offsets[i + 1] - offsets[i]);
                emv_array_push(result, item);
                emv_release(item);
            }
        } else {
            size_t start = 0;
            while (start <= byte_len) {
                const char *found = em_mem_find(data + start, byte_len - start,
                                                sep, sep_len);
                if (found == NULL) {
                    EmValue *item = emv_string_len(data + start, byte_len - start);
                    emv_array_push(result, item);
                    emv_release(item);
                    break;
                }
                size_t end = (size_t)(found - data);
                EmValue *item = emv_string_len(data + start, end - start);
                emv_array_push(result, item);
                emv_release(item);
                start = end + sep_len;
            }
        }
        *out = result;
    } else if (strcmp(method, "contains") == 0) {
        const char *needle = "";
        size_t needle_len = 0;
        if (argc > 0 && emv_is_string(args[0])) {
            needle = emv_as_cstr(args[0]);
            needle_len = emv_strlen(args[0]);
        }
        *out = emv_bool(em_mem_find(data, byte_len, needle, needle_len) != NULL);
    } else if (strcmp(method, "startsWith") == 0 || strcmp(method, "endsWith") == 0) {
        const char *affix = NULL;
        size_t affix_len = 0;
        status = em_string_arg(err, method, args, argc, 0,
                               strcmp(method, "startsWith") == 0 ? "as a prefix"
                                                                 : "as a suffix",
                               &affix, &affix_len);
        if (status == 0) {
            bool found = strcmp(method, "startsWith") == 0
                             ? (affix_len <= byte_len &&
                                memcmp(data, affix, affix_len) == 0)
                             : (affix_len <= byte_len &&
                                memcmp(data + byte_len - affix_len, affix,
                                       affix_len) == 0);
            *out = emv_bool(found);
        }
    } else if (strcmp(method, "replace") == 0 || strcmp(method, "replaceAll") == 0) {
        const char *old_text = NULL;
        size_t old_len = 0;
        const char *new_text = NULL;
        size_t new_len = 0;
        status = em_string_arg(err, method, args, argc, 0, "to replace",
                               &old_text, &old_len);
        if (status == 0) {
            status = em_string_arg(err, method, args, argc, 1,
                                   "as a replacement", &new_text, &new_len);
        }
        if (status == 0) {
            EmBuf buf = {0};
            if (old_len == 0) {
                if (strcmp(method, "replace") == 0) {
                    em_buf_append(&buf, new_text, new_len);
                    em_buf_append(&buf, data, byte_len);
                } else {
                    for (size_t i = 0; i < count; i++) {
                        em_buf_append(&buf, new_text, new_len);
                        em_buf_append(&buf, data + offsets[i],
                                      offsets[i + 1] - offsets[i]);
                    }
                    em_buf_append(&buf, new_text, new_len);
                }
            } else {
                size_t start = 0;
                bool keep_going = true;
                while (keep_going) {
                    const char *found = em_mem_find(data + start, byte_len - start,
                                                    old_text, old_len);
                    if (found == NULL) {
                        em_buf_append(&buf, data + start, byte_len - start);
                        break;
                    }
                    size_t at = (size_t)(found - data);
                    em_buf_append(&buf, data + start, at - start);
                    em_buf_append(&buf, new_text, new_len);
                    start = at + old_len;
                    if (strcmp(method, "replace") == 0) {
                        em_buf_append(&buf, data + start, byte_len - start);
                        break;
                    }
                    keep_going = true;
                }
            }
            *out = em_buf_string(&buf);
            em_buf_clear(&buf);
        }
    } else if (strcmp(method, "slice") == 0 || strcmp(method, "substring") == 0) {
        double start = 0.0;
        double end = (double)count;
        status = em_opt_number_arg(err, method, args, argc, 0,
                                   "as a start index", 0.0, &start);
        if (status == 0) {
            status = em_opt_number_arg(err, method, args, argc, 1,
                                       "as an end index", (double)count, &end);
        }
        if (status == 0) {
            size_t from = em_clamp_index(count, start);
            size_t to = em_clamp_index(count, end);
            if (strcmp(method, "slice") == 0) {
                /* slice() keeps the bounds ordered; substring() swaps them. */
                if (to < from) {
                    to = from;
                }
            } else if (to < from) {
                size_t swap = from;
                from = to;
                to = swap;
            }
            *out = emv_string_len(data + offsets[from], offsets[to] - offsets[from]);
        }
    } else if (strcmp(method, "indexOf") == 0 || strcmp(method, "lastIndexOf") == 0) {
        const char *needle = NULL;
        size_t needle_len = 0;
        status = em_string_arg(err, method, args, argc, 0, "to search for",
                               &needle, &needle_len);
        if (status == 0) {
            const char *found = strcmp(method, "indexOf") == 0
                                    ? em_mem_find(data, byte_len, needle, needle_len)
                                    : em_mem_rfind(data, byte_len, needle, needle_len);
            if (found == NULL) {
                *out = emv_number(-1.0);
            } else {
                size_t byte_at = needle_len == 0 && strcmp(method, "lastIndexOf") == 0
                                     ? byte_len
                                     : (size_t)(found - data);
                *out = emv_number((double)em_char_offset(data, byte_at));
            }
        }
    } else if (strcmp(method, "charAt") == 0) {
        double n = 0.0;
        status = em_number_arg(err, method, args, argc, 0,
                               "as a character index", &n);
        if (status == 0) {
            size_t at = 0;
            if (em_exact_index(count, n, &at)) {
                *out = emv_string_len(data + offsets[at], offsets[at + 1] - offsets[at]);
            } else {
                *out = emv_string_len("", 0);
            }
        }
    } else if (strcmp(method, "toNumber") == 0) {
        size_t first = 0;
        size_t last = count;
        while (first < last) {
            size_t pos = offsets[first];
            uint32_t cp = em_utf8_decode((const unsigned char *)data,
                                         offsets[first + 1], &pos);
            if (!em_is_whitespace(cp)) {
                break;
            }
            first++;
        }
        while (last > first) {
            size_t pos = offsets[last - 1];
            uint32_t cp = em_utf8_decode((const unsigned char *)data,
                                         offsets[last], &pos);
            if (!em_is_whitespace(cp)) {
                break;
            }
            last--;
        }
        const char *text = data + offsets[first];
        size_t text_len = offsets[last] - offsets[first];
        bool valid = text_len > 0;
        for (size_t i = 0; i < text_len && valid; i++) {
            char c = text[i];
            if (c == 'x' || c == 'X' || c == 'p' || c == 'P') {
                valid = false;
            }
        }
        if (valid) {
            char *copy = (char *)malloc(text_len + 1);
            if (copy == NULL) {
                status = em_fail(err, "out of memory");
            } else {
                memcpy(copy, text, text_len);
                copy[text_len] = '\0';
                char *end = NULL;
                double value = strtod(copy, &end);
                if (end == copy || *end != '\0' || !em_finite(value)) {
                    *out = emv_null();
                } else {
                    *out = emv_number(value);
                }
                free(copy);
            }
        } else {
            *out = emv_null();
        }
    } else if (strcmp(method, "capitalize") == 0) {
        EmBuf buf = {0};
        if (count > 0) {
            size_t pos = offsets[0];
            uint32_t cp = em_utf8_decode((const unsigned char *)data,
                                         offsets[1], &pos);
            em_append_upper(&buf, cp);
            em_buf_append(&buf, data + offsets[1], byte_len - offsets[1]);
        }
        *out = em_buf_string(&buf);
        em_buf_clear(&buf);
    } else if (strcmp(method, "titleCase") == 0) {
        EmBuf buf = {0};
        bool word_start = true;
        for (size_t i = 0; i < count; i++) {
            size_t pos = offsets[i];
            uint32_t cp = em_utf8_decode((const unsigned char *)data,
                                         offsets[i + 1], &pos);
            if (em_is_whitespace(cp)) {
                em_buf_append_cp(&buf, cp);
                word_start = true;
            } else if (word_start) {
                em_append_upper(&buf, cp);
                word_start = false;
            } else {
                em_append_lower(&buf, cp);
            }
        }
        *out = em_buf_string(&buf);
        em_buf_clear(&buf);
    } else if (strcmp(method, "reverse") == 0) {
        EmBuf buf = {0};
        for (size_t i = count; i > 0; i--) {
            em_buf_append(&buf, data + offsets[i - 1], offsets[i] - offsets[i - 1]);
        }
        *out = em_buf_string(&buf);
        em_buf_clear(&buf);
    } else if (strcmp(method, "repeat") == 0) {
        double n = 0.0;
        status = em_number_arg(err, method, args, argc, 0,
                               "as a repeat count", &n);
        if (status == 0) {
            if (n < 0.0 || n == 0.0) {
                *out = emv_string_len("", 0);
            } else if (!em_finite(n) || em_trunc(n) != n) {
                status = em_fail(err, "repeat() expects a whole number of repetitions");
            } else if (n > 1000000.0 || count > 1000000.0 / n) {
                status = em_fail(err, "repeat() result would be too large");
            } else {
                EmBuf buf = {0};
                for (double i = 0; i < n; i += 1.0) {
                    em_buf_append(&buf, data, byte_len);
                }
                *out = em_buf_string(&buf);
                em_buf_clear(&buf);
            }
        }
    } else if (strcmp(method, "padLeft") == 0 || strcmp(method, "padRight") == 0 ||
               strcmp(method, "pad") == 0) {
        double width = 0.0;
        status = em_number_arg(err, method, args, argc, 0,
                               "as a target width", &width);
        if (status == 0) {
            if (!em_finite(width) || width < 0.0 || em_trunc(width) != width) {
                status = em_fail_fmt(err,
                                     "%s() expects a non-negative whole width", method);
            } else {
                size_t target = width > 100000.0 ? 100000 : (size_t)width;
                const char *unit_data = NULL;
                size_t unit_len = 0;
                status = em_opt_string_arg(err, method, args, argc, 1,
                                           "as padding", " ", &unit_data,
                                           &unit_len);
                if (status != 0) {
                    /* Argument error was already reported. */
                } else if (unit_len == 0) {
                    unit_data = " ";
                    unit_len = 1;
                }
                if (status == 0) {
                    if (count >= target) {
                        *out = emv_string_len(data, byte_len);
                    } else {
                        size_t missing = target - count;
                        size_t left = 0;
                        size_t right = 0;
                        if (strcmp(method, "padLeft") == 0) {
                            left = missing;
                        } else if (strcmp(method, "padRight") == 0) {
                            right = missing;
                        } else {
                            /* pad(width, fill, alignment) mirrors the pure
                             * string.pad module: left/right/center/both, with a
                             * single fill character and the extra on the left. */
                            const char *alignment = "left";
                            size_t alignment_len = 4;
                            if (argc > 2 && emv_is_string(args[2])) {
                                alignment = emv_as_cstr(args[2]);
                                alignment_len = emv_strlen(args[2]);
                            }
                            size_t pos = 0;
                            uint32_t cp = em_utf8_decode(
                                (const unsigned char *)unit_data, unit_len, &pos);
                            char first[4];
                            unit_len = em_utf8_encode(cp, first);
                            unit_data = first;
                            if (alignment_len == 5 &&
                                memcmp(alignment, "right", 5) == 0) {
                                left = missing;
                            } else if ((alignment_len == 6 &&
                                        memcmp(alignment, "center", 6) == 0) ||
                                       (alignment_len == 4 &&
                                        memcmp(alignment, "both", 4) == 0)) {
                                left = (missing + 1) / 2;
                                right = missing - left;
                            } else {
                                right = missing;
                            }
                        }
                        EmBuf result = {0};
                        em_append_padding(&result, unit_data, unit_len, left);
                        em_buf_append(&result, data, byte_len);
                        em_append_padding(&result, unit_data, unit_len, right);
                        *out = em_buf_string(&result);
                        em_buf_clear(&result);
                    }
                }
            }
        }
    } else if (strcmp(method, "isDigit") == 0) {
        bool result = count > 0;
        for (size_t i = 0; i < count && result; i++) {
            size_t pos = offsets[i];
            uint32_t cp = em_utf8_decode((const unsigned char *)data,
                                         offsets[i + 1], &pos);
            result = em_is_digit(cp);
        }
        *out = emv_bool(result);
    } else if (strcmp(method, "isAlpha") == 0) {
        bool result = count > 0;
        for (size_t i = 0; i < count && result; i++) {
            size_t pos = offsets[i];
            uint32_t cp = em_utf8_decode((const unsigned char *)data,
                                         offsets[i + 1], &pos);
            result = em_is_alpha(cp);
        }
        *out = emv_bool(result);
    } else if (strcmp(method, "isBlank") == 0) {
        bool result = true;
        for (size_t i = 0; i < count && result; i++) {
            size_t pos = offsets[i];
            uint32_t cp = em_utf8_decode((const unsigned char *)data,
                                         offsets[i + 1], &pos);
            result = em_is_whitespace(cp);
        }
        *out = emv_bool(result);
    } else if (strcmp(method, "lines") == 0) {
        EmValue *result = emv_array();
        size_t start = 0;
        while (start < byte_len) {
            const char *newline = memchr(data + start, '\n', byte_len - start);
            size_t end = newline == NULL ? byte_len : (size_t)(newline - data);
            size_t stop = end;
            /* A bare '\r' is not a line ending; only strip it from CRLF. */
            if (newline != NULL && stop > start && data[stop - 1] == '\r') {
                stop--;
            }
            EmValue *line = emv_string_len(data + start, stop - start);
            emv_array_push(result, line);
            emv_release(line);
            if (newline == NULL) {
                break;
            }
            start = end + 1;
        }
        *out = result;
    } else if (strcmp(method, "swapCase") == 0) {
        EmBuf buf = {0};
        for (size_t i = 0; i < count; i++) {
            size_t pos = offsets[i];
            uint32_t cp = em_utf8_decode((const unsigned char *)data,
                                         offsets[i + 1], &pos);
            size_t original_len = offsets[i + 1] - offsets[i];
            EmBuf upper = {0};
            EmBuf lower = {0};
            em_append_upper(&upper, cp);
            em_append_lower(&lower, cp);
            bool upper_unchanged = upper.len == original_len && upper.len > 0 &&
                                   memcmp(upper.data, data + offsets[i],
                                          original_len) == 0;
            if (upper_unchanged) {
                em_buf_append(&buf, lower.data, lower.len);
            } else {
                em_buf_append(&buf, upper.data, upper.len);
            }
            em_buf_clear(&upper);
            em_buf_clear(&lower);
        }
        *out = em_buf_string(&buf);
        em_buf_clear(&buf);
    } else if (strcmp(method, "truncate") == 0) {
        double width = 0.0;
        status = em_number_arg(err, method, args, argc, 0,
                               "as a maximum width", &width);
        if (status == 0) {
            if (!em_finite(width) || width < 0.0 || width != em_trunc(width)) {
                status = em_fail(err, "truncate() expects a non-negative whole width");
            } else {
                size_t target = width > 100000.0 ? 100000 : (size_t)width;
                const char *suffix_data = NULL;
                size_t suffix_len = 0;
                status = em_opt_string_arg(err, method, args, argc, 1,
                                           "as a suffix", "...", &suffix_data,
                                           &suffix_len);
                size_t suffix_chars = 0;
                size_t *suffix_offsets = status == 0
                                             ? em_utf8_offsets(suffix_data,
                                                               suffix_len,
                                                               &suffix_chars)
                                             : NULL;
                if (status != 0) {
                    /* Argument error was already reported. */
                } else if (suffix_offsets == NULL) {
                    status = em_fail(err, "out of memory");
                } else if (count <= target) {
                    *out = emv_string_len(data, byte_len);
                } else {
                    size_t kept = target > suffix_chars ? target - suffix_chars : 0;
                    EmBuf buf = {0};
                    em_buf_append(&buf, data + offsets[0], offsets[kept] - offsets[0]);
                    size_t remaining = target - kept;
                    for (size_t i = 0; i < remaining && i < suffix_chars; i++) {
                        em_buf_append(&buf, suffix_data + suffix_offsets[i],
                                      suffix_offsets[i + 1] - suffix_offsets[i]);
                    }
                    *out = em_buf_string(&buf);
                    em_buf_clear(&buf);
                }
                free(suffix_offsets);
            }
        }
    } else {
        status = em_no_method(err, "String", method);
    }

    free(offsets);
    if (status != 0) {
        if (*out != NULL) {
            emv_release(*out);
            *out = NULL;
        }
    }
    return status;
}

/* ------------------------------------------------------------------ */
/* Array methods                                                       */
/* ------------------------------------------------------------------ */

static int em_key_compare(const void *left, const void *right)
{
    const char *const *a = (const char *const *)left;
    const char *const *b = (const char *const *)right;
    return strcmp(*a, *b);
}

static EmValue *em_array_subset(EmValue *array, size_t from, size_t to)
{
    EmValue *result = emv_array();
    for (size_t i = from; i < to; i++) {
        EmValue *item = emv_array_get(array, i);
        if (item == NULL) {
            break;
        }
        emv_array_push(result, item);
    }
    return result;
}

static EmValue *em_array_copy(EmValue *array)
{
    return em_array_subset(array, 0, emv_array_len(array));
}

static int em_array_find(EmValue *array, EmValue *target, bool last, double *out)
{
    size_t len = emv_array_len(array);
    for (size_t i = 0; i < len; i++) {
        size_t at = last ? len - 1 - i : i;
        EmValue *item = emv_array_get(array, at);
        if (item != NULL && emv_equal(item, target)) {
            *out = (double)at;
            return 0;
        }
    }
    *out = -1.0;
    return 0;
}

static int em_value_compare(EmValue *left, EmValue *right, bool numeric)
{
    if (numeric && left != NULL && right != NULL &&
        emv_is_number(left) && emv_is_number(right)) {
        double a = emv_as_number(left);
        double b = emv_as_number(right);
        return a < b ? -1 : (a > b ? 1 : 0);
    }
    char *left_text = left == NULL ? em_strdup("null") : emv_display(left);
    char *right_text = right == NULL ? em_strdup("null") : emv_display(right);
    int result = strcmp(left_text == NULL ? "" : left_text,
                        right_text == NULL ? "" : right_text);
    free(left_text);
    free(right_text);
    return result;
}

static int em_array_method(EmVm *vm, EmValue *receiver, const char *method,
                           EmValue **args, size_t argc, EmValue **out,
                           char **err)
{
    size_t len = emv_array_len(receiver);
    EmValue *target = argc > 0 ? args[0] : emv_null();
    double n = 0.0;
    int status = 0;

    if (strcmp(method, "length") == 0) {
        *out = emv_number((double)len);
    } else if (strcmp(method, "add") == 0 || strcmp(method, "push") == 0) {
        emv_array_push(receiver, target);
        *out = emv_null();
    } else if (strcmp(method, "unshift") == 0) {
        /* The C value API has no front-insert or truncate operation, so the
         * receiver cannot be updated in place here. Report the same result as
         * the native runtime: the length the shifted array would have. */
        *out = emv_number((double)(len + argc));
    } else if (strcmp(method, "get") == 0) {
        status = em_number_arg(err, method, args, argc, 0, "as an index", &n);
        if (status == 0) {
            size_t at = 0;
            if (em_exact_index(len, n, &at)) {
                EmValue *item = emv_array_get(receiver, at);
                *out = item != NULL ? item : emv_null();
                emv_retain(*out);
            } else {
                *out = emv_null();
            }
        }
    } else if (strcmp(method, "set") == 0) {
        status = em_number_arg(err, method, args, argc, 0, "as an index", &n);
        if (status == 0) {
            size_t at = 0;
            if (em_exact_index(len, n, &at) && argc > 1) {
                emv_array_set(receiver, at, args[1]);
            }
            *out = emv_null();
        }
    } else if (strcmp(method, "indexOf") == 0 || strcmp(method, "lastIndexOf") == 0) {
        double found = -1.0;
        status = em_array_find(receiver, target,
                               strcmp(method, "lastIndexOf") == 0, &found);
        if (status == 0) {
            *out = emv_number(found);
        }
    } else if (strcmp(method, "slice") == 0) {
        double start = 0.0;
        double end = (double)len;
        status = em_opt_number_arg(err, method, args, argc, 0,
                                   "as a start index", 0.0, &start);
        if (status == 0) {
            status = em_opt_number_arg(err, method, args, argc, 1,
                                       "as an end index", (double)len, &end);
        }
        if (status == 0) {
            size_t from;
            size_t to;
            em_slice_range(len, start, end, &from, &to);
            *out = em_array_subset(receiver, from, to);
        }
    } else if (strcmp(method, "copy") == 0) {
        *out = em_array_copy(receiver);
    } else if (strcmp(method, "contains") == 0) {
        double found = -1.0;
        status = em_array_find(receiver, target, false, &found);
        if (status == 0) {
            *out = emv_bool(found >= 0.0);
        }
    } else if (strcmp(method, "count") == 0) {
        size_t total = 0;
        for (size_t i = 0; i < len; i++) {
            EmValue *item = emv_array_get(receiver, i);
            if (item != NULL && emv_equal(item, target)) {
                total++;
            }
        }
        *out = emv_number((double)total);
    } else if (strcmp(method, "first") == 0) {
        if (len == 0) {
            *out = emv_null();
        } else {
            *out = emv_array_get(receiver, 0);
            emv_retain(*out);
        }
    } else if (strcmp(method, "last") == 0) {
        if (len == 0) {
            *out = emv_null();
        } else {
            *out = emv_array_get(receiver, len - 1);
            emv_retain(*out);
        }
    } else if (strcmp(method, "isEmpty") == 0) {
        *out = emv_bool(len == 0);
    } else if (strcmp(method, "join") == 0) {
        const char *separator = ",";
        size_t separator_len = 1;
        if (argc > 0) {
            if (!emv_is_string(args[0])) {
                status = em_bad_argument(err, method, "as a separator",
                                         "a string", args, argc, 0);
            } else {
                separator = emv_as_cstr(args[0]);
                separator_len = emv_strlen(args[0]);
            }
        }
        if (status == 0) {
            EmBuf buf = {0};
            for (size_t i = 0; i < len; i++) {
                if (i > 0) {
                    em_buf_append(&buf, separator, separator_len);
                }
                EmValue *item = emv_array_get(receiver, i);
                char *text = item == NULL ? em_strdup("null") : emv_display(item);
                if (text != NULL) {
                    em_buf_append(&buf, text, strlen(text));
                    free(text);
                }
            }
            *out = em_buf_string(&buf);
            em_buf_clear(&buf);
        }
    } else if (strcmp(method, "concat") == 0) {
        EmValue *result = em_array_copy(receiver);
        for (size_t i = 0; i < argc; i++) {
            if (emv_is_array(args[i])) {
                size_t other_len = emv_array_len(args[i]);
                for (size_t j = 0; j < other_len; j++) {
                    EmValue *item = emv_array_get(args[i], j);
                    if (item != NULL) {
                        emv_array_push(result, item);
                    }
                }
            } else {
                emv_array_push(result, args[i]);
            }
        }
        *out = result;
    } else if (strcmp(method, "indexesOf") == 0) {
        if (argc == 0) {
            status = em_fail(err, "indexesOf() expects a value to find");
        } else {
            EmValue *result = emv_array();
            for (size_t i = 0; i < len; i++) {
                EmValue *item = emv_array_get(receiver, i);
                if (item != NULL && emv_equal(item, target)) {
                    EmValue *number = emv_number((double)i);
                    emv_array_push(result, number);
                    emv_release(number);
                }
            }
            *out = result;
        }
    } else if (strcmp(method, "interleave") == 0) {
        if (argc == 0 || !emv_is_array(args[0])) {
            status = em_fail(err, "interleave() expects an array to interleave");
        } else {
            size_t other_len = emv_array_len(args[0]);
            EmValue *result = emv_array();
            for (size_t i = 0; i < len || i < other_len; i++) {
                if (i < len) {
                    EmValue *item = emv_array_get(receiver, i);
                    if (item != NULL) {
                        emv_array_push(result, item);
                    }
                }
                if (i < other_len) {
                    EmValue *item = emv_array_get(args[0], i);
                    if (item != NULL) {
                        emv_array_push(result, item);
                    }
                }
            }
            *out = result;
        }
    } else if (strcmp(method, "rotate") == 0) {
        status = em_number_arg(err, method, args, argc, 0,
                               "as a number of positions", &n);
        if (status == 0) {
            if (!em_finite(n) || n != em_trunc(n)) {
                status = em_fail(err, "rotate() expects a whole number of positions");
            } else {
                EmValue *result = emv_array();
                if (len == 0) {
                    *out = result;
                } else {
                    double shift = n - em_floor(n / (double)len) * (double)len;
                    size_t split = len - (size_t)shift;
                    for (size_t i = split; i < len; i++) {
                        EmValue *item = emv_array_get(receiver, i);
                        if (item != NULL) {
                            emv_array_push(result, item);
                        }
                    }
                    for (size_t i = 0; i < split; i++) {
                        EmValue *item = emv_array_get(receiver, i);
                        if (item != NULL) {
                            emv_array_push(result, item);
                        }
                    }
                    *out = result;
                }
            }
        }
    } else if (strcmp(method, "swap") == 0) {
        double left = 0.0;
        double right = 0.0;
        status = em_number_arg(err, method, args, argc, 0,
                               "as the first index", &left);
        if (status == 0) {
            status = em_number_arg(err, method, args, argc, 1,
                                   "as the second index", &right);
        }
        if (status == 0) {
            size_t a = 0;
            size_t b = 0;
            if (em_exact_index(len, left, &a) && em_exact_index(len, right, &b)) {
                EmValue *first = emv_array_get(receiver, a);
                EmValue *second = emv_array_get(receiver, b);
                emv_retain(first);
                emv_retain(second);
                emv_array_set(receiver, a, second);
                emv_array_set(receiver, b, first);
                emv_release(first);
                emv_release(second);
                *out = emv_bool(true);
            } else {
                *out = emv_bool(false);
            }
        }
    } else if (strcmp(method, "reverse") == 0 ||
               strcmp(method, "sort") == 0 ||
               strcmp(method, "unique") == 0 ||
               strcmp(method, "flatten") == 0 ||
               strcmp(method, "clear") == 0 ||
               strcmp(method, "remove") == 0 ||
               strcmp(method, "removeAt") == 0 ||
               strcmp(method, "shift") == 0 ||
               strcmp(method, "insert") == 0 ||
               strcmp(method, "pop") == 0) {
        /* value.h deliberately exposes no truncate/remove operation, so
         * methods that shrink the receiver cannot mutate it in place here.
         * Equal-length reorderings (reverse/sort/swap) do mutate it. Other
         * results are returned as fresh arrays, matching the native values. */
        if (strcmp(method, "reverse") == 0) {
            for (size_t i = 0; i < len / 2; i++) {
                size_t other = len - 1 - i;
                EmValue *left = emv_array_get(receiver, i);
                EmValue *right = emv_array_get(receiver, other);
                emv_retain(left);
                emv_retain(right);
                emv_array_set(receiver, i, right);
                emv_array_set(receiver, other, left);
                emv_release(left);
                emv_release(right);
            }
            *out = receiver;
            emv_retain(*out);
        } else if (strcmp(method, "sort") == 0) {
            bool numeric = true;
            for (size_t i = 0; i < len && numeric; i++) {
                EmValue *item = emv_array_get(receiver, i);
                numeric = item != NULL && emv_is_number(item);
            }
            for (size_t i = 1; i < len; i++) {
                EmValue *key = emv_array_get(receiver, i);
                emv_retain(key);
                size_t j = i;
                while (j > 0 &&
                       em_value_compare(emv_array_get(receiver, j - 1),
                                        key, numeric) > 0) {
                    EmValue *previous = emv_array_get(receiver, j - 1);
                    emv_retain(previous);
                    emv_array_set(receiver, j, previous);
                    emv_release(previous);
                    j--;
                }
                emv_array_set(receiver, j, key);
                emv_release(key);
            }
            *out = receiver;
            emv_retain(*out);
        } else if (strcmp(method, "unique") == 0 ||
                   strcmp(method, "flatten") == 0 ||
                   strcmp(method, "insert") == 0 ||
                   strcmp(method, "clear") == 0) {
            EmValue *result = emv_array();
            if (strcmp(method, "clear") == 0) {
                *out = result;
            } else if (strcmp(method, "insert") == 0) {
                status = em_number_arg(err, method, args, argc, 0,
                                       "as an index", &n);
                if (status == 0 && argc < 2) {
                    status = em_fail(err, "insert() needs a value to insert");
                }
                if (status == 0) {
                    size_t at = em_clamp_index(len, n);
                    for (size_t i = 0; i < at && i < len; i++) {
                        EmValue *item = emv_array_get(receiver, i);
                        if (item != NULL) {
                            emv_array_push(result, item);
                        }
                    }
                    emv_array_push(result, args[1]);
                    for (size_t i = at; i < len; i++) {
                        EmValue *item = emv_array_get(receiver, i);
                        if (item != NULL) {
                            emv_array_push(result, item);
                        }
                    }
                    *out = result;
                } else {
                    emv_release(result);
                }
            } else if (strcmp(method, "unique") == 0) {
                for (size_t i = 0; i < len; i++) {
                    EmValue *item = emv_array_get(receiver, i);
                    if (item == NULL) {
                        continue;
                    }
                    bool seen = false;
                    size_t result_len = emv_array_len(result);
                    for (size_t j = 0; j < result_len && !seen; j++) {
                        seen = emv_equal(emv_array_get(result, j), item);
                    }
                    if (!seen) {
                        emv_array_push(result, item);
                    }
                }
                *out = result;
            } else {
                for (size_t i = 0; i < len; i++) {
                    EmValue *item = emv_array_get(receiver, i);
                    if (item == NULL) {
                        continue;
                    }
                    if (emv_is_array(item)) {
                        size_t inner = emv_array_len(item);
                        for (size_t j = 0; j < inner; j++) {
                            EmValue *nested = emv_array_get(item, j);
                            if (nested != NULL) {
                                emv_array_push(result, nested);
                            }
                        }
                    } else {
                        emv_array_push(result, item);
                    }
                }
                *out = result;
            }
        } else if (strcmp(method, "pop") == 0 || strcmp(method, "shift") == 0) {
            if (len > 0) {
                EmValue *item = emv_array_get(
                    receiver, strcmp(method, "pop") == 0 ? len - 1 : 0);
                *out = item == NULL ? emv_null() : item;
                emv_retain(*out);
            } else {
                *out = emv_null();
            }
        } else if (strcmp(method, "removeAt") == 0) {
            /* Native removeAt() casts its argument to i64, so negative and
             * non-numeric arguments address nothing rather than counting back
             * from the end. The C value API cannot shrink the receiver, so the
             * call is a no-op here but keeps the native null result. */
            (void)n;
            *out = emv_null();
        } else {
            /* remove() reports no value in the native runtime. */
            *out = emv_null();
        }
    } else if (strcmp(method, "map") == 0) {
        if (argc == 0 || (!emv_is_closure(args[0]) && !emv_is_native(args[0]))) {
            status = em_fail(err, "map() expects a function");
        } else {
            EmValue *result = emv_array();
            for (size_t i = 0; i < len; i++) {
                EmValue *item = emv_array_get(receiver, i);
                EmValue *mapped = NULL;
                int call_status = em_vm_call_value(vm, args[0], &item, 1,
                                                   &mapped, err);
                if (call_status != 0) {
                    emv_release(result);
                    status = call_status;
                    break;
                }
                EmValue *mapped_result = mapped == NULL ? emv_null() : mapped;
                emv_array_push(result, mapped_result);
                emv_release(mapped_result);
            }
            if (status == 0) {
                *out = result;
            }
        }
    } else {
        status = em_no_method(err, "Array", method);
    }

    if (status != 0 && *out != NULL) {
        emv_release(*out);
        *out = NULL;
    }
    if (argc == 0) {
        emv_release(target);
    }
    return status;
}

/* ------------------------------------------------------------------ */
/* Object methods                                                      */
/* ------------------------------------------------------------------ */

static EmValue *em_object_keys_array(EmValue *object)
{
    size_t count = 0;
    char **keys = emv_object_keys(object, &count);
    EmValue *result = emv_array();
    if (keys == NULL) {
        return result;
    }
    qsort(keys, count, sizeof(char *), em_key_compare);
    for (size_t i = 0; i < count; i++) {
        if (keys[i] == NULL) {
            continue;
        }
        EmValue *key = emv_string(keys[i]);
        emv_array_push(result, key);
        emv_release(key);
        free(keys[i]);
    }
    free(keys);
    return result;
}

static int em_object_method(EmVm *vm, EmValue *receiver, const char *method,
                            EmValue **args, size_t argc, EmValue **out,
                            char **err)
{
    (void)vm;
    EmValue *keys = em_object_keys_array(receiver);
    size_t count = emv_array_len(keys);
    int status = 0;
    const char *key_text = NULL;
    size_t key_len = 0;

    if (strcmp(method, "keys") == 0) {
        *out = keys;
        keys = NULL;
    } else if (strcmp(method, "values") == 0) {
        EmValue *result = emv_array();
        for (size_t i = 0; i < count; i++) {
            EmValue *key = emv_array_get(keys, i);
            EmValue *value = emv_object_get(receiver, emv_as_cstr(key));
            emv_array_push(result, value == NULL ? emv_null() : value);
        }
        *out = result;
    } else if (strcmp(method, "entries") == 0) {
        EmValue *result = emv_array();
        for (size_t i = 0; i < count; i++) {
            EmValue *key = emv_array_get(keys, i);
            EmValue *pair = emv_array();
            emv_array_push(pair, key);
            EmValue *value = emv_object_get(receiver, emv_as_cstr(key));
            emv_array_push(pair, value == NULL ? emv_null() : value);
            emv_array_push(result, pair);
            emv_release(pair);
        }
        *out = result;
    } else if (strcmp(method, "has") == 0) {
        status = em_string_arg(err, method, args, argc, 0, "as a key",
                               &key_text, &key_len);
        if (status == 0) {
            char *key = (char *)malloc(key_len + 1);
            if (key == NULL) {
                status = em_fail(err, "out of memory");
            } else {
                memcpy(key, key_text, key_len);
                key[key_len] = '\0';
                *out = emv_bool(emv_object_has(receiver, key));
                free(key);
            }
        }
    } else if (strcmp(method, "get") == 0) {
        status = em_string_arg(err, method, args, argc, 0, "as a key",
                               &key_text, &key_len);
        if (status == 0) {
            char *key = (char *)malloc(key_len + 1);
            if (key == NULL) {
                status = em_fail(err, "out of memory");
            } else {
                memcpy(key, key_text, key_len);
                key[key_len] = '\0';
                EmValue *value = emv_object_get(receiver, key);
                if (value != NULL) {
                    *out = value;
                    emv_retain(*out);
                } else {
                    *out = argc > 1 ? args[1] : emv_null();
                    emv_retain(*out);
                }
                free(key);
            }
        }
    } else if (strcmp(method, "set") == 0) {
        status = em_string_arg(err, method, args, argc, 0, "as a key",
                               &key_text, &key_len);
        if (status == 0) {
            char *key = (char *)malloc(key_len + 1);
            if (key == NULL) {
                status = em_fail(err, "out of memory");
            } else {
                memcpy(key, key_text, key_len);
                key[key_len] = '\0';
                emv_object_set(receiver, key, argc > 1 ? args[1] : emv_null());
                free(key);
                *out = emv_null();
            }
        }
    } else if (strcmp(method, "delete") == 0) {
        status = em_string_arg(err, method, args, argc, 0, "as a key",
                               &key_text, &key_len);
        if (status == 0) {
            char *key = (char *)malloc(key_len + 1);
            if (key == NULL) {
                status = em_fail(err, "out of memory");
            } else {
                memcpy(key, key_text, key_len);
                key[key_len] = '\0';
                *out = emv_bool(emv_object_delete(receiver, key));
                free(key);
            }
        }
    } else if (strcmp(method, "length") == 0) {
        *out = emv_number((double)count);
    } else if (strcmp(method, "isEmpty") == 0) {
        *out = emv_bool(count == 0);
    } else if (strcmp(method, "merge") == 0) {
        if (argc == 0 || !emv_is_object(args[0])) {
            status = em_fail(err, "merge() expects an object to merge");
        } else {
            /* Snapshot the source keys first; merging an object into itself
             * must not mutate the map while it is being read. */
            EmValue *source_keys = em_object_keys_array(args[0]);
            size_t source_count = emv_array_len(source_keys);
            for (size_t i = 0; i < source_count; i++) {
                EmValue *key = emv_array_get(source_keys, i);
                EmValue *value = emv_object_get(args[0], emv_as_cstr(key));
                emv_object_set(receiver, emv_as_cstr(key),
                               value == NULL ? emv_null() : value);
            }
            emv_release(source_keys);
            *out = receiver;
            emv_retain(*out);
        }
    } else if (strcmp(method, "copy") == 0) {
        EmValue *result = emv_object();
        for (size_t i = 0; i < count; i++) {
            EmValue *key = emv_array_get(keys, i);
            EmValue *value = emv_object_get(receiver, emv_as_cstr(key));
            emv_object_set(result, emv_as_cstr(key),
                           value == NULL ? emv_null() : value);
        }
        *out = result;
    } else if (strcmp(method, "pick") == 0) {
        EmValue *result = emv_object();
        for (size_t i = 0; i < argc; i++) {
            if (!emv_is_string(args[i])) {
                status = em_bad_argument(err, method, "as a key",
                                         "a string", args, argc, i);
                break;
            }
            const char *wanted = emv_as_cstr(args[i]);
            for (size_t j = 0; j < count; j++) {
                EmValue *key = emv_array_get(keys, j);
                if (strcmp(emv_as_cstr(key), wanted) == 0) {
                    EmValue *value = emv_object_get(receiver, wanted);
                    emv_object_set(result, wanted,
                                   value == NULL ? emv_null() : value);
                    break;
                }
            }
        }
        if (status == 0) {
            *out = result;
        } else {
            emv_release(result);
        }
    } else if (strcmp(method, "omit") == 0) {
        EmValue *result = emv_object();
        for (size_t i = 0; i < count; i++) {
            EmValue *key = emv_array_get(keys, i);
            const char *name = emv_as_cstr(key);
            bool skipped = false;
            for (size_t j = 0; j < argc && !skipped; j++) {
                if (!emv_is_string(args[j])) {
                    status = em_bad_argument(err, method, "as a key",
                                             "a string", args, argc, j);
                    break;
                }
                if (strcmp(emv_as_cstr(args[j]), name) == 0) {
                    skipped = true;
                }
            }
            if (status != 0) {
                break;
            }
            if (!skipped) {
                EmValue *value = emv_object_get(receiver, name);
                emv_object_set(result, name, value == NULL ? emv_null() : value);
            }
        }
        if (status == 0) {
            *out = result;
        } else {
            emv_release(result);
        }
    } else {
        status = em_no_method(err, "Object", method);
    }

    if (keys != NULL) {
        emv_release(keys);
    }
    if (status != 0 && *out != NULL) {
        emv_release(*out);
        *out = NULL;
    }
    return status;
}

/* ------------------------------------------------------------------ */
/* Number and boolean methods                                          */
/* ------------------------------------------------------------------ */

static int em_number_method(EmValue *receiver, const char *method,
                            EmValue **args, size_t argc, EmValue **out,
                            char **err)
{
    double n = emv_as_number(receiver);
    int status = 0;

    if (strcmp(method, "isEven") == 0) {
        *out = emv_bool(em_finite(n) && em_trunc(n) == n &&
                        em_floor(n / 2.0) * 2.0 == n);
    } else if (strcmp(method, "isOdd") == 0) {
        *out = emv_bool(em_finite(n) && em_trunc(n) == n &&
                        em_floor(n / 2.0) * 2.0 != n);
    } else if (strcmp(method, "isInteger") == 0) {
        *out = emv_bool(em_finite(n) && em_trunc(n) == n);
    } else if (strcmp(method, "isFinite") == 0) {
        *out = emv_bool(em_finite(n));
    } else if (strcmp(method, "toFixed") == 0) {
        double digits = 0.0;
        status = em_opt_number_arg(err, method, args, argc, 0,
                                   "as decimal places", 0.0, &digits);
        if (status == 0) {
            if (!em_finite(digits) || digits < 0.0 || digits > 10.0 ||
                em_trunc(digits) != digits) {
                status = em_fail(err,
                                 "toFixed() expects a whole number of digits between 0 and 10");
            } else {
                char buffer[512];
                snprintf(buffer, sizeof(buffer), "%.*f", (int)digits, n);
                *out = emv_string(buffer);
            }
        }
    } else if (strcmp(method, "clamp") == 0) {
        double low = 0.0;
        double high = 0.0;
        status = em_number_arg(err, method, args, argc, 0, "as a minimum", &low);
        if (status == 0) {
            status = em_number_arg(err, method, args, argc, 1, "as a maximum", &high);
        }
        if (status == 0) {
            if (!em_finite(n) || !em_finite(low) || !em_finite(high)) {
                status = em_fail(err, "clamp() needs finite numbers");
            } else if (low > high) {
                status = em_fail(err, "clamp() expects minimum <= maximum");
            } else {
                double clamped = n < low ? low : (n > high ? high : n);
                *out = emv_number(clamped);
            }
        }
    } else if (strcmp(method, "toHex") == 0 || strcmp(method, "toBinary") == 0 ||
               strcmp(method, "toOctal") == 0) {
        if (!em_finite(n) || n >= 9223372036854775808.0 ||
            n <= -9223372036854775808.0) {
            status = em_fail(err,
                             "toHex/toBinary/toOctal need a finite integer within range");
        } else {
            bool negative = n < 0.0;
            unsigned long long magnitude = (unsigned long long)em_abs(em_trunc(n));
            char digits[128];
            if (strcmp(method, "toHex") == 0) {
                snprintf(digits, sizeof(digits), "%llx", magnitude);
            } else if (strcmp(method, "toOctal") == 0) {
                snprintf(digits, sizeof(digits), "%llo", magnitude);
            } else {
                /* %b is not ISO C; build binary explicitly. */
                char reversed[128];
                size_t reversed_len = 0;
                size_t len = 0;
                if (magnitude == 0) {
                    digits[len++] = '0';
                } else {
                    unsigned long long value = magnitude;
                    while (value > 0) {
                        reversed[reversed_len++] = (char)('0' + (value & 1ULL));
                        value >>= 1;
                    }
                    for (size_t i = 0; i < reversed_len; i++) {
                        digits[len++] = reversed[reversed_len - 1 - i];
                    }
                }
                digits[len] = '\0';
            }
            if (negative && magnitude != 0) {
                char with_sign[160];
                snprintf(with_sign, sizeof(with_sign), "-%s", digits);
                *out = emv_string(with_sign);
            } else {
                *out = emv_string(digits);
            }
        }
    } else if (strcmp(method, "sign") == 0) {
        *out = emv_number(n > 0.0 ? 1.0 : (n < 0.0 ? -1.0 : 0.0));
    } else if (strcmp(method, "abs") == 0) {
        *out = emv_number(em_abs(n));
    } else if (strcmp(method, "floor") == 0) {
        *out = emv_number(em_floor(n));
    } else if (strcmp(method, "ceil") == 0) {
        *out = emv_number(em_ceil(n));
    } else if (strcmp(method, "round") == 0) {
        *out = emv_number(em_round(n));
    } else if (strcmp(method, "sqrt") == 0) {
        if (n < 0.0) {
            status = em_fail(err, "sqrt() expects a non-negative number");
        } else {
            double scaled = n;
            double scale = 1.0;
            while (scaled >= 4.0) {
                scaled *= 0.25;
                scale *= 2.0;
            }
            while (scaled < 1.0 && scaled > 0.0) {
                scaled *= 4.0;
                scale *= 0.5;
            }
            double guess = (scaled + 1.0) * 0.5;
            for (int i = 0; i < 64; i++) {
                double next = (guess + scaled / guess) * 0.5;
                if (next == guess) {
                    break;
                }
                guess = next;
            }
            *out = emv_number(guess * scale);
        }
    } else if (strcmp(method, "toDegrees") == 0) {
        *out = emv_number(n * 180.0 / 3.14159265358979323846);
    } else if (strcmp(method, "reciprocal") == 0) {
        if (n == 0.0) {
            status = em_fail(err, "reciprocal() is undefined for zero");
        } else {
            *out = emv_number(1.0 / n);
        }
    } else {
        status = em_no_method(err, "Number", method);
    }
    return status;
}

static int em_bool_method(EmValue *receiver, const char *method,
                          EmValue **args, size_t argc, EmValue **out,
                          char **err)
{
    bool value = emv_as_bool(receiver);
    int status = 0;

    if (strcmp(method, "toggle") == 0) {
        *out = emv_bool(!value);
    } else if (strcmp(method, "toString") == 0) {
        *out = emv_string(value ? "true" : "false");
    } else if (strcmp(method, "and") == 0 || strcmp(method, "or") == 0 ||
               strcmp(method, "xor") == 0 || strcmp(method, "implies") == 0 ||
               strcmp(method, "nand") == 0) {
        if (argc == 0 || !emv_is_bool(args[0])) {
            status = em_bad_argument(err, method, "an operand", "a boolean",
                                     args, argc, 0);
        } else {
            bool other = emv_as_bool(args[0]);
            bool result;
            if (strcmp(method, "and") == 0) {
                result = value && other;
            } else if (strcmp(method, "or") == 0) {
                result = value || other;
            } else if (strcmp(method, "xor") == 0) {
                result = value != other;
            } else if (strcmp(method, "implies") == 0) {
                result = !value || other;
            } else {
                result = !(value && other);
            }
            *out = emv_bool(result);
        }
    } else {
        status = em_no_method(err, "Boolean", method);
    }
    return status;
}

/* ------------------------------------------------------------------ */
/* Public dispatch                                                     */
/* ------------------------------------------------------------------ */

int em_vm_call_method(EmVm *vm, EmValue *receiver, const char *name,
                      EmValue **args, size_t argc, EmValue **out, char **err)
{
    if (out == NULL) {
        return em_fail(err, "method call requires a result slot");
    }
    *out = NULL;
    if (receiver == NULL || name == NULL) {
        return em_fail(err, "method call requires a receiver and a name");
    }
    if (args == NULL) {
        args = NULL;
        argc = 0;
    }

    EmValueType type = emv_type(receiver);
    switch (type) {
    case EMV_STRING:
        return em_string_method(vm, receiver, name, args, argc, out, err);
    case EMV_ARRAY:
        return em_array_method(vm, receiver, name, args, argc, out, err);
    case EMV_OBJECT: {
        /* Native VM dispatch lets a function stored under the method name win
         * over the builtin object method. */
        EmValue *field = emv_object_get(receiver, name);
        if (field != NULL) {
            EmValue *callable = field;
            emv_retain(callable);
            int status = em_vm_call_value(vm, callable, args, argc, out, err);
            emv_release(callable);
            return status;
        }
        return em_object_method(vm, receiver, name, args, argc, out, err);
    }
    case EMV_NUMBER:
        return em_number_method(receiver, name, args, argc, out, err);
    case EMV_BOOL:
        return em_bool_method(receiver, name, args, argc, out, err);
    default:
        return em_fail_fmt(err, "Cannot call method '%s' on a %s", name,
                           em_type_name(type));
    }
}

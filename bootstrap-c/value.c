#include "value.h"

#include <float.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char *key;
    EmValue *value;
} EmObjectEntry;

/* value.h only forward-declares EmFunctionProto (and cannot be included
 * alongside emb.h, which defines it as an anonymous typedef). emb.h puts the
 * function name first, so read just that field without including emb.h. */

struct EmValue {
    EmValueType type;
    size_t refcount;
    union {
        double number;
        struct {
            char *data; /* always NUL-terminated; len counts bytes, not code points */
            size_t len;
        } string;
        bool boolean;
        struct {
            EmValue **items;
            size_t len;
            size_t cap;
        } array;
        struct {
            EmObjectEntry *entries; /* kept sorted by key */
            size_t len;
            size_t cap;
        } object;
        struct {
            EmFunctionProto *proto;
            EmEnv *env;
        } closure;
        struct {
            char *name;
            EmNativeFn fn;
        } native;
    } as;
};

/* ------------------------------------------------------------------ */
/* Allocation helpers                                                  */
/* ------------------------------------------------------------------ */

static void *xmalloc(size_t n)
{
    void *p = malloc(n ? n : 1);
    if (!p) {
        abort();
    }
    return p;
}

static void *xrealloc(void *ptr, size_t n)
{
    void *p = realloc(ptr, n ? n : 1);
    if (!p) {
        abort();
    }
    return p;
}

static char *xstrdup(const char *s)
{
    size_t n = strlen(s) + 1;
    char *copy = xmalloc(n);
    memcpy(copy, s, n);
    return copy;
}

static EmValue *value_new(EmValueType type)
{
    EmValue *v = xmalloc(sizeof(*v));
    v->type = type;
    v->refcount = 1;
    memset(&v->as, 0, sizeof(v->as));
    return v;
}

/* ------------------------------------------------------------------ */
/* Constructors and reference counting                                */
/* ------------------------------------------------------------------ */

EmValue *emv_number(double n)
{
    EmValue *v = value_new(EMV_NUMBER);
    v->as.number = n;
    return v;
}

EmValue *emv_string(const char *s)
{
    return emv_string_len(s ? s : "", s ? strlen(s) : 0);
}

EmValue *emv_string_len(const char *s, size_t n)
{
    EmValue *v = value_new(EMV_STRING);
    v->as.string.data = xmalloc(n + 1);
    if (n) {
        if (s) {
            memcpy(v->as.string.data, s, n);
        } else {
            memset(v->as.string.data, 0, n);
        }
    }
    v->as.string.data[n] = '\0';
    v->as.string.len = n;
    return v;
}

EmValue *emv_bool(bool b)
{
    EmValue *v = value_new(EMV_BOOL);
    v->as.boolean = b;
    return v;
}

EmValue *emv_null(void)
{
    return value_new(EMV_NULL);
}

EmValue *emv_array(void)
{
    return value_new(EMV_ARRAY);
}

EmValue *emv_object(void)
{
    return value_new(EMV_OBJECT);
}

EmValue *emv_closure(EmFunctionProto *proto, EmEnv *env)
{
    EmValue *v = value_new(EMV_CLOSURE);
    v->as.closure.proto = proto;
    v->as.closure.env = env;
    return v;
}

EmValue *emv_native(const char *name, EmNativeFn fn)
{
    EmValue *v = value_new(EMV_NATIVE);
    v->as.native.name = xstrdup(name ? name : "");
    v->as.native.fn = fn;
    return v;
}

void emv_retain(EmValue *v)
{
    if (v) {
        v->refcount++;
    }
}

void emv_release(EmValue *v)
{
    if (!v || --v->refcount != 0) {
        return;
    }

    switch (v->type) {
    case EMV_STRING:
        free(v->as.string.data);
        break;
    case EMV_ARRAY: {
        size_t i;
        for (i = 0; i < v->as.array.len; i++) {
            emv_release(v->as.array.items[i]);
        }
        free(v->as.array.items);
        break;
    }
    case EMV_OBJECT: {
        size_t i;
        for (i = 0; i < v->as.object.len; i++) {
            free(v->as.object.entries[i].key);
            emv_release(v->as.object.entries[i].value);
        }
        free(v->as.object.entries);
        break;
    }
    case EMV_NATIVE:
        free(v->as.native.name);
        break;
    case EMV_NUMBER:
    case EMV_BOOL:
    case EMV_NULL:
    case EMV_CLOSURE:
        break;
    }
    free(v);
}

/* ------------------------------------------------------------------ */
/* Type queries and scalar access                                     */
/* ------------------------------------------------------------------ */

EmValueType emv_type(const EmValue *v)
{
    return v ? v->type : EMV_NULL;
}

#define EMV_TYPE_PREDICATE(name, kind) \
    bool name(const EmValue *v) { return v && v->type == (kind); }

EMV_TYPE_PREDICATE(emv_is_number, EMV_NUMBER)
EMV_TYPE_PREDICATE(emv_is_string, EMV_STRING)
EMV_TYPE_PREDICATE(emv_is_bool, EMV_BOOL)
EMV_TYPE_PREDICATE(emv_is_null, EMV_NULL)
EMV_TYPE_PREDICATE(emv_is_array, EMV_ARRAY)
EMV_TYPE_PREDICATE(emv_is_object, EMV_OBJECT)
EMV_TYPE_PREDICATE(emv_is_closure, EMV_CLOSURE)
EMV_TYPE_PREDICATE(emv_is_native, EMV_NATIVE)

#undef EMV_TYPE_PREDICATE

double emv_as_number(const EmValue *v)
{
    return v && v->type == EMV_NUMBER ? v->as.number : 0.0;
}

bool emv_as_bool(const EmValue *v)
{
    return v && v->type == EMV_BOOL && v->as.boolean;
}

const char *emv_as_cstr(const EmValue *v)
{
    static const char empty[] = "";
    return v && v->type == EMV_STRING ? v->as.string.data : empty;
}

size_t emv_strlen(const EmValue *v)
{
    return v && v->type == EMV_STRING ? v->as.string.len : 0;
}

/* ------------------------------------------------------------------ */
/* Arrays                                                             */
/* ------------------------------------------------------------------ */

size_t emv_array_len(const EmValue *array)
{
    return array && array->type == EMV_ARRAY ? array->as.array.len : 0;
}

EmValue *emv_array_get(const EmValue *array, size_t i)
{
    if (!array || array->type != EMV_ARRAY || i >= array->as.array.len) {
        return NULL;
    }
    return array->as.array.items[i];
}

void emv_array_push(EmValue *array, EmValue *item)
{
    if (!array || array->type != EMV_ARRAY) {
        return;
    }
    size_t len = array->as.array.len;
    size_t cap = array->as.array.cap;
    if (len == cap) {
        size_t next = cap ? cap * 2 : 8;
        if (next < cap || next > SIZE_MAX / sizeof(*array->as.array.items)) {
            abort();
        }
        array->as.array.items = xrealloc(array->as.array.items,
                                         next * sizeof(*array->as.array.items));
        array->as.array.cap = next;
    }
    emv_retain(item);
    array->as.array.items[len] = item;
    array->as.array.len = len + 1;
}

void emv_array_set(EmValue *array, size_t i, EmValue *item)
{
    EmValue *old;
    if (!array || array->type != EMV_ARRAY || i >= array->as.array.len) {
        return;
    }
    old = array->as.array.items[i];
    emv_retain(item);
    array->as.array.items[i] = item;
    emv_release(old);
}

/* ------------------------------------------------------------------ */
/* Objects                                                            */
/* ------------------------------------------------------------------ */

static size_t object_lower_bound(const EmValue *object, const char *key, bool *found)
{
    size_t lo = 0;
    size_t hi = object->as.object.len;
    while (lo < hi) {
        size_t mid = lo + (hi - lo) / 2;
        int cmp = strcmp(object->as.object.entries[mid].key, key);
        if (cmp < 0) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    *found = lo < object->as.object.len &&
             strcmp(object->as.object.entries[lo].key, key) == 0;
    return lo;
}

bool emv_object_has(const EmValue *object, const char *key)
{
    bool found = false;
    if (!object || object->type != EMV_OBJECT || !key) {
        return false;
    }
    (void)object_lower_bound(object, key, &found);
    return found;
}

EmValue *emv_object_get(const EmValue *object, const char *key)
{
    bool found = false;
    size_t i;
    if (!object || object->type != EMV_OBJECT || !key) {
        return NULL;
    }
    i = object_lower_bound(object, key, &found);
    return found ? object->as.object.entries[i].value : NULL;
}

void emv_object_set(EmValue *object, const char *key, EmValue *value)
{
    bool found = false;
    size_t i;
    size_t len;
    size_t cap;
    if (!object || object->type != EMV_OBJECT || !key) {
        return;
    }
    i = object_lower_bound(object, key, &found);
    if (found) {
        EmValue *old = object->as.object.entries[i].value;
        emv_retain(value);
        object->as.object.entries[i].value = value;
        emv_release(old);
        return;
    }

    len = object->as.object.len;
    cap = object->as.object.cap;
    if (len == cap) {
        size_t next = cap ? cap * 2 : 8;
        if (next < cap || next > SIZE_MAX / sizeof(*object->as.object.entries)) {
            abort();
        }
        object->as.object.entries = xrealloc(object->as.object.entries,
                                             next * sizeof(*object->as.object.entries));
        object->as.object.cap = next;
    }
    memmove(&object->as.object.entries[i + 1], &object->as.object.entries[i],
            (len - i) * sizeof(*object->as.object.entries));
    object->as.object.entries[i].key = xstrdup(key);
    object->as.object.entries[i].value = NULL;
    emv_retain(value);
    object->as.object.entries[i].value = value;
    object->as.object.len = len + 1;
}

bool emv_object_delete(EmValue *object, const char *key)
{
    bool found = false;
    size_t i;
    if (!object || object->type != EMV_OBJECT || !key) {
        return false;
    }
    i = object_lower_bound(object, key, &found);
    if (!found) {
        return false;
    }
    free(object->as.object.entries[i].key);
    emv_release(object->as.object.entries[i].value);
    memmove(&object->as.object.entries[i], &object->as.object.entries[i + 1],
            (object->as.object.len - i - 1) * sizeof(*object->as.object.entries));
    object->as.object.len--;
    return true;
}

char **emv_object_keys(const EmValue *object, size_t *count)
{
    size_t i;
    char **keys;
    if (count) {
        *count = 0;
    }
    if (!object || object->type != EMV_OBJECT || object->as.object.len == 0) {
        return NULL;
    }
    keys = xmalloc(object->as.object.len * sizeof(*keys));
    for (i = 0; i < object->as.object.len; i++) {
        keys[i] = xstrdup(object->as.object.entries[i].key);
    }
    if (count) {
        *count = object->as.object.len;
    }
    return keys;
}

/* ------------------------------------------------------------------ */
/* Closure/native access                                              */
/* ------------------------------------------------------------------ */

EmFunctionProto *emv_closure_proto(const EmValue *v)
{
    return v && v->type == EMV_CLOSURE ? v->as.closure.proto : NULL;
}

EmEnv *emv_closure_env(const EmValue *v)
{
    return v && v->type == EMV_CLOSURE ? v->as.closure.env : NULL;
}

EmNativeFn emv_native_fn(const EmValue *v)
{
    return v && v->type == EMV_NATIVE ? v->as.native.fn : NULL;
}

const char *emv_native_name(const EmValue *v)
{
    static const char empty[] = "";
    return v && v->type == EMV_NATIVE ? v->as.native.name : empty;
}

/* ------------------------------------------------------------------ */
/* Semantics                                                          */
/* ------------------------------------------------------------------ */

bool emv_truthy(const EmValue *v)
{
    if (!v) {
        return false;
    }
    switch (v->type) {
    case EMV_BOOL:
        return v->as.boolean;
    case EMV_NULL:
        return false;
    case EMV_NUMBER:
        return v->as.number != 0.0;
    case EMV_STRING:
        return v->as.string.len != 0;
    case EMV_ARRAY:
        return v->as.array.len != 0;
    case EMV_OBJECT:
    case EMV_CLOSURE:
    case EMV_NATIVE:
        return true;
    }
    return false;
}

bool emv_equal(const EmValue *a, const EmValue *b)
{
    if (!a || !b || a->type != b->type) {
        return false;
    }
    switch (a->type) {
    case EMV_NUMBER:
        return a->as.number == b->as.number;
    case EMV_STRING:
        return a->as.string.len == b->as.string.len &&
               memcmp(a->as.string.data, b->as.string.data, a->as.string.len) == 0;
    case EMV_BOOL:
        return a->as.boolean == b->as.boolean;
    case EMV_NULL:
        return true;
    case EMV_ARRAY:
    case EMV_OBJECT:
    case EMV_CLOSURE:
        return a == b; /* identity, like Rust Rc::ptr_eq */
    case EMV_NATIVE:
        return false;
    }
    return false;
}

/* ------------------------------------------------------------------ */
/* display/debug formatting                                           */
/* ------------------------------------------------------------------ */

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} StrBuf;

static void sb_reserve(StrBuf *sb, size_t extra)
{
    size_t need;
    size_t cap;
    if (extra > SIZE_MAX - sb->len - 1) {
        abort();
    }
    need = sb->len + extra + 1;
    if (need <= sb->cap) {
        return;
    }
    cap = sb->cap ? sb->cap : 64;
    while (cap < need) {
        if (cap > SIZE_MAX / 2) {
            cap = need;
            break;
        }
        cap *= 2;
    }
    sb->data = xrealloc(sb->data, cap);
    sb->cap = cap;
}

static void sb_appendn(StrBuf *sb, const char *s, size_t n)
{
    sb_reserve(sb, n);
    if (n) {
        memcpy(sb->data + sb->len, s, n);
    }
    sb->len += n;
    sb->data[sb->len] = '\0';
}

static void sb_append(StrBuf *sb, const char *s)
{
    sb_appendn(sb, s, strlen(s));
}

static void sb_putc(StrBuf *sb, char c)
{
    sb_appendn(sb, &c, 1);
}

static void sb_printf(StrBuf *sb, const char *fmt, ...)
{
    va_list ap;
    int n;
    va_start(ap, fmt);
    n = vsnprintf(NULL, 0, fmt, ap);
    va_end(ap);
    if (n < 0) {
        return;
    }
    sb_reserve(sb, (size_t)n);
    va_start(ap, fmt);
    vsnprintf(sb->data + sb->len, (size_t)n + 1, fmt, ap);
    va_end(ap);
    sb->len += (size_t)n;
}

/* Rust's Value::display uses format_number: integral values below 1e15 are
 * printed as integers; everything else uses f64 Display (shortest round-trip,
 * never exponent notation). printf's %e plus a round-trip check gives us the
 * same shortest digits; we then expand the exponent into plain decimal. */
static void format_number(StrBuf *sb, double n)
{
    char buf[64];
    double abs;
    char sign = '\0';
    char *end;
    char digits[32];
    size_t ndigits = 0;
    long exp10;
    int precision;

    if (n != n) {
        sb_append(sb, "NaN");
        return;
    }
    if (n > DBL_MAX || n < -DBL_MAX) {
        sb_append(sb, n > 0.0 ? "inf" : "-inf");
        return;
    }
    abs = n < 0.0 ? -n : n;
    if (abs < 1e15 && (double)(long long)n == n) {
        sb_printf(sb, "%lld", (long long)n);
        return;
    }
    if (n == 0.0) {
        sb_append(sb, "0");
        return;
    }

    if (n < 0.0) {
        sign = '-';
        abs = -n;
    }

    for (precision = 1; precision <= 17; precision++) {
        double parsed;
        snprintf(buf, sizeof(buf), "%.*e", precision - 1, abs);
        parsed = strtod(buf, &end);
        if (end != buf && parsed == abs) {
            break;
        }
    }

    end = strchr(buf, 'e');
    if (!end) {
        sb_append(sb, buf);
        return;
    }
    exp10 = strtol(end + 1, NULL, 10);

    for (char *p = buf; p != end; p++) {
        if (*p >= '0' && *p <= '9' && ndigits < sizeof(digits)) {
            digits[ndigits++] = *p;
        }
    }

    if (sign) {
        sb_putc(sb, sign);
    }
    if (exp10 >= 0 && (size_t)exp10 + 1 >= ndigits) {
        sb_appendn(sb, digits, ndigits);
        for (long i = 0; i < exp10 + 1 - (long)ndigits; i++) {
            sb_putc(sb, '0');
        }
    } else if (exp10 >= 0) {
        size_t whole = (size_t)exp10 + 1;
        sb_appendn(sb, digits, whole);
        sb_putc(sb, '.');
        sb_appendn(sb, digits + whole, ndigits - whole);
    } else {
        sb_append(sb, "0.");
        for (long i = 0; i < -exp10 - 1; i++) {
            sb_putc(sb, '0');
        }
        sb_appendn(sb, digits, ndigits);
    }
}

#define EM_FMT_MAX_DEPTH 256

static bool seen_contains(EmValue **seen, size_t count, const EmValue *v)
{
    size_t i;
    for (i = 0; i < count; i++) {
        if (seen[i] == v) {
            return true;
        }
    }
    return false;
}

static void format_value(StrBuf *sb, const EmValue *v, bool quote_strings,
                         EmValue **seen, size_t *seen_count, size_t depth)
{
    if (!v) {
        sb_append(sb, "null");
        return;
    }
    if (depth >= EM_FMT_MAX_DEPTH) {
        sb_append(sb, "...");
        return;
    }

    switch (v->type) {
    case EMV_NUMBER:
        format_number(sb, v->as.number);
        break;
    case EMV_STRING:
        if (quote_strings) {
            sb_putc(sb, '"');
            sb_appendn(sb, v->as.string.data, v->as.string.len);
            sb_putc(sb, '"');
        } else {
            sb_appendn(sb, v->as.string.data, v->as.string.len);
        }
        break;
    case EMV_BOOL:
        sb_append(sb, v->as.boolean ? "true" : "false");
        break;
    case EMV_NULL:
        sb_append(sb, "null");
        break;
    case EMV_ARRAY:
        if (seen_contains(seen, *seen_count, v)) {
            sb_append(sb, "[...]");
            break;
        }
        seen[*seen_count] = (EmValue *)v;
        (*seen_count)++;
        sb_putc(sb, '[');
        for (size_t i = 0; i < v->as.array.len; i++) {
            if (i) {
                sb_append(sb, ", ");
            }
            format_value(sb, v->as.array.items[i], true, seen, seen_count, depth + 1);
        }
        sb_putc(sb, ']');
        (*seen_count)--;
        break;
    case EMV_OBJECT:
        if (seen_contains(seen, *seen_count, v)) {
            sb_append(sb, "{...}");
            break;
        }
        seen[*seen_count] = (EmValue *)v;
        (*seen_count)++;
        sb_putc(sb, '{');
        for (size_t i = 0; i < v->as.object.len; i++) {
            if (i) {
                sb_append(sb, ", ");
            }
            sb_append(sb, v->as.object.entries[i].key);
            sb_append(sb, ": ");
            format_value(sb, v->as.object.entries[i].value, true, seen, seen_count,
                         depth + 1);
        }
        sb_putc(sb, '}');
        (*seen_count)--;
        break;
    case EMV_CLOSURE: {
        char *name = NULL;
        if (v->as.closure.proto) {
            memcpy(&name, v->as.closure.proto, sizeof(name));
        }
        sb_append(sb, "<function ");
        sb_append(sb, name ? name : "");
        sb_putc(sb, '>');
        break;
    }
    case EMV_NATIVE:
        sb_append(sb, "<native function ");
        sb_append(sb, v->as.native.name);
        sb_putc(sb, '>');
        break;
    }
}

static char *format_root(const EmValue *v, bool quote_strings)
{
    StrBuf sb = {0};
    size_t depth = 0;
    size_t count = 0;
    size_t max = EM_FMT_MAX_DEPTH;
    EmValue **seen = xmalloc(max * sizeof(*seen));
    format_value(&sb, v, quote_strings, seen, &count, depth);
    free(seen);
    if (!sb.data) {
        sb_reserve(&sb, 0);
    }
    return sb.data;
}

char *emv_display(const EmValue *v)
{
    return format_root(v, false);
}

char *emv_debug(const EmValue *v)
{
    return format_root(v, true);
}

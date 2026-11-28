/*
 * Standalone tests for bootstrap-c/value.c.
 *
 * Build and run from the repository root (no Makefile is added here; the
 * parent owns the shared Makefile):
 *
 *   cc -std=c11 -Wall -Wextra -Werror -Ibootstrap-c \
 *      bootstrap-c/value.c bootstrap-c/tests/test_value.c \
 *      -o /tmp/opencode/em_test_value && /tmp/opencode/em_test_value
 */

#include "value.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures = 0;

#define CHECK(cond)                                                            \
    do {                                                                       \
        if (!(cond)) {                                                         \
            fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond);    \
            failures++;                                                        \
        }                                                                      \
    } while (0)

#define CHECK_STR(actual, expected)                                            \
    do {                                                                       \
        const char *a_ = (actual);                                             \
        const char *e_ = (expected);                                           \
        if (!a_ || strcmp(a_, e_) != 0) {                                      \
            fprintf(stderr, "FAIL %s:%d: expected \"%s\", got \"%s\"\n",       \
                    __FILE__, __LINE__, e_, a_ ? a_ : "(null)");               \
            failures++;                                                        \
        }                                                                      \
    } while (0)

static void check_display(const EmValue *v, const char *expected)
{
    char *s = emv_display(v);
    CHECK_STR(s, expected);
    free(s);
}

static void check_debug(const EmValue *v, const char *expected)
{
    char *s = emv_debug(v);
    CHECK_STR(s, expected);
    free(s);
}

static EmValue *dummy_native(EmVm *vm, EmValue **args, size_t argc, char **err)
{
    (void)vm;
    (void)args;
    (void)argc;
    (void)err;
    return emv_null();
}

/* emb.h defines EmFunctionProto as an anonymous typedef that conflicts with
 * value.h's forward declaration, so the closure test only needs its first
 * field (the function name). */
struct FakeProto {
    char *name;
};

static void test_scalars(void)
{
    EmValue *n = emv_number(2.5);
    EmValue *s = emv_string("hi");
    EmValue *b = emv_bool(true);
    EmValue *z = emv_null();

    CHECK(emv_type(n) == EMV_NUMBER && emv_is_number(n));
    CHECK(emv_as_number(n) == 2.5);
    CHECK(!emv_is_string(n) && emv_strlen(n) == 0 && emv_as_cstr(n)[0] == '\0');

    CHECK(emv_type(s) == EMV_STRING && emv_is_string(s));
    CHECK_STR(emv_as_cstr(s), "hi");
    CHECK(emv_strlen(s) == 2 && !emv_is_number(s));

    CHECK(emv_type(b) == EMV_BOOL && emv_is_bool(b) && emv_as_bool(b));
    CHECK(!emv_as_bool(emv_bool(false)));

    CHECK(emv_type(z) == EMV_NULL && emv_is_null(z));
    CHECK(!emv_as_cstr(z)[0] && emv_as_number(z) == 0.0);

    emv_release(n);
    emv_release(s);
    emv_release(b);
    emv_release(z);
}

static void test_utf8_and_refcount(void)
{
    const char *utf8 = "h\303\251llo"; /* héllo: 6 bytes, 5 code points */
    EmValue *s = emv_string_len(utf8, strlen(utf8));
    EmValue *embedded = emv_string_len("a\0b", 3);
    char *shown;

    CHECK(emv_strlen(s) == 6);
    CHECK_STR(emv_as_cstr(s), utf8);
    check_display(s, utf8);
    check_debug(s, "\"h\303\251llo\"");

    CHECK(emv_strlen(embedded) == 3);
    CHECK(memcmp(emv_as_cstr(embedded), "a\0b", 3) == 0);
    shown = emv_display(embedded);
    CHECK(memcmp(shown, "a\0b", 3) == 0 && shown[3] == '\0');
    free(shown);
    {
        EmValue *blank = emv_string_len(NULL, 3);
        CHECK(emv_strlen(blank) == 3);
        CHECK(memcmp(emv_as_cstr(blank), "\0\0\0", 3) == 0);
        emv_release(blank);
    }

    emv_retain(s);
    emv_release(s);
    CHECK(emv_type(s) == EMV_STRING);
    CHECK_STR(emv_as_cstr(s), utf8);
    emv_release(s);
    emv_release(embedded);
}

static void test_arrays(void)
{
    EmValue *a = emv_array();
    EmValue *item = emv_number(7);
    EmValue *inner = emv_array();
    EmValue *outer = emv_array();
    EmValue *got;

    CHECK(emv_type(a) == EMV_ARRAY && emv_is_array(a));
    emv_array_push(a, item);
    emv_release(item);
    CHECK(emv_array_len(a) == 1);
    got = emv_array_get(a, 0);
    CHECK(got && emv_as_number(got) == 7);
    CHECK(emv_array_get(a, 1) == NULL);

    item = emv_bool(true);
    emv_array_set(a, 0, item);
    emv_release(item);
    CHECK(emv_as_bool(emv_array_get(a, 0)));
    emv_array_set(a, 5, emv_null()); /* out of range: no-op */
    CHECK(emv_array_len(a) == 1);
    check_display(a, "[true]");

    emv_array_push(inner, emv_string("x"));
    emv_array_push(inner, emv_bool(false));
    CHECK(emv_array_len(inner) == 2);
    check_display(inner, "[\"x\", false]");
    check_debug(inner, "[\"x\", false]");

    emv_array_push(outer, emv_number(1));
    emv_array_push(outer, inner);
    check_display(outer, "[1, [\"x\", false]]");

    emv_release(outer);
    emv_release(inner);
    emv_release(a);
}

static void test_objects(void)
{
    EmValue *o = emv_object();
    EmValue *v = emv_number(2);
    size_t count = 99;
    char **keys;

    CHECK(emv_type(o) == EMV_OBJECT && emv_is_object(o));
    emv_object_set(o, "b", v);
    emv_release(v);
    emv_object_set(o, "a", emv_string("A"));
    CHECK(emv_object_has(o, "a") && emv_object_has(o, "b"));
    CHECK(!emv_object_has(o, "missing"));
    CHECK(emv_as_number(emv_object_get(o, "b")) == 2);
    CHECK(emv_object_get(o, "missing") == NULL);
    check_display(o, "{a: \"A\", b: 2}");

    keys = emv_object_keys(o, &count);
    CHECK(count == 2 && keys != NULL);
    CHECK_STR(keys[0], "a");
    CHECK_STR(keys[1], "b");
    free(keys[0]);
    free(keys[1]);
    free(keys);

    emv_object_set(o, "b", emv_bool(true));
    CHECK(emv_as_bool(emv_object_get(o, "b")));
    CHECK(emv_object_delete(o, "a"));
    CHECK(!emv_object_delete(o, "a"));
    CHECK(!emv_object_has(o, "a"));
    check_debug(o, "{b: true}");

    emv_release(o);
}

static void test_truthiness(void)
{
    EmValue *empty_array = emv_array();
    EmValue *full_array = emv_array();
    EmValue *empty_object = emv_object();
    struct FakeProto proto = {"f"};
    EmValue *closure = emv_closure((EmFunctionProto *)&proto, NULL);
    EmValue *native = emv_native("echo", dummy_native);
    double zero = 0.0;
    double nan = zero / zero; /* quiet NaN without math.h */

    CHECK(!emv_truthy(emv_null()));
    CHECK(!emv_truthy(emv_bool(false)));
    CHECK(emv_truthy(emv_bool(true)));
    CHECK(!emv_truthy(emv_number(0.0)));
    CHECK(emv_truthy(emv_number(1.0)));
    CHECK(emv_truthy(emv_number(nan)));
    CHECK(!emv_truthy(emv_string("")));
    CHECK(emv_truthy(emv_string("x")));
    CHECK(!emv_truthy(empty_array));
    emv_array_push(full_array, emv_null());
    CHECK(emv_truthy(full_array));
    CHECK(emv_truthy(empty_object));
    CHECK(emv_truthy(closure));
    CHECK(emv_truthy(native));

    emv_release(empty_array);
    emv_release(full_array);
    emv_release(empty_object);
    emv_release(closure);
    emv_release(native);
}

static void test_equality(void)
{
    EmValue *one = emv_number(1);
    EmValue *one_again = emv_number(1);
    EmValue *two = emv_number(2);
    EmValue *yes = emv_bool(true);
    EmValue *yes_again = emv_bool(true);
    EmValue *no = emv_bool(false);
    EmValue *null1 = emv_null();
    EmValue *null2 = emv_null();
    EmValue *abc = emv_string("abc");
    EmValue *abc_again = emv_string("abc");
    EmValue *abd = emv_string("abd");
    EmValue *a1 = emv_array();
    EmValue *a2 = emv_array();
    EmValue *o1 = emv_object();
    EmValue *o2 = emv_object();
    struct FakeProto proto = {"f"};
    EmValue *c1 = emv_closure((EmFunctionProto *)&proto, NULL);
    EmValue *c2 = emv_closure((EmFunctionProto *)&proto, NULL);
    EmValue *n1 = emv_native("echo", dummy_native);
    EmValue *n2 = emv_native("echo", dummy_native);
    EmValue *s1 = emv_string_len("a\0b", 3);
    EmValue *s1_again = emv_string_len("a\0b", 3);
    EmValue *s2 = emv_string_len("a\0c", 3);

    CHECK(emv_equal(one, one_again));
    CHECK(!emv_equal(one, two));
    CHECK(emv_equal(yes, yes_again));
    CHECK(!emv_equal(yes, no));
    CHECK(emv_equal(null1, null2));
    CHECK(!emv_equal(one, yes));
    CHECK(emv_equal(abc, abc_again));
    CHECK(!emv_equal(abc, abd));
    CHECK(emv_equal(s1, s1_again));
    CHECK(!emv_equal(s1, s2));
    CHECK(emv_equal(a1, a1) && !emv_equal(a1, a2));
    CHECK(emv_equal(o1, o1) && !emv_equal(o1, o2));
    CHECK(emv_equal(c1, c1) && !emv_equal(c1, c2));
    CHECK(!emv_equal(n1, n2) && !emv_equal(n1, n1));

    emv_release(one);
    emv_release(one_again);
    emv_release(two);
    emv_release(yes);
    emv_release(yes_again);
    emv_release(no);
    emv_release(null1);
    emv_release(null2);
    emv_release(abc);
    emv_release(abc_again);
    emv_release(abd);
    emv_release(s1);
    emv_release(s1_again);
    emv_release(s2);
    emv_release(a1);
    emv_release(a2);
    emv_release(o1);
    emv_release(o2);
    emv_release(c1);
    emv_release(c2);
    emv_release(n1);
    emv_release(n2);
}

static void test_formatting(void)
{
    struct FakeProto proto = {"f"};
    EmValue *v = emv_number(0.0);
    check_display(v, "0");
    emv_release(v);
    v = emv_number(-0.0);
    check_display(v, "0");
    emv_release(v);
    v = emv_number(1.5);
    check_display(v, "1.5");
    emv_release(v);
    v = emv_number(0.1);
    check_display(v, "0.1");
    emv_release(v);
    v = emv_number(0.1 + 0.2);
    check_display(v, "0.30000000000000004");
    emv_release(v);
    v = emv_number(1e16);
    check_display(v, "10000000000000000");
    emv_release(v);
    v = emv_number(1e-5);
    check_display(v, "0.00001");
    emv_release(v);

    v = emv_bool(true);
    check_display(v, "true");
    check_debug(v, "true");
    emv_release(v);
    v = emv_null();
    check_display(v, "null");
    emv_release(v);

    v = emv_closure((EmFunctionProto *)&proto, NULL);
    check_display(v, "<function f>");
    CHECK(emv_closure_proto(v) == (EmFunctionProto *)&proto);
    CHECK(emv_closure_env(v) == NULL);
    emv_release(v);

    v = emv_native("echo", dummy_native);
    check_display(v, "<native function echo>");
    CHECK_STR(emv_native_name(v), "echo");
    CHECK(emv_native_fn(v) == dummy_native);
    emv_release(v);

    /* Cyclic containers must terminate rather than recurse forever. */
    v = emv_array();
    emv_array_push(v, v);
    check_display(v, "[[...]]");
    emv_release(v);
}

static void test_null_safety(void)
{
    CHECK(emv_type(NULL) == EMV_NULL);
    CHECK(!emv_truthy(NULL));
    CHECK(!emv_equal(NULL, emv_null()));
    CHECK(emv_array_len(NULL) == 0 && emv_array_get(NULL, 0) == NULL);
    CHECK(!emv_object_has(NULL, "x") && emv_object_get(NULL, "x") == NULL);
    CHECK(!emv_object_delete(NULL, "x"));
    CHECK(emv_object_keys(NULL, NULL) == NULL);
    CHECK(emv_closure_proto(NULL) == NULL && emv_native_fn(NULL) == NULL);
    emv_retain(NULL);
    emv_release(NULL);
    check_display(NULL, "null");
    check_debug(NULL, "null");
}

int main(void)
{
    test_scalars();
    test_utf8_and_refcount();
    test_arrays();
    test_objects();
    test_truthiness();
    test_equality();
    test_formatting();
    test_null_safety();

    if (failures) {
        fprintf(stderr, "%d check(s) failed\n", failures);
        return 1;
    }
    puts("test_value: ok");
    return 0;
}

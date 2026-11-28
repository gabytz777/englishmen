#ifndef EM_BOOTSTRAP_VALUE_H
#define EM_BOOTSTRAP_VALUE_H

#include <stdbool.h>
#include <stddef.h>

typedef enum {
    EMV_NUMBER = 0,
    EMV_STRING,
    EMV_BOOL,
    EMV_NULL,
    EMV_ARRAY,
    EMV_OBJECT,
    EMV_CLOSURE,
    EMV_NATIVE
} EmValueType;

typedef struct EmValue EmValue;
typedef struct EmVm EmVm;
typedef struct EmEnv EmEnv;
typedef struct EmChunk EmChunk;
typedef struct EmFunctionProto EmFunctionProto;

typedef EmValue *(*EmNativeFn)(EmVm *vm, EmValue **args, size_t argc, char **err);

/* Constructors return one strong reference. */
EmValue *emv_number(double n);
EmValue *emv_string(const char *s);
EmValue *emv_string_len(const char *s, size_t n);
EmValue *emv_bool(bool b);
EmValue *emv_null(void);
EmValue *emv_array(void);
EmValue *emv_object(void);
EmValue *emv_closure(EmFunctionProto *proto, EmEnv *env);
EmValue *emv_native(const char *name, EmNativeFn fn);

/* Reference counting. */
void emv_retain(EmValue *v);
void emv_release(EmValue *v);

/* Type queries and scalar access. */
EmValueType emv_type(const EmValue *v);
bool emv_is_number(const EmValue *v);
bool emv_is_string(const EmValue *v);
bool emv_is_bool(const EmValue *v);
bool emv_is_null(const EmValue *v);
bool emv_is_array(const EmValue *v);
bool emv_is_object(const EmValue *v);
bool emv_is_closure(const EmValue *v);
bool emv_is_native(const EmValue *v);
double emv_as_number(const EmValue *v);
bool emv_as_bool(const EmValue *v);
const char *emv_as_cstr(const EmValue *v);
size_t emv_strlen(const EmValue *v);

/* Arrays. Push/set retain; get returns a borrowed pointer. */
size_t emv_array_len(const EmValue *array);
EmValue *emv_array_get(const EmValue *array, size_t i);
void emv_array_push(EmValue *array, EmValue *item);
void emv_array_set(EmValue *array, size_t i, EmValue *item);

/* Objects. Set retains; get returns a borrowed pointer or NULL. */
bool emv_object_has(const EmValue *object, const char *key);
EmValue *emv_object_get(const EmValue *object, const char *key);
void emv_object_set(EmValue *object, const char *key, EmValue *value);
bool emv_object_delete(EmValue *object, const char *key);
char **emv_object_keys(const EmValue *object, size_t *count); /* caller frees each + array */

/* Closure/native access. */
EmFunctionProto *emv_closure_proto(const EmValue *v);
EmEnv *emv_closure_env(const EmValue *v);
EmNativeFn emv_native_fn(const EmValue *v);
const char *emv_native_name(const EmValue *v);

/* Semantics. */
bool emv_truthy(const EmValue *v);
bool emv_equal(const EmValue *a, const EmValue *b);
char *emv_display(const EmValue *v); /* caller frees */
char *emv_debug(const EmValue *v);   /* caller frees */

#endif

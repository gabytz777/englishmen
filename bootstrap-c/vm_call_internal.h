#ifndef EM_BOOTSTRAP_VM_CALL_INTERNAL_H
#define EM_BOOTSTRAP_VM_CALL_INTERNAL_H

#include <stdbool.h>
#include <stddef.h>

/*
 * emb.h owns the concrete EmFunctionProto/EmChunk definitions. The shared
 * value.h currently forward-declares EmFunctionProto as a tagged struct while
 * emb.h defines it as an anonymous typedef, so including both in one
 * translation unit is ill-formed. This private header therefore declares the
 * value/VM entry points it needs directly; the signatures match value.h and
 * vm.h and keep compiling when the parent fixes that header mismatch.
 */
#include "emb.h"

typedef struct EmValue EmValue;
typedef struct EmVm EmVm;
typedef struct EmEnv EmEnv;
typedef EmValue *(*EmNativeFn)(EmVm *vm, EmValue **args, size_t argc, char **err);

/* value.h subset used by the call machinery. */
EmValue *emv_number(double n);
EmValue *emv_string(const char *s);
EmValue *emv_string_len(const char *s, size_t n);
EmValue *emv_bool(bool b);
EmValue *emv_null(void);
EmValue *emv_array(void);
EmValue *emv_object(void);
EmValue *emv_closure(EmFunctionProto *proto, EmEnv *env);
EmValue *emv_native(const char *name, EmNativeFn fn);
void emv_retain(EmValue *v);
void emv_release(EmValue *v);
bool emv_is_number(const EmValue *v);
bool emv_is_string(const EmValue *v);
bool emv_is_null(const EmValue *v);
bool emv_is_array(const EmValue *v);
bool emv_is_object(const EmValue *v);
bool emv_is_closure(const EmValue *v);
bool emv_is_native(const EmValue *v);
double emv_as_number(const EmValue *v);
const char *emv_as_cstr(const EmValue *v);
size_t emv_strlen(const EmValue *v);
size_t emv_array_len(const EmValue *array);
EmValue *emv_array_get(const EmValue *array, size_t i);
void emv_array_push(EmValue *array, EmValue *item);
void emv_array_set(EmValue *array, size_t i, EmValue *item);
bool emv_object_has(const EmValue *object, const char *key);
EmValue *emv_object_get(const EmValue *object, const char *key);
void emv_object_set(EmValue *object, const char *key, EmValue *value);
char **emv_object_keys(const EmValue *object, size_t *count);
EmFunctionProto *emv_closure_proto(const EmValue *v);
EmEnv *emv_closure_env(const EmValue *v);
EmNativeFn emv_native_fn(const EmValue *v);
const char *emv_native_name(const EmValue *v);
bool emv_truthy(const EmValue *v);
bool emv_equal(const EmValue *a, const EmValue *b);
char *emv_display(const EmValue *v);

/* vm.h subset used by the call machinery. */
EmEnv *em_vm_global_env(EmVm *vm);
EmValue *em_vm_lookup_import(EmVm *vm, const char *name);
int em_vm_call_method(EmVm *vm, EmValue *receiver, const char *name,
                      EmValue **args, size_t argc, EmValue **out, char **err);

/* Environment API: keeps closure captures alive across call frames. */
EmEnv *em_call_env_new(EmEnv *parent);
void em_call_env_retain(EmEnv *env);
void em_call_env_release(EmEnv *env);
bool em_call_env_has(const EmEnv *env, const char *name);
EmValue *em_call_env_get(const EmEnv *env, const char *name); /* borrowed */
void em_call_env_define(EmEnv *env, const char *name, EmValue *value); /* retains */
void em_call_env_assign(EmEnv *env, const char *name, EmValue *value); /* retains */
char **em_call_env_names(const EmEnv *env, size_t *count); /* caller frees */

/* Call entry points. */
int em_vm_call_value(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                     EmValue **out, char **err);
int em_vm_call_value_named(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                           const char *name, EmValue **out, char **err);
int em_vm_call_value_fixed(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                           size_t fixed_arity, EmValue **out, char **err);

/* Helpers corresponding to the MakeClosure / Return / ReturnNull opcodes. */
EmValue *em_vm_call_make_closure(EmFunctionProto *proto, EmEnv *env);
int em_vm_call_return(EmVm *vm, EmValue *value, EmValue **out, char **err);
int em_vm_call_return_null(EmVm *vm, EmValue **out, char **err);

#endif

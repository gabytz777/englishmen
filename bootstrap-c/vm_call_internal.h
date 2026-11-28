#ifndef EM_BOOTSTRAP_VM_CALL_INTERNAL_H
#define EM_BOOTSTRAP_VM_CALL_INTERNAL_H

#include <stdbool.h>
#include <stddef.h>

#include "emb.h"
#include "value.h"
#include "vm.h"

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

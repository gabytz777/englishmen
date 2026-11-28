#ifndef EM_BOOTSTRAP_VM_H
#define EM_BOOTSTRAP_VM_H

#include "value.h"
#include "emb.h"

typedef struct EmFrame EmFrame;

EmVm *em_vm_new(void);
void em_vm_free(EmVm *vm);

int em_vm_register_native(EmVm *vm, const char *name, EmNativeFn fn);
int em_vm_register_import(EmVm *vm, const char *name, EmValue *module);
EmValue *em_vm_lookup_import(EmVm *vm, const char *name);
EmEnv *em_vm_global_env(EmVm *vm);

int em_vm_run(EmVm *vm, const EmChunk *chunk, char **err);
int em_vm_call_value(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                     EmValue **out, char **err);
int em_vm_call_method(EmVm *vm, EmValue *receiver, const char *name,
                      EmValue **args, size_t argc, EmValue **out, char **err);

void em_vm_error(EmVm *vm, uint32_t line, uint32_t col, const char *message);
char *em_vm_error_message(EmVm *vm);

#endif

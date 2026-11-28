#ifndef EM_BOOTSTRAP_BUILTINS_H
#define EM_BOOTSTRAP_BUILTINS_H

#include "value.h"

int em_builtins_register(EmVm *vm, char **err);
int em_builtin_call(EmVm *vm, const char *name, EmValue **args, size_t argc,
                    EmValue **out, char **err);
bool em_builtin_exists(const char *name);

#endif

#ifndef EM_BOOTSTRAP_HOST_H
#define EM_BOOTSTRAP_HOST_H

#include "value.h"

int em_host_register(EmVm *vm, int argc, char **argv, char **err);
EmValue *em_host_module(EmVm *vm);

#endif

/* emrun: the small C host for .emb version 2 images.
 *
 * Usage: emrun <image.emb> [program args...]
 *
 * The image is parsed with the shared emb loader, the VM is populated with
 * the global builtins and the `host` import, and the root chunk is executed.
 * All failures are reported on stderr and mapped to a nonzero exit status.
 */

#include <stdio.h>
#include <stdlib.h>

/* value.h forward-declares EmFunctionProto as a tagged struct while emb.h
 * typedefs an anonymous struct under the same name. Rename the emb.h typedef
 * for this translation unit so the shared headers can be included together;
 * the parent-owned headers themselves are left untouched. */
#define EmFunctionProto EmEmbFunctionProto
#include "emb.h"
#undef EmFunctionProto

#include "builtins.h"
#include "host.h"
#include "value.h"
#include "vm.h"

#define EM_EXIT_FAILURE 1
#define EM_EXIT_USAGE 2

static void report(const char *context, const char *message)
{
    if (message != NULL && message[0] != '\0') {
        fprintf(stderr, "%s: %s\n", context, message);
    } else {
        fprintf(stderr, "%s\n", context);
    }
}

static void report_vm_error(EmVm *vm, const char *context)
{
    char *message = em_vm_error_message(vm);
    report(context, message);
}

int main(int argc, char **argv)
{
    const char *image_path;
    const char *program = "emrun";
    EmProgram *image = NULL;
    EmVm *vm = NULL;
    char *err = NULL;
    int status = EM_EXIT_FAILURE;

    if (argc > 0 && argv[0] != NULL && argv[0][0] != '\0') {
        program = argv[0];
    }
    if (argc < 2 || argv[1] == NULL) {
        fprintf(stderr, "usage: %s <image.emb> [program args...]\n", program);
        return EM_EXIT_USAGE;
    }

    image_path = argv[1];

    if (em_emb_parse_file(image_path, &image, &err) != 0 || image == NULL) {
        report(image_path, err);
        free(err);
        return EM_EXIT_FAILURE;
    }
    free(err);
    err = NULL;

    vm = em_vm_new();
    if (vm == NULL) {
        report(program, "could not create VM");
        goto cleanup;
    }

    if (em_builtins_register(vm, &err) != 0) {
        report(program, err != NULL ? err : "could not register builtins");
        goto cleanup;
    }
    free(err);
    err = NULL;

    /* em_host_register is handed the original argv: argv[0] is emrun and
     * argv[1] is the image path, so host.args() is argv[2..argc-1]. */
    if (em_host_register(vm, argc, argv, &err) != 0) {
        report(program, err != NULL ? err : "could not register host module");
        goto cleanup;
    }
    free(err);
    err = NULL;

    /* Registration normally installs the import itself. Cover hosts that only
     * construct the module and leave the import table to the caller. */
    if (em_vm_lookup_import(vm, "host") == NULL) {
        EmValue *module = em_host_module(vm);
        if (module != NULL && em_vm_register_import(vm, "host", module) != 0) {
            report(program, err != NULL ? err : "could not register 'host' import");
            goto cleanup;
        }
    }

    if (em_vm_run(vm, &image->root, &err) != 0) {
        if (err != NULL && err[0] != '\0') {
            report(program, err);
        } else {
            report_vm_error(vm, program);
        }
        goto cleanup;
    }

    status = 0;

cleanup:
    free(err);
    em_vm_free(vm);
    em_emb_free(image);
    return status;
}

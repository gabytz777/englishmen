# bootstrap-c: the C `emrun` host

`emrun` executes version 2 `.emb` images without linking to the Rust crate.
It is the small host program described in [ABI.md](ABI.md): it loads an image,
installs the builtins and the `host` import, and runs the root chunk.

## Build

```sh
cd bootstrap-c
make
```

`make` compiles every C implementation present in this directory with
`cc -std=c11 -Wall -Wextra -O2` into `bootstrap-c/emrun` (`-I.` and `-lm` are
also used). The implementations are:

| File | Responsibility |
| --- | --- |
| `value.c` | reference-counted values |
| `emb.c` | `.emb` v2 decoder |
| `vm.c` | VM core and interpreter loop |
| `vm_call.c` | function/native calls |
| `vm_method.c` | runtime method dispatch |
| `builtins.c` | global builtins |
| `host.c` | the `host` module |
| `main.c` | this CLI |

The round-4 agents own these files separately. While other implementations are
still being integrated, `make` does not fail on missing sources: it prints the
missing file names and skips the link step. `make check-sources` reports the
same state explicitly, and `make clean` removes objects and the binary.

Integration note for the parent: the shared `value.h` and `emb.h` currently
declare `EmFunctionProto` differently (tagged struct vs. anonymous struct), so
including both at once is an error. `main.c` works around this locally by
renaming the `emb.h` typedef for its own translation unit; the parent-owned
headers should eventually be reconciled.

## Run

```sh
./emrun <image.emb> [program args...]
```

`main.c`:

1. parses the image with `em_emb_parse_file`;
2. creates a VM with `em_vm_new`;
3. registers global builtins via `em_builtins_register`;
4. registers the host module via `em_host_register`, passing the **original**
   `argc`/`argv` (`argv[0]` is `emrun`, `argv[1]` is the image path, so
   `host.args()` should be `argv[2..argc-1]`);
5. makes sure the `host` import is present (a host implementation may install
   it itself; otherwise `em_host_module` is registered by the CLI);
6. executes `image->root` with `em_vm_run`.

Parse, registration and execution errors go to stderr. Exit status is `0` on
success, `2` for a usage error (no image argument), and `1` for any other
failure. Program output is produced by the VM's `say`/`stdout` paths, not by
the CLI itself.

## Test

```sh
cd bootstrap-c
make test          # or: ./tests/test_cli.sh [image.emb]
```

The test skips cleanly when `emrun` has not been built yet. Without an explicit
image (or `EMRUN_TEST_IMAGE`), it looks for `tests/tiny.emb` /
`tests/smoke.emb` and otherwise generates a minimal 39-byte image in a temp
directory whose root chunk is one `OP_RETURN_NULL` instruction.

## Acceptance

See [ABI.md](ABI.md) for the full proof: build with `make`, produce the compiler
image with the self-hosted compiler, then check that `emrun` and the Rust
`em run-fixed` print identical output for `examples/hello.enm`.

# Round 4 C bootstrap VM contract

## Goal

Replace the Rust `em` CLI as the execution host for the round-3 self-hosted
compiler with a small independent C program:

```text
emrun <image.emb> [program args...]
```

The image is produced by the pure-Englishmen compiler in `selfhost/full.enm`.
The C runtime loads `.emb` v2 directly, implements the bytecode instruction
set, exposes the global builtins, and provides one host module named `host`.
Pure Englishmen module objects in `selfhost/pure_*.enm` wrap `host` primitives;
the compiler source must contain no native `import fs/json/os/...` statements.

## Non-goals

* No linking against the Rust crate and no use of `src/vm.rs` at runtime.
* No replacement for the Rust CLI's installer/appimage/updater/repl features.
* No claim of a runtime-free bootstrap: some host program must execute bytes.
  This project makes that host small, explicit, and non-Rust.

## Shared headers

Parent owns these headers. Do not edit them in agent branches:

* `bootstrap-c/value.h`
* `bootstrap-c/emb.h`
* `bootstrap-c/vm.h`
* `bootstrap-c/builtins.h`
* `bootstrap-c/host.h`

Implementations own only their assigned `.c` files and any private `_internal.h`
file in their own branch.

## `.emb` format

Read `src/bytecode.rs`; the C decoder must match it exactly:

* magic `EMBC`, version `u16` little-endian (`2`)
* `write_chunk`: constant count, constants, function count, prototypes, code count, instructions
* constant tags: `0` number/f64 LE, `1` length-prefixed UTF-8 string, `2` bool byte, `3` null
* every instruction: tag byte, `a:u32` LE, `b:u32` LE, `line:u32` LE, `col:u32` LE
* op tags are enumerated in `bootstrap-c/emb.h` and mirror `encode_op`

## Host module

The C VM exposes `host` through the existing `Import` opcode. Required keys:

* `args()` -> array of program arguments after the image path
* `env(name, fallback)` -> string or fallback/null
* `readFile(path)` -> string contents, or error object with `__hostError`
* `writeFile(path, text)` -> bool, or error object with `__hostError`
* `readBytes(path)` -> array of byte numbers, or error object
* `writeBytes(path, bytes)` -> bool, or error object
* `stdout(text)` -> bool
* `stderr(text)` -> bool
* `exit(code)` -> no return
* `cwd()` -> string
* `tempDir()` -> string
* `pid()` -> number

## Required builtins

Implement the global names listed in `selfhost/vm_runtime.enm`'s `rtGlobalEnv`,
plus `jsonEncode`/`jsonDecode`/`args`/`assert`. Method dispatch for runtime
methods may be implemented in the VM core and must cover the methods used by
the self-hosted compiler.

## Acceptance proof

1. Build `bootstrap-c/emrun` with `make`.
2. Regenerate `selfhost/full.enm` and `selfhost/bootstrap_compile.enm` with no
   native `import` statements outside guarded self-tests.
3. Use the existing Rust `em` binary only as a one-time oracle to emit the
   compiler image: `full.enm` compiles `bootstrap_compile.enm` to an `.emb`.
4. Run that image with `bootstrap-c/emrun`; it must print exactly:

   `{"astStatements":1,"encodedBytes":104,"irInstructions":4,"output":[]}`

   for `examples/hello.enm`.
5. Run the same `.emb` with the native Rust `em run-fixed` and compare stdout.

## Agent branches

Every agent works in its own worktree under
`/home/larpington/.cache/em-round4-wt/<slug>` from `origin/selfhost-base`,
commits, and pushes `round4/<slug>`. Never edit `/home/larpington/englishmen`,
`master`, `release/2.0.0`, or another agent's files.

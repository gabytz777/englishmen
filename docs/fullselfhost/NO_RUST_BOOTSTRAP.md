# Round 4: no-Rust bootstrap proof

This is the executable form of the acceptance proof in
`bootstrap-c/ABI.md`. The Rust `em` binary is allowed **only** as the one-time
oracle that emits the compiler image. Execution of that image must be performed
by the independent C runtime `bootstrap-c/emrun`.

Nothing here weakens the required output: both runtimes must print exactly

```text
{"astStatements":1,"encodedBytes":104,"irInstructions":4,"output":[]}
```

for `examples/hello.enm`, with byte-for-byte identical stdout.

## Environment

All commands below use the round-4 scratch directories:

```sh
export TMPDIR=/home/larpington/.cache/em-round4-tmp
export CARGO_TARGET_DIR=/home/larpington/.cache/em-round3-target
export EM_BIN="$CARGO_TARGET_DIR/debug/em"
```

Run them from the repository root.

## 1. Build the C runtime

`bootstrap-c/Makefile` and `bootstrap-c/emrun` are produced by the C runtime
agents. Until `bootstrap-c/emrun` exists, `tests/no_rust_bootstrap.rs` prints:

```text
SKIP no-rust-bootstrap: bootstrap-c/emrun is not built yet. Build the independent C runtime with `make -C bootstrap-c` ...
```

The skip is explicit; the test never substitutes `em` for `emrun`.

```sh
make -C bootstrap-c
test -x bootstrap-c/emrun
```

## 2. Regenerate the compiler sources

`tools/regen_selfhost.py` (derived from the round-3 parent `regen.py`) inlines
`compiler`, `codegen_ir`, `ir_encode`, `lib_bytes`, `lib_project`,
`lib_modules`, `vm_core`, `vm_runtime`, then every `selfhost/pure_*.enm` in
dependency order, then the driver from the existing generated file. It cuts
guarded self-tests, rewrites `os.env("EM_SELFTEST", ...)` guards to the global
`env` builtin, and strips every non-guarded `import` statement. `host` is kept
because it is the C runtime bridge, not a native module.

```sh
python3 tools/regen_selfhost.py \
  --root selfhost \
  --driver-from selfhost/full.enm \
  --marker '$- === combined full-self-hosting pipeline ===' \
  --out selfhost/full.enm --format

python3 tools/regen_selfhost.py \
  --root selfhost \
  --driver-from selfhost/bootstrap_compile.enm \
  --marker '$- === compile-only bootstrap driver ===' \
  --out selfhost/bootstrap_compile.enm --no-vm-exec --format
```

`--no-vm-exec` removes `vmRun` from `vm_core` and `vmRunWithImports` /
`vmRunWithRuntime` from `vm_runtime`, so the compile-only source cannot recurse.

Verify import cleanliness and canonical formatting:

```sh
! grep -nE '^[[:space:]]*import[[:space:]]+(fs|json|os|emb|bytes|project|math|cowsay|string)([[:space:]]|$)' \
  selfhost/full.enm selfhost/bootstrap_compile.enm
cargo run --quiet -- fmt --check selfhost/full.enm
cargo run --quiet -- fmt --check selfhost/bootstrap_compile.enm
cargo run --quiet -- build selfhost/bootstrap_compile.enm --stats
```

## 3. Emit the compiler image with the Rust oracle (once)

```sh
cargo run --quiet -- run selfhost/full.enm selfhost/bootstrap_compile.enm \
  --write-emb "$TMPDIR/bootstrap_compile.emb"
head -c 4 "$TMPDIR/bootstrap_compile.emb"   # EMBC
```

`full.enm` uses the pure lexer, parser, AST→IR code generator and encoder;
the Rust CLI only compiles `full.enm` and writes the resulting bytes.

## 4. Execute with the C runtime — the proof

```sh
bootstrap-c/emrun "$TMPDIR/bootstrap_compile.emb" examples/hello.enm
```

Required stdout (one line, no other output):

```text
{"astStatements":1,"encodedBytes":104,"irInstructions":4,"output":[]}
```

## 5. Compare with the native Rust VM

```sh
cargo run --quiet -- run-fixed "$TMPDIR/bootstrap_compile.emb" examples/hello.enm
```

Its stdout must be identical to step 4. `run-fixed` executes the same `.emb`
bytes with `src/vm.rs`; it is a comparison oracle, not the bootstrap host.

## Automated test

```sh
cargo test --all-targets no_rust_bootstrap
```

`generated_sources_have_no_unguarded_native_imports` always runs.
`no_rust_bootstrap_proof` skips with the message above until `emrun` exists,
then asserts steps 2–5 with exact string equality and a valid `EMBC` header.

## Integration notes

`selfhost/pure_*.enm` files are owned by other round-4 agents. Regeneration on
a branch that does not yet contain them produces import-free sources whose
native module globals (`fs`, `os`, `emb`, …) are supplied only after those pure
wrappers are merged. The parent must re-run the two regeneration commands
after merging the pure-module branches; that is what makes the step-4 image
loadable by `emrun`.

Step 5 also needs the round-4 CLI agent's passthrough of trailing program
arguments to `em run-fixed`; without it the CLI rejects `examples/hello.enm`
as an unknown option. The assertion is left strict — a missing passthrough is
an integration failure, not a reason to compare a different command.

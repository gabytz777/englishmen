# Round 3: finishing self-hosting

Base: `selfhost-base` at `f3ebefc`. Do not touch `master` or `release/2.0.0`.

Round 2 delivered all compiler/runtime logic in Englishmen, but three things
still block genuine self-hosting:

| Problem | Gap |
| --- | --- |
| A | The self-hosted VM rejects every `import`; `full.enm` cannot execute programs that use modules |
| B | The pure encoder returns a byte array; there is no host primitive to save it as a loadable `.emb` |
| C | There is no compile-only self-hostable source, so no proof that the toolchain can compile its own code |

All six agents branch from this contract commit. File ownership is disjoint;
do not edit files owned by another agent.

## Shared contracts

### Import bridge (A1)

`selfhost/vm_runtime.enm` gains:

```text
vmRunWithImports(ir, imports)
```

`imports` is an object mapping module name → module object, e.g.
`{bytes: {...}, fs: {...}}`. `vmRunWithRuntime(ir)` remains as a wrapper that
passes an empty table, preserving existing callers. On the `Import` opcode the
runtime binds the named object into the global environment under the module
name. A missing module must raise a clear error listing available names.

### Pure module objects (A2)

`selfhost/lib_modules.enm` exposes `lmStringModule()` and `lmJsonModule()`,
each returning an object whose keys are the native-style function names
(`upper`, `lower`, `encode`, `decode`, …). A helper `lmAllModules()` returns
`{string: ..., json: ...}`. Implementations must be pure Englishmen; native
modules are test oracles only.

### Binary image writer (B1)

`emb.writeBytes(path, bytes)` is a new native host primitive in
`src/modules/emb.rs`: it accepts a path string and an array of whole numbers
`0..=255` and writes those exact bytes. It exists so the pure encoder's output
can be saved without a UTF-8 conversion.

### Combined driver (B2)

`selfhost/full.enm` gains an optional `--write-emb <path>` flag. After
`ieEncode(irFull)` it writes the byte array with the already-available native
`bytes.write(path, encodedFull)`. B1 adds `emb.writeBytes` as a convenience
alias in the bytecode module; both paths must produce a loadable image. Keep
the existing `vmRunWithRuntime(irFull)` call unchanged: the parent wires the
import table and switches to `vmRunWithImports` after A1/A2 land, so this PR
stays independent and testable.

### Compile-only source (C1)

`selfhost/bootstrap_compile.enm` is a self-contained, no-execution variant of
the combined pipeline: lex → parse → AST → IR → bytes → print one JSON summary
line with `astStatements`, `irInstructions`, `encodedBytes`, and `output: []`.
It must contain no `vmRunWithRuntime`/`vmRunWithImports` call, so running its
compiled IR cannot recurse.

## Language constraints

- Englishmen has **no local `.enm` imports** between self-hosted files. Every
  library script is self-contained, with prefixed global function names.
- Self-tests go at the bottom, guarded by `if os.env("EM_SELFTEST", "") == "1"`.
  With that variable set, `em run selfhost/<file>.enm` must print `ok`.
- Native modules (`emb`, `bytes`, `project`, `fs`, `os`, `json`, `string`) are
  reference oracles or host substrate, never implementations being replaced.
  The pure `bytes`/`project`/`string`/`json` wrappers are preferred when
  building the import table.
- Run `em fmt --check` on every `.enm` file and `cargo test --all-targets` for
  Rust tests. Use `TMPDIR=/home/larpington/.cache/em-round3-tmp` and
  `CARGO_TARGET_DIR=/home/larpington/.cache/em-round3-target`.
- Push your branch, then open a PR against `selfhost-base` with
  `gh pr create --base selfhost-base`.

## Agent ownership

| Agent | Problem | Branch | Owns |
| --- | --- | --- | --- |
| A1 | A | `sh3-import-bridge` | `selfhost/vm_runtime.enm` |
| A2 | A | `sh3-pure-modules` | `selfhost/lib_modules.enm`, `docs/fullselfhost/IMPORTS.md` |
| B1 | B | `sh3-emb-bytes` | `src/modules/emb.rs`, `tests/emb_module.rs`, `docs/fullselfhost/BINARY_IO.md` |
| B2 | B | `sh3-emb-driver` | `selfhost/full.enm`, `tests/selfhost_full_pipeline.rs`, `docs/fullselfhost/STATUS.md` |
| C1 | C | `sh3-bootstrap-source` | `selfhost/bootstrap_compile.enm` |
| C2 | C | `sh3-self-compile-test` | `tests/selfhost_self_compile.rs`, `docs/fullselfhost/BOOTSTRAP.md` |

## Integration order

A1 → A2 → B1 → B2 → C1 → C2. Sequential squash merges, as before.

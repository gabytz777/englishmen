# Round 2: closing the remaining self-hosting gaps

Base: `selfhost-base` at `9feecc2`. Do not touch `master` or `release/2.0.0`.

This round targets the three things that are still Rust-only:

| Problem | Gap |
| --- | --- |
| A | `selfhost/compiler.enm` stops at AST JSON; no Englishmen AST→bytecode code generation |
| B | `bytes`, `emb`, `project` are native Rust modules; no `.enm` implementation exists |
| C | The VM and runtime that execute everything are still Rust |

All six agents branch from this contract commit. File ownership is disjoint; do not edit files owned by another agent.

## Shared data shapes

**AST envelope** (produced by `selfhost/compiler.enm`, canonical `json.encode` output):

```text
{version: 1, statements: [...]}
```

Object keys are alphabetical at every level because Englishmen objects are `BTreeMap`-backed. Inspect `docs/selfhost/AST_JSON.md` for node fields.

**`emb` IR** (from `docs/selfhost/EMB_AND_MANIFEST.md`):

```text
{
  version: 2,
  constants: [{tag: "number", value: 1}, {tag: "string", value: "hi"}, {tag: "bool", value: true}, {tag: "null"}],
  functions: [{name: "f", params: ["x"], chunk: <chunk>}],
  code: [{op: "LoadConst", a: 0, b: 0, line: 1, col: 1}, ...]
}
```

A *chunk* is `{constants: [...], functions: [...], code: [...]}`; the top-level IR adds `version`. Op names are the Rust `Op` variant names in `docs/selfhost/EMB_AND_MANIFEST.md`; `emb.encodeOp(name)` is the reference mapping.

**Buffers** are ordinary Englishmen arrays of whole numbers `0..=255`, exactly as in `src/modules/bytes.rs`. `fs.write` only accepts UTF-8 strings, so a pure-Englishmen encoder returns a byte array; it cannot yet write an arbitrary binary file by itself.

## Language constraints

- Englishmen has **no local `.enm` imports**. Every library script must be self-contained, with prefixed global function names and no `import` of another `selfhost/*.enm` file.
- Self-tests go at the bottom of the script, guarded by `if os.env("EM_SELFTEST", "") == "1"`. With that variable set, `em run selfhost/<file>.enm` must print `ok` and exit 0 on success.
- Use native modules (`emb`, `bytes`, `project`, `fs`, `os`) only as **reference oracles** in tests, never as the implementation being replaced.
- Run `em fmt --check` on every new `.enm` file and `cargo test --all-targets` for Rust-side tests.
- Keep commits small and focused. Push your branch; open a PR against `selfhost-base` using `gh pr create --base selfhost-base`.

## Agent ownership

| Agent | Problem | Branch | Owns (new files only) |
| --- | --- | --- | --- |
| A1 | A | `sh2-codegen-ir` | `selfhost/codegen_ir.enm` |
| A2 | A | `sh2-ir-encode` | `selfhost/ir_encode.enm`, `tests/selfhost_ir_encode.rs`, `docs/fullselfhost/CODEGEN.md` |
| B1 | B | `sh2-bytes-lib` | `selfhost/lib_bytes.enm` |
| B2 | B | `sh2-project-lib` | `selfhost/lib_project.enm`, `tests/selfhost_enm_modules.rs`, `docs/fullselfhost/MODULES.md` |
| C1 | C | `sh2-vm-core` | `selfhost/vm_core.enm` |
| C2 | C | `sh2-vm-runtime` | `selfhost/vm_runtime.enm`, `tests/selfhost_enm_vm.rs`, `docs/fullselfhost/VM.md` |

## Deliverables

**A1 — `codegen_ir.enm`**
Implement `cgCodegen(ast)` that converts the AST envelope into an `emb` IR object. Required subset: number/string/bool/null literals, `say`, variable define/get/assign, unary and binary operators, blocks, `if`/`else`, `while`/`repeat`/`for`, function declarations and calls. Unsupported nodes should raise a clear error rather than emit wrong bytecode. Inline self-tests build AST fixtures by hand and assert IR shape.

**A2 — `ir_encode.enm`**
Implement `ieEncode(ir)` that returns an array of bytes byte-identical to native `emb.encode(ir)` / `bytecode::serialize` for the IR subset above. Include magic, LE u16/u32, f64 constants, length-prefixed UTF-8 strings, recursive functions, and 17-byte instructions with opcode tags. Rust test compares many fixtures against `emb.encode`. Do not implement file writing.

**B1 — `lib_bytes.enm`**
Implement pure-Englishmen equivalents of the `bytes` module: `btFromString`, `btToString`, `btOf`, `btLength`, `btGet`, `btSet`, `btConcat`, `btSlice`, `btCopy`, `btFromHex`, `btToHex`, `btU8`, `btU16`, `btU32`, `btU64`, `btI16`, `btI32`, `btI64`, `btF64`, and LE readers. Self-tests compare against native `bytes.*`.

**B2 — `lib_project.enm`**
Implement pure-Englishmen `.emc` handling: `pjParse(text)`, `pjName(manifest)`, `pjVersion(manifest)`, `pjMain(manifest)`, `pjWrite(manifest)`. Match native `project.*` parsing/escaping/round-trip behavior. Rust test compares parsed objects and rewritten text against native module calls.

**C1 — `vm_core.enm`**
Implement `vmRun(ir)` executing the `emb` IR subset: constants, globals, stack, arithmetic/comparison/logical ops, jumps, conditional jumps, function calls/returns, arrays and objects. Return an array of values produced by `Say` so tests can assert output. Self-tests on hand-built IR fixtures.

**C2 — `vm_runtime.enm`**
Implement runtime builtins and a CLI-shaped entry point for the self-hosted VM: `rtSay`, `rtLen`, numeric/string/array/object methods, and `vmRunWithRuntime(ir)` that shares output with C1's convention. Rust test compares its output array with the Rust VM for the same IR fixtures. Document what still needs native `emb.encode` bytes.

## Integration note

Because local `.enm` imports do not exist yet, the parent will integrate these self-contained pieces by inlining/layering them into a combined `selfhost/full.enm` after all six PRs are reviewed. Each agent should keep its code easy to concatenate: unique prefixes, no top-level side effects outside the self-test guard, and no duplicate function names.

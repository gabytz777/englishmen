# Road to a self-hosting Englishmen compiler

**Status: 5 of 5 bootstrap blockers complete, plus a self-hosted codegen,
encoder, VM, and runtime.** A compiler front end written in Englishmen produces
the same AST JSON as the Rust compiler for every bundled example, and the
combined `selfhost/full.enm` runs a source file end to end through the
Englishmen implementations. A Rust-built `em` host is still needed to execute
any `.enm` script. See [round-2 status](fullselfhost/STATUS.md).

## Why this is a useful goal

A self-hosted front end forces the language to have first-class text and
collection processing, reliable file I/O and deterministic output, structured
errors with enough control flow for a recursive-descent parser, and a stable
binary container. Those are useful language features in their own right.

## Capability checklist

| Area | Needed by a self-hosted compiler | Delivered |
| --- | --- | --- |
| Lexing | character access, string indexing, escape handling, comments | `selfhost/lexer.enm`, token-compatible with `src/lexer.rs` on every example |
| Parsing | arrays, objects, recursion, loop control, ternaries | `selfhost/parser.enm`, full AST envelopes plus structured errors |
| AST | tagged objects with nested fields | schema-ordered objects, `docs/selfhost/AST_JSON.md` |
| Binary I/O | little-endian packing and raw buffers | `bytes` module (`src/modules/bytes.rs`) |
| Bytecode output | a writer for the `.emb` container | `emb` module (`src/modules/emb.rs`), byte-identical to `bytecode::serialize` |
| Metadata | project name/version and argv | `project` module (`src/modules/project.rs`) for `.emc` manifests |
| Emitter | AST to canonical JSON | `selfhost/emit.enm` |
| Bootstrap | one CLI entry point | `selfhost/compiler.enm`, inlined lexer/parser/emitter |
| Verification | compare against the Rust front end | `em build <file> --emit ast-json`, byte-for-byte tests |

The five original blockers were: (1) byte buffers and LE packing, (2) an `.emb`
writer, (3) an `.emc` manifest API, (4) a lexer/parser/AST emitter in
Englishmen, and (5) bootstrap output parity. All five are implemented and
tested. A second round added `cgCodegen` (AST → IR), `ieEncode` (IR → bytes),
pure-Englishmen `bytes`/`project` libraries, and a VM plus runtime, combined in
`selfhost/full.enm`.

## How to verify

```sh
cargo test --all-targets
em fmt --check selfhost/*.enm examples/*.enm
em build examples/hello.enm --emit ast-json
em run selfhost/compiler.enm examples/hello.enm
em run selfhost/emit.enm examples/hello.enm
```

The last three commands print the same single JSON line. `tests/selfhost_bootstrap.rs`
and `tests/selfhost_emitter.rs` assert that equality for every `examples/*.enm`
on every full test run.

## Key conventions

- AST JSON object keys are **alphabetical at every level**, because all
  producers use `json.encode` and Englishmen objects are `BTreeMap`-backed.
  Field lists in the docs are in reading order only; consumers must look up
  fields by name.
- `em run <script> <file>` passes the full argv to the script, so the `.enm`
  CLIs scan for the last existing `.enm` path rather than assuming `args[0]`.
- Interpolated strings lower to `String("")` plus `+` chains with source spans
  matching `src/parser.rs::parse_interpolated`.
- `em run-fixed <file.emb>` runs a bytecode image directly through
  `bytecode::deserialize` and the VM.

## Non-goals for this bootstrap

- Native machine code. Englishmen targets its own bytecode; that is a design
  decision, not a temporary limitation.
- Replacing the Rust host executable. The VM and runtime logic also exist in
  Englishmen now, but a Rust-built `em` is still needed to run any `.enm`.
- Backwards compatibility with `.em` source files. That extension is retired.

Further work could replace the inlined pieces with local `.enm` imports once
those exist, support native module imports inside the self-hosted VM, add
binary file writing, and add multi-error recovery. None of these are required
for the current parity guarantee.

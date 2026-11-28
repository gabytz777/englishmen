# `.emb` IR and the `project` manifest API

This document describes the two Englishmen-callable surfaces used to move from
a project manifest to a runnable bytecode image:

- **`emb`** — build an intermediate representation (IR) of a `Chunk` and write a
  byte-for-byte valid `.emb` file from an Englishmen program.
- **`project`** — read, inspect and write `.emc` configuration files from an
  Englishmen program.

Both modules are part of the self-hosting contract in
[`CONTRACT.md`](CONTRACT.md) (sections 2 and 3). They are native Rust modules
registered by the parent integrator; no `.enm` file can implement them yet.

> **Parent TODO:** the worked example below invokes `em run-fixed <file.emb>`.
> That CLI alias is not present in the current tree. The parent must add it as
> a thin wrapper around the existing bytecode runner (`cmd_run_bytecode`), or
> tell this branch the intended command name. Until then `em <file.emb>` runs
> the same image; the integration tests deliberately keep using `run-fixed`.

## 1. The `.emb` container

`bytecode::serialize` writes:

```
"EMBC"                       4 bytes, magic
version                      u16 little-endian (currently 2)
chunk                        see below
```

A *chunk* is a compiled unit of code: a program or one function body. The
encoding is recursive:

```
u32 constant_count
  for each constant: u8 tag, then payload
u32 function_count
  for each function: u32 name_len, name bytes,
                     u32 param_count, params (length-prefixed strings),
                     chunk
u32 instruction_count
  for each instruction: u8 tag, u32 a, u32 b, u32 line, u32 col
```

Constant tags:

| Tag | Payload | Englishmen IR |
| --- | --- | --- |
| `0` | `f64` little-endian | `{tag: "number", value: 1}` |
| `1` | length-prefixed UTF-8 string | `{tag: "string", value: "hi"}` |
| `2` | one byte, `0` or `1` | `{tag: "bool", value: true}` |
| `3` | none | `{tag: "null"}` |

Every instruction is 17 bytes on disk (`1 + 4 + 4 + 4 + 4`). Jump targets are
absolute instruction addresses; one past the last instruction means "done".
The loader rejects bad magic, unknown versions/tags/opcodes, truncated files,
name or constant indexes outside the pool, jump targets past the end, and
functions nested more than 32 levels deep.

### 1.1 Opcode tags

`op` in the IR uses the Rust `Op` variant name. `emb.encodeOp(name)` returns the
numeric tag, which is handy when an emitter needs to pack instructions itself.

```
LoadConst=0 Pop=1 GetVar=2 SetVar=3 DefineVar=4 GetProp=5 SetProp=6
GetVarOrObject=7 GetPropOrObject=8 GetIndex=9 SetIndex=10 MakeArray=11
MakeObject=12 MakeClosure=13 Call=14 MethodCall=15 Say=16 Add=17 Sub=18
Mul=19 Div=20 Mod=21 FloorDiv=42 Pow=43 BitAnd=44 BitOr=45 BitXor=46
Shl=47 Shr=48 Eq=22 NotEq=23 Gt=24 Lt=25 GtEq=26 LtEq=27 Neg=28 Not=29
BitNot=49 ToBool=30 Jump=31 JumpIfFalse=32 JumpIfFalsePeek=33
JumpIfTruePeek=34 RepeatSetup=35 RepeatNext=36 IterSetup=37 IterNext=38
Return=39 ReturnNull=40 Import=41 Dup=50 Rotate=51 GetPropKeep=52
GetIndexKeep=53 JumpIfNullPeek=54
```

Note the two-operand shapes: `Call(argc, nameIndex)` and
`MethodCall(nameIndex, argc)` both store `a`/`b`, and `IterNext(nameIndex,
target)` uses both too. For name operands the index must point at a **string**
constant in the same chunk.

## 2. The `emb` module

```js
import emb

emb.magic()          // "EMBC"
emb.version()        // 2, the VM's EMB_VERSION
emb.encodeOp("Add")  // 17
emb.encode(ir)       // the .emb image as a byte buffer
emb.write(path, ir)  // write that image to disk; returns true
```

The IR object has four top-level fields:

```js
{
  version: 2,               // must equal emb.version()
  constants: [              // array order is the constant-pool index
    {tag: "number", value: 1},
    {tag: "string", value: "hi"},
    {tag: "bool", value: true},
    {tag: "null"}
  ],
  functions: [              // nested prototypes
    {
      name: "f",
      params: ["a"],
      chunk: {constants: [], functions: [], code: []}
    }
  ],
  code: [
    {op: "LoadConst", a: 0, b: 0, line: 1, col: 1}
  ]
}
```

Nested `chunk` objects contain only `constants`, `functions` and `code` — no
`version`. `a` and `b` default to `0`; `line` and `col` default to `1`.
Unknown constant tags, unknown op names, non-integral operands and version
mismatches are reported as Englishmen-style errors rather than panics.
`emb.write` uses `std::fs::write` directly, so it does not depend on `bytes`;
`emb.encode` returns a buffer for callers that want to process the bytes.

A minimal program that says one line is:

```js
ir = {
  version: 2,
  constants: [{tag: "string", value: "hello from emb"}],
  functions: [],
  code: [
    {op: "LoadConst", a: 0},
    {op: "Say", a: 1},
    {op: "ReturnNull"}
  ]
}
emb.write("hello.emb", ir)
```

`Say` takes an argument count in `a`, so this image prints exactly one value.
Run it with `em run-fixed hello.emb` (or `em hello.emb` on the current CLI).

## 3. The `project` module

`project` mirrors `src/config.rs`, which parses the small `.emc`
(Englishmen Configuration) format used by `em new`.

| Function | Result |
| --- | --- |
| `project.parse(text)` | `{root: object, sections: object}` |
| `project.load(path?)` | Parse a file; default `englishmen.emc` in `fs.cwd()` |
| `project.get(config, key)` | Root value, or `null` when absent |
| `project.section(config, name)` | Section object, or an empty object |
| `project.name(config)` | Shortcut for `get(config, "name")` |
| `project.version(config)` | Shortcut for `get(config, "version")` |
| `project.main(config)` | Shortcut for `get(config, "main")` |
| `project.default(name)` | Same text as `config::default_emc(name)` |
| `project.write(path, config)` | Serialize and write `.emc` text; returns `true` |

Parsing trims each line, ignores blanks and `#` comments, starts a section with
`[name]`, and stores `key=value` with the value trimmed. Surrounding double
quotes are removed. Serialization writes root keys first, then sections with
`[name]` headers and one `key = "value"` line each; `BTreeMap` ordering makes
the output stable. `Number`, `Bool` and `Null` values are formatted as bare
values instead of quoted strings.

```js
import project

config = project.parse("name = \"Demo\"\nmain = \"main.enm\"\n")
say(project.name(config))        // Demo
say(project.main(config))        // main.enm
say(project.get(config, "missing") == null)  // true
```

## 4. Worked example: manifest to runnable image

[`examples/emb_manifest.enm`](../../examples/emb_manifest.enm) is a complete,
runnable program. It parses an inline sample `.emc`, builds an IR whose `Say`
text includes the manifest name, writes a `.emb`, and prints the name and the
number of bytes actually written. The destination comes from `EM_EMB_OUT` and
falls back to a pid-scoped path in `os.tempDir()`, so the example is
side-effect-free in the repository and safe to run on both backends.

```sh
em run examples/emb_manifest.enm
# ManifestDemo
# <byte length>

EM_EMB_OUT=/tmp/demo.emb em run examples/emb_manifest.enm
em run-fixed /tmp/demo.emb
# Hello from ManifestDemo
```

The image written by the example is:

```js
ir = {
  version: emb.version(),
  constants: [{tag: "string", value: "Hello from " + name}],
  functions: [],
  code: [
    {op: "LoadConst", a: 0},
    {op: "Say", a: 1},
    {op: "ReturnNull"}
  ]
}
```

Because the IR is built in Englishmen and serialized by the same native
function the Rust compiler uses, `em run-fixed` goes straight to
`bytecode::deserialize` and the VM: no source file is consulted.

## 5. Tests

- `tests/emb_manifest_example.rs` runs the example on the compiler/VM and the
  tree-walking interpreter, requiring byte-identical stdout.
- `tests/emb_roundtrip.rs` runs a small Englishmen generator that builds a
  multi-constant IR with a nested function, calls `emb.write`, then invokes
  `em run-fixed` and checks the VM's output.

Both tests require the parent to register `emb` and `project` in
`STD_MODULES` and `load_module`, and require the `run-fixed` CLI alias. They
are expected to fail in this agent branch until that integration lands; the
failures are integration gaps, not test bugs.

### TODO for the parent integrator

1. Register `emb` and `project` in `src/modules/mod.rs` (`STD_MODULES` and
   `load_module`) and in the CLI module catalogue.
2. Add `em run-fixed <file.emb>` as an alias for `cmd_run_bytecode`.
3. Run `cargo test --test emb_manifest_example --test emb_roundtrip`.
4. If an explicit Rust-side `bytecode::deserialize` assertion is wanted in
   addition to the VM check, add it as a unit test in the branch that owns
   `bytecode.rs` or introduce a library target: a `tests/*.rs` integration test
   cannot import modules from this binary-only crate.

# EMB IR — the self-hosted bytecode interchange format

This document is the authoritative, human-readable specification of the object
value that the `emb` module consumes. It is the contract-side companion to
section 2 of [`CONTRACT.md`](CONTRACT.md) and describes the exact shape that
`emb.encode(ir)` and `emb.write(path, ir)` accept.

The IR is deliberately just an Englishmen object. A program written in
Englishmen can therefore build a bytecode image with ordinary object and array
literals, hand it to `emb.write`, and have the resulting `.emb` file run on the
VM like any compiler-produced file.

## Compatibility oracle

> **`bytecode::deserialize(&emb.encode(ir))` is the compatibility oracle.**

`emb.encode` must return a byte buffer that is **byte-for-byte identical** to
`bytecode::serialize` applied to the equivalent Rust `Chunk`. In particular:

- `bytecode::deserialize` must accept the bytes without error.
- Constants, nested functions, instructions and spans must round-trip.
- `emb.write` writes exactly the bytes that `emb.encode` returns.

If the two ever disagree, the Rust implementation in `src/bytecode.rs` is
right and the `emb` module is wrong. The wire layout in
[Binary layout](#binary-layout) is the exact layout `bytecode::serialize`
emits; an emitter that follows it produces canonical images.

## Top-level object shape

The root value passed to `emb.encode`/`emb.write` is an object with exactly
four fields:

```js
{
  version: 2,               // must equal the VM's EMB_VERSION (currently 2)
  constants: [              // the root chunk's constant pool
    {tag: "number", value: 1},
    {tag: "string", value: "hi"},
    {tag: "bool", value: true},
    {tag: "null"}
  ],
  functions: [              // nested function prototypes
    {
      name: "f",
      params: ["a"],
      chunk: {constants: [], functions: [], code: []}
    }
  ],
  code: [                   // instructions in execution order
    {op: "LoadConst", a: 0, b: 0, line: 1, col: 1}
  ]
}
```

| Field | Type | Required at root? | Meaning |
| --- | --- | --- | --- |
| `version` | number | yes (root only) | Format version; must equal `emb.version()` and the VM's `EMB_VERSION`. |
| `constants` | array of constant objects | yes | Index in this array is the constant-pool index used by instructions. |
| `functions` | array of function objects | yes | Index in this array is the prototype index used by `MakeClosure`. |
| `code` | array of instruction objects | yes | Instruction address is the array index; `Jump` targets are absolute addresses. |

Field order in the Englishmen object is irrelevant: values are looked up by
key. Array order is significant everywhere.

## Nesting rules

A compiled program is a **tree of chunks**. The root is the whole program;
each function prototype owns one nested chunk.

- The root object has `version`, `constants`, `functions` and `code`.
- A **chunk object** has only `constants`, `functions` and `code`. It must **not**
  carry a `version` field — version is a property of the file, not of a chunk.
- A **function object** has exactly `name` (string), `params` (array of
  strings) and `chunk` (a chunk object). `name` and each parameter are written
  into the binary as UTF-8 strings.
- Chunks may nest arbitrarily deep, but the loader caps recursion at depth 32
  (`MAX_CHUNK_DEPTH` in `src/bytecode.rs`). Deeper images are rejected as
  `Corrupt bytecode: functions are nested too deeply`.
- **Constant pools are per chunk.** An instruction's `a` index always refers
  to the constants of the chunk that contains that instruction, never to an
  ancestor's pool. Names, numeric literals and strings used by a nested
  function must be present in that function's own `constants` array.
- `MakeClosure(a)` selects `functions[a]` of the containing chunk; it does not
  reach into a nested chunk's `functions` array.
- Every root and nested `code` array may be empty. An empty root program is a
  valid `.emb` image.

## Constant tags

Constants are objects of the form `{tag: "<tag>", value: <value>}`. The tag is
a **string**; the on-disk numeric tag is derived from it as shown here.

| IR `tag` | `value` | On-disk tag | Encoded payload |
| --- | --- | --- | --- |
| `"number"` | a number | `0` | IEEE-754 `f64`, little-endian (8 bytes) |
| `"string"` | a string | `1` | u32 byte length + UTF-8 bytes |
| `"bool"` | `true` or `false` | `2` | one byte, `0` or `1` |
| `"null"` | omitted (the object has only `tag`) | `3` | none |

Examples:

```js
{tag: "number", value: 42}
{tag: "string", value: "hi"}
{tag: "bool", value: false}
{tag: "null"}
```

An unknown tag, or a missing/invalid `value` for a tag, is an error. Only
scalars are constants: arrays, objects and closures are built at runtime by
`MakeArray`, `MakeObject` and `MakeClosure`.

## Instructions and defaults

Instructions are objects with up to four keys:

```js
{op: "LoadConst", a: 0, b: 0, line: 1, col: 1}
```

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `op` | string | **required** | One of the opcode names in the table below (the Rust `Op` variant name, case-sensitive). |
| `a` | number | `0` | First operand — meaning depends on `op`. |
| `b` | number | `0` | Second operand — meaning depends on `op`. |
| `line` | number | `1` | Source line recorded in the instruction's span, used for runtime errors. |
| `col` | number | `1` | Source column recorded in the instruction's span. |

So `{op: "Say", a: 1}` is identical to
`{op: "Say", a: 1, b: 0, line: 1, col: 1}`. `a` and `b` must be **integral**
numbers in `u32` range; fractional operands are errors. `line`/`col` are
stored as `u32` as well. Unknown opcode names are errors.

`emb.encodeOp(name)` returns the numeric tag for an opcode name (for example
`emb.encodeOp("LoadConst") == 0`, `emb.encodeOp("Say") == 16`), which is handy
for `.enm` emitters that want to check their own tables.

## Opcode table

Names match the Rust `Op` variants; tags match `bytecode::encode_op`. `—`
means the operand is ignored and defaults to `0`.

| Tag | `op` | `a` | `b` |
| --- | --- | --- | --- |
| 0 | `LoadConst` | constant index | — |
| 1 | `Pop` | — | — |
| 2 | `GetVar` | name index | — |
| 3 | `SetVar` | name index | — |
| 4 | `DefineVar` | name index | — |
| 5 | `GetProp` | name index | — |
| 6 | `SetProp` | name index | — |
| 7 | `GetVarOrObject` | name index | — |
| 8 | `GetPropOrObject` | name index | — |
| 9 | `GetIndex` | — | — |
| 10 | `SetIndex` | — | — |
| 11 | `MakeArray` | item count | — |
| 12 | `MakeObject` | key/value **pair** count | — |
| 13 | `MakeClosure` | function index | — |
| 14 | `Call` | argument count | name index |
| 15 | `MethodCall` | name index | argument count |
| 16 | `Say` | argument count | — |
| 17 | `Add` | — | — |
| 18 | `Sub` | — | — |
| 19 | `Mul` | — | — |
| 20 | `Div` | — | — |
| 21 | `Mod` | — | — |
| 22 | `Eq` | — | — |
| 23 | `NotEq` | — | — |
| 24 | `Gt` | — | — |
| 25 | `Lt` | — | — |
| 26 | `GtEq` | — | — |
| 27 | `LtEq` | — | — |
| 28 | `Neg` | — | — |
| 29 | `Not` | — | — |
| 30 | `ToBool` | — | — |
| 31 | `Jump` | absolute target address | — |
| 32 | `JumpIfFalse` | absolute target address | — |
| 33 | `JumpIfFalsePeek` | absolute target address | — |
| 34 | `JumpIfTruePeek` | absolute target address | — |
| 35 | `RepeatSetup` | — | — |
| 36 | `RepeatNext` | absolute target address | — |
| 37 | `IterSetup` | — | — |
| 38 | `IterNext` | name index (loop variable) | absolute target address |
| 39 | `Return` | — | — |
| 40 | `ReturnNull` | — | — |
| 41 | `Import` | module name index | — |
| 42 | `FloorDiv` | — | — |
| 43 | `Pow` | — | — |
| 44 | `BitAnd` | — | — |
| 45 | `BitOr` | — | — |
| 46 | `BitXor` | — | — |
| 47 | `Shl` | — | — |
| 48 | `Shr` | — | — |
| 49 | `BitNot` | — | — |
| 50 | `Dup` | — | — |
| 51 | `Rotate` | rotation count (at most 16) | — |
| 52 | `GetPropKeep` | name index | — |
| 53 | `GetIndexKeep` | — | — |
| 54 | `JumpIfNullPeek` | absolute target address | — |

A **name index** must point at a string constant in the same chunk, or the
loader rejects the image with `Corrupt bytecode: name index out of range`.
`LoadConst` must point at an existing constant and `MakeClosure` at an existing
function. Jump targets are absolute instruction addresses; one past the last
instruction means "finished". `Rotate`'s count is capped at 16. These are the
checks performed by `bytecode::validate`, which `deserialize` runs before
returning the chunk.

Note the two call instructions use their operands differently: `Call` is
`(argument count, name)` while `MethodCall` is `(name, argument count)`.
`Say` takes an argument count, not a name — the VM drains that many stack
values and prints them separated by spaces.

## Binary layout

`emb.encode`/`emb.write` produce the same bytes as `bytecode::serialize`.
All integers are little-endian; strings are a u32 **byte** length followed by
UTF-8 bytes.

```
"EMBC"                     4 magic bytes (emb.magic())
u16 version                2 bytes  (emb.version(), currently 2)
chunk:
  u32 constant count
  for each constant:
    u8 tag                 0=number 1=string 2=bool 3=null
    payload                f64 | string | u8 | nothing
  u32 function count
  for each function:
    string name
    u32 parameter count, then each parameter as a string
    chunk                  (recursive; same layout, no version)
  u32 instruction count
  for each instruction:
    u8 tag, u32 a, u32 b, u32 line, u32 col
```

## Module surface used by this spec

| Function | Result |
| --- | --- |
| `emb.encode(ir)` | The canonical `.emb` bytes as an Englishmen array of numbers. |
| `emb.write(path, ir)` | Writes those bytes to `path`; returns `true`. |
| `emb.encodeOp(name)` | Numeric tag for an opcode name. |
| `emb.magic()` | `"EMBC"`. |
| `emb.version()` | `2` (the VM's `EMB_VERSION`). |

Every function reports argument problems as an error value with an
Englishmen-style message rather than panicking.

## Worked example: write an image from Englishmen and run it

The following program builds the smallest useful image: push a string and
`say` it. `LoadConst` reads constant 0; `Say` takes one argument from the
stack. Save it as `build_image.enm`; the emitted file is a normal `.emb`
program.

```js
$- Build a tiny EMB image from Englishmen. -$
import emb

ir = {
    version: 2,
    constants: [
        {tag: "string", value: "hello from emb"}
    ],
    functions: [],
    code: [
        {op: "LoadConst", a: 0, b: 0, line: 1, col: 1},
        {op: "Say", a: 1, b: 0, line: 1, col: 1}
    ]
}

encoded = emb.encode(ir)
emb.write("hello.emb", ir)

say(encoded.length)            # 71, the canonical image size
say(emb.magic())               # EMBC
say(emb.version())             # 2
say(emb.encodeOp("LoadConst")) # 0
say(emb.encodeOp("Say"))       # 16
```

```sh
em run build_image.enm       # prints five lines: 71 / EMBC / 2 / 0 / 16
em run-fixed hello.emb       # prints: hello from emb
```

`71` follows from the binary layout above: 6 header bytes, 4 bytes for the
constant count, 19 for the string constant (tag + length + 14 characters), 4
for the function count, 4 for the instruction count, and 17 bytes per
instruction × 2. `em run-fixed` deserialises the file with
`bytecode::deserialize` — the compatibility oracle — and executes it on the
VM, so a successful run proves the self-hosted `emb` emitter produced a valid
image.

## Verifying an emitter

- **Rust oracle:** `bytecode::deserialize(&emb.encode(ir))` must succeed and
  round-trip constants, functions and instructions. This is the definitive
  check and belongs in `tests/emb_roundtrip.rs`.
- **CLI check:** `tests/emb_ir.rs` builds an IR in a temporary `.enm` program,
  calls `emb.write`, then runs `em run-fixed <path>` and checks the VM's
  stdout. It also exercises the helpers and confirms a corrupted version field
  is rejected.

Both are needed: the Rust test proves structural compatibility, the CLI test
proves the whole file path works from a self-hosted program.

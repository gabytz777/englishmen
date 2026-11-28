# Pure-Englishmen IR encoding (`selfhost/ir_encode.enm`)

`ieEncode(ir)` converts the `emb` IR described in
[`docs/selfhost/EMB_AND_MANIFEST.md`](../selfhost/EMB_AND_MANIFEST.md) into an
array of whole numbers `0..=255`. The result is byte-for-byte identical to
native `emb.encode(ir)` and therefore to `bytecode::serialize`.

The script is self-contained: Englishmen has no local `.enm` imports. Every
global helper is prefixed with `ie`, there are no top-level side effects, and
`emb`/`os` are imported only to run the guarded self-tests. Native modules are
oracles, never part of the encoder.

## Byte layout

```text
"EMBC"                       4 bytes
version                      u16 little-endian (2)
chunk                        recursive, see below
```

A chunk is encoded as:

```text
u32 constant_count
  per constant: u8 tag + payload
u32 function_count
  per function: length-prefixed UTF-8 name,
                u32 param_count, length-prefixed UTF-8 params,
                nested chunk
u32 instruction_count
  per instruction: u8 tag, u32 a, u32 b, u32 line, u32 col
```

Length prefixes are u32 little-endian counts of **UTF-8 bytes**, not
characters. Every instruction occupies exactly 17 bytes.

### Constant tags

| Tag | Payload | IR |
| --- | --- | --- |
| `0` | eight bytes, IEEE-754 binary64 little-endian | `{tag: "number", value: 1.5}` |
| `1` | u32 byte length + UTF-8 bytes | `{tag: "string", value: "héllo"}` |
| `2` | one byte, `0` or `1` | `{tag: "bool", value: true}` |
| `3` | none | `{tag: "null"}` |

### Instruction tags

`ieOps` maps every `Op` variant name from `emb.encodeOp` to `[tag, shape]`.
Shape `0` zeroes both operands, shape `1` keeps `a` and zeroes `b`, and shape
`2` keeps both. This reproduces `bytecode::encode_op`, which discards unused
operands even though `emb.encode` still validates them. Missing `a`/`b` default
to `0`; missing `line`/`col` default to `1`. All four operands are emitted as
u32 little-endian values, so non-integral, negative or out-of-range operands
are rejected with an `ieEncode:` assertion message.

## Implementation notes

- **f64 bits without a native helper.** `ieAppendF64` normalises the magnitude
  by repeatedly halving/doubling by powers of two (exact in binary floating
  point) until it lies in `[1, 2)`, or until the subnormal floor at `2^-1022`
  is reached. The 52-bit fraction is then recovered by 52 exact doublings and
  packed into eight bytes. Infinities use exponent `2047` and fraction `0`;
  `-0.0` is detected with `toFixed(1)`.
- **UTF-8.** `ieTextBytes` walks characters with `charCode` and emits the
  one-to-four-byte UTF-8 sequence for each code point, matching Rust's
  `String::as_bytes`.
- **Recursion.** `ieAppendChunk` walks function prototypes recursively and
  enforces the same 32-level depth cap as the native encoder and loader.
- **Validation.** Missing chunk arrays, bad tags, unknown op names, non-string
  names/params and invalid operands raise clear `ieEncode:` errors via
  `assert`. Index validity (constant/name/jump targets) is a deserializer
  concern and is not checked here, matching `emb.encode`.

### Caveat: NaN payloads

Infinities and the canonical positive quiet NaN produced by `parseFloat("NaN")`
round-trip exactly. Englishmen does not expose `f64::to_bits`, and formatting or
comparisons lose both the sign and payload of a NaN, so a negatively signed or
non-canonical NaN constant is serialised as the canonical positive NaN. Such
values are not produced by the compiler/AST→IR path; the Rust fixture set uses
the canonical spelling only.

## File writing is still a host concern

`ieEncode` returns a byte array. `fs.write` in this build accepts only UTF-8
strings, so a pure-Englishmen program cannot yet turn that array into an
arbitrary binary `.emb` file on its own. Writing the image still needs the
native `emb.write` (or a future binary-write API / host step that consumes the
returned bytes). This is deliberate: the encoder is the piece being replaced;
file I/O is outside this PR's scope.

## Tests

- Inline: `EM_SELFTEST=1 em run selfhost/ir_encode.enm` compares `ieEncode`
  with `emb.encode` on empty/hello chunks, all constant tags (including
  subnormals, `f64::MAX`, infinities and canonical NaN), all 55 opcode tags,
  operand defaults/masking, unicode nested functions and wide operands, and
  prints `ok`.
- Rust: `cargo test --test selfhost_ir_encode` concatenates the script with a
  broader fixture table and compares both encoders on both the VM and
  interpreter backends. It covers 25+ constants, all 55 opcodes, 200 mixed
  instructions, 32 nested functions, u32-max operands, and a load-and-run
  check that feeds `ieEncode` bytes to `em run-fixed`.
- Formatting: `em fmt --check selfhost/ir_encode.enm`.

Run the full verification with:

```sh
cargo run --quiet -- fmt --check selfhost/ir_encode.enm
EM_SELFTEST=1 cargo run --quiet -- run selfhost/ir_encode.enm
cargo test --all-targets
```

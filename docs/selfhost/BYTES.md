# `bytes` module

`import bytes` adds binary-safe byte buffers to Englishmen. A buffer is an
**array of numbers in `0..=255`** — the same runtime value you get from
`[1, 2, 3]` — so ordinary array indexing, `.length()` and `say()` work on it.
Packers always return a **new** array. `bytes.set` changes a buffer in place and
returns that same buffer, so calls can be chained.

```text
import bytes

text = "hi"
buffer = bytes.fromString(text)      $- [104, 105] -$
say(bytes.toString(buffer))          $- hi -$
```

The module is part of the self-hosting contract (`docs/selfhost/CONTRACT.md`
§1). It is registered by the parent integration step; `em run` must be built
from a tree that includes `src/modules/bytes.rs` and its module registration.

## Reference

| Function | Result |
| --- | --- |
| `bytes.fromString(text)` | UTF-8 bytes of `text`, as a buffer. |
| `bytes.toString(buffer)` | Decode the buffer as UTF-8 into a string; invalid UTF-8 is an error. |
| `bytes.of(...nums)` | A new buffer from explicit integers; rejects non-integers and anything outside `0..=255`. |
| `bytes.length(buffer)` | Number of bytes in the buffer. |
| `bytes.get(buffer, index)` | Byte at `index`; negative indexes count from the end (`-1` is the last byte). |
| `bytes.set(buffer, index, value)` | Set one byte, returning the same buffer for chaining. |
| `bytes.concat(...buffers)` | Concatenate buffers in argument order into a new buffer. |
| `bytes.slice(buffer, start, end?)` | Half-open slice `[start, end)`; `end` defaults to `bytes.length(buffer)`. |
| `bytes.copy(buffer)` | An independent copy; changing it leaves the original alone. |
| `bytes.fromHex(text)` | Parse `aa bb cc` or `aabbcc` style hex into a buffer. |
| `bytes.toHex(buffer)` | Lowercase hex, two digits per byte, separated by spaces. |
| `bytes.u8(n)`, `bytes.u16(n)`, `bytes.u32(n)`, `bytes.u64(n)` | Unsigned **little-endian** packing. |
| `bytes.i16(n)`, `bytes.i32(n)`, `bytes.i64(n)` | Signed two's-complement **little-endian** packing. |
| `bytes.f64(n)` | IEEE-754 double-precision **little-endian** packing. |
| `bytes.read(path)` | Read a file as a buffer, without UTF-8 decoding (binary-safe). |
| `bytes.write(path, buffer)` | Write a buffer to a file, creating or replacing it; returns `true`. |

`bytes.read`/`bytes.write` are the binary counterparts of `fs.read`/`fs.write`.
Use `fs` for text: it reads and writes UTF-8 strings and returns `null` from
`fs.write`. Use `bytes` when the file may contain `0x00`, invalid UTF-8, or
fixed-width numeric fields.

## Working with buffers

```text
import bytes

buf = bytes.of(10, 20, 30, 40)
say(buf)                         $- [10, 20, 30, 40] -$
say(bytes.length(buf))           $- 4 -$
say(bytes.get(buf, 0))           $- 10 -$
say(bytes.get(buf, -1))          $- 40 -$

bytes.set(buf, 1, 99)            $- same buffer, now [10, 99, 30, 40] -$
say(buf)

say(bytes.concat(bytes.of(1, 2), bytes.of(3)))  $- [1, 2, 3] -$
say(bytes.slice(buf, 1, 3))      $- [99, 30] -$
say(bytes.slice(buf, 2))         $- [30, 40] -$

clone = bytes.copy(buf)
bytes.set(clone, 0, 0)
say(buf)                         $- [10, 99, 30, 40]: original unchanged -$
say(clone)                       $- [0, 99, 30, 40] -$
```

Indexes start at `0`; negative indexes count back from the end. Out-of-range
indexes and slices, non-integer byte values, and values outside `0..=255` are
errors rather than silent truncation.

## Text, hex and Unicode

`bytes.fromString` encodes a string as UTF-8, so non-ASCII characters occupy
more than one byte. `bytes.toString` reverses that conversion and errors when
the bytes are not valid UTF-8.

```text
import bytes

utf8 = bytes.fromString("hé")    $- [104, 195, 169] -$
say(utf8)
say(bytes.toString(utf8))        $- hé -$

say(bytes.fromHex("aa bb cc"))   $- [170, 187, 204] -$
say(bytes.fromHex("0a0b0c"))     $- [10, 11, 12] -$
say(bytes.toHex(bytes.of(0, 15, 255)))  $- 00 0f ff -$
```

`fromHex` accepts both space-separated pairs and one continuous string.
`toHex` always emits lowercase, two digits per byte, with single spaces
between bytes.

## Little-endian packing

Little-endian means the least significant byte comes first. Packing functions
return fresh arrays and never modify their inputs.

```text
import bytes

say(bytes.u8(255))                   $- [255] -$
say(bytes.u16(0x1234))               $- [52, 18] -$
say(bytes.u32(0x12345678))           $- [120, 86, 52, 18] -$
say(bytes.u64(0x00000000ffffffff))   $- [255, 255, 255, 255, 0, 0, 0, 0] -$

say(bytes.i16(-2))                   $- [254, 255] -$
say(bytes.i32(-2))                   $- [254, 255, 255, 255] -$
say(bytes.i64(-2))                   $- [254, 255, 255, 255, 255, 255, 255, 255] -$

say(bytes.f64(1.5))                  $- [0, 0, 0, 0, 0, 0, 248, 63] -$
```

Signed values use two's complement: `-2` is `0xfffe`, `0xfffffffe`, and so on.
`f64` packs the eight bytes of an IEEE-754 binary64 value; `1.5` is
`0x3ff8000000000000`, hence the trailing `248, 63`.

Englishmen numbers are doubles, so integers larger than `2^53` cannot be
represented exactly before packing. For `u64`/`i64`, use values that are exact
in `f64` (as the examples above do).

## Binary file I/O

`bytes.write(path, buffer)` writes the raw bytes, including zero bytes and
bytes that are not valid UTF-8. `bytes.read(path)` returns a buffer with the
file's exact contents. Both return errors for bad paths or I/O failures.

```text
import bytes
import fs

path = "packet.bin"
bytes.write(path, bytes.fromHex("00 01 fe ff"))
say(bytes.read(path))                 $- [0, 1, 254, 255] -$
say(bytes.toHex(bytes.read(path)))    $- 00 01 fe ff -$
say(fs.remove(path))                  $- true -$
```

The same pair can carry packed numeric headers:

```text
import bytes

header = bytes.concat(bytes.u16(0x1234), bytes.u32(0x12345678))
bytes.write("header.bin", header)     $- 6 bytes: 52 18 120 86 52 18 -$
```

## Errors

Every function validates its arguments and raises a normal Englishmen error
instead of panicking. Expect errors for a missing path argument, a non-buffer
where a buffer is required, a non-integer or out-of-range byte, an invalid hex
string, an out-of-range index or slice, and invalid UTF-8 in `toString` or
`bytes.read`'s text counterpart `fs.read`. Invalid numeric ranges for a packer
(for example `bytes.u8(256)`) are errors too.

## Runnable example

`examples/bytes.enm` exercises every function in the table and prints
deterministic output. Run it on either backend:

```text
em run examples/bytes.enm
em run examples/bytes.enm --interpret
```

`tests/bytes_example.rs` runs both commands and asserts that their stdout is
byte-for-byte identical. The test requires the `bytes` module to be registered
in the binary; until the parent wires it in, it is expected to fail at
`import bytes`.

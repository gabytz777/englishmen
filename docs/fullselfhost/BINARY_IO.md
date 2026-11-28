# Binary I/O for loadable images

`emb.writeBytes(path, bytes)` is a native host primitive in the `emb` module. It
writes the given array of whole numbers `0..=255` to the file at `path`, byte for
byte, and returns `true`.

```text
import emb

encoded = emb.encode(ir)    // pure encoder output: array of byte numbers
emb.writeBytes("out.emb", encoded)
// em run-fixed out.emb
```

## Why it exists

The self-hosted encoder produces an array of numbers. Saving that array as an
image needs a host primitive, but text-oriented file APIs cannot represent all
byte values. `emb.writeBytes` is the convenience alias that closes that gap: it
is equivalent to `bytes.write(path, emb.encode(ir))` when `bytes` is available,
but it accepts any validated byte array, so a program that computed the bytes
itself (the pure `ieEncode` pipeline) can save them directly. The resulting
file is exactly the same canonical `.emb` image as `emb.write(path, ir)` and can
be loaded with `em run-fixed <path>`.

`emb.writeBytes` does not inspect or re-encode the bytes. Validation is limited
to the argument shape: `path` must be a string and every array item must be a
whole number from 0 to 255. Errors follow the `bytes.write` style, naming the
function, argument and failing index, e.g.
`Expected a whole number from 0 to 255 at index 3 for emb.writeBytes, found 256`.
The whole array is validated before the file is created, so a malformed input
never leaves a partial file behind. An empty array writes an empty file. As with
`bytes.write`, filesystem failures are reported as
`Could not write '<path>': <reason>`.

## Relationship to `bytes.write`

`bytes.write(path, buffer)` is the general-purpose binary file writer. Both
functions accept the same kind of buffer (an array of numbers `0..=255`) and
use the same validation rules and error wording. The difference is intent:
`bytes.write` is the general primitive; `emb.writeBytes` is the bytecode-module
alias that makes "save these encoded bytes as an image" explicit and keeps a
self-hosted driver independent of the `bytes` import. Existing `emb.encode` and
`emb.write` APIs are unchanged, and the on-disk bytecode format is unchanged.

## The UTF-8-only limitation of `fs.write`

`fs.write(path, content)` takes an Englishmen **string**, not a byte array. It
writes that string's UTF-8 encoding, so it can only produce a valid text file.
Encoder output contains arbitrary bytes: `0x00` terminators, little-endian
`u16`/`u32` values and UTF-8-invalid sequences are all normal in an `.emb`
image. Passing encoded bytes through `fs.write` is not a safe substitute: the
bytes must first be interpreted as a string, and any non-UTF-8 byte would be
lost or rejected. Use `emb.writeBytes` (or `bytes.write`) for binary images and
reserve `fs.write` for text.

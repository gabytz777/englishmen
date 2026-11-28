# Pure-Englishmen native-module replacements: `project`

`selfhost/lib_project.enm` is a self-contained Englishmen implementation of the
`.emc` manifest surface described in [`../selfhost/EMB_AND_MANIFEST.md`](../selfhost/EMB_AND_MANIFEST.md).
It is the B2 deliverable for round 2 of the full-self-hosting contract.

The file defines only prefixed globals, has no top-level side effects outside
its self-test guard, and does not import another `.enm` file. It is intended to
be concatenated into a future `selfhost/full.enm`.

## Public API

| Function | Result | Native counterpart |
| --- | --- | --- |
| `pjParse(text)` | `{root: object, sections: object}` | `project.parse(text)` |
| `pjName(manifest)` | Root `name`, or `null` | `project.name(config)` |
| `pjVersion(manifest)` | Root `version`, or `null` | `project.version(config)` |
| `pjMain(manifest)` | Root `main`, or `null` | `project.main(config)` |
| `pjWrite(manifest)` | Serialized `.emc` text | Text written by `project.write(path, config)` |

`pjWrite` deliberately has no path argument: a pure-Englishmen library cannot
depend on a native writer, and `fs.write(path, pjWrite(config))` already works.
Everything else mirrors the native shape and ordering.

Example:

```js
config = pjParse("name = \"Demo\"\nmain = \"main.enm\"\n")
say(pjName(config))                 // Demo
say(pjMain(config))                 // main.enm
say(pjWrite(config))                // main = "main.enm"\nname = "Demo"\n
```

## Parsing parity

`pjParse` follows `src/modules/project.rs` / `src/config.rs` line for line:

- Lines are trimmed with the same Unicode `White_Space` definition as Rust
  `str::trim`; blank lines and lines beginning with `#` are ignored.
- A trimmed line that both starts with `[` and ends with `]` opens (or reopens)
  a section. The name is the trimmed text between the brackets; repeated
  headers merge into the same section object.
- The first `=` splits a key/value pair. Keys are trimmed; the value is trimmed
  again, and one surrounding pair of `"` is removed.
- Escapes inside quoted values are decoded exactly as native: `\"`, `\\`,
  `\n`, `\r`, `\t`, and `\uXXXX` with 1-4 hex digits (an optional leading `+`
  is accepted, matching `u32::from_str_radix`; surrogates and out-of-range
  code points are invalid). Invalid or unknown escapes are preserved
  literally, including a trailing lone backslash.
- All values are read as strings; bare `2.1`, `true` and `null` stay textual,
  exactly as in the native parser.

## Writing parity

`pjWrite` reproduces `serialize_config`:

- Root keys come first, then section names and their keys in `BTreeMap` order
  (the same order Englishmen objects and `json.encode` use).
- Strings are quoted with `\"`, `\\`, `\n`, `\r`, `\t`, and `\u{:04x}` for
  code points below U+0020; other Unicode characters are written raw.
- Numbers use `toString`, which is the same `value::format_number` the native
  writer uses; booleans and null are written bare as `true`, `false`, `null`.
- Non-finite numbers, unsupported value types, malformed configs and keys or
  section names containing whitespace, `#` prefixes, `[`, `]` or `=` are
  rejected with the same core message as native.
- One caveat: Englishmen has no raw `raise`, so failures use `assert(false,
  message)`. The engine prefixes those messages with `Assertion failed: `;
  the Rust parity test strips that prefix before comparing.

## Declaration scoping note

Englishmen assignments can see and update an outer variable with the same
name. Every local variable and parameter in this file is therefore prefixed
with its function name (for example `pjParse_i`, `pjSelfTest_samples`), not
just the globally visible helper functions. Concatenating the file must not
shadow a caller's `i`, `text`, `key`, or other common names.

## Native dependencies and what remains native

Implementation dependencies are only global builtins (`charCode`,
`fromCharCode`, `floor`, `pow`, `isNaN`, `type`, `isString`, `isObject`,
`toString`, `jsonEncode`, `readFile`, `deleteFile`) and `os.env` for the
self-test guard. No native module is used by the implementation.

Still native-only, and intentionally out of scope for this deliverable:

- `project.load(path?)`, `project.get(config, key)`,
  `project.section(config, name)` and `project.default(name)`. `pjName`,
  `pjVersion` and `pjMain` cover the three shortcuts requested for round 2;
  callers can read other root keys directly from `pjParse(...).root`.
- The path-taking `project.write(path, config)`. Use
  `fs.write(path, pjWrite(config))` instead.
- Binary `.emb` encoding and execution (`emb`, `bytecode`, VM) are separate
  self-hosting gaps, not part of this module.

The native `project` module is imported **only** inside the
`if os.env("EM_SELFTEST", "") == "1"` block at the bottom. With that variable
set, `em run selfhost/lib_project.enm` compares parsing, accessors, writes and
round trips against native on nine inline samples plus a mixed-type manifest,
then prints `ok`.

## Rust parity coverage

`tests/selfhost_enm_modules.rs` generates a driver that concatenates this
script with native-oracle calls and checks both the compiler/VM and the
reference interpreter:

- 14 `.emc` fixtures: empty/default manifests, comments and whitespace
  (including CRLF and Unicode whitespace), bare/quoted/single-quoted values,
  all supported escapes, malformed `\u` sequences, actual control characters
  and NUL, repeated keys and sections, Unicode keys/values, embedded `=` and
  `#`, and odd quote placement.
- Three mixed manifests with strings, numbers (including large/small/boundary
  values), booleans, null, sections and escaped Unicode/control text.
- Seven writer error cases plus a `parse` type error. Pure and native error
  messages are compared after removing the `Assertion failed: ` prefix.
- The inline `EM_SELFTEST` guard is asserted to be quiet by default and to
  print `ok` when enabled.

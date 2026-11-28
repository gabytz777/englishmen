# Import bridge: pure `string` and `json` module objects

`selfhost/lib_modules.enm` is the A2 deliverable for round 3. It supplies
ordinary Englishmen objects that can stand in for the native `string` and
`json` modules when the self-hosted VM runs program IR through the A1 import
bridge.

The file is self-contained: it has no local `.enm` imports, every global helper
starts with `lm`, and every local variable or parameter is prefixed with the
name of its function (for example `lmJsonParseString_out`) so concatenating it
into `selfhost/full.enm` cannot shadow a caller's `text`, `i`, `key`, or other
common names. The only import is `os`, used solely for the
`EM_SELFTEST` guard. Native `string` and `json` are imported **only** inside
that guard, where they act as reference oracles.

## Module-object shape

```text
lmStringModule() -> {
    length, upper, lower, trim, split, join, contains, startsWith, endsWith,
    replace, replaceAll, slice, substring, indexOf, lastIndexOf, charAt,
    toNumber, capitalize, titleCase, reverse, repeat, pad, padLeft, padRight,
    isDigit, isAlpha, isAlphanumeric, isWhitespace, isEmpty, isBlank, lines,
    chars, swapCase, truncate
}
lmJsonModule() -> {
    encode, decode, pretty, isValid, get, set
}
lmAllModules() -> {string: lmStringModule(), json: lmJsonModule()}
```

Every value in these objects is an Englishmen function value, not a native
function. Callers use the same member syntax as with native modules:

```text
modules = lmAllModules()
say(modules.string.upper("héllo"))
say(modules.json.get(modules.json.decode("{\"a\":1}"), "a"))
```

`keys(lmStringModule())` has 34 entries; `keys(lmJsonModule())` has 6.

### Fixed arity and `null`

Native module functions are variadic; Englishmen functions have fixed arity.
An omitted optional native argument is therefore passed as `null`:

| Pure call | Native equivalent |
| --- | --- |
| `string.split(text, null)` | `string.split(text)` |
| `string.slice(text, 1, null)` | `string.slice(text, 1)` |
| `string.substring(text, 0, null)` | `string.substring(text, 0)` |
| `string.join(items, null)` | `string.join(items)` |
| `string.pad(text, 5, null, null)` | `string.pad(text, 5)` |
| `string.padLeft(text, 5, null)` | `text.padLeft(5)` |
| `string.truncate(text, 8, null)` | `text.truncate(8)` |

## Function coverage and parity target

Names that also exist in `src/modules/string.rs` follow the **native module**
semantics. Names that only exist as string methods follow the corresponding
method in `src/runtime.rs`.

| Group | Functions | Oracle |
| --- | --- | --- |
| Case and whitespace | `upper`, `lower`, `trim`, `reverse`, `capitalize`, `titleCase`, `swapCase` | `string.upper/lower/trim/reverse` or the matching string method |
| Splitting and joining | `split`, `join`, `lines`, `chars` | `string.split/join/lines` or `text.chars()` |
| Search | `contains`, `startsWith`, `endsWith`, `indexOf`, `lastIndexOf`, `charAt` | `string.contains/startsWith/endsWith/indexOf/lastIndexOf/charAt` |
| Slices and replacement | `slice`, `substring`, `replace`, `replaceAll` | `text.slice(...)` for `slice`; native module for the others |
| Numbers and padding | `toNumber`, `repeat`, `pad`, `padLeft`, `padRight`, `truncate` | `text.toNumber/padLeft/padRight/truncate` or native `string.repeat/pad` |
| Predicates | `length`, `isDigit`, `isAlpha`, `isAlphanumeric`, `isWhitespace`, `isEmpty`, `isBlank` | native module predicates or `text.isBlank()` |
| JSON data | `encode`, `decode`, `pretty`, `isValid`, `get`, `set` | matching `json.*` function |

Notable semantic choices:

- `replace` replaces **all** occurrences, like `src/modules/string.rs`;
  `replaceAll` is an alias. `substring` rejects reversed/out-of-range bounds
  and `charAt` rejects out-of-range indexes, like the native module.
- `slice` uses the string-method rules: negative indices count from the end,
  bounds clamp, and reversed bounds produce an empty string.
- `isDigit` is ASCII-only (native module), while `isAlpha`,
  `isAlphanumeric` and `isWhitespace` use Unicode character properties.
  `isBlank` follows the string method and is true for the empty string.
- `toNumber` returns `null` for non-finite or malformed text, like the string
  method. The global `toNumber` builtin is not used for this because it raises
  on failure and Englishmen has no `try`/`catch`.
- `repeat` uses the native module's 10,000,000-byte output guard;
  `padLeft`, `padRight` and `truncate` use the string methods' 100,000-character
  width cap.
- `lines()` follows Rust `str::lines()`: CRLF pairs lose the `\r`, a trailing
  newline does not create an extra empty line, and the empty string yields
  `[]`.

### JSON coverage

`json.encode` writes compact JSON with sorted object keys (Englishmen objects
are `BTreeMap`-backed). Strings escape `"`, `\`, `\n`, `\r`, `\t`, backspace,
form feed, and other controls as lowercase `\u00xx`; other Unicode is written
raw. Numbers use `toString`, which is the same formatter the native module
uses. Functions and non-finite numbers are rejected, and recursion is capped
at depth 100 so cyclic values fail instead of overflowing.

`json.decode` is a hand-written parser over the string's characters. It
accepts the same subset as `src/modules/json.rs`: the six JSON value kinds,
whitespace, standard escapes, `\uXXXX` escapes including surrogate pairs, and
the strict number grammar (no leading zeros, `+`, or trailing dot). Invalid
text and trailing characters are rejected. Like native, a valid number that
overflows to infinity still decodes to a non-finite number; encoding that value
afterwards is rejected by both implementations.

`json.pretty` uses two-space indentation and the same key ordering and scalar
encoding as `json.encode`. `json.get` and `json.set` accept dotted paths such
as `user.name` and `items.0`. Missing paths read as `null`; `set` creates
missing objects, appends when an array index equals its length, and mutates
shared containers before returning the root. Failures are raised with
`assert(false, message)`, so callers see the engine's `Assertion failed: `
prefix, as in the pure `project` library.

## How the import bridge consumes these objects

Problem A in the round-3 contract adds `vmRunWithImports(ir, imports)` to
`selfhost/vm_runtime.enm`. `imports` is an object keyed by module name; on an
`Import` opcode the bridge binds the named object into the environment under
that module name. A missing module must raise an error listing the available
names. `vmRunWithRuntime(ir)` keeps working by passing an empty table.

To wire this library in, concatenate `selfhost/lib_modules.enm` before the
driver and pass the object table:

```text
modules = lmAllModules()
vmRunWithImports(irFull, modules)
```

A program containing `import string` / `import json` then sees `string` and
`json` as the pure objects above. Because the functions close over the `lm*`
globals, the whole file must be present in the same program; there is no
local import mechanism between self-hosted files. Native modules are not
needed for the implementation; only `os.env` is used to enable the self-test.

The object shape matches what native `load_module(name)` returns (an object of
callable members), so no bridge-side conversion is required: bind the value
as-is.

## Self-tests and verification

At the bottom of the file:

```text
if os.env("EM_SELFTEST", "") == "1"
    import string
    import json
    ...
    say("ok")
end
```

The guarded block compares pure results against native `string.*` and
`json.*` (or the equivalent string methods) across a broad fixture set:

- 14 text fixtures including empty/ASCII text, Unicode (`é`, `☃`, Japanese,
  emoji), CRLF/newlines/tabs, quotes and backslashes, repeated patterns, and
  case-folding edge cases (`ß`, `İ`, `ı`, `ǅ`).
- Unary predicates and case operations, eight needles per text for search and
  slice operations, several pad widths/alignments, and numeric parsing
  fixtures including whitespace, signs, exponents, `NaN`, `inf`, and
  overflowing exponents.
- 15 JSON values covering every representable type, escaping, nested
  containers and key ordering; 14 JSON text fixtures covering literals,
  escapes, surrogate pairs, whitespace, duplicate keys and huge numbers;
  14 invalid JSON texts; and dotted `get`/`set` paths on objects and arrays.

With the guard disabled the file defines its functions and prints nothing.
With `EM_SELFTEST=1` it prints exactly `ok`.

Verification performed on branch `sh3-pure-modules`:

- `cargo run --quiet -- fmt --check selfhost/lib_modules.enm` — clean.
- `EM_SELFTEST=1 cargo run --quiet -- run selfhost/lib_modules.enm` — `ok`.
- `cargo test --all-targets` — all tests passed.

All commands used `TMPDIR=/home/larpington/.cache/em-round3-tmp` and
`CARGO_TARGET_DIR=/home/larpington/.cache/em-round3-target`.

## Caveats

- Optional arguments use `null` instead of being omitted (see above).
- `swapCase` infers upper/lower status from reversible case mappings rather
  than a Unicode property table. It matches the fixture set, including
  multi-character expansions and titlecase `ǅ`, but exotic case-ignorable
  code points could differ from Rust's `char::is_uppercase` /
  `char::is_lowercase`.
- Error text is an `assert` message with the engine's `Assertion failed: `
  prefix; native functions return plainer error strings. This matches the
  approach used by the other pure libraries in `selfhost/`.

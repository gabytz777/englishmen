# Road to a self-hosting Englishmen compiler

A self-hosting compiler is one written in the language it compiles. Englishmen
is not there yet: its lexer, parser, compiler and VM are Rust programs, and a
`.enm` program cannot currently inspect the structure of its own source or
emit the `.emb` container byte-for-byte. This document records what is still
needed and the order in which it is cheapest to add it.

## Why this is a useful goal

A self-hosted front end forces the language to have:

- first-class text and collection processing,
- reliable file I/O and deterministic output,
- structured errors and enough control flow to express a recursive-descent
  parser and a multi-pass compiler,
- stable bytecode and a way to serialise it.

Those are useful language features in their own right, which is why the
current feature work focuses on them rather than on a one-off trick.

## Capability checklist

| Area | Needed by a self-hosted compiler | Present today | Work in progress |
| --- | --- | --- | --- |
| Lexing | character access, string indexing, escape handling, regex-like predicates | string indexing, `length`, `split`, `contains` | `charAt`, `startsWith`, `endsWith`, `replace`, `lines`, digit/alpha tests, extra escapes, `//` and `#` comments |
| Parsing | arrays, objects, recursion, early exit from loops, ternaries | arrays, objects, recursion, `if`, `for ... in`, `repeat` | `while`, `break`, `continue`, ternary, `??`, compound assignment |
| AST | tagged objects with nested fields | objects, dot notation, arrays | richer object helpers (`has`, `get`, `set`, `delete`, `merge`, `copy`) |
| Code generation | numbers, bitwise operations, byte packing, integer rounding | arithmetic, `floor`, `ceil`, `round` | `**`, `//`, bitwise ops, `toHex`, `toBinary`, numeric literals in hex/binary |
| Output | write bytes, append, create directories, list files | none in the language itself | global `writeFile`, `readFile`, `appendFile`, `makeDir`, `listFiles`, plus the `fs` module |
| Metadata | project name/version, command-line arguments, environment | `.emc` files read by the CLI only | global `args`, `env`, `os` module, `json` module |
| Diagnostics | structured errors with codes and source positions | errors with line/column and hints | error codes, JSON diagnostics, multi-label rendering |
| Testing | run many cases with one command | `cargo test` for the implementation, `em build --run` for programs | `em check`, `em eval`, `em test` support in the CLI |

## Suggested order of work

1. **Finish the language surface.** Loops with `break`/`continue`, compound
   assignment, a full set of string/array/object methods, and global builtins
   make it possible to *express* a parser without awkward workarounds.
2. **Make output deterministic.** A `json` module and file helpers let a
   compiler write an intermediate representation before native `.emb` writing
   exists. JSON is a convenient bootstrap format because it is text and easy
   to inspect.
3. **Add binary I/O.** Emitting a real `.emb` needs writing little-endian
   integers and raw bytes. A small `bytes`/`binary` module on top of `fs`
   would complete this step.
4. **Write the compiler in `.enm`.** Start with the lexer: it is pure string
   processing and can be tested against the Rust lexer on the bundled
   examples. Then the parser, then a simple AST-to-JSON emitter.
5. **Bootstrap.** Run the `.enm` compiler on its own source. The first target
   is not "replace Rust" but "produce identical JSON for every example".
6. **Optional final step.** Compile the AST to bytecode with a `.enm` writer,
   then load it with the existing Rust VM. At that point the compiler is
   self-hosting even though the runtime still is not.

## Non-goals for the first bootstrap

- Native machine code. Englishmen targets its own bytecode; that is a design
  decision, not a temporary limitation.
- Replacing the Rust VM. A self-hosted *compiler* already meets the goal; a
  self-hosted runtime is a separate, much larger project.
- Backwards compatibility with `.em` source files. That extension is retired.

## How to verify progress

For each capability in the checklist, add a `.enm` example under `examples/`
and an integration test under `tests/` that runs it through both backends.
When a compiler written in Englishmen can process all of those examples and
produce the same output as `em build --emit ast`, the bootstrap is ready.

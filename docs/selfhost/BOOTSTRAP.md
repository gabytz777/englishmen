# Bootstrap compiler runner

`selfhost/compiler.enm` is the self-contained CLI entry point for parsing
Englishmen source with Englishmen itself. Englishmen cannot import local `.enm`
files yet, so the lexer, parser and emitter are inlined in that single file;
it uses only `fs.read`, `json.encode` and `say` for I/O.

## Bootstrap command

From the repository root:

```sh
em run selfhost/compiler.enm examples/hello.enm
```

That prints exactly one compact JSON line:

```json
{"statements":[{"expr":{"args":[{"kind":"String","value":"Hello, world!"}],"callee":{"kind":"Variable","name":"say"},"kind":"Call"},"kind":"ExprStmt","span":{"col":1,"line":2}}],"version":1}
```

The result is the schema from `docs/selfhost/CONTRACT.md` section 6. The
compiler builds ordinary Englishmen objects and arrays and calls `json.encode`,
so key ordering, string escaping and number formatting are the same as Rust's
`json::encode` rather than a hand-written approximation.

To compare against the Rust implementation (requires parent integration step
3):

```sh
em build examples/hello.enm --emit ast-json
```

`tests/selfhost_bootstrap.rs` runs both commands for every `examples/*.enm` and
asserts the stdout is byte-identical after trailing newlines are trimmed.

## Arguments

Today `args()` is the full process argv:

```text
[em, run, selfhost/compiler.enm, examples/hello.enm]
```

The runner starts with the last argument, then scans for the first non-flag
argument after the `compiler.enm` path. This also works if the parent later
narrows `args()` to script-only values, where the source path is simply
`args[0]`. Do not introduce an `--input` flag: the contract pins the positional
argument form.

## Integration status

- `em build <file> --emit ast-json` is wired and executed by the parity test;
  the skip only fires if the compiler still contains the literal `TODO`, which
  it does not.
- The compiler scans the full `args()` array for the source path, so no special
  script argument forwarding is required.
- Interpolated strings are lowered to chained `+` expressions with the same
  spans and leading empty string as `src/parser.rs::parse_interpolated`.
- `//` is a comment unless the previous token on the same line can end an
  expression; `div` is the unambiguous floor-division spelling.
- Englishmen reentrant calls share same-named locals, so every function in the
  inlined compiler uses function-specific local and parameter names. Renaming
  them back to shared names silently corrupts nested AST nodes.
- The inlined lexer covers the bundled examples and the full token surface of
  `src/lexer.rs`; it reports malformed input with `assert` rather than a
  structured JSON error object.

Verification command: `cargo test --test selfhost_bootstrap`. It compares the
compiler's stdout with `em build <example> --emit ast-json` byte-for-byte after
trimming trailing newlines for every bundled example.

## Adding an example

1. Add a valid `examples/<name>.enm`. It must compile with
   `em check examples/<name>.enm` and should avoid unbounded runtime work: the
   parser reads it as text, but the CLI test also builds it.
2. Verify the bootstrap directly:
   `em run selfhost/compiler.enm examples/<name>.enm` should print one JSON
   line and exit successfully.
3. Once `--emit ast-json` is wired, run
   `cargo test --test selfhost_bootstrap -- --nocapture`; the new example is
   discovered automatically and compared byte-for-byte.
4. Keep `selfhost/compiler.enm` formatted with `em fmt` and free of the literal
   `TODO` (use a different marker such as `FIXME` for unfinished work).

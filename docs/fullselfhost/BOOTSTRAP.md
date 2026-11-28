# Self-compilation bootstrap proof (C2)

`tests/selfhost_self_compile.rs` is the round-3 proof that the toolchain can
compile its own compile-only source. After integrating C1
(`selfhost/bootstrap_compile.enm`), the import bridge (A1/A2), B1/B2 image
writing, and the parent `--` pass-through, both commands below run to
completion. It must not be worked around by changing the commands.

## What the test proves

Two commands are compared:

```sh
# 1. Native oracle: the host compiler/VM runs the compile-only source.
em run selfhost/bootstrap_compile.enm examples/hello.enm

# 2. Self-hosted: full.enm compiles the compiler source with the pure
#    Englishmen lexer/parser/code generator, encodes it with the pure encoder,
#    and executes the generated IR on the pure Englishmen VM.
em run selfhost/full.enm selfhost/bootstrap_compile.enm -- examples/hello.enm
```

The native run prints exactly one JSON summary line, shaped like:

```json
{"astStatements":1,"irInstructions":4,"encodedBytes":104,"output":[]}
```

`output: []` is deliberate: `bootstrap_compile.enm` contains no VM entry point,
so running its generated IR cannot recurse into another compiler. The argument
after `--` is the file the generated compiler is asked to compile.

The self-hosted run prints its own summary line with the same keys, where
`output` holds the lines produced by the generated compiler on the self-hosted
VM. The test requires that array to be **exactly** `[<native summary line>]`
(with JSON string escaping). That is the actual self-compilation proof: the
code emitted by the pure Englishmen compiler ran on the pure Englishmen VM and
reproduced the native compiler's result byte-for-byte.

The host `em` binary still launches both processes; it is not replaced. What is
replaced, for the inner compilation, are the lexer, parser, AST→IR generator,
byte encoder, and VM execution of the generated IR.

## Statistics checked

The test asserts that `astStatements`, `irInstructions`, and `encodedBytes` are
present and nonzero in both summary lines. It also performs native
`--emit stats`-style checks:

```sh
em build examples/hello.enm --stats
em build examples/hello.enm -o <temp>/hello.emb
em build examples/hello.enm --emit ast-json
em build selfhost/bootstrap_compile.enm --stats
em build selfhost/bootstrap_compile.enm --emit ast-json
```

* The oracle line's three numbers for `examples/hello.enm` must match the Rust
  AST statement count, the native `--stats` instruction count, and the native
  `.emb` byte count. This proves the generated compiler reports honest numbers
  for the program it compiles.
* The self-hosted summary's top-level `astStatements` for
  `selfhost/bootstrap_compile.enm` must match the Rust AST JSON statement count
  for that same source. Its `irInstructions` and `encodedBytes` must be
  nonzero, and the native numbers are included in failure messages.

All child processes use the isolation convention from
`tests/selfhost_full_pipeline.rs`: a private `EM_HOME`, an unused
`EM_PROJECTS_DIR`, `EM_NO_UPDATE_CHECK=1`, and `EM_SELFTEST` removed so the
compile-only source takes its normal path rather than its inline self-test.

## Remaining host and import caveats

* The Rust CLI remains the host substrate. The test depends on pass-through of
  `--` to the script's `args()`/`os.args()`; that is a parent integration step,
  together with `--write-emb` handling. The test does not use `--write-emb`.
* Native modules (`fs`, `os`, `json`, `bytes`, `project`, `emb`, `math`,
  `cowsay`, `string`) are oracles or host substrate, not implementations being
  replaced here. The self-hosted VM uses the A1 import bridge plus the A2 pure
  module objects where available; native-backed wrappers cover the remaining
  module calls. An optional `__arities` entry in the import table pads variadic
  pure adapters such as `bytes.of`, `bytes.concat` and `bytes.slice` up to
  Englishmen's fixed function arity.
* Instruction and byte counts from the Rust compiler and the pure code generator
  are not guaranteed to be identical for every language construct. Equality is
  therefore asserted where it is practical and meaningful (the oracle program
  and the top-level AST statement count of the compiler source), while the
  compiler source's own IR/byte counts are checked for stability and nonzero
  values rather than cross-compiler equality.
* Writing a native `.emb` during a test goes to a private temporary directory
  (honouring `TMPDIR` from the round-3 contract), never into the repository.
* This is a compile-only bootstrap. Loading the generated image from disk and
  running it without a host `em` process is still future work.

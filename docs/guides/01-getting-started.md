# Getting started

Englishmen programs are plain UTF-8 text in `.enm` files. The `em` tool lexes, parses, compiles to bytecode, and runs them on the VM.

```text
$- hello.enm -$
say("Hello, world!")
```

```bash
em hello.enm
```

## Everyday commands

| Command | What it does |
| --- | --- |
| `em hello.enm` | Compile and run a program |
| `em hello.enm --interpret` | Run on the reference tree-walking interpreter |
| `em check hello.enm` | Report syntax/compiler errors without writing files |
| `em fmt hello.enm` | Format in place; `--check` only reports differences |
| `em eval '1 + 2'` | Compile and run one expression |
| `em doc` | List builtins, modules, and language keywords |
| `em build hello.enm` | Write `hello.emb` bytecode |
| `em build hello.enm --run` | Save bytecode, then run it |
| `em build hello.enm --emit ast` | Print the parsed AST |
| `em build hello.enm --emit asm` | Print the VM instruction stream |
| `em build hello.enm --stats` | Print compile statistics |
| `em build hello.enm --exe` | Bundle the runtime into a standalone executable |
| `em --version` / `em --help` | Version and full CLI help |

`em build` refuses conflicting modes: `--emit`, `--stats`, `--run`, `--exe`, and `--appimage` cannot be combined in contradictory ways.

## The REPL

Run `em` with no installed file to open the REPL. Multi-line blocks (`if`, `repeat`, `for`, `function`) are buffered until you type `end`.

```text
> age = 18
> say(age)
18
> :time age * 2
```

Meta commands: `:help`, `:vars`, `:clear`, `:history`, `:quit`/`:exit`, `:load <file>`, `:save <file>`, `:time <expr>`, `:about`, `:echo <text>`, `:source`, `:again`, `:eval <expr>`, `:type <expr>`, `:repeat <n> <src>`, and `:reset`. `:reset` also works while a multi-line block is open; `:type` evaluates in an isolated snapshot so it cannot mutate session globals.

## Comments

```text
$- single line -$

$-
Several lines.
-$
```

Both forms are preserved by `em fmt`, including comments inside interpolation and multi-line literals.

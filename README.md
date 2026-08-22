# Englishmen

Englishmen is an English-inspired, beginner-friendly general-purpose programming
language, implemented entirely in Rust. It uses `.em` source files and a `em`
command-line tool to run them.

```text
$- Your first Englishmen program -$

say("Hello, world!")
```

```bash
em hello.em
```

## Why Englishmen exists

Most beginner languages either dumb down programming into blocks/visual tools,
or throw learners straight into C-family punctuation soup. Englishmen aims for
a middle ground: real, predictable, parseable syntax that reads like plain
English sentences — `if age >= 18`, `say("Hello")`, `for person in people` —
without trying to understand arbitrary natural language. It has a controlled
grammar, not an AI sentence parser.

**Current status: compiled to Englishmen bytecode.** Programs are lexed,
parsed into an AST, compiled to bytecode, and executed by a virtual machine —
all in Rust. `em build` saves that bytecode as a `.emb` file you can run
directly. The original tree-walking interpreter is still there behind
`--interpret`. There is no *native* machine-code compiler — see
[Compiler](#compiler) for exactly what is and is not there.

## Installation

Download `em` (or `em.exe` on Windows) and run it once. If Englishmen isn't
installed yet, running `em` with no arguments enters a first-launch installer
that:

- Installs the runtime and standard library into a per-user directory (no
  admin rights required — e.g. `%LOCALAPPDATA%\Englishmen` on Windows)
- Adds that directory to your user `PATH`
- Optionally creates a projects folder at `Documents/EMProjects`
- If VS Code is detected on your machine, offers to install the free
  ["Code Runner"](https://marketplace.visualstudio.com/items?itemName=formulahendry.code-runner)
  extension and wire it up so `.em` files get a ▶ Run button
- Pauses for you to read the summary before the window closes (the
  installer used to exit the instant it finished, which — especially when
  launched by double-clicking `em.exe` — closed the console before anyone
  could see what had happened)

After installation, open a **new terminal** (PATH changes don't apply
retroactively to already-open shells) and run:

```bash
em --version
```

You can also build Englishmen from source with a standard Rust toolchain:

```bash
cargo build --release
```

## The `.em` format

`.em` files contain Englishmen source code. They're plain UTF-8 text files
parsed by Englishmen's own lexer/parser — not a subset of any other language.

## The `.emc` format

`.emc` ("Englishmen Configuration") files hold project metadata using a small
TOML-style key/value format:

```toml
name = "MyProject"
version = "1.0.0"
main = "main.em"
```

This is intentionally a simple, predictable configuration format — not a
natural-language parser. A `.emc` file is optional; you can always run or
build a single file directly with `em file.em` or `em build file.em`. Inside
a project, `em build` with no arguments compiles the file named by `main`.

## The `.emb` format

`.emb` ("Englishmen Bytecode") files are compiled programs, produced by
`em build` and run with `em file.emb`. They are binary, version-stamped, and
validated on load, so a truncated or hand-edited file is refused with a clear
message rather than misbehaving halfway through a run. See
[Compiler](#compiler).

## Running programs

```bash
em hello.em
```

This compiles the program and runs it. To run a program you compiled earlier:

```bash
em hello.emb
```

To run it on the original tree-walking interpreter instead:

```bash
em hello.em --interpret
```

## `em new`

Creates a new Englishmen project (an `englishmen.emc` plus a starter
`main.em`). If Englishmen is installed and a projects directory is
configured, new projects go there by default; otherwise they're created in
the current directory.

```bash
em new MyProject
```

`em new` never overwrites an existing project directory.

## `em build`

Compiles a program to bytecode. See [Compiler](#compiler).

```bash
em build main.em
```

## `em repair`

Diagnoses and repairs an Englishmen installation: the executable, runtime,
standard library modules, configuration, `PATH` entry, and projects
directory. It prints `[OK]`/`[WARN]` for each check, and offers to fix
anything broken — it never deletes existing project files.

```bash
em repair
```

## REPL

Running `em` with no arguments (once installed) launches an interactive
REPL:

```text
Englishmen REPL

> age = 18
> say(age)
18
```

Multiline constructs (`if`/`function`/`repeat`/`for` ... `end`) are buffered
until they're complete. Each completed snippet is compiled and run on the same
VM as a program file, so the REPL and `em file.em` behave identically; the
variables you define stay in the VM between inputs.

## Editing in VS Code

If VS Code is installed, the first-launch installer offers to set this up for
you automatically. To do it by hand instead:

1. Install the [Code Runner](https://marketplace.visualstudio.com/items?itemName=formulahendry.code-runner) extension.
2. Add to your VS Code `settings.json`:

   ```json
   "code-runner.executorMapByFileExtension": {
       ".em": "em"
   },
   "code-runner.runInTerminal": true,
   "code-runner.saveFileBeforeRun": true
   ```

Reopen a `.em` file and a ▶ Run button appears in the editor's title bar
(also available via right-click → "Run Code", or `Ctrl+Alt+N`); it runs
`em <file>` in the integrated terminal.

## Language tour

### Variables

No `var`/`let` keyword — assignment creates a variable:

```text
age = 18
name = "Alex"
active = true
```

### Dot notation / objects

```text
person.age = 18
person.name = "Alex"
say(person.name)

person.address.city = "London"
```

Object literals:

```text
person = {
    name: "Alex",
    age: 18,
    active: true
}
```

### Arrays

```text
people = ["Alex", "Sam", "John"]
say(people[0])
people[0] = "Sarah"

people.add("Zoe")
people.remove("Sam")
say(people.length())
```

### Conditions

```text
if age >= 18
    say("Adult")
else if age >= 13
    say("Teen")
else
    say("Child")
end
```

Boolean logic: `and`, `or`, `not`.

### Functions

```text
function add(a, b)
    return a + b
end

say(add(10, 20))
```

### Loops

```text
repeat 10
    say("Hello")
end

for person in people
    say(person)
end
```

### Comments

```text
$- single-line comment -$

$-
multiline
comment
-$
```

### Standard libraries

```text
import math
import time
import qrc
import cowsay

say(math.sqrt(25))
say(math.pow(2, 8))

cowsay.say("Moo from Englishmen!")

qrc.make("https://example.com", "code.png")
```

- **math** — `sqrt`, `abs`, `floor`, `ceil`, `round`, `pow`, `min`, `max`, `pi`, `e`
- **time** — `now`, `sleep`, `year`, `month`, `day`, `hour`, `minute`, `second`
- **qrc** — `make(text, path)` generates a QR code PNG (no external binaries)
- **cowsay** — `say(message)` prints ASCII-art cow speech (implemented natively, no `cowsay` executable required)

All standard libraries are implemented in Rust and ship inside the `em`
binary — no Python, Node, or other external runtimes required.

## Project structure

```text
EMProjects/
└── MyProject/
    ├── englishmen.emc
    ├── main.em
    └── main.emb        (after 'em build')
```

Larger projects can add more `.em` files and an `assets/` directory
alongside `main.em`. Compiled `.emb` files are build output — they can be
deleted and rebuilt at any time with `em build`.

## Examples

See [`examples/`](examples/) for runnable programs: `hello.em`,
`conditions.em`, `objects.em`, `libraries.em`, and the canonical
`example.em`.

## Compiler

Lexing, parsing, and the AST are shared stages with two execution backends
behind them:

```text
                AST
               /   \
              /     \
     Interpreter    Compiler
         ↓             ↓
      run .em       bytecode  →  VM  →  run
                       ↓
                    .emb file
```

Running a program compiles it first — `em hello.em` lexes, parses, compiles
and runs the bytecode in one go, leaving no files behind. `em build` keeps the
bytecode instead:

```bash
em build hello.em
```

```text
Compiled hello.em -> hello.emb (4 instructions, 104 bytes)
```

```bash
em hello.emb
```

A `.emb` file is a self-contained image of the compiled program: constants,
instructions, function prototypes, and the line/column of every instruction so
runtime errors still point at the right place in the original source. It is
*not* a native executable — it needs `em` to run, and it is portable across
machines that have the same `em` version.

### What `em build` does

| Command | Effect |
| --- | --- |
| `em build hello.em` | Compile to `hello.emb` |
| `em build hello.em -o out.emb` | Choose the output path |
| `em build hello.em --run` | Compile, save, then run |
| `em build hello.em --emit asm` | Print the disassembly, write nothing |
| `em build` | Compile the `main` of the `englishmen.emc` in this directory |

`--emit asm` prints the instruction stream, which is the readable form of
exactly what the VM executes:

```text
== hello.em ==
   3     0  LOAD_CONST     0  ("Hello, world!")
   3     1  SAY            1
   3     2  POP
   3     3  RETURN_NULL
```

### What the compiler does to your program

- **Control flow becomes jumps.** `if`, `repeat` and `for ... in` compile to
  conditional and unconditional jumps instead of a recursive walk over the AST.
- **Constants are interned and folded.** `2 + 3 * 4` is computed once at
  compile time and stored as a single constant; repeated literals share one
  slot in the constant pool.
- **Functions are compiled once.** Each function becomes a prototype with its
  own chunk; calling it binds that prototype to the scope it was defined in.
- **Calls run on the VM's own frame stack**, so runaway recursion is reported
  as an Englishmen error instead of crashing the process.
- **Errors that cannot be decided at compile time stay at runtime.** Division
  by zero is never folded away, so `1 / 0` still reports `Division by zero`
  with the same wording as the interpreter.

### The interpreter is still there

```bash
em hello.em --interpret
```

Both backends implement the same language, and the test suite checks that they
agree — on program output for every bundled example, and on the exact error
message and hint for each runtime error. The interpreter remains the
executable reference for the semantics; the compiler is the fast path.

### Not included

No native code generation, no `.exe` output, no LLVM, and no optimization
levels. `em build` produces Englishmen bytecode, not machine code.

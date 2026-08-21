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

**Current status: interpreter only.** Englishmen programs are lexed, parsed
into an AST, and walked by a tree-walking interpreter written in Rust. There is
no native compiler yet — see [Future compiler roadmap](#future-compiler-roadmap).

## Installation

Download `em` (or `em.exe` on Windows) and run it once. If Englishmen isn't
installed yet, running `em` with no arguments enters a first-launch installer
that:

- Installs the runtime and standard library into a per-user directory (no
  admin rights required — e.g. `%LOCALAPPDATA%\Englishmen` on Windows)
- Adds that directory to your user `PATH`
- Optionally creates a projects folder at `Documents/EMProjects`

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
natural-language parser. A `.emc` file is optional; you can always run a
single file directly with `em file.em`. As the future native compiler is
added, `.emc` will grow build-related keys, but it currently only carries
interpreter/project metadata.

## Running programs

```bash
em hello.em
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
until they're complete.

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
    └── main.em
```

Larger projects can add more `.em` files and an `assets/` directory
alongside `main.em`.

## Examples

See [`examples/`](examples/) for runnable programs: `hello.em`,
`conditions.em`, `objects.em`, `libraries.em`, and the canonical
`example.em`.

## Future compiler roadmap

The current version is **interpreter only** — there is no `em build` and no
native code generation. The architecture is deliberately laid out so a
compiler backend can be added later without redesigning the language itself:

```text
                AST
               /   \
              /     \
     Interpreter    Compiler   (future)
         ↓             ↓
      run .em        native code
```

Lexing, parsing, and the AST are already shared, reusable stages; the
interpreter is just one execution backend walking that AST. A future
compiler would be a second backend consuming the same AST (or an IR derived
from it), eventually enabling:

```bash
em build hello.em   # future
em build             # future, whole project
```

None of this exists yet — do not expect `.exe` generation, LLVM, or
optimization settings in the current version.

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
directly, and `em build --exe` bundles it with the runtime into a single
executable that needs nothing installed. On Linux, add `--appimage` to package
that executable as a portable AppImage. There is no *native* machine-code
compiler; the original tree-walking interpreter is still available behind
`--interpret`. See [Compiler](#compiler) for exactly what is and is not
there.

## Installation

### Linux and macOS

The easiest way is the one-line installer:

```bash
curl -fsSL https://raw.githubusercontent.com/gabytz777/englishmen/master/install.sh | sh
```

It installs entirely under your home directory — no `sudo`, no system
packages. If the current release has a prebuilt binary for your platform it
uses that; otherwise it builds from source, which needs a Rust toolchain
(https://rustup.rs). The script accepts `--from-source`, `--version <tag>`,
`--no-projects`, and `--optional`; run it with `--help` for details.

`em` is linked into `~/.local/bin`, and the installer adds that directory to
your shell profile (`~/.bashrc`, `~/.zshrc`, `~/.profile`, or the fish config)
when it isn't already on `PATH`. Open a new terminal, then check:

```bash
em --version
```

### Windows

Download `em.exe` from the [releases page](https://github.com/gabytz777/englishmen/releases)
and run it once.

### First launch (any platform)

Running an uninstalled `em` with no arguments enters a first-launch installer
that:

- Installs the runtime and standard library into a per-user directory (no
  admin rights required — e.g. `%LOCALAPPDATA%\Englishmen` on Windows,
  `~/.local/share/Englishmen` on Linux)
- Makes the `em` command available (`PATH` on Windows, a `~/.local/bin`
  symlink plus a shell-profile entry on Linux/macOS)
- Optionally creates a projects folder at `Documents/EMProjects`
- Offers **optional packages** with their download size and what they unlock —
  currently `appimagetool` (~15 MB), needed for `em build --appimage`
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

You can also install a local checkout without the script:

```bash
cargo build --release
./target/release/em install --yes
```

`em install` is idempotent and also upgrades an existing installation. Add
`--no-projects` to skip the projects folder, `--yes` to run without prompts,
and `--optional` / `--no-optional` to decide up front whether to install the
optional packages. Scripted installs (`--yes`, including `install.sh`) skip
them unless `--optional` is passed.

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

## Standalone executables

`em build --exe` produces a single file that runs on a machine with no
Englishmen installed:

```bash
em build hello.em --exe
```

```text
Compiled hello.em -> hello.exe (4 instructions, 913 KB, runs on its own)
```

```bash
./hello.exe
```

This is not native code generation. The `em` runtime is copied, your compiled
bytecode is appended to it, and a 16-byte trailer records where that payload
starts:

```text
[ em runtime ][ bytecode ][ length: u64 ][ magic: 8 bytes ]
```

Executables are loaded using the headers at the *front* of the file, so the
appended bytes are ignored by the operating system. On startup `em` checks its
own file for that trailer; when it finds one, it runs the embedded program
instead of reading the command line. The same trick is how PyInstaller and
`deno compile` produce single-file programs.

What that means in practice:

- **The file is self-contained.** No install, no PATH entry, no `.emb`
  alongside it.
- **It is the program, not `em`.** A bundled `hello.exe --version` runs your
  program; it is no longer the Englishmen CLI.
- **Every build is about 900 KB**, because the whole runtime rides along —
  a hello-world and a large program are nearly the same size.
- **It is not cross-platform.** Building on Windows produces a Windows
  executable, since it is a copy of the Windows `em`.

### Linux AppImages

On Linux, `--appimage` packages the standalone build as a portable
[AppImage](https://appimage.org/):

```bash
em build hello.em --exe --appimage
```

```text
Compiled hello.em -> hello-x86_64.AppImage (4 instructions, 1518072 bytes, portable Linux AppImage)
```

The result runs on x86-64 (or aarch64) Linux without Englishmen installed and
includes a desktop entry and icon for launchers. `--appimage` implies `--exe`;
pass `-o <file>` to choose the output name.

Packaging uses the official
[`appimagetool`](https://github.com/AppImage/appimagetool/releases). Install it
automatically with:

```bash
em install --optional
```

It is stored under the Englishmen install directory (`tools/appimagetool`), so
`em build --appimage` finds it without changing your `PATH`. You can also point
`APPIMAGETOOL` at your own copy:

```bash
APPIMAGETOOL=/path/to/appimagetool em build hello.em --exe --appimage
```

The build runs `appimagetool` with `APPIMAGE_EXTRACT_AND_RUN=1`, so FUSE is not
required on the build machine. AppImages are only produced on Linux.

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

Compiles a program to bytecode, or to a standalone executable with `--exe`.
See [Compiler](#compiler) and
[Standalone executables](#standalone-executables).

```bash
em build main.em
em build main.em --exe
```

## `em repair`

Diagnoses and repairs an Englishmen installation: the executable, runtime,
standard library modules, configuration, `PATH` entry (including the
`~/.local/bin/em` launcher on Linux/macOS), and projects directory. It prints
`[OK]`/`[WARN]` for each check, and offers to fix anything broken — it never
deletes existing project files.

```bash
em repair
```

## `em uninstall`

Removes Englishmen from the machine: the runtime, configuration, optional
tools, the `em` launcher, and the guarded `PATH` block in your shell profile.
Your projects folder is kept unless you ask otherwise.

```bash
em uninstall          # asks first
em uninstall --yes    # no questions, keeps projects
em uninstall --purge  # also delete the projects folder
```

`em uninstall` never removes a launcher or profile entry it does not recognise
as its own, and an interactive uninstall asks separately before deleting
projects.

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
- **html** — `page`, `h1`–`h6`, `heading`, `p`, `div`, `section`, `span`, `link`, `image`, `list`, `ol`, `tag`, `element`, `escape`, and `save` build and write HTML pages

All standard libraries are implemented in Rust and ship inside the `em`
binary — no Python, Node, or other external runtimes required.

## Building a website

The `html` library builds pages from ordinary Englishmen values and writes
them to disk:

```text
import html

page = html.page("My Site", [
    html.h1("Hello, world!"),
    html.p("Welcome to my corner of the internet."),
    html.link("About me", "about.html"),
    html.list(["Home", "About", "Contact"])
])

html.save("site/index.html", page)
```

Open `site/index.html` in a browser. `html.save` creates parent directories
when needed, and text helpers escape `&`, `<`, `>`, `"`, and `'` automatically.
When you need nested markup or attributes, compose raw elements:

```text
html.element("div", { class: "card" }, [
    html.h2("A card"),
    html.p("Built with Englishmen.")
])
```

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
`conditions.em`, `objects.em`, `libraries.em`, `site.em`, and the canonical
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
| `em build hello.em --exe` | Build a standalone executable (see [Standalone executables](#standalone-executables)) |
| `em build hello.em --exe --appimage` | Package a Linux AppImage (see [Linux AppImages](#linux-appimages)) |
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

No native code generation, no LLVM, and no optimization levels. `em build`
produces Englishmen bytecode, not machine code — including `--exe`, which
bundles that bytecode with the runtime rather than compiling to machine code.

The reason is that Englishmen is dynamically typed: a variable can hold a
number now and a string later, so `a + b` cannot become a CPU `add`
instruction. Native code would have to call back into a runtime that checks
the types anyway. This is the same reason Python, Ruby, Java, and C# all ship
bytecode and a VM.

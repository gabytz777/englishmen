# Englishmen — Chatbot Mini FAQ

> For ChatGPT, Claude (chat), DeepSeek, Gemini, and similar assistants.
> Not for Codex, Claude Code, Cursor, or other coding agents.
>
> Chatbots only read what you paste or upload. Add this file to a Project's
> knowledge or paste it into the chat. Target: **Englishmen 1.5.x**.

## If you are the assistant

- Answer from this file and target stable 1.5.x.
- Do not translate requests into Python or JavaScript.
- If something is not covered, say it is not supported rather than inventing it.
- Return complete `.em` code in a `text` fence, then give the run command.

## Fast facts

| Thing | Englishmen |
| --- | --- |
| Source file | `.em` |
| Output | `say(...)` |
| Variable | `age = 18` |
| Equality | `==` |
| Block end | `end` |
| Comment | `$- ... -$` |
| Run | `em hello.em` |

---

### What is Englishmen?

A beginner-friendly, English-inspired language implemented in Rust. The
grammar is small and strict; it is not a natural-language parser.

```text
say("Hello, world!")
```

### How do I run a program?

Save it as `hello.em`, then:

```bash
em hello.em
```

`em hello.em` compiles to bytecode and runs it immediately.

### Is it Python or JavaScript? Is it compiled?

No, it is its own language. It has no semicolons, no `let`/`const`, and no
indentation-based blocks. It compiles to Englishmen bytecode and runs on a VM:

```bash
em build hello.em      # writes hello.emb
em hello.emb           # runs the bytecode
```

`em build hello.em --exe` bundles the runtime into a standalone executable.
That is still bytecode plus a runtime, not native machine code.

### How do I print something?

```text
say("Hello")
say("Age:", 18)   $- Age: 18 -$
```

`say` separates multiple values with spaces and adds a newline. There is no
`print`, `console.log`, or `System.out.println`.

### How do I create a variable?

Assign to it — there is no declaration keyword:

```text
age = 18
name = "Alex"
active = true
nothing = null
```

### How do I write a comment?

```text
$- a comment -$

$-
    a multiline
    comment
-$
```

Comments are not `//`, `#`, or `/* */`.

### How do I write if / else if / else?

```text
age = 20

if age >= 18
    say("Adult")
else if age >= 13
    say("Teen")
else
    say("Child")
end
```

Do not wrap the condition in parentheses, and close the chain with one `end`.

### How do I write and / or / not?

```text
age = 20
active = true

if age >= 18 and active
    say("Welcome")
end

if not active
    say("Inactive")
end
```

Use `and`, `or`, `not` — not `&&`, `||`, `!`.

### How do I repeat something N times?

```text
repeat 3
    say("Hello")
end
```

The count must be non-negative; any fractional part is truncated.

### How do I loop over an array?

```text
people = ["Alex", "Sam", "John"]

for person in people
    say(person)
end
```

`for ... in` only iterates arrays.

### How do I loop with an index?

There is no index variable or `range()`. Keep a counter:

```text
items = ["a", "b", "c"]
i = 0

for item in items
    say(i + ": " + item)
    i = i + 1
end
```

### Is there a while loop?

No, and there is no `break` or `continue`. Use `repeat` for a fixed count, or
recursion when you need a condition:

```text
function countdown(n)
    if n <= 0
        return
    end
    say(n)
    countdown(n - 1)
end

countdown(3)
```

### How do I define a function?

```text
function add(a, b)
    return a + b
end

say(add(10, 20))   $- 30 -$
```

A function without `return` returns `null`. Define it before you call it.
There are no default arguments or keyword arguments.

### Does it have classes, exceptions, or switch?

No. There are no classes, inheritance, `try`/`catch`/`throw`, `switch`, or
`match`. Use objects and functions instead.

### How do I concatenate strings?

Use `+`:

```text
name = "Alex"
say("Hello, " + name + "!")
say("Age: " + 18)
```

If one side is a string, the other is converted to text. There is no string
interpolation — no `${name}`, `f"..."`, or `%s`.

### What string methods exist?

`.length()`, `.upper()`, `.lower()`, `.trim()`, `.split(separator)`, and
`.contains(text)`:

```text
s = "  Hello World  "

say(s.length())
say(s.trim())
say(s.contains("World"))
parts = s.trim().split(" ")
```

Indexing returns one character: `say("Alex"[0])` prints `A`. Indexes start at
0; negative indexes are errors.

### How do I get a substring?

There is no `substring` or `slice`. Build it with a loop:

```text
text = "Englishmen"
part = ""
i = 0

repeat 7
    part = part + text[i]
    i = i + 1
end

say(part)   $- English -$
```

### What array methods exist?

```text
people = ["Alex", "Sam"]

people.add("Zoe")       $- append -$
people.remove("Sam")    $- remove by value -$
people.removeAt(0)      $- remove by index -$
say(people.length())
say(people.contains("Zoe"))
```

There is no `map`, `filter`, `reduce`, `sort`, `reverse`, `join`, `indexOf`,
`slice`, `pop`, or `shift`.

### How do I sort an array?

There is no built-in sort. Use a simple loop:

```text
numbers = [3, 1, 2]

i = 0
repeat numbers.length()
    j = 0
    repeat numbers.length() - 1
        if numbers[j] > numbers[j + 1]
            temp = numbers[j]
            numbers[j] = numbers[j + 1]
            numbers[j + 1] = temp
        end
        j = j + 1
    end
    i = i + 1
end

say(numbers)   $- [1, 2, 3] -$
```

### How do I create an object?

Use dot notation or an object literal:

```text
person.name = "Alex"
person.age = 18
person.address.city = "London"

person = {
    name: "Alex",
    age: 18
}
```

Keys are bare identifiers: `{ name: "Alex" }`, not `{ "name": "Alex" }`.
Dot assignment creates nested objects as needed.

### How do I loop over an object?

Use `.keys()` or `.values()`:

```text
person = { name: "Alex", age: 18 }

for key in person.keys()
    say(key)
end

for value in person.values()
    say(value)
end
```

There is no `for key, value in person` syntax.

### How do I check for a key or count fields?

```text
person = { name: "Alex", age: 18 }

if person.keys().contains("name")
    say("has name")
end

say(person.keys().length())
```

Objects have no `length()` method of their own.

### Can I access an object field with a variable key?

No. `person[key]` and `person["name"]` are not valid. Use dot access with a key
you know when writing the program.

### How do I represent "no value"?

Use `null`:

```text
result = null

if result == null
    say("nothing")
end
```

There is no `None`, `nil`, or `undefined`.

### How do I import modules, and what is in math/time?

Only five modules exist: `math`, `time`, `qrc`, `cowsay`, and `html`. Import
before use:

```text
import math
import time

say(math.sqrt(25))
say(math.pow(2, 8))
say(math.pi)
say(time.year())
time.sleep(1.5)
```

`math` has `sqrt`, `abs`, `floor`, `ceil`, `round`, `pow`, `min`, `max`, `pi`,
and `e`. `min` and `max` take exactly two arguments, and `pow` takes two.
`time` has `now`, `sleep`, `year`, `month`, `day`, `hour`, `minute`, and
`second`; `time.now()` returns seconds since the Unix epoch.

### How do I build HTML, a QR code, or a cow?

```text
import html
import qrc
import cowsay

page = html.page("My Site", [
    html.h1("Hello"),
    html.p("Welcome."),
    html.list(["Home", "About"])
])
html.save("site/index.html", page)

qrc.make("https://example.com", "code.png")
cowsay.say("Moo from Englishmen!")
```

Other HTML helpers: `h1`–`h6`, `heading`, `div`, `section`, `span`, `link`,
`image`, `ol`, `tag`, `element`, and `escape`. `html.save` creates missing
parent directories.

### How do I build a standalone executable?

```bash
em build hello.em --exe
```

On Windows the result is a `.exe`; on Unix it has no extension. It runs without
Englishmen installed, but it is not native machine code and is not
cross-platform. On Linux, `em build hello.em --appimage` packages it as an
AppImage.

### How do I start a project?

```bash
em new MyProject
```

A project has an `englishmen.emc` file plus `main.em`. Inside it, `em build`
with no arguments compiles the `main` file.

### What is missing from the language?

- No `while`, `break`, or `continue`.
- No classes, exceptions, or `switch`.
- No `+=`, `-=`, `++`, `--`, or `**`.
- No string interpolation, ternary, or `typeof`.
- No `map`/`filter`/`sort` on arrays, and no `length()` on objects.
- No user input, general file I/O, random numbers, subprocesses, or networking.
- No user-defined modules or package manager; only the five standard modules.
- String-to-number conversion is not available in 1.5.x.

If a user asks for one of these, say it is unsupported and offer the closest
Englishmen-compatible approach.

### What mistakes should I avoid?

| Wrong | Correct |
| --- | --- |
| `print("Hi")` | `say("Hi")` |
| `let x = 1` | `x = 1` |
| `x = 1;` | `x = 1` |
| `// comment` | `$- comment -$` |
| `if (x > 0) { ... }` | `if x > 0` … `end` |
| `&&`, `\|\|`, `!` | `and`, `or`, `not` |
| `elif` | `else if` |
| `def f(x):` | `function f(x)` … `end` |
| `x++`, `x += 1` | `x = x + 1` |
| `x ** 2` | `math.pow(x, 2)` |
| `f"Hello {name}"` | `"Hello " + name` |
| `None`, `nil` | `null` |
| `True`, `False` | `true`, `false` |
| `len(x)` | `x.length()` for strings/arrays |
| `obj["key"]` | `obj.key` |

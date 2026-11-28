# Control flow

Englishmen uses indentation for readability and `end` to close blocks.

## Conditions

```text
age = 18

if age >= 18
    say("Adult")
else if age >= 13
    say("Teen")
else
    say("Child")
end
```

Boolean logic uses the words `and`, `or`, and `not`:

```text
if active and not blocked
    say("Ready")
end
```

## Repeat loops

```text
repeat 3
    say("Hello")
end
```

`repeat` takes a non-negative number; fractional counts are truncated. It can be nested inside `if` and other loops.

## For loops

```text
people = ["Alex", "Sam", "John"]

for person in people
    say("Hi, {person}!")
end
```

`for ... in` iterates arrays; objects can be walked through `keys()` or `values()`:

```text
person = {name: "Alex", age: 18}

for key in person.keys()
    say("{key}: {person.get(key)}")
end
```

## While loops and loop control

```text
n = 0
while n < 10
    n += 1
    if n == 3
        continue
    end
    if n == 8
        break
    end
    say(n)
end
```

`break` and `continue` work in `repeat`, `for ... in`, and `while` loops. They are rejected outside loops, including inside a function declared inside a loop.

## Operators and compact expressions

```text
a = 7
b = 2

say(a ** b)        $- 49, exponentiation -$
say(a // b)        $- 3, floor division; `div` also works -$
say(a & b)         $- bitwise and; also |, ^, ~, <<, >> -$
say(a % 2 == 1 ? "odd" : "even")
say(null ?? "fallback")
a += 1             $- also -=, *=, /=, %=, **=, //= and bitwise variants -$
a++                $- or a-- -$
```

`&&` and `||` are accepted alongside `and`/`or`; `!` and `not` are both negation. `//` is floor division when it follows an expression, and starts a comment at the start of a line.

## Returning early

`return` exits a function.

## Errors

Runtime failures carry a stable code, source span, hint, and optional secondary labels. `em check` catches syntax/compiler problems before you run, and `em build --emit ast` helps inspect how a program was parsed.

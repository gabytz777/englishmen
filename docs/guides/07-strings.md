# Strings

Strings are UTF-8 and use double quotes. Interpolation inserts expression results into `{ ... }`.

```text
name = "Alex"
say("Hello, {name}!")
say("{2 + 3} is five")
```

Comments can live inside an interpolation: `say("value={$- debug -$ x}")`. `em fmt` preserves them.

## Escapes

`\n`, `\r`, `\t`, `\"`, `\\`, `\u{...}` (Unicode scalar), and common single-character escapes are supported. The lexer also accepts hex, binary, and exponent-form numeric literals (`0xff`, `0b1010`, `1.5e3`).

## String methods

| Method | Purpose |
| --- | --- |
| `length()` | Character count (not bytes) |
| `upper()`, `lower()`, `trim()` | Case and whitespace |
| `capitalize()`, `titleCase()` | Title-style casing |
| `split(separator)`, `lines()`, `chars()` | Splitting |
| `contains(text)`, `startsWith(text)`, `endsWith(text)` | Searching |
| `indexOf(text)`, `lastIndexOf(text)`, `charAt(index)` | Position and character access |
| `slice(start, end)`, `substring(start, end)` | Substrings |
| `replace(from, to)`, `replaceAll(from, to)` | First or all occurrences |
| `repeat(count)` | Repetition with a bounded output size |
| `padLeft(width, fill)`, `padRight(width, fill)` | Padding |
| `reverse()` | Character reversal |
| `toNumber()` | Number or `null` on failure |
| `isDigit()`, `isAlpha()` | True only when every character is a digit/alphabetic; false on empty strings |

## Global string builtins

`len`, `format`, `upper`, `lower`, `trim`, `split`, `join`, `replace`, `startsWith`, `endsWith`, `repeat`, `indexOf`, and `contains` are also available globally. Global `indexOf` returns a character index, matching string methods.

```text
say("héllo".indexOf("é"))  $- 1 -$
say("ha".repeat(3))         $- hahaha -$
```

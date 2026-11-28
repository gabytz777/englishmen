# Global builtins

Global builtins are available in every scope without `import`. `em doc` prints the live catalogue; the groups below cover all 69 registered functions.

## Types and conversions

`type`, `toNumber`, `toString`, `toBool`, `isNumber`, `isString`, `isArray`, `isObject`, `isBool`, `isNull`, `isFunction`, `parseInt`, `parseFloat`.

```text
say(type([1, 2]))        $- array -$
say(toNumber("3.5"))     $- 3.5 -$
say(parseInt("ff", 16))  $- 255 -$
```

## Strings and output

`len`, `input`, `print`, `format`, `trim`, `upper`, `lower`, `split`, `join`, `replace`, `startsWith`, `endsWith`, `repeat`, `indexOf`, `contains`.

`say(...)` is a language statement rather than a global function; it adds a newline. `input()` reads a line from stdin, `print` writes without a trailing newline, and `format` builds a string from a template and values.

## Collections

`range`, `sum`, `average`, `min`, `max`, `abs`, `clamp`, `sorted`, `reversed`, `zip`, `keys`, `values`, `first`, `last`, `slice`.

```text
say(range(4))                $- [0, 1, 2, 3] -$
say(sorted([3, 1, 2]))       $- [1, 2, 3] -$
say(keys({a: 1, b: 2}))      $- ["a", "b"] -$
```

## Math and randomness

`pow`, `sqrt`, `floor`, `ceil`, `round`, `sign`, `random`, `randomInt`, `seed`, `randomChoice`, `shuffle`.

`seed(n)` makes interpreter and VM runs reproducible. `random.uuid()` uses OS entropy and is intentionally unaffected by `seed`.

## Files, environment, and time

`readFile`, `writeFile`, `appendFile`, `fileExists`, `deleteFile`, `listFiles`, `makeDir`, `clock`, `timestamp`, `env`, `args`, `sleep`.

File builtins return clear errors for missing files or invalid paths. `deleteFile` removes files recursively when given a directory, so pass exact paths.

## JSON and assertions

`jsonEncode`, `jsonDecode`, `assert`.

```text
text = jsonEncode({ok: true, n: 3})
say(jsonDecode(text).n)
assert(3 > 2, "math still works")
```

## Error behaviour

Builtins return errors rather than panicking. Notable guards include bounded `repeat`, finite-only `toNumber`, Unicode-correct string `indexOf`, and poison-safe random state.

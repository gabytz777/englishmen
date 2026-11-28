# Variables and types

Assignment creates a variable — there is no `var` or `let`. Variables are dynamically typed and can be reassigned to a different type.

```text
age = 18
name = "Alex"
active = true
nothing = null
scores = [10, 20, 30]
person = {name: "Alex", age: 18}

say(type(age))       $- number -$
say(type(name))      $- string -$
say(type(scores))    $- array -$
```

## Values

| Type | Example | Notes |
| --- | --- | --- |
| number | `3`, `3.5`, `-2` | IEEE-754 doubles; `NaN`/infinite values are rejected by conversion builtins |
| string | `"hello"` | Double quoted, UTF-8, supports interpolation and escapes |
| boolean | `true`, `false` | `and`, `or`, `not` work on booleans |
| null | `null` | Absence of a value |
| array | `[1, 2, 3]` | Indexable with `[0]`; grows with `.add()` |
| object | `{name: "Alex"}` | Dot access and nested objects supported |
| function/native | `add`, `say` | First-class callable values |

## Conversion builtins

```text
say(toNumber("42"))      $- 42 -$
say(toString(42))        $- "42" -$
say(toBool(""))          $- false -$
say(parseInt("ff", 16))  $- 255 -$
say(parseFloat("2.5e2")) $- 250 -$
```

`toNumber` rejects strings such as `NaN` and `inf` rather than injecting non-finite values. String `.toNumber()` returns `null` when conversion fails, so choose the form that matches your error-handling style.

## Type predicates

`isNumber`, `isString`, `isArray`, `isObject`, `isBool`, `isNull`, and `isFunction` return booleans. `type(value)` returns the lowercase type name as a string.

Objects can be created with literals or extended with dotted assignment:

```text
person = {}
person.name = "Sam"
person.address = {}
person.address.city = "London"
```

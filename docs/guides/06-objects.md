# Objects

Objects are ordered maps from string keys to values. They support literals, dot access, and nested assignment.

```text
person = {
    name: "Alex",
    age: 18,
    active: true
}

say(person.name)
person.city = "London"
person.address = {postcode: "NW1"}
say(person.address.postcode)
```

Missing fields raise an `Unknown property` error; use `obj.get(key)` or `obj.get(key, fallback)` for `null`/default behavior.

## Object methods

| Method | Purpose |
| --- | --- |
| `keys()`, `values()` | Lists of keys and values |
| `has(key)` | Whether a key exists |
| `get(key, fallback)` | Read with optional fallback |
| `set(key, value)`, `delete(key)` | Mutate the object |
| `length()`, `isEmpty()` | Size information |
| `merge(other)` | Copy fields from another object into this one |
| `copy()` | Shallow copy |

```text
for key in person.keys()
    say("{key}: {person.get(key)}")
end
```

## JSON and collections helpers

```text
import json
import collections

text = json.encode(person)
clone = json.decode(text)

say(collections.get(clone, "name", "unknown"))
say(collections.merge({a: 1}, {b: 2}))
```

`json.encode` and `json.pretty` reject non-finite numbers, functions, and excessively nested or cyclic values with a clear error instead of crashing.

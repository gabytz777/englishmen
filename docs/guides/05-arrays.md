# Arrays

Arrays are ordered lists. Indexing is zero-based and out-of-range reads return `null`.

```text
xs = [10, 20, 30]
say(xs[0])       $- 10 -$
xs[0] = 11
xs.add(40)       $- append -$
say(xs.length()) $- 4 -$
```

## Mutating methods

| Method | Purpose |
| --- | --- |
| `add(value)`, `push(value)` | Append |
| `remove(value)`, `removeAt(index)` | Remove by value or index |
| `pop()`, `shift()` | Remove from the end or front |
| `unshift(value)`, `insert(index, value)` | Insert at the front or at `index` |
| `clear()` | Empty the array |
| `reverse()`, `sort()` | Reorder in place |
| `swap(i, j)` | Exchange two positions |

## Query and transform methods

| Method | Purpose |
| --- | --- |
| `length()`, `isEmpty()` | Size information |
| `contains(value)`, `indexOf(value)`, `lastIndexOf(value)`, `count(value)` | Searching |
| `first()`, `last()` | Endpoints |
| `slice(start, end)` | Range copy; out-of-range ends are clamped |
| `join(separator)` | Stringify elements with a separator |
| `concat(other)` | Shallow concatenation |
| `copy()` | Shallow copy |
| `unique()` | Remove duplicates |
| `flatten()` | Flatten one or more nested array levels |

Indices count characters for strings and elements for arrays. Methods return `null` or an empty result for out-of-range positions rather than panicking.

Global array builtins complement the methods:

```text
say(range(5))                  $- [0, 1, 2, 3, 4] -$
say(sum([1, 2, 3]))            $- 6 -$
say(sorted([3, 1, 2]))         $- [1, 2, 3] -$
say(reversed([1, 2, 3]))       $- [3, 2, 1] -$
say(zip([1, 2], ["a", "b"]))   $- [[1, "a"], [2, "b"]] -$
```

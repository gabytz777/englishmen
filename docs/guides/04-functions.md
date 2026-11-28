# Functions

Functions are declared with `function`, end with `end`, and return with `return`. A function with no `return` produces `null`.

```text
function add(a, b)
    return a + b
end

say(add(10, 20))  $- 30 -$
```

## Recursion

```text
function factorial(n)
    if n <= 1
        return 1
    end
    return n * factorial(n - 1)
end

say(factorial(6))  $- 720 -$
```

Calls use the VM's own frame stack, so runaway recursion is reported as an Englishmen error rather than crashing the process.

## Functions are values

Named functions and native functions can be stored, passed, and called:

```text
function twice(f, x)
    return f(f(x))
end

function inc(n)
    return n + 1
end

say(twice(inc, 5))  $- 7 -$
```

## Builtins and module functions

Global builtins such as `len`, `range`, `pow`, and `jsonEncode` are available in every scope. `say(...)` is a statement, not a callable value. Module functions are accessed with dot notation:

```text
import math

say(math.sqrt(25))       $- 5 -$
say(sorted([3, 1, 2]))   $- [1, 2, 3] -$
```

`em doc` prints the current builtin, module, and keyword catalogue. Both the interpreter and VM see the same native functions.

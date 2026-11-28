# Numbers and math

Numbers are IEEE-754 doubles. Integers, decimals, and exponent notation all produce numbers:

```text
a = 42
b = 3.5
c = 1.5e3
hex = 0xff
bits = 0b1010
```

## Arithmetic and operators

`+`, `-`, `*`, `/`, `%`, `**` (power), and `//` or `div` (floor division) work on numbers. Bitwise operators `&`, `|`, `^`, `~`, `<<`, and `>>` are available too. `+=`, `-=`, `*=`, `/=`, `%=`, `**=`, `//=`, `++`, and `--` update variables in place.

Division by zero is a runtime error with the same wording on both backends.

## Number methods

| Method | Purpose |
| --- | --- |
| `isEven()`, `isOdd()`, `isInteger()`, `isFinite()` | Predicates |
| `toFixed(places)` | Fixed-decimal string |
| `clamp(low, high)` | Constrain to a range |
| `toHex()`, `toBinary()` | Integer formatting; non-finite values error |
| `sign()`, `abs()`, `floor()`, `ceil()`, `round()`, `sqrt()` | Common numeric operations |

## Math module

```text
import math

say(math.pi)
say(math.sqrt(25))
say(math.pow(2, 10))
say(math.sin(math.pi / 2))
say(math.gcd(12, 18))
say(math.clamp(15, 0, 10))
```

The module also provides `abs`, `floor`, `ceil`, `round`, `min`, `max`, `log`, `ln`, `exp`, `tan`, `cos`, `hypot`, `sign`, `lcm`, `degrees`, `radians`, `isPrime`, `trunc`, `roundTo`, `factorial`, `toHex`, and `toBinary`, plus `e`.

## Global numeric builtins

`range`, `sum`, `average`, `min`, `max`, `abs`, `clamp`, `pow`, `sqrt`, `floor`, `ceil`, `round`, `sign`, `parseInt`, `parseFloat`, and `toNumber` are available without an import. Conversion builtins reject non-finite tokens such as `NaN` and `inf`.

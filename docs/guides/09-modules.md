# Modules

`import <name>` loads a standard module into the current scope. All modules are compiled into the `em` binary; no Python, Node, or external tools are required.

```text
import math
import string
import json
import fs
import os
import gfx

say(math.sqrt(25))
say(string.upper("hello"))
```

## Available modules

| Module | Highlights |
| --- | --- |
| `math` | Trig, powers, rounding, GCD/LCM, primes, constants |
| `time` | `now`, `sleep`, date/time parts, `format`, `iso`, `isoUtc`, `millis`, `elapsed` |
| `qrc` | `make(text, path)` writes a QR-code PNG |
| `cowsay` | `say(text)` prints ASCII-art speech |
| `string` | Case, `pad`, splitting, joining, predicates, repeat/trim helpers |
| `json` | `encode`, `decode`, `pretty`, `isValid`, `get`, `set` |
| `fs` | `read`, `write`, `append`, `exists`, `list`, `mkdir`, `remove`, `copy`, `move`, `metadata`, `size` |
| `os` | `name`, `arch`, `platform`, `args`, `env`, `cwd`, `tempDir`, `homeDir`, `pid`, `version` |
| `random` | `float`, `int`, `choice`, `sample`, `shuffle`, `seed`, `uuid` |
| `collections` | Sorting, ranges, sums, zip/enumerate/chunk, object helpers |
| `gfx` | `canvas(w, h)` with `set`, `get`, `clear`, `text`, `save` |

## Example: files and JSON

```text
import json
import fs

data = {name: "Alex", scores: [9, 7, 10]}
fs.write("data.json", json.encode(data))
loaded = json.decode(fs.read("data.json"))
say(loaded.name)
```

## Module vs global APIs

Some names exist both as global builtins and module functions. Globals are convenient for one-off scripts; modules group related operations and are preferred for larger programs. Where semantics differ, the module form follows the behaviour documented in its guide (for example, `string.pad` alignment names and JSON cycle errors).

`import gfx` also powers the bundled 3D renderer examples in `renderer/render.enm` and `renderer/render_ascii.enm`.

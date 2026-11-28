# Self-hosted VM runtime (`selfhost/vm_runtime.enm`)

This file implements the runtime layer of the self-hosted Englishmen VM in pure
Englishmen. It consumes the same EMB IR object that `emb.encode` serialises to a
`.emb` image (see [`../selfhost/EMB_AND_MANIFEST.md`](../selfhost/EMB_AND_MANIFEST.md)),
executes it, and returns the values produced by `Say` instead of printing them.
That matches the output-array convention used by the C1 VM core so the two
pieces can be layered into a combined `full.enm`.

The file is self-contained: there are no local `.enm` imports. Every global
helper is prefixed with `rt` (the public entry point is `vmRunWithRuntime`), so
it can be concatenated with the other Round 2 scripts. There are no top-level
side effects outside the self-test guard.

## Entry points

```js
output = vmRunWithRuntime(ir)   // ir is an EMB IR object; output is an array of strings
em run selfhost/vm_runtime.enm ir.json
EM_SELFTEST=1 em run selfhost/vm_runtime.enm   // prints ok and exits 0
```

`vmRunWithRuntime(ir)` creates a fresh VM state
(`{stack: [], frames: [], output: [], globals: rtGlobalEnv()}`), runs the IR,
and returns `rtVm.output`. Each `Say` instruction appends one string built from
the displayed arguments, separated by spaces, exactly like native
`runtime::say` but without writing to stdout. The CLI reads a JSON IR file and
prints `jsonEncode(outputArray)`.

## IR and execution model

The IR shape is the documented chunk tree: a top-level object with `version`,
`constants`, `functions` and `code`; nested function prototypes contain only
`constants`, `functions` and `code`. Constants are tagged objects
(`number`, `string`, `bool`, `null`). Instructions are objects with `op`, `a`,
`b`, `line` and `col`; names and jump targets are indices in the enclosing
chunk, so nested functions cannot accidentally see the top-level constant pool.

Execution mirrors `src/vm.rs`:

- `rtVm.stack` is an ordinary Englishmen array used as a value stack.
- `rtVm.frames` holds `{rtUnit, rtIp, rtEnv, rtBase}` records. A closure call
  pushes a frame whose environment is a child of the closure environment and
  whose stack base is the current stack height; `rtPopFrame` truncates the
  stack to that base and hands the return value to the caller.
- `rtEnvNew`/`rtEnvGet`/`rtEnvDefine`/`rtEnvAssign` implement the parent-chain
  scope rules. Assignment walks up the chain and defines locally if not found.
- `rtStep` dispatches every opcode from `src/bytecode.rs`, including
  `GetPropKeep`, `GetIndexKeep`, `Rotate`, the four conditional jumps,
  `RepeatSetup`/`RepeatNext`, `IterSetup`/`IterNext`, and the newer bitwise,
  exponent and floor-division instructions. Operand order follows the Rust
  definition: `Call(argc, nameIndex)`, `MethodCall(nameIndex, argc)` and
  `IterNext(nameIndex, target)`. The callee/receiver is pushed before its
  arguments, as in the compiler.
- `rtInvoke` returns `{rtPushed, rtValue}` so a frame push does not also push a
  value. Native builtins are tagged objects (`rtKind: "native"`) in the global
  environment; IR closures are tagged objects (`rtKind: "closure"`) carrying a
  prototype and environment.
- `rtBinary` reproduces the native arithmetic, comparison, equality and string
  repetition rules. `rtGetProperty`, `rtGetIndex` and `rtSetIndex` provide the
  native error wording for bad property/index access.

`Say` lines are accumulated as strings; the CLI is the only place that prints
them. `Import` raises a clear error because a pure-Englishmen module loader is
outside this file's scope.

## Runtime builtins covered

`rtSay` and `rtLen` are implemented directly. The method dispatchers cover the
full native method lists:

| Receiver | Methods |
| --- | --- |
| number | `isEven`, `isOdd`, `isInteger`, `isFinite`, `toFixed`, `clamp`, `toHex`, `toBinary`, `toOctal`, `sign`, `abs`, `floor`, `ceil`, `round`, `sqrt`, `toDegrees`, `reciprocal` |
| string | `length`, `upper`, `lower`, `trim`, `split`, `contains`, `startsWith`, `endsWith`, `replace`, `replaceAll`, `slice`, `substring`, `indexOf`, `lastIndexOf`, `charAt`, `toNumber`, `capitalize`, `titleCase`, `reverse`, `repeat`, `padLeft`, `padRight`, `isDigit`, `isAlpha`, `lines`, `chars`, `swapCase`, `isBlank`, `truncate` |
| array | `add`, `push`, `remove`, `removeAt`, `length`, `contains`, `pop`, `shift`, `unshift`, `insert`, `clear`, `reverse`, `sort`, `slice`, `join`, `indexOf`, `lastIndexOf`, `count`, `first`, `last`, `concat`, `copy`, `isEmpty`, `swap`, `unique`, `flatten`, `rotate`, `indexesOf`, `interleave` |
| object | `keys`, `values`, `has`, `get`, `set`, `delete`, `length`, `merge`, `copy`, `isEmpty`, `pick`, `omit`, `entries` |
| bool | `toggle`, `toString`, `and`, `or`, `xor`, `implies`, `nand` |

Object fields that hold functions win over builtin methods, matching the native
VM. `rtGlobalEnv` registers the remaining global builtins as tagged natives and
`rtNativeCall` delegates them to host intrinsics where the language itself has
an equivalent (`type`, `toNumber`, `parseInt`, `range`, `sum`, `sorted`, `pow`,
`upper`, `join`, `format`, and so on). `say` and `len` are routed back to
`rtSay`/`rtLen` rather than to the host.

## Tests

`tests/selfhost_enm_vm.rs` drives `em` as a black box. Every fixture is an IR
object written to a temporary JSON file. A tiny generated `.enm` emitter reads
it with `jsonDecode` and writes the same object to a `.emb` image with native
`emb.write`; `em run-fixed` then executes that image on `bytecode::Vm`. The
test compares the native `Say` lines with the JSON array printed by
`em run selfhost/vm_runtime.enm <ir.json>`. Fixtures cover literals, arithmetic,
variables and `repeat`, `if`/`else`, closures and calls, array/object/string
methods, unicode `length`, and multi-argument `Say`.

Verification commands:

```sh
em fmt --check selfhost/vm_runtime.enm
EM_SELFTEST=1 em run selfhost/vm_runtime.enm
cargo test --all-targets
```

Use `CARGO_TARGET_DIR=/home/larpington/.cache/c2-em-target` in this workspace
when the default `/tmp` target directory runs out of tmpfs quota.

## Remaining native dependencies and caveats

- **CLI plumbing:** `fs.read`, `os.args`, `os.env`, `jsonDecode` and
  `jsonEncode` are host builtins. They are only the shell around the runtime;
  the VM state, stack, frames and method implementations are Englishmen code.
- **Binary serialization:** this file executes IR objects; it does not encode
  them to `.emb` bytes. `emb.encode` / `bytecode::serialize` are still native,
  and are used only as the test oracle. No local `.enm` imports exist yet, so
  combining this with the A2 encoder requires concatenating scripts.
- **Delegated intrinsics:** text/math/random/JSON globals delegated by
  `rtNativeCall` still execute native code. File and OS globals (`readFile`,
  `sleep`, `input`, modules, etc.) are registered as names but are not needed
  by the IR subset and are not delegated. An `Import` instruction fails
  explicitly instead of loading a native module.
- **Diagnostics:** error messages follow the native wording for common type
  and index failures, but source spans, suggestion hints, the 512-frame depth
  limit and some rare arity/argument-validation details of the Rust VM are not
  reproduced. The self-hosted runtime reports failures through `assert`, so a
  malformed IR aborts with an Englishmen error rather than an `EmError`.
- **Value display:** arrays and objects are displayed with native-style
  quoting and key ordering, but tagged closure/native objects are internal
  representations and are not intended to round-trip through `jsonEncode`.

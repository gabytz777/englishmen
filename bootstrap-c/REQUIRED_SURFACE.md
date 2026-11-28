# Round-4 C VM / host required surface

This inventory answers one question: **what must an independent C host implement
so that the round-3 self-hosted compiler image can replace the Rust `em` CLI?**

Target command:

```text
emrun <image.emb> [program args...]
```

Method: static reading of `bootstrap-c/ABI.md`, all `bootstrap-c/*.h`, every
component `.enm`, `src/builtins.rs`, `src/runtime.rs`, `src/modules/*.rs`, plus
`src/bytecode.rs`, `src/vm.rs`, and the CLI paths that emit/run `.emb`.
`selfhost/full.enm` and `selfhost/bootstrap_compile.enm` are **generated
concatenations** of the component files plus a driver; they are not independent
surface sources.

No existing header, `.enm`, or `.rs` file is modified by this document.

---

## 1. Acceptance path

The required proof is:

1. `make` builds `bootstrap-c/emrun`.
2. The Rust compiler (one-time oracle only) emits a compiler image by compiling
   `selfhost/bootstrap_compile.enm` with `selfhost/full.enm`.
3. `emrun <compiler-image.emb> examples/hello.enm` runs that image.
4. stdout is exactly:

   ```json
   {"astStatements":1,"encodedBytes":104,"irInstructions":4,"output":[]}
   ```

5. The same `.emb` run through `em run-fixed <file.emb>` produces identical
   stdout.

The Rust CLI accepts only `--time` as an extra flag; the C host does not need
that flag. The C host must not link against the Rust crate or call `src/vm.rs`.

---

## 2. `.emb` decoder surface

Source of truth: `src/bytecode.rs` and `bootstrap-c/emb.h`.

Format v2:

* magic `EMBC`
* `u16` little-endian version, must be `2`
* `write_chunk` order: constant count, constants, function count, prototypes,
  code count, instructions
* constant tags:
  * `0` number, little-endian `f64`
  * `1` length-prefixed UTF-8 string
  * `2` bool byte
  * `3` null
* every instruction is `tag:u8`, `a:u32` LE, `b:u32` LE, `line:u32` LE,
  `col:u32` LE
* limits: `EMB_MAX_CHUNK_DEPTH 32`, `EMB_MAX_ROTATE 16`

Decoder must reject malformed tags, truncated chunks, bad string lengths,
unknown constant tags, out-of-range function/name/constant indices, and depth
over 32. `line`/`col` are diagnostic-only but must be preserved for error
messages.

Exact API:

```c
int em_emb_parse_bytes(const uint8_t *bytes, size_t len, EmProgram **out, char **err);
int em_emb_parse_file(const char *path, EmProgram **out, char **err);
void em_emb_free(EmProgram *program);
const char *em_op_name(uint8_t tag);
```

`EmProgram`, `EmChunk`, `EmFunctionProto`, `EmConst`, `EmInstr`, `EmConstTag`,
and `EmOp` are defined in `bootstrap-c/emb.h`.

---

## 3. Emitted opcode surface

Authoritative emitter: `selfhost/codegen_ir.enm` (`cgEmit` and its lowering
helpers). Authoritative name/tag table: `selfhost/ir_encode.enm` (`ieOps`).
Tags mirror `encode_op` in `src/bytecode.rs`.

`ieOps` shape notation:

* `0` — both operands zeroed
* `1` — instruction keeps `a`
* `2` — instruction keeps `a` and `b`

`IterNext` keeps `a` as the loop variable name index and patches `b` as the
jump target; other jumps patch `a`.

All 55 names below are emitted/encoded by the compiler and must be executable.
An unknown tag is a hard error.

| Name | Tag | Shape | Required semantics |
|---|---:|---:|---|
| `LoadConst` | 0 | 1 | push constant `a` |
| `Pop` | 1 | 0 | pop and discard |
| `GetVar` | 2 | 1 | push variable named by `a` |
| `SetVar` | 3 | 1 | assign variable `a` from top |
| `DefineVar` | 4 | 1 | define variable `a` from top |
| `GetProp` | 5 | 1 | pop base, push property `a` |
| `SetProp` | 6 | 1 | pop value and base, set property `a` |
| `GetVarOrObject` | 7 | 1 | get variable `a`, defining an empty object first if absent |
| `GetPropOrObject` | 8 | 1 | get property `a`; on object base, auto-create empty object if missing |
| `GetIndex` | 9 | 0 | pop index then base, push element |
| `SetIndex` | 10 | 0 | pop value, index, base; assign element |
| `MakeArray` | 11 | 1 | pop `a` values, push array |
| `MakeObject` | 12 | 1 | pop `2*a` key/value pairs, push object |
| `MakeClosure` | 13 | 1 | push closure over function prototype `a` and current env |
| `Call` | 14 | 2 | pop `a` args then callee; `b` is name index for diagnostics |
| `MethodCall` | 15 | 2 | pop `b` args then receiver; `a` is method name index |
| `Say` | 16 | 1 | pop `a` args, write display forms separated by spaces, push null |
| `Add` | 17 | 0 | binary add (numeric/string/list behaviour) |
| `Sub` | 18 | 0 | numeric subtraction |
| `Mul` | 19 | 0 | numeric multiplication |
| `Div` | 20 | 0 | numeric division |
| `Mod` | 21 | 0 | numeric modulo |
| `FloorDiv` | 42 | 0 | floor division |
| `Pow` | 43 | 0 | exponentiation |
| `BitAnd` | 44 | 0 | `i64` bitwise and |
| `BitOr` | 45 | 0 | `i64` bitwise or |
| `BitXor` | 46 | 0 | `i64` bitwise xor |
| `Shl` | 47 | 0 | `i64` shift left |
| `Shr` | 48 | 0 | `i64` shift right |
| `Eq` | 22 | 0 | deep equality |
| `NotEq` | 23 | 0 | negated equality |
| `Gt` | 24 | 0 | greater than |
| `Lt` | 25 | 0 | less than |
| `GtEq` | 26 | 0 | greater than or equal |
| `LtEq` | 27 | 0 | less than or equal |
| `Neg` | 28 | 0 | numeric negation |
| `Not` | 29 | 0 | logical negation |
| `BitNot` | 49 | 0 | `i64` bitwise not |
| `ToBool` | 30 | 0 | truthiness coercion |
| `Jump` | 31 | 1 | unconditional jump to `a` |
| `JumpIfFalse` | 32 | 1 | pop condition, jump if false |
| `JumpIfFalsePeek` | 33 | 1 | peek condition, jump if false, do not pop |
| `JumpIfTruePeek` | 34 | 1 | peek condition, jump if true, do not pop |
| `RepeatSetup` | 35 | 0 | pop count, push integer countdown |
| `RepeatNext` | 36 | 1 | decrement countdown or pop it and jump to `a` |
| `IterSetup` | 37 | 0 | pop array, push snapshot array and cursor `0` |
| `IterNext` | 38 | 2 | bind name `a` to next item or pop iterator state and jump to `b` |
| `Return` | 39 | 0 | pop return value and leave frame |
| `ReturnNull` | 40 | 0 | return null |
| `Import` | 41 | 1 | define module named by `a` in current env |
| `Dup` | 50 | 0 | duplicate top |
| `Rotate` | 51 | 1 | rotate top `a+1` stack values; `a` must be `< 16` |
| `GetPropKeep` | 52 | 1 | like `GetProp`, but keep base on stack |
| `GetIndexKeep` | 53 | 0 | like `GetIndex`, but keep base and index on stack |
| `JumpIfNullPeek` | 54 | 1 | peek top, jump to `a` if null, do not pop |

`RepeatSetup` rejects negative counts. `IterSetup` accepts arrays only and must
snapshot the array so body mutation cannot disturb iteration. `MakeObject`
converts keys with display semantics. `GetVarOrObject`/`GetPropOrObject` are
required for lazy object construction used by the compiler IR.

---

## 4. Value and VM API surface

Exact signatures from `bootstrap-c/value.h` and `bootstrap-c/vm.h`.

```c
typedef EmValue *(*EmNativeFn)(EmVm *vm, EmValue **args, size_t argc, char **err);

EmValue *emv_number(double n);
EmValue *emv_string(const char *s);
EmValue *emv_string_len(const char *s, size_t n);
EmValue *emv_bool(bool b);
EmValue *emv_null(void);
EmValue *emv_array(void);
EmValue *emv_object(void);
EmValue *emv_closure(EmFunctionProto *proto, EmEnv *env);
EmValue *emv_native(const char *name, EmNativeFn fn);
void emv_retain(EmValue *v);
void emv_release(EmValue *v);
EmValueType emv_type(const EmValue *v);
bool emv_is_number(const EmValue *v);
bool emv_is_string(const EmValue *v);
bool emv_is_bool(const EmValue *v);
bool emv_is_null(const EmValue *v);
bool emv_is_array(const EmValue *v);
bool emv_is_object(const EmValue *v);
bool emv_is_closure(const EmValue *v);
bool emv_is_native(const EmValue *v);
double emv_as_number(const EmValue *v);
bool emv_as_bool(const EmValue *v);
const char *emv_as_cstr(const EmValue *v);
size_t emv_strlen(const EmValue *v);
size_t emv_array_len(const EmValue *array);
EmValue *emv_array_get(const EmValue *array, size_t i);
void emv_array_push(EmValue *array, EmValue *item);
void emv_array_set(EmValue *array, size_t i, EmValue *item);
bool emv_object_has(const EmValue *object, const char *key);
EmValue *emv_object_get(const EmValue *object, const char *key);
void emv_object_set(EmValue *object, const char *key, EmValue *value);
bool emv_object_delete(const EmValue *object, const char *key);
char **emv_object_keys(const EmValue *object, size_t *count);
EmFunctionProto *emv_closure_proto(const EmValue *v);
EmEnv *emv_closure_env(const EmValue *v);
EmNativeFn emv_native_fn(const EmValue *v);
const char *emv_native_name(const EmValue *v);
bool emv_truthy(const EmValue *v);
bool emv_equal(const EmValue *a, const EmValue *b);
char *emv_display(const EmValue *v);
char *emv_debug(const EmValue *v);

EmVm *em_vm_new(void);
void em_vm_free(EmVm *vm);
int em_vm_register_native(EmVm *vm, const char *name, EmNativeFn fn);
int em_vm_register_import(EmVm *vm, const char *name, EmValue *module);
EmValue *em_vm_lookup_import(EmVm *vm, const char *name);
EmEnv *em_vm_global_env(EmVm *vm);
int em_vm_run(EmVm *vm, const EmChunk *chunk, char **err);
int em_vm_call_value(EmVm *vm, EmValue *callee, EmValue **args, size_t argc,
                     EmValue **out, char **err);
int em_vm_call_method(EmVm *vm, EmValue *receiver, const char *name,
                      EmValue **args, size_t argc, EmValue **out, char **err);
void em_vm_error(EmVm *vm, uint32_t line, uint32_t col, const char *message);
char *em_vm_error_message(EmVm *vm);
```

Ownership rules in the headers are binding: constructors and `display`/`debug`
return owned values/strings; `array_get` and `object_get` return borrowed
pointers; `array_push`, `array_set`, and `object_set` retain.

`em_vm_run` must maintain a frame stack, stack underflow checks, lexical env
chains, closure capture, and `Import` lookup through `em_vm_lookup_import`.
`em_vm_call_method` must implement the dispatch rules in section 6.

Builtin registration API:

```c
int em_builtins_register(EmVm *vm, char **err);
int em_builtin_call(EmVm *vm, const char *name, EmValue **args, size_t argc,
                    EmValue **out, char **err);
bool em_builtin_exists(const char *name);
```

Host registration API:

```c
int em_host_register(EmVm *vm, int argc, char **argv, char **err);
EmValue *em_host_module(EmVm *vm);
```

---

## 5. Global builtins

### 5.1 Mandatory baseline: `rtGlobalEnv`

`selfhost/vm_runtime.enm` builds exactly these 65 global native names. The C
VM's global environment must define all of them, even if some are only used by
runtime delegate methods in the image.

```text
say print len type toString toNumber toBool isNumber isString isArray isObject
isBool isNull isFunction isNaN parseInt parseFloat range sum average min max
abs clamp sorted reversed zip take drop keys values entries fromEntries first
last indexOf contains slice pow sqrt floor ceil round sign trim upper lower
split join replace startsWith endsWith repeat padStart padEnd charCode
fromCharCode random randomInt seed format assert jsonEncode jsonDecode args
```

`say` is special in Rust (the compiler lowers it to `Say`, and the interpreter
handles it directly), but the self-hosted runtime env still defines it as a
native. Register it as a builtin that writes to stdout.

Heavily used by compiler production code: `assert`, `len`, `toString`, `type`,
`isArray`, `isObject`, `isString`, `isNumber`, `isBool`, `isNull`, `isNaN`,
`floor`, `ceil`, `abs`, `pow`, `sqrt`, `round`, `min`, `max`, `range`, `join`,
`slice`, `contains`, `startsWith`, `endsWith`, `charCode`, `fromCharCode`,
`parseInt`, `parseFloat`, `toNumber`, `toBool`, `args`, `say`, `jsonEncode`,
`jsonDecode`, `keys`, `values`, `entries`.

The remaining `rtGlobalEnv` names are still mandatory because the image may
call them from `vm_runtime.enm` delegate paths; they are not optional
simply because their direct call sites are rarer.

### 5.2 Full Rust global surface

`src/builtins.rs::globals()` registers 83 names. Relative to the 65 above it
adds:

```text
median log2 log10 toDegrees toRadians randomChoice shuffle input readFile
writeFile appendFile fileExists deleteFile listFiles makeDir clock timestamp
env sleep
```

These are not all required for the acceptance image. Implement them for
language parity as follows:

* **Pure/math parity:** `median`, `log2`, `log10`, `toDegrees`, `toRadians`,
  `randomChoice`, `shuffle`, `clock`, `timestamp`.
* **Host-backed parity:** `input`, `readFile`, `writeFile`, `appendFile`,
  `fileExists`, `deleteFile`, `listFiles`, `makeDir`, `env`, `sleep`. These
  should delegate to the host module primitives or private C helpers.
* **Oracle/self-test-only in selfhost sources:** `readFile` and `deleteFile`
  appear only in guarded self-tests in `lib_project.enm`; they are not on the
  production compiler image path.

`jsonEncode`/`jsonDecode`/`args`/`assert` are explicitly called out in
`ABI.md` and are mandatory even though they are already in the 65-name list.

---

## 6. Runtime method dispatch by value type

Source of truth: `src/runtime.rs` and `selfhost/vm_runtime.enm`.
`em_vm_call_method` and `MethodCall` must dispatch as follows.

### 6.1 Array

Full Rust dispatch list:

```text
add push remove removeAt length contains pop shift unshift insert clear reverse
sort slice join indexOf lastIndexOf count first last concat copy isEmpty swap
unique flatten rotate indexesOf interleave
```

Required by self-hosted compiler production code:

```text
add pop length() indexOf join slice copy contains insert
```

`sort` appears only in guarded self-tests. `push` is an alias of `add`. The
other listed methods are parity surface.

### 6.2 Object

Full Rust dispatch list:

```text
keys values has get set delete length merge copy isEmpty pick omit entries
```

Required by self-hosted compiler production code:

```text
get set has delete keys values entries length
```

Important dispatch rule: if a function-valued field with the method's name is
present on an object, call that field instead of the builtin method. This is how
the self-hosted compiler gives module objects custom behaviour.

### 6.3 String

Full Rust dispatch list:

```text
length upper lower trim split contains startsWith endsWith replace replaceAll
slice substring indexOf lastIndexOf charAt toNumber capitalize titleCase
reverse repeat padLeft padRight isDigit isAlpha lines chars swapCase isBlank
truncate
```

Required by self-hosted compiler production code:

```text
chars charAt indexOf lastIndexOf startsWith endsWith split slice substring
lower upper trim toNumber isDigit isAlpha isBlank repeat padLeft padRight
replace replaceAll capitalize titleCase swapCase truncate lines length
```

The self-hosted runtime also implements this same set in `rtNumberMethod` /
`rtStringMethod` helpers; C must match display/index semantics, including
UTF-8 character counts rather than byte counts for `length`, `charAt`, and
slice indices.

### 6.4 Number

Full Rust dispatch list:

```text
isEven isOdd isInteger isFinite toFixed clamp toHex toBinary sign abs floor
ceil round sqrt toOctal toDegrees reciprocal
```

Direct compiler production use: `toFixed`, `isFinite`.
The self-hosted runtime dispatch lists the same names; implement all for parity.
`toFixed` accepts 0–10 digits. Conversions reject non-finite/out-of-range
numbers.

### 6.5 Boolean

Full Rust dispatch list:

```text
toggle toString and or xor implies nand
```

Present in the self-hosted runtime dispatch but not prominently used by
compiler production code. Implement for parity.

### 6.6 Null / functions

No builtin methods. Method calls on these types must produce a typed error with
the instruction's `line`/`col`.

---

## 7. Native module functions

### 7.1 Current self-hosted component imports

These are the native module members actually referenced by the component
`.enm` files. Generated drivers add the larger set listed in section 7.2.

| Module | Members used by components |
|---|---|
| `fs` | `read`, `exists`, `isFile`, `write`, `append` |
| `os` | `env`, `args`, `pid`, `tempDir` |
| `emb` | `encode` (self-test only in components), `write`, `writeBytes` |
| `json` | `encode`, `decode`, plus self-test `get`, `set`, `isValid`, `pretty` |
| `bytes` | production `read`, `write`; nearly all pure byte helpers are self-test-only |
| `math` | adapters used by generated driver; components mostly use pure wrappers |
| `project` | `parse`, `name`, `version`, `main`, `write` |
| `cowsay` | `say` (driver adapter only) |

`full.enm` builds `emVmImports()` with adapters for `fs`, `os`, `emb`, `bytes`,
`math`, `cowsay`, `project`, plus the pure `json` and `string` module objects.
`rtNativeCall` additionally requires the driver-provided `emProgramArgs`,
`emJsonEncodeValue(value)`, and `emJsonDecodeValue(text)`; in `full.enm` these
are defined at lines 7691–7698 using `programArgsFull` and pure `emPureJson`.

### 7.2 Generated driver / tool surface

`full.enm` and `bootstrap_compile.enm` additionally call:

```text
fs.read fs.exists fs.isFile fs.write fs.append fs.size fs.metadata fs.copy
fs.move fs.remove fs.list fs.cwd fs.mkdir fs.isDir
os.env os.args os.pid os.tempDir
emb.encode emb.write emb.writeBytes
json.encode json.decode
bytes.read bytes.write
math.sqrt math.abs math.floor math.ceil math.round math.pow math.min
math.max math.log math.ln math.exp math.hypot math.sign math.gcd math.lcm
math.clamp math.degrees math.radians math.isPrime math.trunc math.roundTo
math.factorial math.toHex math.toBinary math.pi math.e
cowsay.say
project.parse project.name project.version project.main project.write
```

### 7.3 Round-4 `host` primitive mapping

`ABI.md` requires exactly one native module named `host`, exposed through
`Import`. Required keys:

| `host` key | Signature / result | Used for |
|---|---|---|
| `args()` | array of program args after image path | `os.args`, `args` builtin |
| `env(name, fallback)` | string or fallback/null | `os.env`, `env` builtin |
| `readFile(path)` | string, or object with `__hostError` | `fs.read`, `readFile` |
| `writeFile(path, text)` | bool, or object with `__hostError` | `fs.write`, `writeFile` |
| `readBytes(path)` | array of byte numbers, or `__hostError` | `fs.readBytes`, `bytes.read` |
| `writeBytes(path, bytes)` | bool, or `__hostError` | `bytes.write`, `emb.writeBytes` |
| `stdout(text)` | bool | pure `say`/`print` adapters |
| `stderr(text)` | bool | diagnostics |
| `exit(code)` | no return | driver failure paths |
| `cwd()` | string | `fs.cwd` |
| `tempDir()` | string | `os.tempDir` |
| `pid()` | number | `os.pid` |

Mapping notes:

* Pure Englishmen modules in `selfhost/pure_*.enm` are expected to wrap these
  primitives; no native `import fs/json/os/...` should remain in compiler
  source outside guarded self-tests.
* `json.encode`/`json.decode` map to builtins `jsonEncode`/`jsonDecode` or a
  pure module; no host key is needed.
* `emb.encode` must be implemented by the pure encoder (`ieEncode` /
  bytecode serialization) or a private host extension; it is not one of the
  required `host` keys. It is not needed to run an already emitted image.
* `fs.append`, `fs.exists`, `fs.size`, `fs.metadata`, `fs.copy`, `fs.move`,
  `fs.remove`, `fs.list`, and `fs.mkdir` can be composed from `readFile` /
  `writeFile` plus private C filesystem helpers. They are driver/tooling
  surface, not required by the acceptance image.
* `math.*` and `cowsay.say` are pure/private driver adapters and are not
  required `host` keys.

---

## 8. Oracle-only / self-test-only surface

Anything below is excluded from the minimum C host unless a future image
actually calls it.

### `EM_SELFTEST` guards

* `emb.encode` calls in `selfhost/codegen_ir.enm` and `selfhost/ir_encode.enm`
  are inside `EM_SELFTEST` guards. `ir_encode.enm`'s "all opcodes" self-test
  exercises all 55 opcode names through `emb.encode`; this proves the opcode
  table but does not require the C host to implement `emb.encode`.
* `cgT*` helpers in `codegen_ir.enm` and `ir_encode.enm` are test fixtures.
* `lmSelfEqual` and `lmSelfSameArray` in `lib_modules.enm` are test helpers.
* `readFile` and `deleteFile` bare builtin calls in `lib_project.enm` are
  self-test fixture setup.
* `json.get`, `json.set`, `json.isValid`, and `json.pretty` from the pure
  `json` module are only used by module self-tests.
* Nearly all pure `bytes.*` helpers (`of`, `fromString`, `toString`,
  `fromHex`, `toHex`, `length`, `get`, `set`, `concat`, `slice`, `copy`, `u8`,
  `u16`, `u32`, `u64`, `i16`, `i32`, `i64`, `f64`) are self-test-only. Only
  `bytes.read` and `bytes.write` are production host adapters.
* Pure `string.*` helpers (`charAt`, `contains`, `endsWith`, `get`,
  `indexOf`, `join`, `lastIndexOf`, `pad`, `repeat`, `replace`, `slice`,
  `split`, `startsWith`, `substring`) are only used by self-tests. Do not
  confuse calls on a variable named `text` with a native `text` module.

### One-time oracle usage

* `selfhost/full.enm` is used by the Rust `em` CLI only to emit the compiler
  image. The C host does not have to be able to compile source text for the
  acceptance proof.
* `em run-fixed <file.emb>` is the comparison oracle, not a runtime dependency.
* `--time` is an accepted Rust CLI flag only.

---

## 9. Minimal implementation checklist

1. Parse `.emb` v2 exactly, including all 55 tags and both limits.
2. Implement the value API with correct ownership and UTF-8-aware strings.
3. Implement stack frames, lexical env, closures, and return propagation.
4. Register all 65 `rtGlobalEnv` names plus the optional parity globals.
5. Implement all method tables in section 6, including object field overrides.
6. Register the `host` module with the 12 keys in section 7.3.
7. Route `Say` and `print`/`say` builtin through `host.stdout`.
8. Preserve `line`/`col` in all VM errors.
9. Run the emitted compiler image and compare stdout byte-for-byte with the
   Rust oracle output in section 1.

That is the complete surface required to replace the Rust `em` execution host
for round 4.

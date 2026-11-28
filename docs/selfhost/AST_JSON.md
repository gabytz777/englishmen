# AST JSON schema (version 1)

`em build <file> --emit ast-json` prints one compact JSON line describing the
parsed program. The `.enm` bootstrap compiler, parser and emitter print the
same bytes. Englishmen objects are `BTreeMap`-backed, so **every object key is
written in alphabetical order** by `json.encode`. The snippets below list the
schema fields in reading order, not serialization order; consumers must access
fields by name rather than by position.

## Root

```js
{"version":1,"statements":[StmtJSON, ...]}
```

A span is `{"line":n,"col":n}`. Lines and columns are 1-based character
positions from the original source. End columns are intentionally omitted.

## Statements

```js
{"kind":"Assign","span":S,"target":AssignableJSON,"value":ExprJSON}
{"kind":"CompoundAssign","span":S,"target":AssignableJSON,"op":Op,"value":ExprJSON}
{"kind":"ExprStmt","span":S,"expr":ExprJSON}
{"kind":"If","span":S,"branches":[{"condition":ExprJSON,"body":[StmtJSON, ...]}, ...],"else":[StmtJSON, ...]|null}
{"kind":"FunctionDef","span":S,"name":"f","params":["a", ...],"body":[StmtJSON, ...]}
{"kind":"Return","span":S,"value":ExprJSON|null}
{"kind":"Repeat","span":S,"count":ExprJSON,"body":[StmtJSON, ...]}
{"kind":"While","span":S,"condition":ExprJSON,"body":[StmtJSON, ...]}
{"kind":"ForIn","span":S,"name":"x","iterable":ExprJSON,"body":[StmtJSON, ...]}
{"kind":"Break","span":S}
{"kind":"Continue","span":S}
{"kind":"Import","span":S,"name":"json"}
```

## Assignment targets

```js
{"kind":"Variable","name":"x"}
{"kind":"Property","base":ExprJSON,"name":"a"}
{"kind":"Index","base":ExprJSON,"index":ExprJSON}
```

## Expressions

```js
{"kind":"Number","value":1}
{"kind":"String","value":"x"}
{"kind":"Bool","value":true}
{"kind":"Null"}
{"kind":"Variable","name":"x"}
{"kind":"PropertyAccess","base":ExprJSON,"name":"a"}
{"kind":"Index","base":ExprJSON,"index":ExprJSON}
{"kind":"Array","items":[ExprJSON, ...]}
{"kind":"Object","fields":[{"name":"a","value":ExprJSON}, ...]}
{"kind":"Call","callee":ExprJSON,"args":[ExprJSON, ...]}
{"kind":"MethodCall","receiver":ExprJSON,"name":"m","args":[ExprJSON, ...]}
{"kind":"Binary","op":Op,"left":ExprJSON,"right":ExprJSON,"span":S}
{"kind":"Unary","op":UnOp,"operand":ExprJSON,"span":S}
{"kind":"Ternary","condition":ExprJSON,"whenTrue":ExprJSON,"whenFalse":ExprJSON,"span":S}
{"kind":"Postfix","op":"++"|"--","target":ExprJSON,"span":S}
```

Binary operators are `+ - * / % // ** & | ^ << >> == != > < >= <= and or ??`.
Unary operators are `-`, `not`, and `~`.

Object fields use an array of `{name,value}` pairs rather than a JSON object so
duplicate keys and source order survive the round trip. Interpolated strings
are lowered to `String("")` plus `+` chains, exactly as the Rust parser does.

## Stability rules

- integers print without a trailing `.0`; finite non-integers use Rust's
  shortest `f64` representation;
- strings use the `json` module escaping (`\"`, `\\`, `\n`, `\r`, `\t`,
  `\b`, `\f`, and `\u00XX` for other control characters);
- no whitespace is emitted between JSON tokens;
- object keys are sorted alphabetically at every level, because all producers
  serialize through `json.encode` (the `.enm` emitter canonicalizes its
  hand-built text with `json.decode` followed by `json.encode`);
- the Rust implementation is `src/ast_json.rs`; the Englishmen implementation
  is `selfhost/emit.enm`.

//! Write `.emb` bytecode images from Englishmen values.
//!
//! The input IR mirrors [`crate::bytecode::Chunk`]: a top-level object with a
//! `version`, a constant pool, nested functions and instructions. This module
//! only translates that description into a `Chunk` and hands it to
//! `bytecode::serialize`, so the emitted bytes are byte-for-byte identical to
//! images produced by the Rust compiler. `writeBytes` writes an already-encoded
//! byte array unchanged, which is how a pure encoder saves a loadable image.

use crate::bytecode::{self, Chunk, Const, FunctionProto, Op};
use crate::error::Span;
#[cfg(test)]
use crate::modules::support::call_native;
use crate::modules::support::{array_items, expect_path};
use crate::runtime::{expect_number, expect_string, native};
use crate::value::Value;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Keep in step with the deserializer's limit in `crate::bytecode`.
const MAX_CHUNK_DEPTH: usize = 32;

#[derive(Clone, Copy)]
enum Operands {
    None,
    A,
    AB,
}

/// Opcode names from the self-hosting contract, with the tag assigned by
/// `bytecode::encode_op` and the operand shape that tag actually emits.
const OPS: &[(&str, u8, Operands)] = &[
    ("LoadConst", 0, Operands::A),
    ("Pop", 1, Operands::None),
    ("GetVar", 2, Operands::A),
    ("SetVar", 3, Operands::A),
    ("DefineVar", 4, Operands::A),
    ("GetProp", 5, Operands::A),
    ("SetProp", 6, Operands::A),
    ("GetVarOrObject", 7, Operands::A),
    ("GetPropOrObject", 8, Operands::A),
    ("GetIndex", 9, Operands::None),
    ("SetIndex", 10, Operands::None),
    ("MakeArray", 11, Operands::A),
    ("MakeObject", 12, Operands::A),
    ("MakeClosure", 13, Operands::A),
    ("Call", 14, Operands::AB),
    ("MethodCall", 15, Operands::AB),
    ("Say", 16, Operands::A),
    ("Add", 17, Operands::None),
    ("Sub", 18, Operands::None),
    ("Mul", 19, Operands::None),
    ("Div", 20, Operands::None),
    ("Mod", 21, Operands::None),
    ("FloorDiv", 42, Operands::None),
    ("Pow", 43, Operands::None),
    ("BitAnd", 44, Operands::None),
    ("BitOr", 45, Operands::None),
    ("BitXor", 46, Operands::None),
    ("Shl", 47, Operands::None),
    ("Shr", 48, Operands::None),
    ("Eq", 22, Operands::None),
    ("NotEq", 23, Operands::None),
    ("Gt", 24, Operands::None),
    ("Lt", 25, Operands::None),
    ("GtEq", 26, Operands::None),
    ("LtEq", 27, Operands::None),
    ("Neg", 28, Operands::None),
    ("Not", 29, Operands::None),
    ("BitNot", 49, Operands::None),
    ("ToBool", 30, Operands::None),
    ("Jump", 31, Operands::A),
    ("JumpIfFalse", 32, Operands::A),
    ("JumpIfFalsePeek", 33, Operands::A),
    ("JumpIfTruePeek", 34, Operands::A),
    ("RepeatSetup", 35, Operands::None),
    ("RepeatNext", 36, Operands::A),
    ("IterSetup", 37, Operands::None),
    ("IterNext", 38, Operands::AB),
    ("Return", 39, Operands::None),
    ("ReturnNull", 40, Operands::None),
    ("Import", 41, Operands::A),
    ("Dup", 50, Operands::None),
    ("Rotate", 51, Operands::A),
    ("GetPropKeep", 52, Operands::A),
    ("GetIndexKeep", 53, Operands::None),
    ("JumpIfNullPeek", 54, Operands::A),
];

/// Native functions exported to Englishmen programs as `emb`.
pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "encode".to_string(),
        native("emb.encode", |args| {
            let bytes = encode_ir(args.get(0).unwrap_or(&Value::Null))?;
            Ok(Value::new_array(
                bytes
                    .into_iter()
                    .map(|byte| Value::Number(byte as f64))
                    .collect(),
            ))
        }),
    );

    fields.insert(
        "write".to_string(),
        native("emb.write", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for emb.write",
            )?;
            let bytes = encode_ir(args.get(1).unwrap_or(&Value::Null))?;
            std::fs::write(&path, bytes)
                .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
            Ok(Value::Bool(true))
        }),
    );

    fields.insert(
        "writeBytes".to_string(),
        native("emb.writeBytes", |args| {
            let path = expect_path(
                arg(args, 0, "emb.writeBytes", "path")?,
                "as the path for emb.writeBytes",
            )?;
            let raw = buffer_bytes(
                arg(args, 1, "emb.writeBytes", "buffer")?,
                "for emb.writeBytes",
            )?;
            std::fs::write(&path, &raw)
                .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
            Ok(Value::Bool(true))
        }),
    );

    fields.insert(
        "encodeOp".to_string(),
        native("emb.encodeOp", |args| {
            let name = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as an opcode name for emb.encodeOp",
            )?;
            Ok(Value::Number(encode_op(&name)? as f64))
        }),
    );

    fields.insert(
        "magic".to_string(),
        native("emb.magic", |_| Ok(Value::String(magic().to_string()))),
    );

    fields.insert(
        "version".to_string(),
        native("emb.version", |_| Ok(Value::Number(version() as f64))),
    );

    Value::new_object(fields)
}

/// Fetch a required argument, matching the diagnostic style of `bytes.write`.
fn arg<'a>(
    args: &'a [Value],
    index: usize,
    function: &str,
    description: &str,
) -> Result<&'a Value, String> {
    args.get(index)
        .ok_or_else(|| format!("{function} expects argument {} ({description})", index + 1))
}

/// Validate one byte, using the same range and wording as `bytes.write`.
fn byte_value(value: &Value, context: &str) -> Result<u8, String> {
    let n = expect_number(value, context)?;
    if !n.is_finite() || n.fract() != 0.0 || !(0.0..=255.0).contains(&n) {
        return Err(format!(
            "Expected a whole number from 0 to 255 {context}, found {n}"
        ));
    }
    Ok(n as u8)
}

/// Copy and validate a whole array before touching the file system.
fn buffer_bytes(value: &Value, context: &str) -> Result<Vec<u8>, String> {
    let items = array_items(value, context)?;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| byte_value(item, &format!("at index {index} {context}")))
        .collect()
}

/// Magic bytes written at the start of every image (`"EMBC"`).
pub fn magic() -> &'static str {
    "EMBC"
}

/// Bytecode version written by this build of Englishmen.
pub fn version() -> u16 {
    bytecode::EMB_VERSION
}

/// Numeric tag for an Englishmen `Op` variant name.
pub fn encode_op(name: &str) -> Result<u8, String> {
    lookup_op(name)
        .map(|(tag, _)| tag)
        .ok_or_else(|| format!("emb.encodeOp: unknown op name '{name}'"))
}

/// Validate an IR value and serialize it as a `.emb` image.
pub fn encode_ir(ir: &Value) -> Result<Vec<u8>, String> {
    let fields = object_fields(ir, "for emb.encode")?;
    let declared = field(&fields, "version", "the top-level IR")?;
    let declared = operand(declared, "for emb.encode version")?;
    if declared != u32::from(version()) {
        return Err(format!(
            "emb.encode: version mismatch: IR declares {declared}, this VM writes {}",
            version()
        ));
    }
    let chunk = read_chunk(ir, 0)?;
    Ok(bytecode::serialize(&chunk))
}

fn lookup_op(name: &str) -> Option<(u8, Operands)> {
    OPS.iter()
        .find(|(candidate, _, _)| *candidate == name)
        .map(|(_, tag, shape)| (*tag, *shape))
}

fn read_chunk(value: &Value, depth: usize) -> Result<Chunk, String> {
    if depth > MAX_CHUNK_DEPTH {
        return Err("emb.encode: functions are nested too deeply".to_string());
    }
    let fields = object_fields(value, "for an emb chunk")?;

    let constants = read_constants(field(&fields, "constants", "a chunk")?)?;
    let functions = read_functions(field(&fields, "functions", "a chunk")?, depth)?;
    let (code, spans) = read_code(field(&fields, "code", "a chunk")?)?;

    Ok(Chunk {
        code,
        spans,
        constants,
        functions,
    })
}

fn read_constants(value: &Value) -> Result<Vec<Const>, String> {
    let items = array_items(value, "for emb.encode constants")?;
    check_count(items.len(), "constants")?;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| read_constant(item, index))
        .collect()
}

fn read_constant(value: &Value, index: usize) -> Result<Const, String> {
    let context = format!("constant {index}");
    let fields = object_fields(value, &format!("for emb.encode {context}"))?;
    let tag = field(&fields, "tag", &context)?;
    let tag = expect_string(tag, &format!("for emb.encode {context} tag"))?;
    match tag.as_str() {
        "number" => {
            let value = field(&fields, "value", &context)?;
            let number = expect_number(value, &format!("for emb.encode {context} value"))?;
            Ok(Const::Number(number))
        }
        "string" => {
            let value = field(&fields, "value", &context)?;
            let text = checked_string(value, &format!("for emb.encode {context} value"))?;
            Ok(Const::String(text))
        }
        "bool" => {
            let value = field(&fields, "value", &context)?;
            Ok(Const::Bool(expect_bool(
                value,
                &format!("for emb.encode {context} value"),
            )?))
        }
        "null" => Ok(Const::Null),
        other => Err(format!(
            "emb.encode: unknown constant tag '{other}' in {context}"
        )),
    }
}

fn read_functions(value: &Value, depth: usize) -> Result<Vec<Rc<FunctionProto>>, String> {
    let items = array_items(value, "for emb.encode functions")?;
    check_count(items.len(), "functions")?;
    let mut functions = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let context = format!("function {index}");
        let fields = object_fields(item, &format!("for emb.encode {context}"))?;

        let name = field(&fields, "name", &context)?;
        let name = checked_string(name, &format!("for emb.encode {context} name"))?;

        let params = field(&fields, "params", &context)?;
        let params = array_items(params, &format!("for emb.encode {context} params"))?;
        check_count(params.len(), "function parameters")?;
        let params = params
            .iter()
            .enumerate()
            .map(|(i, param)| {
                checked_string(param, &format!("for emb.encode {context} parameter {i}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let chunk = field(&fields, "chunk", &context)?;
        let chunk = read_chunk(chunk, depth + 1)?;
        functions.push(Rc::new(FunctionProto {
            name,
            params,
            chunk,
        }));
    }
    Ok(functions)
}

fn read_code(value: &Value) -> Result<(Vec<Op>, Vec<Span>), String> {
    let items = array_items(value, "for emb.encode code")?;
    check_count(items.len(), "instructions")?;
    let mut code = Vec::with_capacity(items.len());
    let mut spans = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let context = format!("instruction {index}");
        let fields = object_fields(item, &format!("for emb.encode {context}"))?;

        let name = field(&fields, "op", &context)?;
        let name = expect_string(name, &format!("for emb.encode {context} op"))?;
        let (tag, shape) = lookup_op(&name)
            .ok_or_else(|| format!("emb.encode: unknown op name '{name}' in {context}"))?;

        let a = optional_operand(
            &fields,
            "a",
            0,
            &format!("for emb.encode {context} operand 'a'"),
        )?;
        let b = optional_operand(
            &fields,
            "b",
            0,
            &format!("for emb.encode {context} operand 'b'"),
        )?;
        let line = optional_operand(
            &fields,
            "line",
            1,
            &format!("for emb.encode {context} line"),
        )?;
        let col = optional_operand(&fields, "col", 1, &format!("for emb.encode {context} col"))?;

        // Unused operands are still validated above, but `bytecode::encode_op`
        // discards them, so mirror that shape exactly in the emitted bytes.
        let (a, b) = match shape {
            Operands::None => (0, 0),
            Operands::A => (a, 0),
            Operands::AB => (a, b),
        };
        code.push(make_op(tag, a, b)?);
        spans.push(Span::new(line as usize, col as usize));
    }
    Ok((code, spans))
}

fn make_op(tag: u8, a: u32, b: u32) -> Result<Op, String> {
    Ok(match tag {
        0 => Op::LoadConst(a),
        1 => Op::Pop,
        2 => Op::GetVar(a),
        3 => Op::SetVar(a),
        4 => Op::DefineVar(a),
        5 => Op::GetProp(a),
        6 => Op::SetProp(a),
        7 => Op::GetVarOrObject(a),
        8 => Op::GetPropOrObject(a),
        9 => Op::GetIndex,
        10 => Op::SetIndex,
        11 => Op::MakeArray(a),
        12 => Op::MakeObject(a),
        13 => Op::MakeClosure(a),
        14 => Op::Call(a, b),
        15 => Op::MethodCall(a, b),
        16 => Op::Say(a),
        17 => Op::Add,
        18 => Op::Sub,
        19 => Op::Mul,
        20 => Op::Div,
        21 => Op::Mod,
        42 => Op::FloorDiv,
        43 => Op::Pow,
        44 => Op::BitAnd,
        45 => Op::BitOr,
        46 => Op::BitXor,
        47 => Op::Shl,
        48 => Op::Shr,
        22 => Op::Eq,
        23 => Op::NotEq,
        24 => Op::Gt,
        25 => Op::Lt,
        26 => Op::GtEq,
        27 => Op::LtEq,
        28 => Op::Neg,
        29 => Op::Not,
        49 => Op::BitNot,
        30 => Op::ToBool,
        31 => Op::Jump(a),
        32 => Op::JumpIfFalse(a),
        33 => Op::JumpIfFalsePeek(a),
        34 => Op::JumpIfTruePeek(a),
        35 => Op::RepeatSetup,
        36 => Op::RepeatNext(a),
        37 => Op::IterSetup,
        38 => Op::IterNext(a, b),
        39 => Op::Return,
        40 => Op::ReturnNull,
        41 => Op::Import(a),
        50 => Op::Dup,
        51 => Op::Rotate(a),
        52 => Op::GetPropKeep(a),
        53 => Op::GetIndexKeep,
        54 => Op::JumpIfNullPeek(a),
        other => return Err(format!("emb.encode: unsupported op tag {other}")),
    })
}

fn object_fields(value: &Value, context: &str) -> Result<BTreeMap<String, Value>, String> {
    match value {
        Value::Object(fields) => Ok(fields.borrow().clone()),
        other => Err(format!(
            "Type mismatch: expected an object {context}, found {}",
            other.type_name()
        )),
    }
}

fn field<'a>(
    fields: &'a BTreeMap<String, Value>,
    name: &str,
    context: &str,
) -> Result<&'a Value, String> {
    fields
        .get(name)
        .ok_or_else(|| format!("emb.encode: {context} is missing '{name}'"))
}

fn expect_bool(value: &Value, context: &str) -> Result<bool, String> {
    match value {
        Value::Bool(b) => Ok(*b),
        other => Err(format!(
            "Type mismatch: expected a boolean {context}, found {}",
            other.type_name()
        )),
    }
}

fn checked_string(value: &Value, context: &str) -> Result<String, String> {
    let text = expect_string(value, context)?;
    if text.len() > u32::MAX as usize {
        return Err(format!("emb.encode: string {context} is too long"));
    }
    Ok(text)
}

fn check_count(count: usize, what: &str) -> Result<(), String> {
    if count > u32::MAX as usize {
        return Err(format!("emb.encode: too many {what} ({count})"));
    }
    Ok(())
}

fn operand(value: &Value, context: &str) -> Result<u32, String> {
    let number = expect_number(value, context)?;
    if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number > u32::MAX as f64 {
        return Err(format!(
            "emb.encode: expected a non-negative whole number {context}, found {number}"
        ));
    }
    Ok(number as u32)
}

fn optional_operand(
    fields: &BTreeMap<String, Value>,
    name: &str,
    default: u32,
    context: &str,
) -> Result<u32, String> {
    match fields.get(name) {
        Some(value) => operand(value, context),
        None => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        call_native(&module(), name, args)
    }

    fn object(fields: Vec<(&str, Value)>) -> Value {
        Value::new_object(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        )
    }

    fn array(items: Vec<Value>) -> Value {
        Value::new_array(items)
    }

    fn number(n: f64) -> Value {
        Value::Number(n)
    }

    fn string(text: &str) -> Value {
        Value::String(text.to_string())
    }

    fn constant(tag: &str, value: Value) -> Value {
        object(vec![("tag", string(tag)), ("value", value)])
    }

    fn null_constant() -> Value {
        object(vec![("tag", string("null"))])
    }

    fn chunk(constants: Vec<Value>, functions: Vec<Value>, code: Vec<Value>) -> Value {
        object(vec![
            ("constants", array(constants)),
            ("functions", array(functions)),
            ("code", array(code)),
        ])
    }

    fn program_at(
        version: f64,
        constants: Vec<Value>,
        functions: Vec<Value>,
        code: Vec<Value>,
    ) -> Value {
        let Value::Object(fields) = chunk(constants, functions, code) else {
            unreachable!("chunk builds an object")
        };
        let mut fields = fields.borrow().clone();
        fields.insert("version".to_string(), number(version));
        Value::new_object(fields)
    }

    fn program(constants: Vec<Value>, functions: Vec<Value>, code: Vec<Value>) -> Value {
        program_at(version() as f64, constants, functions, code)
    }

    fn program_with_fields(constants: Value, functions: Value, code: Value) -> Value {
        object(vec![
            ("version", number(version() as f64)),
            ("constants", constants),
            ("functions", functions),
            ("code", code),
        ])
    }

    fn instruction(
        op: &str,
        a: Option<f64>,
        b: Option<f64>,
        line: Option<f64>,
        col: Option<f64>,
    ) -> Value {
        let mut fields = vec![("op", string(op))];
        if let Some(a) = a {
            fields.push(("a", number(a)));
        }
        if let Some(b) = b {
            fields.push(("b", number(b)));
        }
        if let Some(line) = line {
            fields.push(("line", number(line)));
        }
        if let Some(col) = col {
            fields.push(("col", number(col)));
        }
        object(fields)
    }

    fn as_bytes(value: Value) -> Vec<u8> {
        let Value::Array(items) = value else {
            panic!("emb.encode should return an array of bytes");
        };
        let borrowed = items.borrow();
        let bytes: Vec<u8> = borrowed
            .iter()
            .map(|item| match item {
                Value::Number(n) => *n as u8,
                other => panic!(
                    "byte array contains {} instead of a number",
                    other.type_name()
                ),
            })
            .collect();
        drop(borrowed);
        bytes
    }

    #[test]
    fn magic_and_version_match_the_vm() {
        assert_eq!(magic(), "EMBC");
        assert_eq!(magic().as_bytes(), bytecode::EMB_MAGIC);
        assert_eq!(version(), 2);
        assert!(matches!(call("magic", &[]).unwrap(), Value::String(s) if s == "EMBC"));
        assert!(matches!(call("version", &[]).unwrap(), Value::Number(n) if n == 2.0));
        assert!(
            matches!(call("encodeOp", &[string("FloorDiv")]).unwrap(), Value::Number(n) if n == 42.0)
        );
        assert!(
            matches!(call("encodeOp", &[string("JumpIfNullPeek")]).unwrap(), Value::Number(n) if n == 54.0)
        );
    }

    #[test]
    fn round_trips_a_multi_function_ir_through_bytecode_deserialize() {
        let inner = chunk(
            vec![
                constant("string", string("name")),
                constant("bool", Value::Bool(true)),
            ],
            vec![],
            vec![
                instruction("GetVar", Some(0.0), None, Some(2.0), Some(3.0)),
                instruction("Call", Some(0.0), Some(0.0), Some(2.0), Some(4.0)),
                instruction("Return", None, None, Some(3.0), Some(1.0)),
            ],
        );
        let function = object(vec![
            ("name", string("f")),
            ("params", array(vec![string("a")])),
            ("chunk", inner),
        ]);
        let source = program(
            vec![
                constant("number", number(1.5)),
                constant("string", string("hi")),
                constant("bool", Value::Bool(false)),
                null_constant(),
                constant("string", string("name")),
            ],
            vec![function],
            vec![
                instruction("LoadConst", Some(0.0), None, Some(1.0), Some(1.0)),
                instruction("MakeClosure", Some(0.0), None, None, None),
                instruction("IterNext", Some(4.0), Some(1.0), Some(7.0), Some(8.0)),
                // `bytecode::encode_op` ignores operands for these two.
                instruction("Add", None, None, None, None),
                instruction("JumpIfNullPeek", Some(1.0), None, Some(9.0), Some(2.0)),
            ],
        );

        let bytes = as_bytes(call("encode", &[source]).unwrap());

        let mut inner_chunk = Chunk::new();
        inner_chunk.constants = vec![Const::String("name".to_string()), Const::Bool(true)];
        inner_chunk.code = vec![Op::GetVar(0), Op::Call(0, 0), Op::Return];
        inner_chunk.spans = vec![Span::new(2, 3), Span::new(2, 4), Span::new(3, 1)];

        let mut expected = Chunk::new();
        expected.constants = vec![
            Const::Number(1.5),
            Const::String("hi".to_string()),
            Const::Bool(false),
            Const::Null,
            Const::String("name".to_string()),
        ];
        expected.functions = vec![Rc::new(FunctionProto {
            name: "f".to_string(),
            params: vec!["a".to_string()],
            chunk: inner_chunk,
        })];
        expected.code = vec![
            Op::LoadConst(0),
            Op::MakeClosure(0),
            Op::IterNext(4, 1),
            Op::Add,
            Op::JumpIfNullPeek(1),
        ];
        expected.spans = vec![
            Span::new(1, 1),
            Span::new(1, 1),
            Span::new(7, 8),
            Span::new(1, 1),
            Span::new(9, 2),
        ];

        assert_eq!(bytes, bytecode::serialize(&expected));
        let restored = bytecode::deserialize(&bytes).expect("emb bytes should load");
        assert_eq!(restored, expected);
        assert_eq!(&bytes[..6], b"EMBC\x02\x00");
    }

    #[test]
    fn every_opcode_name_matches_its_bytecode_tag() {
        let mut names: Vec<&str> = OPS.iter().map(|(name, _, _)| *name).collect();
        names.sort_unstable();
        assert!(names.windows(2).all(|pair| pair[0] != pair[1]));
        let mut tags: Vec<u8> = OPS.iter().map(|(_, tag, _)| *tag).collect();
        tags.sort_unstable();
        assert_eq!(tags, (0..=54).collect::<Vec<u8>>());

        for &(name, tag, shape) in OPS {
            assert_eq!(encode_op(name).unwrap(), tag, "op {name}");
            assert!(
                matches!(call("encodeOp", &[string(name)]).unwrap(), Value::Number(n) if n == f64::from(tag)),
                "encodeOp did not return the tag for {name}"
            );

            let source = program(
                vec![],
                vec![],
                vec![instruction(
                    name,
                    Some(2.0),
                    Some(3.0),
                    Some(4.0),
                    Some(5.0),
                )],
            );
            let (a, b) = match shape {
                Operands::None => (0, 0),
                Operands::A => (2, 0),
                Operands::AB => (2, 3),
            };
            let mut expected = Chunk::new();
            expected.code.push(make_op(tag, a, b).unwrap());
            expected.spans.push(Span::new(4, 5));
            assert_eq!(
                as_bytes(call("encode", &[source]).unwrap()),
                bytecode::serialize(&expected),
                "bytes for {name}"
            );
        }
    }

    #[test]
    fn operands_and_spans_default_to_zero_and_one() {
        let source = program(
            vec![],
            vec![],
            vec![
                instruction("Add", None, None, None, None),
                instruction("Call", None, None, None, None),
            ],
        );
        let mut expected = Chunk::new();
        expected.code = vec![Op::Add, Op::Call(0, 0)];
        expected.spans = vec![Span::new(1, 1), Span::new(1, 1)];
        assert_eq!(
            as_bytes(call("encode", &[source]).unwrap()),
            bytecode::serialize(&expected)
        );
    }

    #[test]
    fn write_creates_a_loadable_emb_file() {
        let path = std::env::temp_dir().join(format!(
            "em_emb_module_test_{}_{}.emb",
            std::process::id(),
            "roundtrip"
        ));
        let _ = std::fs::remove_file(&path);
        let source = program(
            vec![constant("number", number(42.0))],
            vec![],
            vec![instruction("LoadConst", Some(0.0), None, None, None)],
        );
        let name = path.display().to_string();
        assert!(matches!(
            call("write", &[string(&name), source.clone()]).unwrap(),
            Value::Bool(true)
        ));
        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(on_disk, as_bytes(call("encode", &[source]).unwrap()));
        assert!(bytecode::deserialize(&on_disk).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn malformed_ir_returns_errors_instead_of_panicking() {
        let good_code = vec![instruction("Add", None, None, None, None)];

        assert!(call("encode", &[]).is_err());
        for value in [Value::Null, number(1.0), string("x"), array(vec![])] {
            assert!(call("encode", &[value]).is_err());
        }

        let err = call("encode", &[chunk(vec![], vec![], good_code.clone())]).unwrap_err();
        assert!(err.contains("missing 'version'"), "{err}");
        for version in [1.0, 3.0] {
            let err = call(
                "encode",
                &[program_at(version, vec![], vec![], good_code.clone())],
            )
            .unwrap_err();
            assert!(err.contains("version mismatch"), "{err}");
        }
        for version in [2.5, f64::NAN] {
            let err = call(
                "encode",
                &[program_at(version, vec![], vec![], good_code.clone())],
            )
            .unwrap_err();
            assert!(err.contains("non-negative whole number"), "{err}");
        }
        for version in [string("2"), Value::Null, array(vec![])] {
            let source = program(vec![], vec![], good_code.clone());
            let Value::Object(fields) = source else {
                unreachable!()
            };
            fields.borrow_mut().insert("version".to_string(), version);
            assert!(call("encode", &[Value::Object(fields)]).is_err());
        }

        let bad_constants = [
            ("not an array", number(1.0)),
            ("not an object", array(vec![string("x")])),
            (
                "missing tag",
                array(vec![object(vec![("value", number(1.0))])]),
            ),
            (
                "unknown tag",
                array(vec![object(vec![("tag", string("float"))])]),
            ),
            (
                "wrong number value",
                array(vec![constant("number", string("1"))]),
            ),
            (
                "wrong string value",
                array(vec![constant("string", number(1.0))]),
            ),
            (
                "wrong bool value",
                array(vec![constant("bool", string("true"))]),
            ),
        ];
        for (label, constants) in bad_constants {
            let err = call(
                "encode",
                &[program_with_fields(
                    constants,
                    array(vec![]),
                    array(good_code.clone()),
                )],
            )
            .unwrap_err();
            assert!(!err.is_empty(), "{label}");
        }
        assert!(call(
            "encode",
            &[program(
                vec![object(vec![("tag", string("float"))])],
                vec![],
                good_code.clone()
            )]
        )
        .unwrap_err()
        .contains("unknown constant tag 'float'"));

        let bad_functions = [
            ("not an array", number(1.0)),
            ("not an object", array(vec![string("f")])),
            (
                "missing name",
                array(vec![object(vec![
                    ("params", array(vec![])),
                    ("chunk", chunk(vec![], vec![], vec![])),
                ])]),
            ),
            (
                "bad params",
                array(vec![object(vec![
                    ("name", string("f")),
                    ("params", array(vec![number(1.0)])),
                    ("chunk", chunk(vec![], vec![], vec![])),
                ])]),
            ),
            (
                "missing chunk",
                array(vec![object(vec![
                    ("name", string("f")),
                    ("params", array(vec![])),
                ])]),
            ),
        ];
        for (label, functions) in bad_functions {
            let err = call(
                "encode",
                &[program_with_fields(
                    array(vec![]),
                    functions,
                    array(good_code.clone()),
                )],
            )
            .unwrap_err();
            assert!(!err.is_empty(), "{label}");
        }

        let bad_instructions = [
            ("not an array", number(1.0)),
            ("not an object", array(vec![string("Add")])),
            ("missing op", array(vec![object(vec![])])),
            (
                "op is not a string",
                array(vec![object(vec![("op", number(1.0))])]),
            ),
            (
                "unknown op",
                array(vec![instruction("Nope", None, None, None, None)]),
            ),
            (
                "fractional a",
                array(vec![instruction("LoadConst", Some(1.5), None, None, None)]),
            ),
            (
                "negative a",
                array(vec![instruction("LoadConst", Some(-1.0), None, None, None)]),
            ),
            (
                "a above u32",
                array(vec![instruction(
                    "LoadConst",
                    Some(4_294_967_296.0),
                    None,
                    None,
                    None,
                )]),
            ),
            (
                "non-finite b",
                array(vec![instruction(
                    "Call",
                    Some(0.0),
                    Some(f64::INFINITY),
                    None,
                    None,
                )]),
            ),
            (
                "fractional line",
                array(vec![instruction("Add", None, None, Some(1.25), None)]),
            ),
            (
                "negative col",
                array(vec![instruction("Add", None, None, None, Some(-2.0))]),
            ),
        ];
        for (label, code) in bad_instructions {
            let err = call(
                "encode",
                &[program_with_fields(array(vec![]), array(vec![]), code)],
            )
            .unwrap_err();
            assert!(!err.is_empty(), "{label}");
        }
        // A string operand is rejected too, so check that explicitly.
        let bad_a = object(vec![("op", string("LoadConst")), ("a", string("0"))]);
        assert!(call("encode", &[program(vec![], vec![], vec![bad_a])]).is_err());

        assert!(call(
            "encode",
            &[program(
                vec![],
                vec![],
                vec![instruction("Nope", None, None, None, None)]
            )]
        )
        .unwrap_err()
        .contains("unknown op name 'Nope'"));

        assert!(call("encodeOp", &[]).is_err());
        assert!(call("encodeOp", &[number(1.0)]).is_err());
        assert!(call("encodeOp", &[string("Nope")])
            .unwrap_err()
            .contains("unknown op name"));

        assert!(call("write", &[]).is_err());
        assert!(call(
            "write",
            &[number(1.0), program(vec![], vec![], good_code.clone())]
        )
        .is_err());
        assert!(call(
            "write",
            &[string("/no/such/directory/out.emb"), Value::Null]
        )
        .is_err());
    }

    #[test]
    fn deep_and_cyclic_chunks_are_errors() {
        let mut nested = chunk(vec![], vec![], vec![]);
        for _ in 0..40 {
            nested = chunk(
                vec![],
                vec![object(vec![
                    ("name", string("f")),
                    ("params", array(vec![])),
                    ("chunk", nested),
                ])],
                vec![],
            );
        }
        let Value::Object(fields) = nested else {
            unreachable!()
        };
        let mut fields = fields.borrow().clone();
        fields.insert("version".to_string(), number(version() as f64));
        let err = call("encode", &[Value::new_object(fields)]).unwrap_err();
        assert!(err.contains("nested too deeply"), "{err}");

        let cyclic = Value::new_object(BTreeMap::new());
        let cyclic_chunk = Value::new_object(BTreeMap::new());
        {
            let Value::Object(fields) = &cyclic_chunk else {
                unreachable!()
            };
            let mut fields = fields.borrow_mut();
            fields.insert("constants".to_string(), array(vec![]));
            fields.insert("functions".to_string(), array(vec![cyclic.clone()]));
            fields.insert("code".to_string(), array(vec![]));
        }
        {
            let Value::Object(fields) = &cyclic else {
                unreachable!()
            };
            let mut fields = fields.borrow_mut();
            fields.insert("name".to_string(), string("loop"));
            fields.insert("params".to_string(), array(vec![]));
            fields.insert("chunk".to_string(), cyclic_chunk);
        }
        let err = call("encode", &[program(vec![], vec![cyclic], vec![])]).unwrap_err();
        assert!(err.contains("nested too deeply"), "{err}");
    }
}

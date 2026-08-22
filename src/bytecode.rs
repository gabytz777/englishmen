//! Englishmen bytecode: the instruction set shared by the compiler and the VM,
//! plus the on-disk `.emb` container format.
//!
//! A [`Chunk`] is one compiled unit of code — a whole program or a single
//! function body. Chunks own their constant pool and any nested function
//! prototypes, so a program is a tree of chunks rooted at the top level.

use crate::error::{EmError, Span};
use std::rc::Rc;

/// Magic bytes at the start of every `.emb` file.
pub const EMB_MAGIC: &[u8; 4] = b"EMBC";
/// Bytecode format version. Bumped whenever the encoding changes
/// incompatibly; the VM refuses files it does not recognise.
pub const EMB_VERSION: u16 = 1;

/// A compile-time constant. Only scalars are constants — arrays, objects and
/// closures are built at runtime by dedicated instructions.
#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
}

/// One bytecode instruction.
///
/// Operands are indices into the enclosing chunk's constant pool
/// (name/constant operands) or absolute instruction addresses (jump targets).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    /// Push a constant from the pool.
    LoadConst(u32),
    /// Discard the top of the stack.
    Pop,

    /// Read a variable by name, walking the scope chain.
    GetVar(u32),
    /// Assign to a variable (Englishmen assignment semantics: walk up the
    /// chain, otherwise define here).
    SetVar(u32),
    /// Define a variable in the *current* scope (parameters, loop variables).
    DefineVar(u32),

    /// Push `base.name`.
    GetProp(u32),
    /// Stack: `value`, `base` -> set `base.name = value`.
    SetProp(u32),
    /// Push a variable, auto-creating it as an empty object if unset.
    GetVarOrObject(u32),
    /// Pop a base object and push `base.name`, auto-creating it as an empty
    /// object if the key is missing.
    GetPropOrObject(u32),

    /// Stack: `base`, `index` -> push `base[index]`.
    GetIndex,
    /// Stack: `value`, `base`, `index` -> set `base[index] = value`.
    SetIndex,

    /// Pop N values and push an array.
    MakeArray(u32),
    /// Pop N key/value pairs (key pushed first) and push an object.
    MakeObject(u32),
    /// Instantiate function prototype N as a closure over the current scope.
    MakeClosure(u32),

    /// Pop N arguments and a callee, then call it. The second operand names
    /// the callee for error messages.
    Call(u32, u32),
    /// Pop N arguments and a receiver, then call a method on it.
    MethodCall(u32, u32),
    /// The builtin `say(...)`.
    Say(u32),

    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Gt,
    Lt,
    GtEq,
    LtEq,
    Neg,
    Not,
    /// Replace the top of the stack with its truthiness as a boolean.
    ToBool,

    /// Unconditional jump to an absolute address.
    Jump(u32),
    /// Pop a value; jump if it is falsey.
    JumpIfFalse(u32),
    /// Jump if the top of the stack is falsey, leaving it in place.
    JumpIfFalsePeek(u32),
    /// Jump if the top of the stack is truthy, leaving it in place.
    JumpIfTruePeek(u32),

    /// Pop a repeat count, validate it and push a loop counter.
    RepeatSetup,
    /// Step the counter on top of the stack; when exhausted, pop it and jump.
    RepeatNext(u32),
    /// Pop an iterable, validate it and push a snapshot plus a cursor.
    IterSetup,
    /// Bind the next item to a variable, or pop the iterator state and jump.
    IterNext(u32, u32),

    /// Return the top of the stack from the current function.
    Return,
    /// Return null from the current function.
    ReturnNull,

    /// Import a standard module and bind it to its name.
    Import(u32),
}

/// A compiled function prototype: everything static about a function,
/// with the closure environment supplied at runtime by `MakeClosure`.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionProto {
    pub name: String,
    pub params: Vec<String>,
    pub chunk: Chunk,
}

/// A unit of compiled code with its own constants and nested functions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chunk {
    pub code: Vec<Op>,
    /// Source position for each instruction, used to report runtime errors.
    pub spans: Vec<Span>,
    pub constants: Vec<Const>,
    pub functions: Vec<Rc<FunctionProto>>,
}

impl Chunk {
    pub fn new() -> Self {
        Chunk::default()
    }

    /// Append an instruction, returning its address.
    pub fn emit(&mut self, op: Op, span: Span) -> usize {
        self.code.push(op);
        self.spans.push(span);
        self.code.len() - 1
    }

    /// The address the next emitted instruction will occupy.
    pub fn here(&self) -> usize {
        self.code.len()
    }

    /// Point a previously emitted jump at a target address.
    pub fn patch_jump(&mut self, at: usize, target: usize) {
        let target = target as u32;
        self.code[at] = match self.code[at] {
            Op::Jump(_) => Op::Jump(target),
            Op::JumpIfFalse(_) => Op::JumpIfFalse(target),
            Op::JumpIfFalsePeek(_) => Op::JumpIfFalsePeek(target),
            Op::JumpIfTruePeek(_) => Op::JumpIfTruePeek(target),
            Op::RepeatNext(_) => Op::RepeatNext(target),
            Op::IterNext(name, _) => Op::IterNext(name, target),
            other => panic!("patch_jump on non-jump instruction {other:?}"),
        };
    }

    /// Intern a constant, reusing an existing slot when possible.
    pub fn add_const(&mut self, value: Const) -> u32 {
        if let Some(idx) = self.constants.iter().position(|c| *c == value) {
            return idx as u32;
        }
        self.constants.push(value);
        (self.constants.len() - 1) as u32
    }

    /// Intern a name (variables, properties, methods, modules).
    pub fn add_name(&mut self, name: &str) -> u32 {
        self.add_const(Const::String(name.to_string()))
    }

    pub fn constant(&self, idx: u32) -> &Const {
        &self.constants[idx as usize]
    }

    /// Read a name constant. Names are always strings by construction.
    pub fn name(&self, idx: u32) -> &str {
        match self.constant(idx) {
            Const::String(s) => s.as_str(),
            other => panic!("expected a name constant, found {other:?}"),
        }
    }

    pub fn span(&self, ip: usize) -> Span {
        self.spans.get(ip).copied().unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// `.emb` serialisation
// ---------------------------------------------------------------------------

/// Serialise a program chunk into the `.emb` container format.
pub fn serialize(chunk: &Chunk) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(EMB_MAGIC);
    out.extend_from_slice(&EMB_VERSION.to_le_bytes());
    write_chunk(&mut out, chunk);
    out
}

/// Parse a `.emb` image back into a chunk.
pub fn deserialize(bytes: &[u8]) -> Result<Chunk, EmError> {
    let mut r = Reader { bytes, pos: 0 };
    let magic = r.take(4)?;
    if magic != EMB_MAGIC {
        return Err(EmError::new("Not an Englishmen bytecode file")
            .with_hint("Expected a '.emb' file produced by 'em build'."));
    }
    let version = r.u16()?;
    if version != EMB_VERSION {
        return Err(EmError::new(format!(
            "Unsupported bytecode version {version} (this build of Englishmen speaks version {EMB_VERSION})"
        ))
        .with_hint("Recompile the program with 'em build'."));
    }
    read_chunk(&mut r)
}

fn write_chunk(out: &mut Vec<u8>, chunk: &Chunk) {
    write_u32(out, chunk.constants.len() as u32);
    for c in &chunk.constants {
        match c {
            Const::Number(n) => {
                out.push(0);
                out.extend_from_slice(&n.to_le_bytes());
            }
            Const::String(s) => {
                out.push(1);
                write_str(out, s);
            }
            Const::Bool(b) => {
                out.push(2);
                out.push(u8::from(*b));
            }
            Const::Null => out.push(3),
        }
    }

    write_u32(out, chunk.functions.len() as u32);
    for f in &chunk.functions {
        write_str(out, &f.name);
        write_u32(out, f.params.len() as u32);
        for p in &f.params {
            write_str(out, p);
        }
        write_chunk(out, &f.chunk);
    }

    write_u32(out, chunk.code.len() as u32);
    for (i, op) in chunk.code.iter().enumerate() {
        let (tag, a, b) = encode_op(*op);
        out.push(tag);
        write_u32(out, a);
        write_u32(out, b);
        let span = chunk.span(i);
        write_u32(out, span.line as u32);
        write_u32(out, span.col as u32);
    }
}

fn read_chunk(r: &mut Reader) -> Result<Chunk, EmError> {
    let mut chunk = Chunk::new();

    let const_count = r.u32()?;
    for _ in 0..const_count {
        let tag = r.u8()?;
        let value = match tag {
            0 => Const::Number(f64::from_le_bytes(r.take(8)?.try_into().expect("8 bytes"))),
            1 => Const::String(r.string()?),
            2 => Const::Bool(r.u8()? != 0),
            3 => Const::Null,
            other => {
                return Err(EmError::new(format!(
                    "Corrupt bytecode: unknown constant tag {other}"
                )))
            }
        };
        chunk.constants.push(value);
    }

    let func_count = r.u32()?;
    for _ in 0..func_count {
        let name = r.string()?;
        let param_count = r.u32()?;
        let mut params = Vec::with_capacity(param_count as usize);
        for _ in 0..param_count {
            params.push(r.string()?);
        }
        let body = read_chunk(r)?;
        chunk.functions.push(Rc::new(FunctionProto {
            name,
            params,
            chunk: body,
        }));
    }

    let code_count = r.u32()?;
    for _ in 0..code_count {
        let tag = r.u8()?;
        let a = r.u32()?;
        let b = r.u32()?;
        let line = r.u32()? as usize;
        let col = r.u32()? as usize;
        chunk.code.push(decode_op(tag, a, b)?);
        chunk.spans.push(Span::new(line, col));
    }

    validate(&chunk)?;
    Ok(chunk)
}

/// Reject images whose operands point outside the chunk, so a damaged or
/// hand-edited file fails with a clear error instead of panicking mid-run.
fn validate(chunk: &Chunk) -> Result<(), EmError> {
    let consts = chunk.constants.len() as u32;
    let funcs = chunk.functions.len() as u32;
    let code = chunk.code.len() as u32;
    let bad = |what: &str| Err(EmError::new(format!("Corrupt bytecode: {what}")));

    for op in &chunk.code {
        // The name an instruction refers to, if any: it must exist and be a string.
        let name = match *op {
            Op::GetVar(a)
            | Op::SetVar(a)
            | Op::DefineVar(a)
            | Op::GetProp(a)
            | Op::SetProp(a)
            | Op::GetVarOrObject(a)
            | Op::GetPropOrObject(a)
            | Op::Import(a)
            | Op::MethodCall(a, _)
            | Op::IterNext(a, _)
            | Op::Call(_, a) => Some(a),
            _ => None,
        };
        if let Some(a) = name {
            if a >= consts || !matches!(chunk.constant(a), Const::String(_)) {
                return bad("name index out of range");
            }
        }

        // Jump targets are absolute addresses; one past the end means "done".
        let target = match *op {
            Op::Jump(a)
            | Op::JumpIfFalse(a)
            | Op::JumpIfFalsePeek(a)
            | Op::JumpIfTruePeek(a)
            | Op::RepeatNext(a)
            | Op::IterNext(_, a) => Some(a),
            _ => None,
        };
        if let Some(a) = target {
            if a > code {
                return bad("jump target out of range");
            }
        }

        match *op {
            Op::LoadConst(a) if a >= consts => return bad("constant index out of range"),
            Op::MakeClosure(a) if a >= funcs => return bad("function index out of range"),
            _ => {}
        }
    }
    Ok(())
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_str(out: &mut Vec<u8>, value: &str) {
    write_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], EmError> {
        if self.pos + n > self.bytes.len() {
            return Err(EmError::new("Corrupt bytecode: unexpected end of file"));
        }
        let slice = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, EmError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, EmError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("2 bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32, EmError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn string(&mut self) -> Result<String, EmError> {
        let len = self.u32()? as usize;
        let raw = self.take(len)?;
        String::from_utf8(raw.to_vec())
            .map_err(|_| EmError::new("Corrupt bytecode: invalid UTF-8 string"))
    }
}

fn encode_op(op: Op) -> (u8, u32, u32) {
    match op {
        Op::LoadConst(a) => (0, a, 0),
        Op::Pop => (1, 0, 0),
        Op::GetVar(a) => (2, a, 0),
        Op::SetVar(a) => (3, a, 0),
        Op::DefineVar(a) => (4, a, 0),
        Op::GetProp(a) => (5, a, 0),
        Op::SetProp(a) => (6, a, 0),
        Op::GetVarOrObject(a) => (7, a, 0),
        Op::GetPropOrObject(a) => (8, a, 0),
        Op::GetIndex => (9, 0, 0),
        Op::SetIndex => (10, 0, 0),
        Op::MakeArray(a) => (11, a, 0),
        Op::MakeObject(a) => (12, a, 0),
        Op::MakeClosure(a) => (13, a, 0),
        Op::Call(a, b) => (14, a, b),
        Op::MethodCall(a, b) => (15, a, b),
        Op::Say(a) => (16, a, 0),
        Op::Add => (17, 0, 0),
        Op::Sub => (18, 0, 0),
        Op::Mul => (19, 0, 0),
        Op::Div => (20, 0, 0),
        Op::Mod => (21, 0, 0),
        Op::Eq => (22, 0, 0),
        Op::NotEq => (23, 0, 0),
        Op::Gt => (24, 0, 0),
        Op::Lt => (25, 0, 0),
        Op::GtEq => (26, 0, 0),
        Op::LtEq => (27, 0, 0),
        Op::Neg => (28, 0, 0),
        Op::Not => (29, 0, 0),
        Op::ToBool => (30, 0, 0),
        Op::Jump(a) => (31, a, 0),
        Op::JumpIfFalse(a) => (32, a, 0),
        Op::JumpIfFalsePeek(a) => (33, a, 0),
        Op::JumpIfTruePeek(a) => (34, a, 0),
        Op::RepeatSetup => (35, 0, 0),
        Op::RepeatNext(a) => (36, a, 0),
        Op::IterSetup => (37, 0, 0),
        Op::IterNext(a, b) => (38, a, b),
        Op::Return => (39, 0, 0),
        Op::ReturnNull => (40, 0, 0),
        Op::Import(a) => (41, a, 0),
    }
}

fn decode_op(tag: u8, a: u32, b: u32) -> Result<Op, EmError> {
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
        22 => Op::Eq,
        23 => Op::NotEq,
        24 => Op::Gt,
        25 => Op::Lt,
        26 => Op::GtEq,
        27 => Op::LtEq,
        28 => Op::Neg,
        29 => Op::Not,
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
        other => {
            return Err(EmError::new(format!(
                "Corrupt bytecode: unknown instruction {other}"
            )))
        }
    })
}

// ---------------------------------------------------------------------------
// Disassembly
// ---------------------------------------------------------------------------

/// Render a chunk (and every function nested inside it) as readable assembly.
pub fn disassemble(chunk: &Chunk, name: &str) -> String {
    let mut out = String::new();
    disassemble_into(&mut out, chunk, name);
    out
}

fn disassemble_into(out: &mut String, chunk: &Chunk, name: &str) {
    out.push_str(&format!("== {name} ==\n"));
    for (ip, op) in chunk.code.iter().enumerate() {
        let span = chunk.span(ip);
        let line = if span.line == 0 {
            "     ".to_string()
        } else {
            format!("{:>4} ", span.line)
        };
        out.push_str(&format!("{line}{ip:>5}  {}\n", format_op(chunk, *op)));
    }
    for (i, f) in chunk.functions.iter().enumerate() {
        out.push('\n');
        disassemble_into(
            out,
            &f.chunk,
            &format!("{name} > function {} [{i}]", f.name),
        );
    }
}

fn format_op(chunk: &Chunk, op: Op) -> String {
    match op {
        Op::LoadConst(a) => format!("LOAD_CONST     {a}  ({})", format_const(chunk.constant(a))),
        Op::Pop => "POP".to_string(),
        Op::GetVar(a) => format!("GET_VAR        {a}  '{}'", chunk.name(a)),
        Op::SetVar(a) => format!("SET_VAR        {a}  '{}'", chunk.name(a)),
        Op::DefineVar(a) => format!("DEFINE_VAR     {a}  '{}'", chunk.name(a)),
        Op::GetProp(a) => format!("GET_PROP       {a}  '{}'", chunk.name(a)),
        Op::SetProp(a) => format!("SET_PROP       {a}  '{}'", chunk.name(a)),
        Op::GetVarOrObject(a) => format!("GET_VAR_OBJ    {a}  '{}'", chunk.name(a)),
        Op::GetPropOrObject(a) => format!("GET_PROP_OBJ   {a}  '{}'", chunk.name(a)),
        Op::GetIndex => "GET_INDEX".to_string(),
        Op::SetIndex => "SET_INDEX".to_string(),
        Op::MakeArray(a) => format!("MAKE_ARRAY     {a}"),
        Op::MakeObject(a) => format!("MAKE_OBJECT    {a}"),
        Op::MakeClosure(a) => format!("MAKE_CLOSURE   {a}  '{}'", chunk.functions[a as usize].name),
        Op::Call(a, b) => format!("CALL           {a}  '{}'", chunk.name(b)),
        Op::MethodCall(a, b) => format!("METHOD_CALL    {b}  '{}'", chunk.name(a)),
        Op::Say(a) => format!("SAY            {a}"),
        Op::Add => "ADD".to_string(),
        Op::Sub => "SUB".to_string(),
        Op::Mul => "MUL".to_string(),
        Op::Div => "DIV".to_string(),
        Op::Mod => "MOD".to_string(),
        Op::Eq => "EQ".to_string(),
        Op::NotEq => "NOT_EQ".to_string(),
        Op::Gt => "GT".to_string(),
        Op::Lt => "LT".to_string(),
        Op::GtEq => "GT_EQ".to_string(),
        Op::LtEq => "LT_EQ".to_string(),
        Op::Neg => "NEG".to_string(),
        Op::Not => "NOT".to_string(),
        Op::ToBool => "TO_BOOL".to_string(),
        Op::Jump(a) => format!("JUMP           -> {a}"),
        Op::JumpIfFalse(a) => format!("JUMP_IF_FALSE  -> {a}"),
        Op::JumpIfFalsePeek(a) => format!("JUMP_FALSE_PK  -> {a}"),
        Op::JumpIfTruePeek(a) => format!("JUMP_TRUE_PK   -> {a}"),
        Op::RepeatSetup => "REPEAT_SETUP".to_string(),
        Op::RepeatNext(a) => format!("REPEAT_NEXT    -> {a}"),
        Op::IterSetup => "ITER_SETUP".to_string(),
        Op::IterNext(a, b) => format!("ITER_NEXT      '{}' -> {b}", chunk.name(a)),
        Op::Return => "RETURN".to_string(),
        Op::ReturnNull => "RETURN_NULL".to_string(),
        Op::Import(a) => format!("IMPORT         {a}  '{}'", chunk.name(a)),
    }
}

fn format_const(c: &Const) -> String {
    match c {
        Const::Number(n) => crate::value::format_number(*n),
        Const::String(s) => format!("\"{s}\""),
        Const::Bool(b) => b.to_string(),
        Const::Null => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    const SOURCE: &str = "import math\nfunction add(a, b)\n  return a + b\nend\nnames = [\"Alex\"]\nfor n in names\n  say(add(1, 2) + \" \" + n)\nend\nrepeat 2\n  say(math.pi)\nend\n";

    fn compiled() -> Chunk {
        compile(&parse(tokenize(SOURCE).unwrap()).unwrap()).unwrap()
    }

    #[test]
    fn round_trips_through_the_emb_format() {
        let chunk = compiled();
        let bytes = serialize(&chunk);
        let restored = deserialize(&bytes).expect("bytecode to load");
        assert_eq!(chunk, restored);
    }

    #[test]
    fn rejects_files_that_are_not_bytecode() {
        let err = deserialize(b"say(\"hello\")\n").unwrap_err();
        assert!(err.message.contains("Not an Englishmen bytecode file"));
    }

    #[test]
    fn rejects_a_future_bytecode_version() {
        let mut bytes = serialize(&compiled());
        bytes[4] = EMB_VERSION as u8 + 1;
        let err = deserialize(&bytes).unwrap_err();
        assert!(err.message.contains("Unsupported bytecode version"));
    }

    #[test]
    fn rejects_truncated_bytecode() {
        let bytes = serialize(&compiled());
        let err = deserialize(&bytes[..bytes.len() / 2]).unwrap_err();
        assert!(err.message.contains("Corrupt bytecode"));
    }

    #[test]
    fn rejects_operands_pointing_outside_the_chunk() {
        let mut chunk = Chunk::new();
        let name = chunk.add_name("x");
        chunk.emit(Op::GetVar(name), Span::new(1, 1));
        chunk.emit(Op::Jump(999), Span::new(1, 1));
        let err = deserialize(&serialize(&chunk)).unwrap_err();
        assert!(err.message.contains("jump target out of range"));
    }

    #[test]
    fn disassembly_names_every_instruction() {
        let text = disassemble(&compiled(), "test.em");
        assert!(text.contains("== test.em =="));
        assert!(text.contains("function add"));
        assert!(text.contains("IMPORT"));
        assert!(text.contains("ITER_NEXT"));
        assert!(
            !text.contains("Op::"),
            "disassembly should not leak Rust debug output"
        );
    }
}

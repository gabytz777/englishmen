//! The Englishmen compiler: lowers the AST into bytecode chunks.
//!
//! This is the second backend for the shared front end — the interpreter walks
//! the AST directly, while the compiler translates it once into a flat
//! instruction stream that the VM (or a saved `.emb` file) executes. Control
//! flow becomes jumps, literals are interned into a constant pool, and
//! constant subexpressions are folded away at compile time.

use crate::ast::{Assignable, BinOp, Expr, Stmt, UnOp};
use crate::bytecode::{Chunk, Const, FunctionProto, Op};
use crate::error::{EmError, Span};
use std::rc::Rc;

/// Compile a whole program into a top-level chunk.
pub fn compile(program: &[Stmt]) -> Result<Chunk, EmError> {
    let mut compiler = Compiler::new();
    compiler.block(program)?;
    let span = compiler.span;
    compiler.chunk.emit(Op::ReturnNull, span);
    Ok(compiler.chunk)
}

struct Compiler {
    chunk: Chunk,
    /// Span of the construct currently being compiled, attached to every
    /// instruction so runtime errors can point back at the source.
    span: Span,
}

impl Compiler {
    fn new() -> Self {
        Compiler {
            chunk: Chunk::new(),
            span: Span::default(),
        }
    }

    fn emit(&mut self, op: Op) -> usize {
        let span = self.span;
        self.chunk.emit(op, span)
    }

    fn block(&mut self, stmts: &[Stmt]) -> Result<(), EmError> {
        for stmt in stmts {
            self.stmt(stmt)?;
        }
        Ok(())
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<(), EmError> {
        match stmt {
            Stmt::Assign(target, value, span) => {
                self.span = *span;
                self.expr(value)?;
                self.span = *span;
                self.assign(target)
            }
            Stmt::ExprStmt(expr, span) => {
                self.span = *span;
                self.expr(expr)?;
                self.span = *span;
                self.emit(Op::Pop);
                Ok(())
            }
            Stmt::If(branches, else_body, span) => {
                self.if_stmt(branches, else_body.as_deref(), *span)
            }
            Stmt::FunctionDef(name, params, body, span) => {
                self.span = *span;
                let proto = self.function(name, params, body, *span)?;
                self.chunk.functions.push(Rc::new(proto));
                let index = (self.chunk.functions.len() - 1) as u32;
                let name_idx = self.chunk.add_name(name);
                self.span = *span;
                self.emit(Op::MakeClosure(index));
                self.emit(Op::DefineVar(name_idx));
                Ok(())
            }
            Stmt::Return(value, span) => {
                // A top-level `return` ends the program, matching the
                // interpreter, which unwinds out of the whole run.
                self.span = *span;
                match value {
                    Some(expr) => {
                        self.expr(expr)?;
                        self.span = *span;
                        self.emit(Op::Return);
                    }
                    None => {
                        self.emit(Op::ReturnNull);
                    }
                }
                Ok(())
            }
            Stmt::Repeat(count, body, span) => self.repeat(count, body, *span),
            Stmt::ForIn(var, iterable, body, span) => self.for_in(var, iterable, body, *span),
            Stmt::Import(name, span) => {
                self.span = *span;
                let idx = self.chunk.add_name(name);
                self.emit(Op::Import(idx));
                Ok(())
            }
        }
    }

    /// Emit the store for an assignment target. The value is already on the
    /// stack, matching the interpreter's "evaluate the right-hand side first"
    /// ordering.
    fn assign(&mut self, target: &Assignable) -> Result<(), EmError> {
        match target {
            Assignable::Variable(name) => {
                let idx = self.chunk.add_name(name);
                self.emit(Op::SetVar(idx));
                Ok(())
            }
            Assignable::Property(base, name) => {
                self.object_base(base)?;
                let idx = self.chunk.add_name(name);
                self.emit(Op::SetProp(idx));
                Ok(())
            }
            Assignable::Index(base, index) => {
                self.expr(base)?;
                self.expr(index)?;
                self.emit(Op::SetIndex);
                Ok(())
            }
        }
    }

    /// Push the base of a property assignment, auto-creating missing objects
    /// along the way so `person.address.city = "London"` works without any
    /// prior declaration.
    fn object_base(&mut self, expr: &Expr) -> Result<(), EmError> {
        match expr {
            Expr::Variable(name) => {
                let idx = self.chunk.add_name(name);
                self.emit(Op::GetVarOrObject(idx));
                Ok(())
            }
            Expr::PropertyAccess(inner, name) => {
                self.object_base(inner)?;
                let idx = self.chunk.add_name(name);
                self.emit(Op::GetPropOrObject(idx));
                Ok(())
            }
            other => self.expr(other),
        }
    }

    fn if_stmt(
        &mut self,
        branches: &[(Expr, Vec<Stmt>)],
        else_body: Option<&[Stmt]>,
        span: Span,
    ) -> Result<(), EmError> {
        let mut end_jumps = Vec::new();
        for (cond, body) in branches {
            self.span = span;
            self.expr(cond)?;
            self.span = span;
            let skip = self.emit(Op::JumpIfFalse(0));
            self.block(body)?;
            self.span = span;
            end_jumps.push(self.emit(Op::Jump(0)));
            let next = self.chunk.here();
            self.chunk.patch_jump(skip, next);
        }
        if let Some(body) = else_body {
            self.block(body)?;
        }
        let end = self.chunk.here();
        for jump in end_jumps {
            self.chunk.patch_jump(jump, end);
        }
        Ok(())
    }

    fn repeat(&mut self, count: &Expr, body: &[Stmt], span: Span) -> Result<(), EmError> {
        self.span = span;
        self.expr(count)?;
        self.span = span;
        self.emit(Op::RepeatSetup);
        let head = self.chunk.here();
        let exit = self.emit(Op::RepeatNext(0));
        self.block(body)?;
        self.span = span;
        self.emit(Op::Jump(head as u32));
        let end = self.chunk.here();
        self.chunk.patch_jump(exit, end);
        Ok(())
    }

    fn for_in(
        &mut self,
        var: &str,
        iterable: &Expr,
        body: &[Stmt],
        span: Span,
    ) -> Result<(), EmError> {
        self.span = span;
        self.expr(iterable)?;
        self.span = span;
        self.emit(Op::IterSetup);
        let name_idx = self.chunk.add_name(var);
        let head = self.chunk.here();
        let exit = self.emit(Op::IterNext(name_idx, 0));
        self.block(body)?;
        self.span = span;
        self.emit(Op::Jump(head as u32));
        let end = self.chunk.here();
        self.chunk.patch_jump(exit, end);
        Ok(())
    }

    /// Compile a function body into its own chunk.
    fn function(
        &mut self,
        name: &str,
        params: &[String],
        body: &[Stmt],
        span: Span,
    ) -> Result<FunctionProto, EmError> {
        if let Some(dup) = first_duplicate(params) {
            return Err(EmError::syntax(format!(
                "Function '{name}' has two parameters named '{dup}'"
            ))
            .with_span(span)
            .with_hint("Give each parameter a different name."));
        }

        let mut inner = Compiler::new();
        inner.span = span;
        inner.block(body)?;
        inner.span = span;
        inner.chunk.emit(Op::ReturnNull, span);

        Ok(FunctionProto {
            name: name.to_string(),
            params: params.to_vec(),
            chunk: inner.chunk,
        })
    }

    fn expr(&mut self, expr: &Expr) -> Result<(), EmError> {
        match expr {
            Expr::Number(n) => self.constant(Const::Number(*n)),
            Expr::String(s) => self.constant(Const::String(s.clone())),
            Expr::Bool(b) => self.constant(Const::Bool(*b)),
            Expr::Null => self.constant(Const::Null),
            Expr::Variable(name) => {
                let idx = self.chunk.add_name(name);
                self.emit(Op::GetVar(idx));
            }
            Expr::PropertyAccess(base, name) => {
                self.expr(base)?;
                let idx = self.chunk.add_name(name);
                self.emit(Op::GetProp(idx));
            }
            Expr::Index(base, index) => {
                self.expr(base)?;
                self.expr(index)?;
                self.emit(Op::GetIndex);
            }
            Expr::Array(items) => {
                for item in items {
                    self.expr(item)?;
                }
                self.emit(Op::MakeArray(items.len() as u32));
            }
            Expr::Object(fields) => {
                for (key, value) in fields {
                    self.constant(Const::String(key.clone()));
                    self.expr(value)?;
                }
                self.emit(Op::MakeObject(fields.len() as u32));
            }
            Expr::Call(callee, args) => {
                // `say` is a builtin, not a variable, exactly as in the interpreter.
                if matches!(callee.as_ref(), Expr::Variable(name) if name == "say") {
                    for arg in args {
                        self.expr(arg)?;
                    }
                    self.emit(Op::Say(args.len() as u32));
                    return Ok(());
                }
                self.expr(callee)?;
                for arg in args {
                    self.expr(arg)?;
                }
                let name_idx = self.chunk.add_name(&describe_callee(callee));
                self.emit(Op::Call(args.len() as u32, name_idx));
            }
            Expr::MethodCall(base, method, args) => {
                self.expr(base)?;
                for arg in args {
                    self.expr(arg)?;
                }
                let idx = self.chunk.add_name(method);
                self.emit(Op::MethodCall(idx, args.len() as u32));
            }
            Expr::Binary(left, op, right, span) => self.binary(left, *op, right, *span)?,
            Expr::Unary(op, operand, span) => self.unary(*op, operand, *span)?,
        }
        Ok(())
    }

    fn constant(&mut self, value: Const) {
        let idx = self.chunk.add_const(value);
        self.emit(Op::LoadConst(idx));
    }

    fn binary(&mut self, left: &Expr, op: BinOp, right: &Expr, span: Span) -> Result<(), EmError> {
        // `and`/`or` short-circuit and always yield a boolean, so they compile
        // to jumps rather than to an instruction.
        if op == BinOp::And || op == BinOp::Or {
            self.expr(left)?;
            self.span = span;
            self.emit(Op::ToBool);
            let jump = if op == BinOp::And {
                self.emit(Op::JumpIfFalsePeek(0))
            } else {
                self.emit(Op::JumpIfTruePeek(0))
            };
            self.emit(Op::Pop);
            self.expr(right)?;
            self.span = span;
            self.emit(Op::ToBool);
            let end = self.chunk.here();
            self.chunk.patch_jump(jump, end);
            return Ok(());
        }

        if let (Some(l), Some(r)) = (fold(left), fold(right)) {
            if let Some(folded) = fold_binary(op, &l, &r) {
                self.span = span;
                self.constant(folded);
                return Ok(());
            }
        }

        self.expr(left)?;
        self.expr(right)?;
        self.span = span;
        self.emit(match op {
            BinOp::Add => Op::Add,
            BinOp::Sub => Op::Sub,
            BinOp::Mul => Op::Mul,
            BinOp::Div => Op::Div,
            BinOp::Mod => Op::Mod,
            BinOp::Eq => Op::Eq,
            BinOp::NotEq => Op::NotEq,
            BinOp::Gt => Op::Gt,
            BinOp::Lt => Op::Lt,
            BinOp::GtEq => Op::GtEq,
            BinOp::LtEq => Op::LtEq,
            BinOp::And | BinOp::Or => unreachable!("handled above"),
        });
        Ok(())
    }

    fn unary(&mut self, op: UnOp, operand: &Expr, span: Span) -> Result<(), EmError> {
        if let Some(folded) = fold(operand).and_then(|value| fold_unary(op, &value)) {
            self.span = span;
            self.constant(folded);
            return Ok(());
        }
        self.expr(operand)?;
        self.span = span;
        self.emit(match op {
            UnOp::Neg => Op::Neg,
            UnOp::Not => Op::Not,
        });
        Ok(())
    }
}

/// Reduce an expression to a constant when it is made only of literals, so
/// nested arithmetic like `2 + 3 * 4` collapses to a single constant.
/// Anything that could fail or observe the program state stays a runtime
/// operation, so its error surfaces exactly as the interpreter's would.
fn fold(expr: &Expr) -> Option<Const> {
    match expr {
        Expr::Number(n) => Some(Const::Number(*n)),
        Expr::String(s) => Some(Const::String(s.clone())),
        Expr::Bool(b) => Some(Const::Bool(*b)),
        Expr::Null => Some(Const::Null),
        Expr::Binary(left, op, right, _) => fold_binary(*op, &fold(left)?, &fold(right)?),
        Expr::Unary(op, operand, _) => fold_unary(*op, &fold(operand)?),
        _ => None,
    }
}

/// Fold a binary operation over two constants. Division and modulo by zero are
/// deliberately left to the VM so the error keeps the interpreter's wording,
/// and `and`/`or` are never folded because they compile to jumps.
fn fold_binary(op: BinOp, left: &Const, right: &Const) -> Option<Const> {
    if let (Const::Number(a), Const::Number(b)) = (left, right) {
        let (a, b) = (*a, *b);
        return Some(match op {
            BinOp::Add => Const::Number(a + b),
            BinOp::Sub => Const::Number(a - b),
            BinOp::Mul => Const::Number(a * b),
            BinOp::Div if b != 0.0 => Const::Number(a / b),
            BinOp::Mod if b != 0.0 => Const::Number(a % b),
            BinOp::Eq => Const::Bool(a == b),
            BinOp::NotEq => Const::Bool(a != b),
            BinOp::Gt => Const::Bool(a > b),
            BinOp::Lt => Const::Bool(a < b),
            BinOp::GtEq => Const::Bool(a >= b),
            BinOp::LtEq => Const::Bool(a <= b),
            _ => return None,
        });
    }

    match op {
        // Adding anything to a string concatenates, exactly as at runtime.
        BinOp::Add => match (left, right) {
            (Const::String(a), b) => Some(Const::String(format!("{a}{}", const_display(b)))),
            (a, Const::String(b)) => Some(Const::String(format!("{}{b}", const_display(a)))),
            _ => None,
        },
        BinOp::Eq => Some(Const::Bool(const_equals(left, right))),
        BinOp::NotEq => Some(Const::Bool(!const_equals(left, right))),
        _ => None,
    }
}

fn fold_unary(op: UnOp, value: &Const) -> Option<Const> {
    match (op, value) {
        (UnOp::Neg, Const::Number(n)) => Some(Const::Number(-n)),
        (UnOp::Not, value) => Some(Const::Bool(!const_truthy(value))),
        _ => None,
    }
}

fn const_display(value: &Const) -> String {
    match value {
        Const::Number(n) => crate::value::format_number(*n),
        Const::String(s) => s.clone(),
        Const::Bool(b) => b.to_string(),
        Const::Null => "null".to_string(),
    }
}

/// Constant equality, matching `Value::equals` for the scalar cases.
fn const_equals(left: &Const, right: &Const) -> bool {
    match (left, right) {
        (Const::Number(a), Const::Number(b)) => a == b,
        (Const::String(a), Const::String(b)) => a == b,
        (Const::Bool(a), Const::Bool(b)) => a == b,
        (Const::Null, Const::Null) => true,
        _ => false,
    }
}

/// Constant truthiness, matching `Value::is_truthy` for the scalar cases.
fn const_truthy(value: &Const) -> bool {
    match value {
        Const::Number(n) => *n != 0.0,
        Const::String(s) => !s.is_empty(),
        Const::Bool(b) => *b,
        Const::Null => false,
    }
}

/// How a callee is referred to in error messages, matching the interpreter.
fn describe_callee(expr: &Expr) -> String {
    match expr {
        Expr::Variable(name) => name.clone(),
        Expr::PropertyAccess(_, name) => name.clone(),
        _ => "value".to_string(),
    }
}

fn first_duplicate(names: &[String]) -> Option<&String> {
    for (i, name) in names.iter().enumerate() {
        if names[..i].contains(name) {
            return Some(name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn compile_src(src: &str) -> Chunk {
        compile(&parse(tokenize(src).unwrap()).unwrap()).unwrap()
    }

    #[test]
    fn folds_constant_arithmetic() {
        let chunk = compile_src("x = 2 + 3 * 4\n");
        assert!(chunk.constants.contains(&Const::Number(14.0)));
        assert!(!chunk.code.iter().any(|op| matches!(op, Op::Add | Op::Mul)));
    }

    #[test]
    fn leaves_division_by_zero_to_the_vm() {
        let chunk = compile_src("x = 1 / 0\n");
        assert!(chunk.code.contains(&Op::Div));
    }

    #[test]
    fn say_compiles_to_the_builtin() {
        let chunk = compile_src("say(\"hi\")\n");
        assert!(chunk.code.contains(&Op::Say(1)));
    }

    #[test]
    fn functions_get_their_own_chunk() {
        let chunk = compile_src("function add(a, b)\n  return a + b\nend\n");
        assert_eq!(chunk.functions.len(), 1);
        let proto = &chunk.functions[0];
        assert_eq!(proto.params, vec!["a".to_string(), "b".to_string()]);
        assert!(proto.chunk.code.contains(&Op::Return));
    }

    #[test]
    fn jumps_are_patched_within_bounds() {
        let chunk = compile_src("if true\n  x = 1\nelse\n  x = 2\nend\n");
        for op in &chunk.code {
            if let Op::Jump(t) | Op::JumpIfFalse(t) = op {
                assert!(
                    *t as usize <= chunk.code.len(),
                    "jump target {t} out of range"
                );
            }
        }
    }

    #[test]
    fn top_level_return_ends_the_program() {
        // The interpreter unwinds out of the whole run, so the compiled form
        // returns from the top-level chunk rather than rejecting the program.
        let chunk = compile_src("return 1\n");
        assert!(chunk.code.contains(&Op::Return));
    }

    #[test]
    fn duplicate_parameters_are_rejected() {
        let program = parse(tokenize("function f(a, a)\n  return a\nend\n").unwrap()).unwrap();
        let err = compile(&program).unwrap_err();
        assert!(err.message.contains("two parameters named"));
    }
}

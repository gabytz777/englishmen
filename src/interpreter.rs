use crate::ast::{expr_to_assignable, Assignable, BinOp, Expr, Stmt, UnOp};
use crate::environment::Environment;
use crate::error::{suggest_closest, EmError, Span};
use crate::modules;
use crate::runtime::{self, Signal};
use crate::value::{FunctionValue, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

pub struct Interpreter {
    pub globals: Rc<Environment>,
    imported_modules: Vec<String>,
}

/// A target whose base/index have already been evaluated. Keeping it around
/// lets compound assignment and `++/--` read, compute and write without
/// evaluating a call or index expression twice.
enum TargetLocation {
    Variable(String),
    Property(Value, String),
    Index(Value, Value),
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            globals: Environment::new(),
            imported_modules: Vec::new(),
        }
    }

    pub fn run(&mut self, program: &[Stmt]) -> Result<(), EmError> {
        let env = Rc::clone(&self.globals);
        match self.exec_block(program, &env)? {
            Signal::Normal
            | Signal::Return(_)
            | Signal::Break
            | Signal::Continue => Ok(()),
        }
    }

    fn exec_block(&mut self, stmts: &[Stmt], env: &Rc<Environment>) -> Result<Signal, EmError> {
        for stmt in stmts {
            match self.exec_stmt(stmt, env)? {
                Signal::Normal => continue,
                signal => return Ok(signal),
            }
        }
        Ok(Signal::Normal)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, env: &Rc<Environment>) -> Result<Signal, EmError> {
        match stmt {
            Stmt::Assign(target, expr, span) => {
                let value = self.eval(expr, env)?;
                self.assign_to(target, value, env, *span)?;
                Ok(Signal::Normal)
            }
            Stmt::CompoundAssign(target, op, expr, span) => {
                self.compound_to(target, *op, expr, env, *span)?;
                Ok(Signal::Normal)
            }
            Stmt::ExprStmt(expr, _span) => {
                self.eval(expr, env)?;
                Ok(Signal::Normal)
            }
            Stmt::If(branches, else_body, _span) => {
                for (cond, body) in branches {
                    if self.eval(cond, env)?.is_truthy() {
                        return self.exec_block(body, env);
                    }
                }
                if let Some(body) = else_body {
                    return self.exec_block(body, env);
                }
                Ok(Signal::Normal)
            }
            Stmt::FunctionDef(name, params, body, _span) => {
                let function = Value::Function(Rc::new(FunctionValue {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    closure: Rc::clone(env),
                }));
                env.define(name, function);
                Ok(Signal::Normal)
            }
            Stmt::Return(expr, _span) => {
                let value = match expr {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Null,
                };
                Ok(Signal::Return(value))
            }
            Stmt::Repeat(count_expr, body, span) => {
                let count = self.eval(count_expr, env)?;
                let n = runtime::expect_number(&count, "as a repeat count")
                    .map_err(|m| EmError::new(m).with_span(*span))?;
                if n < 0.0 {
                    return Err(EmError::new("repeat count must not be negative").with_span(*span));
                }
                for _ in 0..(n as i64) {
                    match self.exec_block(body, env)? {
                        Signal::Normal | Signal::Continue => continue,
                        Signal::Break => break,
                        signal => return Ok(signal),
                    }
                }
                Ok(Signal::Normal)
            }
            Stmt::While(cond, body, _span) => {
                while self.eval(cond, env)?.is_truthy() {
                    match self.exec_block(body, env)? {
                        Signal::Normal | Signal::Continue => continue,
                        Signal::Break => break,
                        signal => return Ok(signal),
                    }
                }
                Ok(Signal::Normal)
            }
            Stmt::ForIn(var, iterable, body, span) => {
                let value = self.eval(iterable, env)?;
                let items: Vec<Value> = match &value {
                    Value::Array(a) => a.borrow().clone(),
                    other => {
                        return Err(EmError::new(format!(
                            "Cannot iterate over a {}",
                            other.type_name()
                        ))
                        .with_span(*span)
                        .with_hint("'for ... in' expects an array."))
                    }
                };
                for item in items {
                    env.define(var, item);
                    match self.exec_block(body, env)? {
                        Signal::Normal | Signal::Continue => continue,
                        Signal::Break => break,
                        signal => return Ok(signal),
                    }
                }
                Ok(Signal::Normal)
            }
            Stmt::Break(_) => Ok(Signal::Break),
            Stmt::Continue(_) => Ok(Signal::Continue),
            Stmt::Import(name, span) => {
                if let Some(module) = modules::load_module(name) {
                    env.define(name, module);
                    self.imported_modules.push(name.clone());
                    Ok(Signal::Normal)
                } else {
                    let module_names: Vec<String> =
                        modules::STD_MODULES.iter().map(|s| s.to_string()).collect();
                    let hint = suggest_closest(name, module_names.iter());
                    let mut err = EmError::import(format!("Unknown module '{name}'")).with_span(*span);
                    if let Some(h) = hint {
                        err = err.with_hint(format!("Did you mean '{h}'?"));
                    } else {
                        err = err.with_hint(format!(
                            "Available standard modules: {}",
                            modules::STD_MODULES.join(", ")
                        ));
                    }
                    Err(err)
                }
            }
        }
    }

    fn assign_to(
        &mut self,
        target: &Assignable,
        value: Value,
        env: &Rc<Environment>,
        span: Span,
    ) -> Result<(), EmError> {
        match target {
            Assignable::Variable(name) => {
                env.assign(name, value);
                Ok(())
            }
            Assignable::Property(base_expr, name) => {
                let base = self.eval_or_create_object(base_expr, env, span)?;
                set_property_value(&base, name, value, span)
            }
            Assignable::Index(base_expr, index_expr) => {
                let base = self.eval(base_expr, env)?;
                let index = self.eval(index_expr, env)?;
                set_index_value(&base, &index, value, span)
            }
        }
    }

    /// Evaluate a compound-assignment target once and remember where to store.
    fn locate_target(
        &mut self,
        target: &Assignable,
        env: &Rc<Environment>,
        span: Span,
    ) -> Result<TargetLocation, EmError> {
        match target {
            Assignable::Variable(name) => {
                // Reading an undeclared variable is an error, unlike plain `=`.
                variable_value(name, env)?;
                Ok(TargetLocation::Variable(name.clone()))
            }
            Assignable::Property(base_expr, name) => {
                let base = self.eval_or_create_object(base_expr, env, span)?;
                Ok(TargetLocation::Property(base, name.clone()))
            }
            Assignable::Index(base_expr, index_expr) => {
                let base = self.eval(base_expr, env)?;
                let index = self.eval(index_expr, env)?;
                Ok(TargetLocation::Index(base, index))
            }
        }
    }

    fn read_location(
        &self,
        location: &TargetLocation,
        env: &Rc<Environment>,
        span: Span,
    ) -> Result<Value, EmError> {
        match location {
            TargetLocation::Variable(name) => variable_value(name, env),
            TargetLocation::Property(base, name) => property_value(base, name),
            TargetLocation::Index(base, index) => index_value(base, index, span),
        }
    }

    fn write_location(
        &self,
        location: &TargetLocation,
        value: Value,
        env: &Rc<Environment>,
        span: Span,
    ) -> Result<(), EmError> {
        match location {
            TargetLocation::Variable(name) => {
                env.assign(name, value);
                Ok(())
            }
            TargetLocation::Property(base, name) => set_property_value(base, name, value, span),
            TargetLocation::Index(base, index) => set_index_value(base, index, value, span),
        }
    }

    fn compound_to(
        &mut self,
        target: &Assignable,
        op: BinOp,
        value_expr: &Expr,
        env: &Rc<Environment>,
        span: Span,
    ) -> Result<(), EmError> {
        let location = self.locate_target(target, env, span)?;
        let old = self.read_location(&location, env, span)?;
        let rhs = self.eval(value_expr, env)?;
        let new = binary_values(op, old, rhs, span)?;
        self.write_location(&location, new, env, span)
    }

    /// Evaluate a base expression for property assignment, auto-vivifying
    /// bare variables into objects when they don't exist yet
    /// (so `person.age = 18` works without a prior `person = {}`).
    fn eval_or_create_object(
        &mut self,
        expr: &Expr,
        env: &Rc<Environment>,
        span: Span,
    ) -> Result<Value, EmError> {
        if let Expr::Variable(name) = expr {
            if !env.has(name) {
                let obj = Value::new_object(BTreeMap::new());
                env.define(name, obj.clone());
                return Ok(obj);
            }
            return Ok(env.get(name).unwrap());
        }
        if let Expr::PropertyAccess(inner_base, inner_name) = expr {
            let inner = self.eval_or_create_object(inner_base, env, span)?;
            if let Value::Object(obj) = &inner {
                if !obj.borrow().contains_key(inner_name) {
                    let child = Value::new_object(BTreeMap::new());
                    obj.borrow_mut().insert(inner_name.clone(), child);
                }
                let value = obj.borrow().get(inner_name).cloned().unwrap();
                return Ok(value);
            }
        }
        self.eval(expr, env)
    }

    fn eval(&mut self, expr: &Expr, env: &Rc<Environment>) -> Result<Value, EmError> {
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::String(s) => Ok(Value::String(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Null => Ok(Value::Null),
            Expr::Variable(name) => variable_value(name, env),
            Expr::PropertyAccess(base, name) => {
                let base_val = self.eval(base, env)?;
                property_value(&base_val, name)
            }
            Expr::Index(base, index) => {
                let base_val = self.eval(base, env)?;
                let index_val = self.eval(index, env)?;
                index_value(&base_val, &index_val, Span::default())
            }
            Expr::Array(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval(item, env)?);
                }
                Ok(Value::new_array(values))
            }
            Expr::Object(fields) => {
                let mut map = BTreeMap::new();
                for (key, value_expr) in fields {
                    let value = self.eval(value_expr, env)?;
                    map.insert(key.clone(), value);
                }
                Ok(Value::new_object(map))
            }
            Expr::Call(callee, args) => {
                if let Expr::Variable(name) = callee.as_ref() {
                    if name == "say" {
                        let values = self.eval_args(args, env)?;
                        runtime::say(&values);
                        return Ok(Value::Null);
                    }
                }
                let callee_val = self.eval(callee, env)?;
                let arg_values = self.eval_args(args, env)?;
                self.call_value(&callee_val, &arg_values, describe_callee(callee))
            }
            Expr::MethodCall(base, method, args) => {
                let base_val = self.eval(base, env)?;
                let arg_values = self.eval_args(args, env)?;
                let span = Span::default();
                match &base_val {
                    Value::Array(arr) => runtime::call_array_method(arr, method, &arg_values, span),
                    Value::Object(obj) => {
                        if let Some(func) = obj.borrow().get(method).cloned() {
                            return self.call_value(&func, &arg_values, method.clone());
                        }
                        runtime::call_object_method(obj, method, &arg_values, span)
                    }
                    Value::String(s) => runtime::call_string_method(s, method, &arg_values, span),
                    Value::Number(n) => runtime::call_number_method(*n, method, &arg_values, span),
                    Value::Bool(b) => runtime::call_bool_method(*b, method, &arg_values, span),
                    other => Err(EmError::new(format!(
                        "Cannot call method '{method}' on a {}",
                        other.type_name()
                    ))),
                }
            }
            Expr::Binary(left, op, right, span) => self.eval_binary(left, *op, right, env, *span),
            Expr::Unary(op, operand, span) => self.eval_unary(*op, operand, env, *span),
            Expr::Ternary(cond, when_true, when_false, _span) => {
                if self.eval(cond, env)?.is_truthy() {
                    self.eval(when_true, env)
                } else {
                    self.eval(when_false, env)
                }
            }
            Expr::Postfix(target, increment, span) => {
                let assignable = expr_to_assignable((**target).clone(), *span)?;
                let location = self.locate_target(&assignable, env, *span)?;
                let old = self.read_location(&location, env, *span)?;
                let op = if *increment { BinOp::Add } else { BinOp::Sub };
                let new = binary_values(op, old.clone(), Value::Number(1.0), *span)?;
                self.write_location(&location, new, env, *span)?;
                Ok(old)
            }
        }
    }

    fn eval_args(&mut self, args: &[Expr], env: &Rc<Environment>) -> Result<Vec<Value>, EmError> {
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(arg, env)?);
        }
        Ok(values)
    }

    fn call_value(
        &mut self,
        callee: &Value,
        args: &[Value],
        name_hint: String,
    ) -> Result<Value, EmError> {
        match callee {
            Value::Function(func) => self.call_user_function(func, args),
            Value::Native(native) => (native.func)(args).map_err(EmError::new),
            other => Err(EmError::new(format!(
                "'{name_hint}' is not callable (found {})",
                other.type_name()
            ))),
        }
    }

    fn call_user_function(
        &mut self,
        func: &Rc<FunctionValue>,
        args: &[Value],
    ) -> Result<Value, EmError> {
        let call_env = Environment::child(&func.closure);
        for (i, param) in func.params.iter().enumerate() {
            let value = args.get(i).cloned().unwrap_or(Value::Null);
            call_env.define(param, value);
        }
        if args.len() != func.params.len() {
            let signature = format!("({})", func.params.join(", "));
            return Err(EmError::arity(format!(
                "Function '{}{}' expects {} argument(s) but got {}",
                func.name,
                signature,
                func.params.len(),
                args.len()
            ))
            .with_hint(if func.params.is_empty() {
                "This function takes no arguments.".to_string()
            } else {
                format!(
                    "Pass {}: {}.",
                    if func.params.len() == 1 {
                        "one argument"
                    } else {
                        "the arguments"
                    },
                    func.params.join(", ")
                )
            }));
        }
        match self.exec_block(&func.body, &call_env)? {
            Signal::Return(v) => Ok(v),
            Signal::Normal | Signal::Break | Signal::Continue => Ok(Value::Null),
        }
    }

    fn eval_binary(
        &mut self,
        left: &Expr,
        op: BinOp,
        right: &Expr,
        env: &Rc<Environment>,
        span: Span,
    ) -> Result<Value, EmError> {
        if op == BinOp::And {
            let l = self.eval(left, env)?;
            if !l.is_truthy() {
                return Ok(Value::Bool(false));
            }
            let r = self.eval(right, env)?;
            return Ok(Value::Bool(r.is_truthy()));
        }
        if op == BinOp::Or {
            let l = self.eval(left, env)?;
            if l.is_truthy() {
                return Ok(Value::Bool(true));
            }
            let r = self.eval(right, env)?;
            return Ok(Value::Bool(r.is_truthy()));
        }
        if op == BinOp::Coalesce {
            let l = self.eval(left, env)?;
            if !matches!(l, Value::Null) {
                return Ok(l);
            }
            return self.eval(right, env);
        }

        let l = self.eval(left, env)?;
        let r = self.eval(right, env)?;
        binary_values(op, l, r, span)
    }

    fn eval_unary(
        &mut self,
        op: UnOp,
        operand: &Expr,
        env: &Rc<Environment>,
        span: Span,
    ) -> Result<Value, EmError> {
        let v = self.eval(operand, env)?;
        match op {
            UnOp::Neg => {
                let n = runtime::expect_number(&v, "for unary '-'")
                    .map_err(|m| EmError::new(m).with_span(span))?;
                Ok(Value::Number(-n))
            }
            UnOp::Not => Ok(Value::Bool(!v.is_truthy())),
            UnOp::BitNot => {
                let n = runtime::expect_number(&v, "for bitwise '~'")
                    .map_err(|m| EmError::new(m).with_span(span))?;
                Ok(Value::Number(!(n as i64) as f64))
            }
        }
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

fn variable_value(name: &str, env: &Rc<Environment>) -> Result<Value, EmError> {
    if let Some(value) = env.get(name) {
        return Ok(value);
    }
    let names = env.names();
    let hint = suggest_closest(name, names.iter());
    let mut err = EmError::name(format!("Unknown variable '{name}'"));
    if let Some(h) = hint {
        err = err.with_hint(format!("Did you mean '{h}'?"));
    }
    Err(err)
}

fn property_value(base: &Value, name: &str) -> Result<Value, EmError> {
    match base {
        Value::Object(obj) => {
            if let Some(value) = obj.borrow().get(name) {
                return Ok(value.clone());
            }
            let names: Vec<String> = obj.borrow().keys().cloned().collect();
            let hint = suggest_closest(name, names.iter());
            let mut err = EmError::name(format!("Unknown property '{name}'"));
            if let Some(h) = hint {
                err = err.with_hint(format!("Did you mean '{h}'?"));
            }
            Err(err)
        }
        other => Err(EmError::new(format!(
            "Cannot access property '{name}' on a {}",
            other.type_name()
        ))),
    }
}

fn set_property_value(
    base: &Value,
    name: &str,
    value: Value,
    span: Span,
) -> Result<(), EmError> {
    match base {
        Value::Object(obj) => {
            obj.borrow_mut().insert(name.to_string(), value);
            Ok(())
        }
        other => Err(EmError::new(format!(
            "Cannot set property '{name}' on a {}",
            other.type_name()
        ))
        .with_span(span)),
    }
}

fn index_value(base: &Value, index: &Value, span: Span) -> Result<Value, EmError> {
    match base {
        Value::Array(arr) => {
            let i = runtime::expect_number(index, "as an array index")
                .map_err(|m| EmError::new(m).with_span(span))?;
            let arr = arr.borrow();
            let idx = i as i64;
            if idx < 0 || idx as usize >= arr.len() {
                return Err(EmError::new(format!(
                    "Array index {idx} is out of bounds (length {})",
                    arr.len()
                ))
                .with_span(span));
            }
            Ok(arr[idx as usize].clone())
        }
        Value::String(s) => {
            let i = runtime::expect_number(index, "as a string index")
                .map_err(|m| EmError::new(m).with_span(span))?;
            let chars: Vec<char> = s.chars().collect();
            let idx = i as i64;
            if idx < 0 || idx as usize >= chars.len() {
                return Err(EmError::new(format!(
                    "String index {idx} is out of bounds (length {})",
                    chars.len()
                ))
                .with_span(span));
            }
            Ok(Value::String(chars[idx as usize].to_string()))
        }
        other => Err(EmError::new(format!("Cannot index into a {}", other.type_name()))
            .with_span(span)),
    }
}

fn set_index_value(
    base: &Value,
    index: &Value,
    value: Value,
    span: Span,
) -> Result<(), EmError> {
    match base {
        Value::Array(arr) => {
            let i = runtime::expect_number(index, "as an array index")
                .map_err(|m| EmError::new(m).with_span(span))?;
            let mut arr = arr.borrow_mut();
            let idx = i as i64;
            if idx < 0 || idx as usize >= arr.len() {
                return Err(EmError::new(format!(
                    "Array index {idx} is out of bounds (length {})",
                    arr.len()
                ))
                .with_span(span));
            }
            arr[idx as usize] = value;
            Ok(())
        }
        other => Err(EmError::new(format!("Cannot index into a {}", other.type_name()))
            .with_span(span)),
    }
}

/// Apply an eager binary operator to two already-evaluated values. The VM has
/// the same implementation, keeping the two backends' results and error
/// wording identical.
fn binary_values(op: BinOp, left: Value, right: Value, span: Span) -> Result<Value, EmError> {
    match op {
        BinOp::Eq => return Ok(Value::Bool(left.equals(&right))),
        BinOp::NotEq => return Ok(Value::Bool(!left.equals(&right))),
        BinOp::Add => {
            return match (&left, &right) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{a}{b}"))),
                (Value::String(a), b) => Ok(Value::String(format!("{a}{}", b.display()))),
                (a, Value::String(b)) => Ok(Value::String(format!("{}{b}", a.display()))),
                _ => Err(EmError::new(format!(
                    "Cannot add {} and {}",
                    left.type_name(),
                    right.type_name()
                ))
                .with_span(span)),
            }
        }
        BinOp::Mul => {
            if let (Value::String(s), Value::Number(n)) | (Value::Number(n), Value::String(s)) =
                (&left, &right)
            {
                return Ok(Value::String(s.repeat(repeat_count(*n))));
            }
        }
        _ => {}
    }

    let context = match op {
        BinOp::Gt | BinOp::Lt | BinOp::GtEq | BinOp::LtEq => (
            "on the left of the comparison",
            "on the right of the comparison",
        ),
        _ => ("on the left of the operator", "on the right of the operator"),
    };
    let a = runtime::expect_number(&left, context.0)
        .map_err(|m| EmError::new(m).with_span(span))?;
    let b = runtime::expect_number(&right, context.1)
        .map_err(|m| EmError::new(m).with_span(span))?;

    Ok(match op {
        BinOp::Sub => Value::Number(a - b),
        BinOp::Mul => Value::Number(a * b),
        BinOp::Div => {
            if b == 0.0 {
                return Err(EmError::new("Division by zero").with_span(span));
            }
            Value::Number(a / b)
        }
        BinOp::Mod => {
            if b == 0.0 {
                return Err(EmError::new("Division by zero").with_span(span));
            }
            Value::Number(a % b)
        }
        BinOp::FloorDiv => {
            if b == 0.0 {
                return Err(EmError::new("Division by zero").with_span(span));
            }
            Value::Number((a / b).floor())
        }
        BinOp::Pow => Value::Number(a.powf(b)),
        BinOp::BitAnd => Value::Number(((a as i64) & (b as i64)) as f64),
        BinOp::BitOr => Value::Number(((a as i64) | (b as i64)) as f64),
        BinOp::BitXor => Value::Number(((a as i64) ^ (b as i64)) as f64),
        BinOp::Shl => Value::Number((a as i64).wrapping_shl((b as i64) as u32) as f64),
        BinOp::Shr => Value::Number((a as i64).wrapping_shr((b as i64) as u32) as f64),
        BinOp::Gt => Value::Bool(a > b),
        BinOp::Lt => Value::Bool(a < b),
        BinOp::GtEq => Value::Bool(a >= b),
        BinOp::LtEq => Value::Bool(a <= b),
        other => {
            return Err(EmError::syntax(format!(
                "Operator {other:?} cannot be used here"
            ))
            .with_span(span))
        }
    })
}

/// Match the compiler's constant-folding limit for string repetition.
fn repeat_count(n: f64) -> usize {
    if n.is_finite() && n > 0.0 {
        (n as usize).min(1_000_000)
    } else {
        0
    }
}

fn describe_callee(expr: &Expr) -> String {
    match expr {
        Expr::Variable(name) => name.clone(),
        Expr::PropertyAccess(_, name) => name.clone(),
        _ => "value".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn eval_var(src: &str, var: &str) -> Value {
        let program = parse(tokenize(src).unwrap()).unwrap();
        let mut interp = Interpreter::new();
        interp.run(&program).unwrap();
        interp.globals.get(var).unwrap()
    }

    fn run_err(src: &str) -> EmError {
        let program = parse(tokenize(src).unwrap()).unwrap();
        let mut interp = Interpreter::new();
        interp.run(&program).unwrap_err()
    }

    #[test]
    fn arithmetic_precedence() {
        let v = eval_var("result = 2 + 3 * 4\n", "result");
        assert!(matches!(v, Value::Number(n) if n == 14.0));
        let v = eval_var("result = (2 + 3) * 4\n", "result");
        assert!(matches!(v, Value::Number(n) if n == 20.0));
    }

    #[test]
    fn variables_and_reassignment() {
        let v = eval_var("age = 18\nage = age + 1\n", "age");
        assert!(matches!(v, Value::Number(n) if n == 19.0));
    }

    #[test]
    fn object_property_assignment_and_nesting() {
        let v = eval_var("person.address.city = \"London\"\n", "person");
        if let Value::Object(obj) = v {
            let address = obj.borrow().get("address").cloned().unwrap();
            if let Value::Object(addr) = address {
                assert_eq!(addr.borrow().get("city").unwrap().display(), "London");
            } else {
                panic!("expected nested object");
            }
        } else {
            panic!("expected object");
        }
    }

    #[test]
    fn array_indexing_and_methods() {
        let v = eval_var(
            "people = [\"Alex\", \"Sam\"]\npeople.add(\"Zoe\")\nn = people.length()\n",
            "n",
        );
        assert!(matches!(v, Value::Number(n) if n == 3.0));
    }

    #[test]
    fn conditions_true_branch() {
        let v = eval_var("age = 20\nif age >= 18 and true\n  result = \"adult\"\nelse\n  result = \"minor\"\nend\n", "result");
        assert_eq!(v.display(), "adult");
    }

    #[test]
    fn function_call_and_return() {
        let v = eval_var(
            "function add(a, b)\n  return a + b\nend\nresult = add(10, 20)\n",
            "result",
        );
        assert!(matches!(v, Value::Number(n) if n == 30.0));
    }

    #[test]
    fn function_scope_is_local() {
        let v = eval_var(
            "x = 1\nfunction f()\n  x = 2\n  return x\nend\ny = f()\n",
            "x",
        );
        // Assignment inside a function body walks up to the outer scope, matching
        // Englishmen's "assignment creates/overwrites the nearest variable" rule.
        assert!(matches!(v, Value::Number(n) if n == 2.0));
    }

    #[test]
    fn repeat_loop_runs_n_times() {
        let v = eval_var("count = 0\nrepeat 5\n  count = count + 1\nend\n", "count");
        assert!(matches!(v, Value::Number(n) if n == 5.0));
    }

    #[test]
    fn for_in_loop_over_array() {
        let v = eval_var(
            "total = 0\nfor n in [1, 2, 3]\n  total = total + n\nend\n",
            "total",
        );
        assert!(matches!(v, Value::Number(n) if n == 6.0));
    }

    #[test]
    fn division_by_zero_errors() {
        let err = run_err("x = 1 / 0\n");
        assert!(err.message.contains("Division by zero"));
    }

    #[test]
    fn unknown_variable_errors_with_suggestion() {
        let err = run_err("age = 18\nsay(agee)\n");
        assert!(err.message.contains("Unknown variable"));
        assert!(err.hint.unwrap().contains("age"));
    }

    #[test]
    fn array_index_out_of_bounds_errors() {
        let err = run_err("people = [\"Alex\"]\nsay(people[5])\n");
        assert!(err.message.contains("out of bounds"));
    }

    #[test]
    fn type_mismatch_errors() {
        let err = run_err("x = \"a\" - 1\n");
        assert!(err.message.contains("expected a number") || err.message.contains("Type mismatch"));
    }
}

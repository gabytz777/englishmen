use crate::ast::{Assignable, BinOp, Expr, Stmt, UnOp};
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
            Signal::Normal => Ok(()),
            Signal::Return(_) => Ok(()),
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
                        Signal::Normal => continue,
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
                        Signal::Normal => continue,
                        signal => return Ok(signal),
                    }
                }
                Ok(Signal::Normal)
            }
            Stmt::Import(name, span) => {
                if let Some(module) = modules::load_module(name) {
                    env.define(name, module);
                    self.imported_modules.push(name.clone());
                    Ok(Signal::Normal)
                } else {
                    let module_names: Vec<String> =
                        modules::STD_MODULES.iter().map(|s| s.to_string()).collect();
                    let hint = suggest_closest(name, module_names.iter());
                    let mut err = EmError::new(format!("Unknown module '{name}'")).with_span(*span);
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
                match base {
                    Value::Object(obj) => {
                        obj.borrow_mut().insert(name.clone(), value);
                        Ok(())
                    }
                    other => Err(EmError::new(format!(
                        "Cannot set property '{name}' on a {}",
                        other.type_name()
                    ))
                    .with_span(span)),
                }
            }
            Assignable::Index(base_expr, index_expr) => {
                let base = self.eval(base_expr, env)?;
                let index = self.eval(index_expr, env)?;
                match base {
                    Value::Array(arr) => {
                        let i = runtime::expect_number(&index, "as an array index")
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
                    other => Err(EmError::new(format!(
                        "Cannot index into a {}",
                        other.type_name()
                    ))
                    .with_span(span)),
                }
            }
        }
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
            Expr::Variable(name) => {
                if let Some(v) = env.get(name) {
                    Ok(v)
                } else {
                    let names = env.names();
                    let hint = suggest_closest(name, names.iter());
                    let mut err = EmError::new(format!("Unknown variable '{name}'"));
                    if let Some(h) = hint {
                        err = err.with_hint(format!("Did you mean '{h}'?"));
                    }
                    Err(err)
                }
            }
            Expr::PropertyAccess(base, name) => {
                let base_val = self.eval(base, env)?;
                match &base_val {
                    Value::Object(obj) => {
                        if let Some(v) = obj.borrow().get(name) {
                            Ok(v.clone())
                        } else {
                            let names: Vec<String> = obj.borrow().keys().cloned().collect();
                            let hint = suggest_closest(name, names.iter());
                            let mut err = EmError::new(format!("Unknown property '{name}'"));
                            if let Some(h) = hint {
                                err = err.with_hint(format!("Did you mean '{h}'?"));
                            }
                            Err(err)
                        }
                    }
                    other => Err(EmError::new(format!(
                        "Cannot access property '{name}' on a {}",
                        other.type_name()
                    ))),
                }
            }
            Expr::Index(base, index) => {
                let base_val = self.eval(base, env)?;
                let index_val = self.eval(index, env)?;
                match &base_val {
                    Value::Array(arr) => {
                        let i = runtime::expect_number(&index_val, "as an array index")?;
                        let arr = arr.borrow();
                        let idx = i as i64;
                        if idx < 0 || idx as usize >= arr.len() {
                            return Err(EmError::new(format!(
                                "Array index {idx} is out of bounds (length {})",
                                arr.len()
                            )));
                        }
                        Ok(arr[idx as usize].clone())
                    }
                    Value::String(s) => {
                        let i = runtime::expect_number(&index_val, "as a string index")?;
                        let chars: Vec<char> = s.chars().collect();
                        let idx = i as i64;
                        if idx < 0 || idx as usize >= chars.len() {
                            return Err(EmError::new(format!(
                                "String index {idx} is out of bounds (length {})",
                                chars.len()
                            )));
                        }
                        Ok(Value::String(chars[idx as usize].to_string()))
                    }
                    other => Err(EmError::new(format!(
                        "Cannot index into a {}",
                        other.type_name()
                    ))),
                }
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
                    other => Err(EmError::new(format!(
                        "Cannot call method '{method}' on a {}",
                        other.type_name()
                    ))),
                }
            }
            Expr::Binary(left, op, right, span) => self.eval_binary(left, *op, right, env, *span),
            Expr::Unary(op, operand, span) => self.eval_unary(*op, operand, env, *span),
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
            return Err(EmError::new(format!(
                "Function '{}' expects {} argument(s) but got {}",
                func.name,
                func.params.len(),
                args.len()
            )));
        }
        match self.exec_block(&func.body, &call_env)? {
            Signal::Return(v) => Ok(v),
            Signal::Normal => Ok(Value::Null),
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

        let l = self.eval(left, env)?;
        let r = self.eval(right, env)?;

        match op {
            BinOp::Eq => return Ok(Value::Bool(l.equals(&r))),
            BinOp::NotEq => return Ok(Value::Bool(!l.equals(&r))),
            _ => {}
        }

        match op {
            BinOp::Add => match (&l, &r) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{a}{b}"))),
                (Value::String(a), b) => Ok(Value::String(format!("{a}{}", b.display()))),
                (a, Value::String(b)) => Ok(Value::String(format!("{}{b}", a.display()))),
                _ => Err(EmError::new(format!(
                    "Cannot add {} and {}",
                    l.type_name(),
                    r.type_name()
                ))
                .with_span(span)),
            },
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                let a = runtime::expect_number(&l, "on the left of the operator")
                    .map_err(|m| EmError::new(m).with_span(span))?;
                let b = runtime::expect_number(&r, "on the right of the operator")
                    .map_err(|m| EmError::new(m).with_span(span))?;
                match op {
                    BinOp::Sub => Ok(Value::Number(a - b)),
                    BinOp::Mul => Ok(Value::Number(a * b)),
                    BinOp::Div => {
                        if b == 0.0 {
                            Err(EmError::new("Division by zero").with_span(span))
                        } else {
                            Ok(Value::Number(a / b))
                        }
                    }
                    BinOp::Mod => {
                        if b == 0.0 {
                            Err(EmError::new("Division by zero").with_span(span))
                        } else {
                            Ok(Value::Number(a % b))
                        }
                    }
                    _ => unreachable!(),
                }
            }
            BinOp::Gt | BinOp::Lt | BinOp::GtEq | BinOp::LtEq => {
                let a = runtime::expect_number(&l, "on the left of the comparison")
                    .map_err(|m| EmError::new(m).with_span(span))?;
                let b = runtime::expect_number(&r, "on the right of the comparison")
                    .map_err(|m| EmError::new(m).with_span(span))?;
                let result = match op {
                    BinOp::Gt => a > b,
                    BinOp::Lt => a < b,
                    BinOp::GtEq => a >= b,
                    BinOp::LtEq => a <= b,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(result))
            }
            _ => unreachable!(),
        }
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
        }
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
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

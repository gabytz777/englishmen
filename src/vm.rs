//! The Englishmen virtual machine: executes the bytecode produced by the
//! compiler (or loaded from a `.emb` file).
//!
//! The VM keeps Englishmen's semantics identical to the tree-walking
//! interpreter — same scoping rules, same error wording — but control flow is
//! flat, so loops and conditionals cost a jump instead of a recursive walk,
//! and calls run on an explicit frame stack rather than the host stack.

use crate::bytecode::{Chunk, Const, FunctionProto, Op};
use crate::environment::Environment;
use crate::error::{suggest_closest, EmError, Span};
use crate::modules;
use crate::runtime;
use crate::value::{CompiledFunction, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

/// Maximum call depth. Englishmen has no tail calls, so runaway recursion is
/// reported as an error instead of crashing the process.
const MAX_FRAMES: usize = 512;

/// The code a frame is executing: either the top-level program or a function.
/// Cloning one is a reference-count bump, so switching frames is cheap.
#[derive(Clone)]
enum CodeUnit {
    Program(Rc<Chunk>),
    Function(Rc<FunctionProto>),
}

impl CodeUnit {
    fn chunk(&self) -> &Chunk {
        match self {
            CodeUnit::Program(c) => c,
            CodeUnit::Function(f) => &f.chunk,
        }
    }
}

struct Frame {
    unit: CodeUnit,
    ip: usize,
    env: Rc<Environment>,
    /// Stack height when the frame was pushed; anything above it belongs to
    /// this frame and is discarded when it returns.
    stack_base: usize,
}

pub struct Vm {
    pub globals: Rc<Environment>,
    stack: Vec<Value>,
    frames: Vec<Frame>,
}

impl Vm {
    pub fn new() -> Self {
        Vm {
            globals: Environment::new(),
            stack: Vec::with_capacity(256),
            frames: Vec::new(),
        }
    }

    /// Execute a compiled program in this VM's global scope.
    pub fn run(&mut self, chunk: Chunk) -> Result<(), EmError> {
        self.run_rc(Rc::new(chunk))
    }

    pub fn run_rc(&mut self, chunk: Rc<Chunk>) -> Result<(), EmError> {
        self.stack.clear();
        self.frames.clear();
        self.frames.push(Frame {
            unit: CodeUnit::Program(chunk),
            ip: 0,
            env: Rc::clone(&self.globals),
            stack_base: 0,
        });
        let result = self.execute();
        // Leave the VM reusable (the REPL keeps globals between inputs).
        self.stack.clear();
        self.frames.clear();
        result
    }

    fn execute(&mut self) -> Result<(), EmError> {
        'frames: loop {
            let (unit, env) = {
                let frame = self.frames.last().expect("a frame to run");
                (frame.unit.clone(), Rc::clone(&frame.env))
            };
            let chunk = unit.chunk();

            loop {
                let ip = self.frames.last().expect("a frame to run").ip;
                if ip >= chunk.code.len() {
                    // Falling off the end of a chunk returns null.
                    if self.pop_frame(Value::Null) {
                        return Ok(());
                    }
                    continue 'frames;
                }
                let op = chunk.code[ip];
                self.frames.last_mut().expect("a frame to run").ip = ip + 1;
                let span = chunk.span(ip);

                match op {
                    Op::LoadConst(idx) => {
                        let value = const_to_value(chunk.constant(idx));
                        self.stack.push(value);
                    }
                    Op::Pop => {
                        self.pop();
                    }

                    Op::GetVar(idx) => {
                        let name = chunk.name(idx);
                        match env.get(name) {
                            Some(v) => self.stack.push(v),
                            None => return Err(at(unknown_variable(name, &env), span)),
                        }
                    }
                    Op::SetVar(idx) => {
                        let value = self.pop();
                        env.assign(chunk.name(idx), value);
                    }
                    Op::DefineVar(idx) => {
                        let value = self.pop();
                        env.define(chunk.name(idx), value);
                    }

                    Op::GetProp(idx) => {
                        let base = self.pop();
                        let name = chunk.name(idx);
                        let value = get_property(&base, name).map_err(|e| at(e, span))?;
                        self.stack.push(value);
                    }
                    Op::SetProp(idx) => {
                        let base = self.pop();
                        let value = self.pop();
                        let name = chunk.name(idx);
                        match base {
                            Value::Object(obj) => {
                                obj.borrow_mut().insert(name.to_string(), value);
                            }
                            other => {
                                return Err(at(
                                    EmError::new(format!(
                                        "Cannot set property '{name}' on a {}",
                                        other.type_name()
                                    )),
                                    span,
                                ))
                            }
                        }
                    }
                    Op::GetVarOrObject(idx) => {
                        let name = chunk.name(idx);
                        if !env.has(name) {
                            env.define(name, Value::new_object(BTreeMap::new()));
                        }
                        let value = env.get(name).expect("just defined");
                        self.stack.push(value);
                    }
                    Op::GetPropOrObject(idx) => {
                        let base = self.pop();
                        let name = chunk.name(idx);
                        let value = match &base {
                            Value::Object(obj) => {
                                let missing = !obj.borrow().contains_key(name);
                                if missing {
                                    obj.borrow_mut().insert(
                                        name.to_string(),
                                        Value::new_object(BTreeMap::new()),
                                    );
                                }
                                obj.borrow().get(name).cloned().expect("just inserted")
                            }
                            other => get_property(other, name).map_err(|e| at(e, span))?,
                        };
                        self.stack.push(value);
                    }

                    Op::GetIndex => {
                        let index = self.pop();
                        let base = self.pop();
                        let value = get_index(&base, &index).map_err(|e| at(e, span))?;
                        self.stack.push(value);
                    }
                    Op::SetIndex => {
                        let index = self.pop();
                        let base = self.pop();
                        let value = self.pop();
                        set_index(&base, &index, value).map_err(|e| at(e, span))?;
                    }

                    Op::MakeArray(count) => {
                        let items = self.drain(count as usize);
                        self.stack.push(Value::new_array(items));
                    }
                    Op::MakeObject(count) => {
                        let flat = self.drain(count as usize * 2);
                        let mut map = BTreeMap::new();
                        let mut it = flat.into_iter();
                        while let (Some(key), Some(value)) = (it.next(), it.next()) {
                            map.insert(key.display(), value);
                        }
                        self.stack.push(Value::new_object(map));
                    }
                    Op::MakeClosure(idx) => {
                        let proto = Rc::clone(&chunk.functions[idx as usize]);
                        self.stack.push(Value::Compiled(Rc::new(CompiledFunction {
                            proto,
                            closure: Rc::clone(&env),
                        })));
                    }

                    Op::Call(argc, name_idx) => {
                        let args = self.drain(argc as usize);
                        let callee = self.pop();
                        if self.call(&callee, args, span, chunk.name(name_idx))? {
                            continue 'frames;
                        }
                    }
                    Op::MethodCall(name_idx, argc) => {
                        let args = self.drain(argc as usize);
                        let receiver = self.pop();
                        let method = chunk.name(name_idx);
                        if self.method_call(&receiver, method, args, span)? {
                            continue 'frames;
                        }
                    }
                    Op::Say(argc) => {
                        let args = self.drain(argc as usize);
                        runtime::say(&args);
                        self.stack.push(Value::Null);
                    }

                    Op::Add
                    | Op::Sub
                    | Op::Mul
                    | Op::Div
                    | Op::Mod
                    | Op::Eq
                    | Op::NotEq
                    | Op::Gt
                    | Op::Lt
                    | Op::GtEq
                    | Op::LtEq => {
                        let right = self.pop();
                        let left = self.pop();
                        let value = binary(op, &left, &right).map_err(|e| at(e, span))?;
                        self.stack.push(value);
                    }
                    Op::Neg => {
                        let value = self.pop();
                        let n = runtime::expect_number(&value, "for unary '-'")
                            .map_err(|m| at(EmError::new(m), span))?;
                        self.stack.push(Value::Number(-n));
                    }
                    Op::Not => {
                        let value = self.pop();
                        self.stack.push(Value::Bool(!value.is_truthy()));
                    }
                    Op::ToBool => {
                        let value = self.pop();
                        self.stack.push(Value::Bool(value.is_truthy()));
                    }

                    Op::Jump(target) => self.jump(target),
                    Op::JumpIfFalse(target) => {
                        if !self.pop().is_truthy() {
                            self.jump(target);
                        }
                    }
                    Op::JumpIfFalsePeek(target) => {
                        if !self.peek().is_truthy() {
                            self.jump(target);
                        }
                    }
                    Op::JumpIfTruePeek(target) => {
                        if self.peek().is_truthy() {
                            self.jump(target);
                        }
                    }

                    Op::RepeatSetup => {
                        let count = self.pop();
                        let n = runtime::expect_number(&count, "as a repeat count")
                            .map_err(|m| at(EmError::new(m), span))?;
                        if n < 0.0 {
                            return Err(at(
                                EmError::new("repeat count must not be negative"),
                                span,
                            ));
                        }
                        self.stack.push(Value::Number((n as i64) as f64));
                    }
                    Op::RepeatNext(target) => {
                        let remaining = match self.peek() {
                            Value::Number(n) => *n,
                            _ => unreachable!("repeat counter is always a number"),
                        };
                        if remaining <= 0.0 {
                            self.pop();
                            self.jump(target);
                        } else {
                            let last = self.stack.len() - 1;
                            self.stack[last] = Value::Number(remaining - 1.0);
                        }
                    }
                    Op::IterSetup => {
                        let iterable = self.pop();
                        // Snapshot the array so mutation inside the body does
                        // not disturb the iteration, as the interpreter does.
                        let items = match &iterable {
                            Value::Array(a) => a.borrow().clone(),
                            other => {
                                return Err(at(
                                    EmError::new(format!(
                                        "Cannot iterate over a {}",
                                        other.type_name()
                                    ))
                                    .with_hint("'for ... in' expects an array."),
                                    span,
                                ))
                            }
                        };
                        self.stack.push(Value::new_array(items));
                        self.stack.push(Value::Number(0.0));
                    }
                    Op::IterNext(name_idx, target) => {
                        let cursor = match self.peek() {
                            Value::Number(n) => *n as usize,
                            _ => unreachable!("iterator cursor is always a number"),
                        };
                        let items = match &self.stack[self.stack.len() - 2] {
                            Value::Array(a) => Rc::clone(a),
                            _ => unreachable!("iterator source is always an array"),
                        };
                        let item = items.borrow().get(cursor).cloned();
                        match item {
                            Some(value) => {
                                env.define(chunk.name(name_idx), value);
                                let last = self.stack.len() - 1;
                                self.stack[last] = Value::Number((cursor + 1) as f64);
                            }
                            None => {
                                self.pop();
                                self.pop();
                                self.jump(target);
                            }
                        }
                    }

                    Op::Return => {
                        let value = self.pop();
                        if self.pop_frame(value) {
                            return Ok(());
                        }
                        continue 'frames;
                    }
                    Op::ReturnNull => {
                        if self.pop_frame(Value::Null) {
                            return Ok(());
                        }
                        continue 'frames;
                    }

                    Op::Import(idx) => {
                        let name = chunk.name(idx);
                        match modules::load_module(name) {
                            Some(module) => env.define(name, module),
                            None => return Err(at(unknown_module(name), span)),
                        }
                    }
                }
            }
        }
    }

    /// Call a value. Returns `true` when a new frame was pushed and the
    /// interpreter loop must switch to it; native calls complete immediately.
    fn call(
        &mut self,
        callee: &Value,
        args: Vec<Value>,
        span: Span,
        name_hint: &str,
    ) -> Result<bool, EmError> {
        match callee {
            Value::Compiled(func) => {
                if args.len() != func.proto.params.len() {
                    return Err(at(
                        EmError::new(format!(
                            "Function '{}' expects {} argument(s) but got {}",
                            func.proto.name,
                            func.proto.params.len(),
                            args.len()
                        )),
                        span,
                    ));
                }
                if self.frames.len() >= MAX_FRAMES {
                    return Err(at(
                        EmError::new(format!(
                            "Too many nested calls while calling '{}'",
                            func.proto.name
                        ))
                        .with_hint("This usually means a function calls itself without a stopping condition."),
                        span,
                    ));
                }
                let call_env = Environment::child(&func.closure);
                for (param, value) in func.proto.params.iter().zip(args) {
                    call_env.define(param, value);
                }
                self.frames.push(Frame {
                    unit: CodeUnit::Function(Rc::clone(&func.proto)),
                    ip: 0,
                    env: call_env,
                    stack_base: self.stack.len(),
                });
                Ok(true)
            }
            Value::Native(native) => {
                let value = (native.func)(&args).map_err(|m| at(EmError::new(m), span))?;
                self.stack.push(value);
                Ok(false)
            }
            other => Err(at(
                EmError::new(format!(
                    "'{name_hint}' is not callable (found {})",
                    other.type_name()
                )),
                span,
            )),
        }
    }

    fn method_call(
        &mut self,
        receiver: &Value,
        method: &str,
        args: Vec<Value>,
        span: Span,
    ) -> Result<bool, EmError> {
        match receiver {
            Value::Array(arr) => {
                let value = runtime::call_array_method(arr, method, &args, span)?;
                self.stack.push(value);
                Ok(false)
            }
            Value::Object(obj) => {
                // A function stored on the object wins over the builtin methods,
                // so objects can carry their own behaviour.
                let field = obj.borrow().get(method).cloned();
                if let Some(func) = field {
                    return self.call(&func, args, span, method);
                }
                let value = runtime::call_object_method(obj, method, &args, span)?;
                self.stack.push(value);
                Ok(false)
            }
            Value::String(s) => {
                let value = runtime::call_string_method(s, method, &args, span)?;
                self.stack.push(value);
                Ok(false)
            }
            other => Err(at(
                EmError::new(format!(
                    "Cannot call method '{method}' on a {}",
                    other.type_name()
                )),
                span,
            )),
        }
    }

    /// Discard the finished frame and hand its result to the caller.
    /// Returns `true` when the program itself has finished.
    fn pop_frame(&mut self, result: Value) -> bool {
        let frame = self.frames.pop().expect("a frame to pop");
        self.stack.truncate(frame.stack_base);
        if self.frames.is_empty() {
            return true;
        }
        self.stack.push(result);
        false
    }

    fn jump(&mut self, target: u32) {
        self.frames.last_mut().expect("a frame to run").ip = target as usize;
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("a value on the stack")
    }

    fn peek(&self) -> &Value {
        self.stack.last().expect("a value on the stack")
    }

    fn drain(&mut self, count: usize) -> Vec<Value> {
        let at = self.stack.len() - count;
        self.stack.split_off(at)
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

fn const_to_value(c: &Const) -> Value {
    match c {
        Const::Number(n) => Value::Number(*n),
        Const::String(s) => Value::String(s.clone()),
        Const::Bool(b) => Value::Bool(*b),
        Const::Null => Value::Null,
    }
}

/// Attach a source position to an error that does not have one yet.
fn at(mut error: EmError, span: Span) -> EmError {
    if error.span.is_none() {
        error.span = Some(span);
    }
    error
}

fn unknown_variable(name: &str, env: &Rc<Environment>) -> EmError {
    let names = env.names();
    let mut err = EmError::new(format!("Unknown variable '{name}'"));
    if let Some(hint) = suggest_closest(name, names.iter()) {
        err = err.with_hint(format!("Did you mean '{hint}'?"));
    }
    err
}

fn unknown_module(name: &str) -> EmError {
    let module_names: Vec<String> = modules::STD_MODULES.iter().map(|s| s.to_string()).collect();
    let mut err = EmError::new(format!("Unknown module '{name}'"));
    if let Some(hint) = suggest_closest(name, module_names.iter()) {
        err = err.with_hint(format!("Did you mean '{hint}'?"));
    } else {
        err = err.with_hint(format!(
            "Available standard modules: {}",
            modules::STD_MODULES.join(", ")
        ));
    }
    err
}

fn get_property(base: &Value, name: &str) -> Result<Value, EmError> {
    match base {
        Value::Object(obj) => {
            if let Some(v) = obj.borrow().get(name) {
                return Ok(v.clone());
            }
            let names: Vec<String> = obj.borrow().keys().cloned().collect();
            let mut err = EmError::new(format!("Unknown property '{name}'"));
            if let Some(hint) = suggest_closest(name, names.iter()) {
                err = err.with_hint(format!("Did you mean '{hint}'?"));
            }
            Err(err)
        }
        other => Err(EmError::new(format!(
            "Cannot access property '{name}' on a {}",
            other.type_name()
        ))),
    }
}

fn get_index(base: &Value, index: &Value) -> Result<Value, EmError> {
    match base {
        Value::Array(arr) => {
            let i = runtime::expect_number(index, "as an array index").map_err(EmError::new)?;
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
            let i = runtime::expect_number(index, "as a string index").map_err(EmError::new)?;
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

fn set_index(base: &Value, index: &Value, value: Value) -> Result<(), EmError> {
    match base {
        Value::Array(arr) => {
            let i = runtime::expect_number(index, "as an array index").map_err(EmError::new)?;
            let mut arr = arr.borrow_mut();
            let idx = i as i64;
            if idx < 0 || idx as usize >= arr.len() {
                return Err(EmError::new(format!(
                    "Array index {idx} is out of bounds (length {})",
                    arr.len()
                )));
            }
            arr[idx as usize] = value;
            Ok(())
        }
        other => Err(EmError::new(format!(
            "Cannot index into a {}",
            other.type_name()
        ))),
    }
}

fn binary(op: Op, left: &Value, right: &Value) -> Result<Value, EmError> {
    match op {
        Op::Eq => return Ok(Value::Bool(left.equals(right))),
        Op::NotEq => return Ok(Value::Bool(!left.equals(right))),
        Op::Add => {
            return match (left, right) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{a}{b}"))),
                (Value::String(a), b) => Ok(Value::String(format!("{a}{}", b.display()))),
                (a, Value::String(b)) => Ok(Value::String(format!("{}{b}", a.display()))),
                _ => Err(EmError::new(format!(
                    "Cannot add {} and {}",
                    left.type_name(),
                    right.type_name()
                ))),
            }
        }
        _ => {}
    }

    let context = match op {
        Op::Gt | Op::Lt | Op::GtEq | Op::LtEq => (
            "on the left of the comparison",
            "on the right of the comparison",
        ),
        _ => (
            "on the left of the operator",
            "on the right of the operator",
        ),
    };
    let a = runtime::expect_number(left, context.0).map_err(EmError::new)?;
    let b = runtime::expect_number(right, context.1).map_err(EmError::new)?;

    Ok(match op {
        Op::Sub => Value::Number(a - b),
        Op::Mul => Value::Number(a * b),
        Op::Div => {
            if b == 0.0 {
                return Err(EmError::new("Division by zero"));
            }
            Value::Number(a / b)
        }
        Op::Mod => {
            if b == 0.0 {
                return Err(EmError::new("Division by zero"));
            }
            Value::Number(a % b)
        }
        Op::Gt => Value::Bool(a > b),
        Op::Lt => Value::Bool(a < b),
        Op::GtEq => Value::Bool(a >= b),
        Op::LtEq => Value::Bool(a <= b),
        other => unreachable!("not a binary instruction: {other:?}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile;
    use crate::interpreter::Interpreter;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn run(src: &str) -> Result<Vm, EmError> {
        let program = parse(tokenize(src).unwrap()).unwrap();
        let chunk = compile(&program).unwrap();
        let mut vm = Vm::new();
        vm.run(chunk)?;
        Ok(vm)
    }

    fn eval_var(src: &str, var: &str) -> Value {
        run(src)
            .unwrap()
            .globals
            .get(var)
            .expect("variable to exist")
    }

    fn run_err(src: &str) -> EmError {
        match run(src) {
            Err(e) => e,
            Ok(_) => panic!("expected an error from: {src}"),
        }
    }

    /// Both backends must agree, so every error case is checked against the
    /// interpreter's wording rather than against a hard-coded string.
    fn assert_matches_interpreter(src: &str) {
        let program = parse(tokenize(src).unwrap()).unwrap();
        let interpreted = Interpreter::new().run(&program).unwrap_err();
        let compiled = run_err(src);
        assert_eq!(
            compiled.message, interpreted.message,
            "message differs for: {src}"
        );
        assert_eq!(compiled.hint, interpreted.hint, "hint differs for: {src}");
    }

    #[test]
    fn arithmetic_precedence() {
        assert!(
            matches!(eval_var("result = 2 + 3 * 4\n", "result"), Value::Number(n) if n == 14.0)
        );
        assert!(
            matches!(eval_var("a = 2\nresult = (a + 3) * 4\n", "result"), Value::Number(n) if n == 20.0)
        );
    }

    #[test]
    fn variables_and_reassignment() {
        assert!(
            matches!(eval_var("age = 18\nage = age + 1\n", "age"), Value::Number(n) if n == 19.0)
        );
    }

    #[test]
    fn object_property_assignment_and_nesting() {
        let value = eval_var("person.address.city = \"London\"\n", "person");
        let Value::Object(obj) = value else {
            panic!("expected an object");
        };
        let address = obj.borrow().get("address").cloned().unwrap();
        let Value::Object(address) = address else {
            panic!("expected a nested object");
        };
        assert_eq!(address.borrow().get("city").unwrap().display(), "London");
    }

    #[test]
    fn array_indexing_and_methods() {
        let src = "people = [\"Alex\", \"Sam\"]\npeople.add(\"Zoe\")\nn = people.length()\nfirst = people[0]\n";
        assert!(matches!(eval_var(src, "n"), Value::Number(n) if n == 3.0));
        assert_eq!(eval_var(src, "first").display(), "Alex");
    }

    #[test]
    fn conditions_pick_the_first_true_branch() {
        let src = "age = 15\nif age >= 18\n  result = \"adult\"\nelse if age >= 13\n  result = \"teen\"\nelse\n  result = \"child\"\nend\n";
        assert_eq!(eval_var(src, "result").display(), "teen");
    }

    #[test]
    fn and_or_short_circuit_to_booleans() {
        assert_eq!(eval_var("x = true and 5\n", "x").display(), "true");
        assert_eq!(eval_var("x = false and 5\n", "x").display(), "false");
        assert_eq!(eval_var("x = false or \"\"\n", "x").display(), "false");
        // The right-hand side must not run when the left already decides it.
        let src = "hits = 0\nfunction bump()\n  hits = hits + 1\n  return true\nend\nx = false and bump()\n";
        assert!(matches!(eval_var(src, "hits"), Value::Number(n) if n == 0.0));
    }

    #[test]
    fn functions_calls_returns_and_recursion() {
        let src = "function fib(n)\n  if n < 2\n    return n\n  end\n  return fib(n - 1) + fib(n - 2)\nend\nresult = fib(10)\n";
        assert!(matches!(eval_var(src, "result"), Value::Number(n) if n == 55.0));
    }

    #[test]
    fn functions_close_over_their_scope() {
        let src = "factor = 3\nfunction scale(n)\n  return n * factor\nend\nresult = scale(5)\n";
        assert!(matches!(eval_var(src, "result"), Value::Number(n) if n == 15.0));
    }

    #[test]
    fn function_without_return_yields_null() {
        let src = "function noop()\n  x = 1\nend\nresult = noop()\n";
        assert_eq!(eval_var(src, "result").display(), "null");
    }

    #[test]
    fn repeat_loop_runs_n_times() {
        assert!(
            matches!(eval_var("count = 0\nrepeat 5\n  count = count + 1\nend\n", "count"), Value::Number(n) if n == 5.0)
        );
        assert!(
            matches!(eval_var("count = 0\nrepeat 0\n  count = count + 1\nend\n", "count"), Value::Number(n) if n == 0.0)
        );
    }

    #[test]
    fn for_in_loop_over_array() {
        assert!(
            matches!(eval_var("total = 0\nfor n in [1, 2, 3]\n  total = total + n\nend\n", "total"), Value::Number(n) if n == 6.0)
        );
    }

    #[test]
    fn return_leaves_a_loop_and_the_function() {
        let src = "function first_big(items)\n  for n in items\n    if n > 2\n      return n\n    end\n  end\n  return 0\nend\nresult = first_big([1, 2, 5, 9])\n";
        assert!(matches!(eval_var(src, "result"), Value::Number(n) if n == 5.0));
    }

    #[test]
    fn imports_bind_standard_modules() {
        assert!(
            matches!(eval_var("import math\nresult = math.sqrt(25)\n", "result"), Value::Number(n) if n == 5.0)
        );
    }

    #[test]
    fn runaway_recursion_is_reported_not_crashed() {
        let err =
            run_err("function loop_forever()\n  return loop_forever()\nend\nx = loop_forever()\n");
        assert!(err.message.contains("Too many nested calls"));
    }

    #[test]
    fn errors_match_the_interpreter() {
        for src in [
            "x = 1 / 0\n",
            "x = 5 % 0\n",
            "age = 18\nsay(agee)\n",
            "people = [\"Alex\"]\nsay(people[5])\n",
            "x = \"a\" - 1\n",
            "x = 1 + true\n",
            "for x in 5\n  say(x)\nend\n",
            "repeat -1\n  say(1)\nend\n",
            "import maths\n",
            "person = 5\nperson.name = \"x\"\n",
            "x = 5\nsay(x.length())\n",
            "person = { name: \"Alex\" }\nsay(person.nme)\n",
            "function f(a)\n  return a\nend\nsay(f(1, 2))\n",
            "x = 5\nx()\n",
        ] {
            assert_matches_interpreter(src);
        }
    }

    #[test]
    fn runtime_errors_carry_a_source_position() {
        let err = run_err("x = 1\ny = x / 0\n");
        assert_eq!(err.span.map(|s| s.line), Some(2));
    }

    #[test]
    fn globals_persist_across_runs_for_the_repl() {
        let mut vm = Vm::new();
        for src in ["age = 18\n", "age = age + 1\n"] {
            let program = parse(tokenize(src).unwrap()).unwrap();
            vm.run(compile(&program).unwrap()).unwrap();
        }
        assert!(matches!(vm.globals.get("age"), Some(Value::Number(n)) if n == 19.0));
    }
}

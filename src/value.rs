use crate::ast::Stmt;
use crate::bytecode::FunctionProto;
use crate::environment::Environment;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

#[derive(Clone)]
pub enum Value {
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<BTreeMap<String, Value>>>),
    Function(Rc<FunctionValue>),
    /// A closure produced by the compiler backend and executed by the VM.
    Compiled(Rc<CompiledFunction>),
    Native(Rc<NativeFunction>),
}

pub struct FunctionValue {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
    pub closure: Rc<Environment>,
}

/// A compiled function bound to the scope it was created in.
pub struct CompiledFunction {
    pub proto: Rc<FunctionProto>,
    pub closure: Rc<Environment>,
}

pub struct NativeFunction {
    pub name: String,
    pub func: Box<dyn Fn(&[Value]) -> Result<Value, String>>,
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Bool(_) => "boolean",
            Value::Null => "null",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            Value::Function(_) => "function",
            Value::Compiled(_) => "function",
            Value::Native(_) => "function",
        }
    }

    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Null => false,
            Value::Number(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.borrow().is_empty(),
            _ => true,
        }
    }

    pub fn new_array(items: Vec<Value>) -> Value {
        Value::Array(Rc::new(RefCell::new(items)))
    }

    pub fn new_object(fields: BTreeMap<String, Value>) -> Value {
        Value::Object(Rc::new(RefCell::new(fields)))
    }

    pub fn display(&self) -> String {
        match self {
            Value::Number(n) => format_number(*n),
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_string(),
            Value::Array(a) => {
                let items: Vec<String> = a.borrow().iter().map(|v| v.debug_display()).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Object(o) => {
                let items: Vec<String> = o
                    .borrow()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.debug_display()))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
            Value::Function(f) => format!("<function {}>", f.name),
            Value::Compiled(f) => format!("<function {}>", f.proto.name),
            Value::Native(f) => format!("<native function {}>", f.name),
        }
    }

    /// Like display, but strings are quoted (used inside array/object printing).
    pub fn debug_display(&self) -> String {
        match self {
            Value::String(s) => format!("\"{s}\""),
            other => other.display(),
        }
    }

    pub fn equals(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            (Value::Compiled(a), Value::Compiled(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

pub fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display())
    }
}

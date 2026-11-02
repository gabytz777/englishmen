use crate::error::{EmError, Span};
use crate::value::{NativeFunction, Value};
use std::collections::BTreeMap;
use std::rc::Rc;

/// Signals used to unwind the call stack for `return`.
pub enum Signal {
    Normal,
    Return(Value),
}

pub fn native(name: &str, func: impl Fn(&[Value]) -> Result<Value, String> + 'static) -> Value {
    Value::Native(Rc::new(NativeFunction {
        name: name.to_string(),
        func: Box::new(func),
    }))
}

pub fn expect_number(value: &Value, context: &str) -> Result<f64, String> {
    match value {
        Value::Number(n) => Ok(*n),
        other => Err(format!(
            "Type mismatch: expected a number {context}, found {}",
            other.type_name()
        )),
    }
}

pub fn expect_string(value: &Value, context: &str) -> Result<String, String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        other => Err(format!(
            "Type mismatch: expected a string {context}, found {}",
            other.type_name()
        )),
    }
}

/// Builtin `say(...)`
pub fn say(args: &[Value]) {
    let parts: Vec<String> = args.iter().map(|v| v.display()).collect();
    println!("{}", parts.join(" "));
}

/// Call an array method like `.add(...)`, `.remove(...)`, `.length()`.
pub fn call_array_method(
    array: &Rc<std::cell::RefCell<Vec<Value>>>,
    method: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EmError> {
    match method {
        "add" | "push" => {
            let value = args.get(0).cloned().unwrap_or(Value::Null);
            array.borrow_mut().push(value);
            Ok(Value::Null)
        }
        "remove" => {
            if let Some(target) = args.get(0) {
                let mut arr = array.borrow_mut();
                if let Some(pos) = arr.iter().position(|v| v.equals(target)) {
                    arr.remove(pos);
                }
            }
            Ok(Value::Null)
        }
        "removeAt" => {
            let idx = args
                .get(0)
                .map(|v| expect_number(v, "as an index").unwrap_or(-1.0))
                .unwrap_or(-1.0) as i64;
            let mut arr = array.borrow_mut();
            if idx >= 0 && (idx as usize) < arr.len() {
                arr.remove(idx as usize);
            }
            Ok(Value::Null)
        }
        "length" => Ok(Value::Number(array.borrow().len() as f64)),
        "contains" => {
            let target = args.get(0).cloned().unwrap_or(Value::Null);
            Ok(Value::Bool(
                array.borrow().iter().any(|v| v.equals(&target)),
            ))
        }
        other => Err(method_error(
            "Array",
            other,
            &["add", "remove", "removeAt", "length", "contains"],
            span,
        )),
    }
}

pub fn call_object_method(
    object: &Rc<std::cell::RefCell<BTreeMap<String, Value>>>,
    method: &str,
    _args: &[Value],
    span: Span,
) -> Result<Value, EmError> {
    match method {
        "keys" => Ok(Value::new_array(
            object
                .borrow()
                .keys()
                .map(|k| Value::String(k.clone()))
                .collect(),
        )),
        "values" => Ok(Value::new_array(
            object.borrow().values().cloned().collect(),
        )),
        other => Err(method_error(
            "Object",
            other,
            &["keys", "values"],
            span,
        )),
    }
}

pub fn call_string_method(
    s: &str,
    method: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EmError> {
    match method {
        "length" => Ok(Value::Number(s.chars().count() as f64)),
        "upper" => Ok(Value::String(s.to_uppercase())),
        "lower" => Ok(Value::String(s.to_lowercase())),
        "trim" => Ok(Value::String(s.trim().to_string())),
        "split" => {
            let sep = args
                .get(0)
                .map(|v| expect_string(v, "as a separator").unwrap_or_default())
                .unwrap_or_default();
            let parts: Vec<Value> = if sep.is_empty() {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            } else {
                s.split(sep.as_str())
                    .map(|p| Value::String(p.to_string()))
                    .collect()
            };
            Ok(Value::new_array(parts))
        }
        "contains" => {
            let target = args
                .get(0)
                .map(|v| expect_string(v, "to search for").unwrap_or_default())
                .unwrap_or_default();
            Ok(Value::Bool(s.contains(target.as_str())))
        }
        other => Err(method_error(
            "String",
            other,
            &["length", "upper", "lower", "trim", "split", "contains"],
            span,
        )),
    }
}

fn method_error(kind: &str, method: &str, available: &[&str], span: Span) -> EmError {
    let candidates: Vec<String> = available.iter().map(|s| s.to_string()).collect();
    let mut err = EmError::type_error(format!("{kind} has no method '{method}'")).with_span(span);
    if let Some(hint) = crate::error::suggest_closest(method, candidates.iter()) {
        err = err.with_hint(format!("Did you mean '{hint}'?"));
    } else {
        err = err.with_hint(format!(
            "Available {} methods: {}.",
            kind.to_lowercase(),
            available.join(", ")
        ));
    }
    err
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn array_add_remove_length() {
        let arr = Rc::new(RefCell::new(vec![Value::String("a".to_string())]));
        call_array_method(
            &arr,
            "add",
            &[Value::String("b".to_string())],
            Span::default(),
        )
        .unwrap();
        assert_eq!(arr.borrow().len(), 2);
        call_array_method(
            &arr,
            "remove",
            &[Value::String("a".to_string())],
            Span::default(),
        )
        .unwrap();
        assert_eq!(arr.borrow().len(), 1);
        let len = call_array_method(&arr, "length", &[], Span::default()).unwrap();
        assert!(matches!(len, Value::Number(n) if n == 1.0));
    }

    #[test]
    fn array_unknown_method_errors() {
        let arr = Rc::new(RefCell::new(vec![]));
        let err = call_array_method(&arr, "bogus", &[], Span::default()).unwrap_err();
        assert!(err.message.contains("bogus"));
    }

    #[test]
    fn string_methods() {
        let v = call_string_method("Hello", "upper", &[], Span::default()).unwrap();
        assert_eq!(v.display(), "HELLO");
        let v = call_string_method("Hello", "length", &[], Span::default()).unwrap();
        assert!(matches!(v, Value::Number(n) if n == 5.0));
    }
}

//! Small helpers shared by the standard-library modules.

use crate::runtime::{expect_number, expect_string};
use crate::value::Value;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

/// Get an array argument, producing the same type-error wording as the
/// built-in `expect_*` helpers.
pub fn expect_array(value: &Value, context: &str) -> Result<Rc<RefCell<Vec<Value>>>, String> {
    match value {
        Value::Array(items) => Ok(Rc::clone(items)),
        other => Err(format!(
            "Type mismatch: expected an array {context}, found {}",
            other.type_name()
        )),
    }
}

/// Take a snapshot of an array argument so nested calls cannot fight over a
/// `RefCell` borrow.
pub fn array_items(value: &Value, context: &str) -> Result<Vec<Value>, String> {
    Ok(expect_array(value, context)?.borrow().clone())
}

/// Convert a number argument to a non-negative integer index.
pub fn as_usize(value: &Value, context: &str) -> Result<usize, String> {
    let n = expect_number(value, context)?;
    if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > usize::MAX as f64 {
        return Err(format!("Expected a non-negative whole number {context}, found {n}"));
    }
    Ok(n as usize)
}

/// Structural equality for values, including arrays and objects. `Value::equals`
/// deliberately uses identity for containers, which is not what collection
/// helpers such as `unique` want.
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            let a = a.borrow();
            let b = b.borrow();
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }
        (Value::Object(a), Value::Object(b)) => {
            let a = a.borrow();
            let b = b.borrow();
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key)
                        .is_some_and(|other| values_equal(value, other))
                })
        }
        _ => a.equals(b),
    }
}

fn type_rank(value: &Value) -> u8 {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
        Value::Function(_) | Value::Compiled(_) | Value::Native(_) => 6,
    }
}

/// A deterministic total order used by `collections.sorted`. Values of
/// different kinds are grouped by kind rather than rejected, so mixed arrays
/// still sort without surprising errors.
pub fn compare_values(a: &Value, b: &Value) -> Ordering {
    let rank = type_rank(a).cmp(&type_rank(b));
    if rank != Ordering::Equal {
        return rank;
    }
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.total_cmp(b),
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Array(a), Value::Array(b)) => {
            let a = a.borrow();
            let b = b.borrow();
            for (x, y) in a.iter().zip(b.iter()) {
                let order = compare_values(x, y);
                if order != Ordering::Equal {
                    return order;
                }
            }
            a.len().cmp(&b.len())
        }
        (Value::Object(a), Value::Object(b)) => {
            let a = a.borrow();
            let b = b.borrow();
            let keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            let mut keys = keys;
            keys.sort();
            keys.dedup();
            for key in keys {
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => {
                        let order = compare_values(x, y);
                        if order != Ordering::Equal {
                            return order;
                        }
                    }
                    (Some(_), None) => return Ordering::Greater,
                    (None, Some(_)) => return Ordering::Less,
                    (None, None) => {}
                }
            }
            Ordering::Equal
        }
        _ => Ordering::Equal,
    }
}

/// Read a path-like string argument (used by the `fs` module).
pub fn expect_path(value: &Value, context: &str) -> Result<std::path::PathBuf, String> {
    Ok(std::path::PathBuf::from(expect_string(
        value, context,
    )?))
}

#[cfg(test)]
pub fn call_native(module: &Value, name: &str, args: &[Value]) -> Result<Value, String> {
    let Value::Object(fields) = module else {
        panic!("module is not an object");
    };
    let Some(func) = fields.borrow().get(name).cloned() else {
        panic!("module has no function named {name}");
    };
    let Value::Native(native) = func else {
        panic!("{name} is not a native function");
    };
    (native.func)(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_usize_rejects_bad_values() {
        assert!(as_usize(&Value::Number(-1.0), "for index").is_err());
        assert!(as_usize(&Value::Number(1.5), "for index").is_err());
        assert!(as_usize(&Value::String("2".to_string()), "for index").is_err());
        assert_eq!(as_usize(&Value::Number(3.0), "for index").unwrap(), 3);
    }

    #[test]
    fn structural_equality_and_sorting() {
        let a = Value::new_array(vec![Value::Number(1.0)]);
        let b = Value::new_array(vec![Value::Number(1.0)]);
        assert!(values_equal(&a, &b));
        assert!(!a.equals(&b));
        assert_eq!(
            compare_values(&Value::Number(2.0), &Value::Number(10.0)),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::String("b".to_string()), &Value::Number(1.0)),
            Ordering::Greater
        );
    }
}

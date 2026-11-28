use crate::error::{EmError, Span};
use crate::value::{NativeFunction, Value};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Signals used to unwind the call stack for `return`.
pub enum Signal {
    Normal,
    Return(Value),
    /// Unwind to the innermost loop and leave it.
    Break,
    /// Unwind to the innermost loop and start the next iteration.
    Continue,
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

// Keep the "available methods" hints in one place per receiver kind. Both the
// dispatch functions and the unknown-method errors use these lists.
const ARRAY_METHODS: &[&str] = &[
    "add", "push", "remove", "removeAt", "length", "contains", "pop", "shift", "unshift",
    "insert", "clear", "reverse", "sort", "slice", "join", "indexOf", "lastIndexOf", "count",
    "first", "last", "concat", "copy", "isEmpty", "swap", "unique", "flatten", "rotate",
    "indexesOf", "interleave",
];
const OBJECT_METHODS: &[&str] = &[
    "keys", "values", "has", "get", "set", "delete", "length", "merge", "copy", "isEmpty", "pick",
    "omit", "entries",
];
const STRING_METHODS: &[&str] = &[
    "length", "upper", "lower", "trim", "split", "contains", "startsWith", "endsWith", "replace",
    "replaceAll", "slice", "substring", "indexOf", "lastIndexOf", "charAt", "toNumber",
    "capitalize", "titleCase", "reverse", "repeat", "padLeft", "padRight", "isDigit", "isAlpha",
    "lines", "chars", "swapCase", "isBlank", "truncate",
];
const NUMBER_METHODS: &[&str] = &[
    "isEven", "isOdd", "isInteger", "isFinite", "toFixed", "clamp", "toHex", "toBinary", "sign",
    "abs", "floor", "ceil", "round", "sqrt", "toOctal", "toDegrees", "reciprocal",
];
const BOOL_METHODS: &[&str] = &[
    "toggle", "toString", "and", "or", "xor", "implies", "nand",
];

/// Turn a type mismatch from [`expect_number`] into a spanned error.
fn number_arg(
    args: &[Value],
    index: usize,
    method: &str,
    context: &str,
    span: Span,
) -> Result<f64, EmError> {
    match args.get(index) {
        Some(value) => expect_number(value, context)
            .map_err(|message| EmError::type_error(format!("{method}(): {message}")).with_span(span)),
        None => Err(EmError::arity(format!(
            "{method}() expects argument {} ({context})",
            index + 1
        ))
        .with_span(span)),
    }
}

/// Like [`number_arg`], but fall back to `default` when the argument is absent.
fn opt_number_arg(
    args: &[Value],
    index: usize,
    method: &str,
    context: &str,
    default: f64,
    span: Span,
) -> Result<f64, EmError> {
    match args.get(index) {
        Some(_) => number_arg(args, index, method, context, span),
        None => Ok(default),
    }
}

fn string_arg(
    args: &[Value],
    index: usize,
    method: &str,
    context: &str,
    span: Span,
) -> Result<String, EmError> {
    match args.get(index) {
        Some(value) => expect_string(value, context)
            .map_err(|message| EmError::type_error(format!("{method}(): {message}")).with_span(span)),
        None => Err(EmError::arity(format!(
            "{method}() expects argument {} ({context})",
            index + 1
        ))
        .with_span(span)),
    }
}

fn opt_string_arg(
    args: &[Value],
    index: usize,
    method: &str,
    context: &str,
    default: &str,
    span: Span,
) -> Result<String, EmError> {
    match args.get(index) {
        Some(_) => string_arg(args, index, method, context, span),
        None => Ok(default.to_string()),
    }
}

fn bool_arg(
    args: &[Value],
    index: usize,
    method: &str,
    context: &str,
    span: Span,
) -> Result<bool, EmError> {
    match args.get(index) {
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => Err(EmError::type_error(format!(
            "{method}(): expected a boolean {context}, found {}",
            other.type_name()
        ))
        .with_span(span)),
        None => Err(EmError::arity(format!(
            "{method}() expects argument {} ({context})",
            index + 1
        ))
        .with_span(span)),
    }
}

/// Resolve a possibly negative index against a length, clamping it into range.
/// Used by slice-like methods where out-of-range indices are not errors.
fn clamp_index(len: usize, n: f64) -> usize {
    if !n.is_finite() {
        return if n.is_sign_negative() { 0 } else { len };
    }
    if n < 0.0 {
        len.saturating_sub((-n) as usize)
    } else {
        (n as usize).min(len)
    }
}

/// Resolve an index for element access: negative values count from the end,
/// fractional or out-of-range values point at no element at all.
fn exact_index(len: usize, n: f64) -> Option<usize> {
    if !n.is_finite() || n.fract() != 0.0 {
        return None;
    }
    if n < 0.0 {
        len.checked_sub((-n) as usize)
    } else {
        let idx = n as usize;
        if idx < len { Some(idx) } else { None }
    }
}

fn slice_range(len: usize, start: f64, end: f64) -> (usize, usize) {
    let from = clamp_index(len, start);
    let to = clamp_index(len, end);
    if to < from { (from, from) } else { (from, to) }
}

fn array_items(array: &Rc<RefCell<Vec<Value>>>) -> Vec<Value> {
    array.borrow().clone()
}

fn compare_numbers(a: &f64, b: &f64) -> Ordering {
    match a.partial_cmp(b) {
        Some(ordering) => ordering,
        None => Ordering::Equal,
    }
}

/// Flatten nested arrays into `out`, skipping circular references so a
/// self-containing array cannot recurse forever.
fn flatten_into(values: &[Value], out: &mut Vec<Value>, seen: &mut Vec<usize>) {
    for value in values {
        if let Value::Array(inner) = value {
            let ptr = Rc::as_ptr(inner) as usize;
            if seen.contains(&ptr) {
                continue;
            }
            seen.push(ptr);
            let nested = inner.borrow().clone();
            flatten_into(&nested, out, seen);
            seen.pop();
        } else {
            out.push(value.clone());
        }
    }
}

/// Call an array method like `.add(...)`, `.remove(...)`, `.length()`.
pub fn call_array_method(
    array: &Rc<RefCell<Vec<Value>>>,
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
        "pop" => {
            let mut arr = array.borrow_mut();
            Ok(match arr.pop() {
                Some(value) => value,
                None => Value::Null,
            })
        }
        "shift" => {
            let mut arr = array.borrow_mut();
            Ok(if arr.is_empty() {
                Value::Null
            } else {
                arr.remove(0)
            })
        }
        "unshift" => {
            let mut arr = array.borrow_mut();
            for (offset, value) in args.iter().enumerate() {
                arr.insert(offset, value.clone());
            }
            Ok(Value::Number(arr.len() as f64))
        }
        "insert" => {
            let index = number_arg(args, 0, "insert", "as an index", span)?;
            let value = args.get(1).cloned().ok_or_else(|| {
                EmError::new("insert() needs a value to insert").with_span(span)
            })?;
            let mut arr = array.borrow_mut();
            let pos = clamp_index(arr.len(), index);
            arr.insert(pos, value);
            Ok(Value::Null)
        }
        "clear" => {
            array.borrow_mut().clear();
            Ok(Value::Null)
        }
        "reverse" => {
            array.borrow_mut().reverse();
            Ok(Value::Array(Rc::clone(array)))
        }
        "sort" => {
            let mut items = array_items(array);
            let numeric = items.iter().all(|v| matches!(v, Value::Number(_)));
            items.sort_by(|a, b| match (a, b) {
                (Value::Number(x), Value::Number(y)) if numeric => compare_numbers(x, y),
                (Value::Number(_), _) if numeric => Ordering::Less,
                (_, Value::Number(_)) if numeric => Ordering::Greater,
                _ => a.display().cmp(&b.display()),
            });
            *array.borrow_mut() = items;
            Ok(Value::Array(Rc::clone(array)))
        }
        "slice" => {
            let len = array.borrow().len();
            let start = opt_number_arg(args, 0, "slice", "as a start index", 0.0, span)?;
            let end = opt_number_arg(args, 1, "slice", "as an end index", len as f64, span)?;
            let (from, to) = slice_range(len, start, end);
            Ok(Value::new_array(
                array.borrow()[from..to].to_vec(),
            ))
        }
        "join" => {
            let separator = opt_string_arg(args, 0, "join", "as a separator", ",", span)?;
            let joined = array
                .borrow()
                .iter()
                .map(|v| v.display())
                .collect::<Vec<String>>()
                .join(&separator);
            Ok(Value::String(joined))
        }
        "indexOf" | "lastIndexOf" => {
            let target = match args.get(0) {
                Some(value) => value.clone(),
                None => Value::Null,
            };
            let arr = array.borrow();
            let pos = if method == "indexOf" {
                arr.iter().position(|v| v.equals(&target))
            } else {
                arr.iter().rposition(|v| v.equals(&target))
            };
            Ok(Value::Number(match pos {
                Some(index) => index as f64,
                None => -1.0,
            }))
        }
        "count" => {
            let target = match args.get(0) {
                Some(value) => value.clone(),
                None => Value::Null,
            };
            let count = array
                .borrow()
                .iter()
                .filter(|v| v.equals(&target))
                .count();
            Ok(Value::Number(count as f64))
        }
        "first" => Ok(match array.borrow().first() {
            Some(value) => value.clone(),
            None => Value::Null,
        }),
        "last" => Ok(match array.borrow().last() {
            Some(value) => value.clone(),
            None => Value::Null,
        }),
        "concat" => {
            let mut result = array_items(array);
            for value in args {
                match value {
                    Value::Array(other) => result.extend(other.borrow().iter().cloned()),
                    other => result.push(other.clone()),
                }
            }
            Ok(Value::new_array(result))
        }
        "copy" => Ok(Value::new_array(array_items(array))),
        "isEmpty" => Ok(Value::Bool(array.borrow().is_empty())),
        "swap" => {
            let left = number_arg(args, 0, "swap", "as the first index", span)?;
            let right = number_arg(args, 1, "swap", "as the second index", span)?;
            let mut arr = array.borrow_mut();
            let len = arr.len();
            match (exact_index(len, left), exact_index(len, right)) {
                (Some(a), Some(b)) => {
                    arr.swap(a, b);
                    Ok(Value::Bool(true))
                }
                _ => Ok(Value::Bool(false)),
            }
        }
        "unique" => {
            let mut items = array_items(array);
            let mut seen: Vec<Value> = Vec::new();
            items.retain(|value| {
                if seen.iter().any(|v| v.equals(value)) {
                    false
                } else {
                    seen.push(value.clone());
                    true
                }
            });
            *array.borrow_mut() = items;
            Ok(Value::Array(Rc::clone(array)))
        }
        "flatten" => {
            let items = array_items(array);
            let mut flat = Vec::new();
            let mut seen = vec![Rc::as_ptr(array) as usize];
            flatten_into(&items, &mut flat, &mut seen);
            *array.borrow_mut() = flat;
            Ok(Value::Array(Rc::clone(array)))
        }
        "rotate" => {
            // Positive shifts move elements towards the tail; negative shifts
            // move them towards the head. The receiver is left untouched.
            let amount = number_arg(args, 0, "rotate", "as a number of positions", span)?;
            if !amount.is_finite() || amount.fract() != 0.0 {
                return Err(EmError::type_error(
                    "rotate() expects a whole number of positions",
                )
                .with_span(span));
            }
            let items = array_items(array);
            if items.is_empty() {
                return Ok(Value::new_array(Vec::new()));
            }
            let len = items.len();
            let shift = (amount as i64).rem_euclid(len as i64) as usize;
            let split = len - shift;
            let mut rotated = Vec::with_capacity(len);
            rotated.extend_from_slice(&items[split..]);
            rotated.extend_from_slice(&items[..split]);
            Ok(Value::new_array(rotated))
        }
        "indexesOf" => {
            let target = args.get(0).cloned().unwrap_or(Value::Null);
            let indexes = array
                .borrow()
                .iter()
                .enumerate()
                .filter(|(_, value)| value.equals(&target))
                .map(|(index, _)| Value::Number(index as f64))
                .collect();
            Ok(Value::new_array(indexes))
        }
        "interleave" => {
            let other = match args.get(0) {
                Some(Value::Array(other)) => other.borrow().clone(),
                Some(value) => {
                    return Err(EmError::type_error(format!(
                        "interleave(): expected an array, found {}",
                        value.type_name()
                    ))
                    .with_span(span));
                }
                None => {
                    return Err(
                        EmError::arity("interleave() expects an array to interleave")
                            .with_span(span),
                    );
                }
            };
            let left = array_items(array);
            let mut result = Vec::with_capacity(left.len() + other.len());
            for index in 0..left.len().max(other.len()) {
                if let Some(value) = left.get(index) {
                    result.push(value.clone());
                }
                if let Some(value) = other.get(index) {
                    result.push(value.clone());
                }
            }
            Ok(Value::new_array(result))
        }
        other => Err(method_error("Array", other, ARRAY_METHODS, span)),
    }
}

pub fn call_object_method(
    object: &Rc<RefCell<BTreeMap<String, Value>>>,
    method: &str,
    args: &[Value],
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
        "has" => {
            let key = string_arg(args, 0, "has", "as a key", span)?;
            Ok(Value::Bool(object.borrow().contains_key(&key)))
        }
        "get" => {
            let key = string_arg(args, 0, "get", "as a key", span)?;
            let found = object.borrow().get(&key).cloned();
            Ok(match found {
                Some(value) => value,
                None => match args.get(1) {
                    Some(default) => default.clone(),
                    None => Value::Null,
                },
            })
        }
        "set" => {
            let key = string_arg(args, 0, "set", "as a key", span)?;
            let value = match args.get(1) {
                Some(value) => value.clone(),
                None => Value::Null,
            };
            object.borrow_mut().insert(key, value);
            Ok(Value::Null)
        }
        "delete" => {
            let key = string_arg(args, 0, "delete", "as a key", span)?;
            Ok(Value::Bool(object.borrow_mut().remove(&key).is_some()))
        }
        "length" => Ok(Value::Number(object.borrow().len() as f64)),
        "merge" => {
            let other = match args.get(0) {
                Some(Value::Object(other)) => Rc::clone(other),
                Some(value) => {
                    return Err(EmError::type_error(format!(
                        "merge(): expected an object to merge, found {}",
                        value.type_name()
                    ))
                    .with_span(span));
                }
                None => {
                    return Err(EmError::arity("merge() expects an object to merge").with_span(span));
                }
            };
            // Copy first: merging an object into itself must not hold an
            // immutable and a mutable borrow of the same RefCell at once.
            let fields: Vec<(String, Value)> = other
                .borrow()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            let mut target = object.borrow_mut();
            for (key, value) in fields {
                target.insert(key, value);
            }
            Ok(Value::Object(Rc::clone(object)))
        }
        "copy" => Ok(Value::new_object(object.borrow().clone())),
        "isEmpty" => Ok(Value::Bool(object.borrow().is_empty())),
        "pick" => {
            let source = object.borrow();
            let mut fields = BTreeMap::new();
            for value in args {
                let key = expect_string(value, "as a key").map_err(|message| {
                    EmError::type_error(format!("pick(): {message}")).with_span(span)
                })?;
                if let Some(value) = source.get(&key) {
                    fields.insert(key, value.clone());
                }
            }
            Ok(Value::new_object(fields))
        }
        "omit" => {
            let mut fields = object.borrow().clone();
            for value in args {
                let key = expect_string(value, "as a key").map_err(|message| {
                    EmError::type_error(format!("omit(): {message}")).with_span(span)
                })?;
                fields.remove(&key);
            }
            Ok(Value::new_object(fields))
        }
        "entries" => Ok(Value::new_array(
            object
                .borrow()
                .iter()
                .map(|(key, value)| {
                    Value::new_array(vec![Value::String(key.clone()), value.clone()])
                })
                .collect(),
        )),
        other => Err(method_error("Object", other, OBJECT_METHODS, span)),
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
        "startsWith" => {
            let prefix = string_arg(args, 0, "startsWith", "as a prefix", span)?;
            Ok(Value::Bool(s.starts_with(&prefix)))
        }
        "endsWith" => {
            let suffix = string_arg(args, 0, "endsWith", "as a suffix", span)?;
            Ok(Value::Bool(s.ends_with(&suffix)))
        }
        "replace" => {
            let old = string_arg(args, 0, "replace", "to replace", span)?;
            let new = string_arg(args, 1, "replace", "as a replacement", span)?;
            Ok(Value::String(s.replacen(&old, &new, 1)))
        }
        "replaceAll" => {
            let old = string_arg(args, 0, "replaceAll", "to replace", span)?;
            let new = string_arg(args, 1, "replaceAll", "as a replacement", span)?;
            Ok(Value::String(s.replace(&old, &new)))
        }
        "slice" => {
            let len = s.chars().count();
            let start = opt_number_arg(args, 0, "slice", "as a start index", 0.0, span)?;
            let end = opt_number_arg(args, 1, "slice", "as an end index", len as f64, span)?;
            let (from, to) = slice_range(len, start, end);
            Ok(Value::String(s.chars().skip(from).take(to - from).collect()))
        }
        "substring" => {
            let len = s.chars().count();
            let start = opt_number_arg(args, 0, "substring", "as a start index", 0.0, span)?;
            let end = opt_number_arg(args, 1, "substring", "as an end index", len as f64, span)?;
            let mut from = clamp_index(len, start);
            let mut to = clamp_index(len, end);
            if to < from {
                std::mem::swap(&mut from, &mut to);
            }
            Ok(Value::String(s.chars().skip(from).take(to - from).collect()))
        }
        "indexOf" | "lastIndexOf" => {
            let needle = string_arg(args, 0, method, "to search for", span)?;
            let found = if method == "indexOf" {
                s.find(&needle)
            } else {
                s.rfind(&needle)
            };
            let index = match found {
                Some(byte_pos) => s[..byte_pos].chars().count() as f64,
                None => -1.0,
            };
            Ok(Value::Number(index))
        }
        "charAt" => {
            let index = number_arg(args, 0, "charAt", "as a character index", span)?;
            let chars: Vec<char> = s.chars().collect();
            Ok(Value::String(match exact_index(chars.len(), index) {
                Some(pos) => chars[pos].to_string(),
                None => String::new(),
            }))
        }
        "toNumber" => {
            let trimmed = s.trim();
            Ok(match trimmed.parse::<f64>() {
                // Rust accepts `NaN`, `inf` and `infinity`; the language only
                // wants finite numeric text, so reject those spellings.
                Ok(number) if number.is_finite() => Value::Number(number),
                _ => Value::Null,
            })
        }
        "capitalize" => {
            let mut chars = s.chars();
            Ok(Value::String(match chars.next() {
                Some(first) => {
                    let mut out: String = first.to_uppercase().collect();
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }))
        }
        "titleCase" => {
            let mut out = String::new();
            let mut start_of_word = true;
            for c in s.chars() {
                if c.is_whitespace() {
                    out.push(c);
                    start_of_word = true;
                } else if start_of_word {
                    out.extend(c.to_uppercase());
                    start_of_word = false;
                } else {
                    out.extend(c.to_lowercase());
                }
            }
            Ok(Value::String(out))
        }
        "reverse" => Ok(Value::String(s.chars().rev().collect())),
        "repeat" => {
            let count = number_arg(args, 0, "repeat", "as a repeat count", span)?;
            if count < 0.0 {
                return Ok(Value::String(String::new()));
            }
            if !count.is_finite() || count.fract() != 0.0 {
                return Err(EmError::type_error(
                    "repeat() expects a whole number of repetitions",
                )
                .with_span(span));
            }
            let times = count as usize;
            let chars = s.chars().count().saturating_mul(times);
            if chars > 1_000_000 {
                return Err(EmError::new("repeat() result would be too large").with_span(span));
            }
            Ok(Value::String(s.repeat(times)))
        }
        "padLeft" | "padRight" => {
            let width = number_arg(args, 0, method, "as a target width", span)?;
            if !width.is_finite() || width.fract() != 0.0 || width < 0.0 {
                return Err(EmError::type_error(format!(
                    "{method}() expects a non-negative whole width"
                ))
                .with_span(span));
            }
            let width = (width as usize).min(100_000);
            let pad = opt_string_arg(args, 1, method, "as padding", " ", span)?;
            let missing = width.saturating_sub(s.chars().count());
            let mut padding = String::new();
            let unit = if pad.is_empty() { " ".to_string() } else { pad };
            while padding.chars().count() < missing {
                padding.push_str(&unit);
            }
            let padding: String = padding.chars().take(missing).collect();
            Ok(Value::String(if method == "padLeft" {
                format!("{padding}{s}")
            } else {
                format!("{s}{padding}")
            }))
        }
        "isDigit" => Ok(Value::Bool(
            !s.is_empty() && s.chars().all(|c| c.is_numeric()),
        )),
        "isAlpha" => Ok(Value::Bool(
            !s.is_empty() && s.chars().all(|c| c.is_alphabetic()),
        )),
        "lines" => Ok(Value::new_array(
            s.lines().map(|line| Value::String(line.to_string())).collect(),
        )),
        "chars" => Ok(Value::new_array(
            s.chars().map(|c| Value::String(c.to_string())).collect(),
        )),
        "swapCase" => {
            let mut out = String::new();
            for c in s.chars() {
                if c.is_uppercase() {
                    out.extend(c.to_lowercase());
                } else if c.is_lowercase() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
            }
            Ok(Value::String(out))
        }
        "isBlank" => Ok(Value::Bool(s.chars().all(char::is_whitespace))),
        "truncate" => {
            let width = number_arg(args, 0, "truncate", "as a maximum width", span)?;
            if !width.is_finite() || width.fract() != 0.0 || width < 0.0 {
                return Err(EmError::type_error(
                    "truncate() expects a non-negative whole width",
                )
                .with_span(span));
            }
            let width = (width as usize).min(100_000);
            let suffix = opt_string_arg(args, 1, "truncate", "as a suffix", "...", span)?;
            let chars: Vec<char> = s.chars().collect();
            if chars.len() <= width {
                return Ok(Value::String(s.to_string()));
            }
            let suffix: Vec<char> = suffix.chars().collect();
            let kept = width.saturating_sub(suffix.len());
            let mut out: String = chars.iter().take(kept).collect();
            out.extend(suffix.iter().take(width - kept));
            Ok(Value::String(out))
        }
        other => Err(method_error("String", other, STRING_METHODS, span)),
    }
}

/// Call a method on a number. More methods are registered in this function as
/// the standard library grows; unknown names get the usual helpful error.
pub fn call_number_method(
    n: f64,
    method: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EmError> {
    match method {
        "isEven" => Ok(Value::Bool(
            n.is_finite() && n.fract() == 0.0 && n % 2.0 == 0.0,
        )),
        "isOdd" => Ok(Value::Bool(
            n.is_finite() && n.fract() == 0.0 && n % 2.0 != 0.0,
        )),
        "isInteger" => Ok(Value::Bool(n.is_finite() && n.fract() == 0.0)),
        "isFinite" => Ok(Value::Bool(n.is_finite())),
        "toFixed" => {
            let digits = opt_number_arg(args, 0, "toFixed", "as decimal places", 0.0, span)?;
            if !digits.is_finite() || digits.fract() != 0.0 || digits < 0.0 || digits > 10.0 {
                return Err(EmError::type_error(
                    "toFixed() expects a whole number of digits between 0 and 10",
                )
                .with_span(span));
            }
            Ok(Value::String(format!("{:.*}", digits as usize, n)))
        }
        "clamp" => {
            let min = number_arg(args, 0, "clamp", "as a minimum", span)?;
            let max = number_arg(args, 1, "clamp", "as a maximum", span)?;
            if !n.is_finite() || !min.is_finite() || !max.is_finite() {
                return Err(EmError::new("clamp() needs finite numbers").with_span(span));
            }
            if min > max {
                return Err(EmError::new("clamp() expects minimum <= maximum").with_span(span));
            }
            Ok(Value::Number(n.max(min).min(max)))
        }
        "toHex" | "toBinary" | "toOctal" => {
            let magnitude = n.trunc().abs();
            // `as i64` saturates for out-of-range or non-finite values, which
            // would silently produce a plausible but wrong string.
            if !n.is_finite() || magnitude >= 9_223_372_036_854_775_808.0 {
                return Err(EmError::new(
                    "toHex/toBinary/toOctal need a finite integer within range",
                )
                .with_span(span));
            }
            let negative = n < 0.0;
            let magnitude = magnitude as i64;
            let digits = match method {
                "toHex" => format!("{magnitude:x}"),
                "toBinary" => format!("{magnitude:b}"),
                _ => format!("{magnitude:o}"),
            };
            Ok(Value::String(if negative && magnitude != 0 {
                format!("-{digits}")
            } else {
                digits
            }))
        }
        "sign" => Ok(Value::Number(if n > 0.0 {
            1.0
        } else if n < 0.0 {
            -1.0
        } else {
            0.0
        })),
        "abs" => Ok(Value::Number(n.abs())),
        "floor" => Ok(Value::Number(n.floor())),
        "ceil" => Ok(Value::Number(n.ceil())),
        "round" => Ok(Value::Number(n.round())),
        "sqrt" => {
            if n < 0.0 {
                Err(EmError::new("sqrt() expects a non-negative number").with_span(span))
            } else {
                Ok(Value::Number(n.sqrt()))
            }
        }
        "toDegrees" => Ok(Value::Number(n * 180.0 / std::f64::consts::PI)),
        "reciprocal" => {
            if n == 0.0 {
                Err(EmError::new("reciprocal() is undefined for zero").with_span(span))
            } else {
                Ok(Value::Number(1.0 / n))
            }
        }
        other => Err(method_error("Number", other, NUMBER_METHODS, span)),
    }
}

/// Call a method on a boolean value.
pub fn call_bool_method(
    b: bool,
    method: &str,
    args: &[Value],
    span: Span,
) -> Result<Value, EmError> {
    match method {
        "toggle" => Ok(Value::Bool(!b)),
        "toString" => Ok(Value::String(b.to_string())),
        "and" => Ok(Value::Bool(b && bool_arg(args, 0, "and", "an operand", span)?)),
        "or" => Ok(Value::Bool(b || bool_arg(args, 0, "or", "an operand", span)?)),
        "xor" => Ok(Value::Bool(b ^ bool_arg(args, 0, "xor", "an operand", span)?)),
        // Validate the operand even when the receiver alone decides the
        // result, so a bad argument never slips through a short circuit.
        "implies" => {
            let other = bool_arg(args, 0, "implies", "an operand", span)?;
            Ok(Value::Bool(!b || other))
        }
        "nand" => {
            let other = bool_arg(args, 0, "nand", "an operand", span)?;
            Ok(Value::Bool(!(b && other)))
        }
        other => Err(method_error("Boolean", other, BOOL_METHODS, span)),
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
    use crate::error::ErrorKind;

    fn arr(values: Vec<Value>) -> Rc<RefCell<Vec<Value>>> {
        Rc::new(RefCell::new(values))
    }

    fn num(n: f64) -> Value {
        Value::Number(n)
    }

    fn text(s: &str) -> Value {
        Value::String(s.to_string())
    }

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

    #[test]
    fn array_stack_and_insert_operations() {
        let a = arr(vec![num(1.0), num(2.0), num(3.0)]);
        let popped = call_array_method(&a, "pop", &[], Span::default()).unwrap();
        assert_eq!(popped.display(), "3");
        let shifted = call_array_method(&a, "shift", &[], Span::default()).unwrap();
        assert_eq!(shifted.display(), "1");
        let len = call_array_method(&a, "unshift", &[num(9.0), num(8.0)], Span::default())
            .unwrap();
        assert_eq!(len.display(), "3");
        assert_eq!(a.borrow().iter().map(Value::display).collect::<Vec<_>>(), ["9", "8", "2"]);
        call_array_method(&a, "insert", &[num(1.0), text("x")], Span::default()).unwrap();
        assert_eq!(a.borrow().iter().map(Value::display).collect::<Vec<_>>(), ["9", "x", "8", "2"]);
        call_array_method(&a, "clear", &[], Span::default()).unwrap();
        assert!(a.borrow().is_empty());
        assert_eq!(
            call_array_method(&a, "pop", &[], Span::default())
                .unwrap()
                .display(),
            "null"
        );
        assert_eq!(
            call_array_method(&a, "shift", &[], Span::default())
                .unwrap()
                .display(),
            "null"
        );
    }

    #[test]
    fn array_lookup_and_slice_operations() {
        let a = arr(vec![text("a"), text("b"), text("c"), text("b")]);
        let slice = call_array_method(&a, "slice", &[num(1.0), num(3.0)], Span::default())
            .unwrap();
        assert_eq!(slice.display(), "[\"b\", \"c\"]");
        let negative = call_array_method(&a, "slice", &[num(-2.0)], Span::default()).unwrap();
        assert_eq!(negative.display(), "[\"c\", \"b\"]");
        let joined = call_array_method(&a, "join", &[text("-")], Span::default()).unwrap();
        assert_eq!(joined.display(), "a-b-c-b");
        assert_eq!(
            call_array_method(&a, "indexOf", &[text("b")], Span::default())
                .unwrap()
                .display(),
            "1"
        );
        assert_eq!(
            call_array_method(&a, "lastIndexOf", &[text("b")], Span::default())
                .unwrap()
                .display(),
            "3"
        );
        assert_eq!(
            call_array_method(&a, "count", &[text("b")], Span::default())
                .unwrap()
                .display(),
            "2"
        );
        assert_eq!(
            call_array_method(&a, "first", &[], Span::default()).unwrap().display(),
            "a"
        );
        assert_eq!(
            call_array_method(&a, "last", &[], Span::default()).unwrap().display(),
            "b"
        );
        assert_eq!(
            call_array_method(&a, "isEmpty", &[], Span::default())
                .unwrap()
                .display(),
            "false"
        );
    }

    #[test]
    fn array_transform_operations() {
        let a = arr(vec![num(3.0), num(1.0), num(2.0)]);
        let sorted = call_array_method(&a, "sort", &[], Span::default()).unwrap();
        assert_eq!(sorted.display(), "[1, 2, 3]");
        let reversed = call_array_method(&a, "reverse", &[], Span::default()).unwrap();
        assert_eq!(reversed.display(), "[3, 2, 1]");
        assert_eq!(
            call_array_method(&a, "swap", &[num(0.0), num(2.0)], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(a.borrow()[0].display(), "1");
        assert_eq!(
            call_array_method(&a, "swap", &[num(0.0), num(99.0)], Span::default())
                .unwrap()
                .display(),
            "false"
        );

        let words = arr(vec![text("b"), text("a"), text("b")]);
        let unique = call_array_method(&words, "unique", &[], Span::default()).unwrap();
        assert_eq!(unique.display(), "[\"b\", \"a\"]");

        let nested = Value::new_array(vec![num(3.0), Value::new_array(vec![num(1.0), num(2.0)])]);
        let mixed = arr(vec![num(0.0), nested, num(4.0)]);
        let flat = call_array_method(&mixed, "flatten", &[], Span::default()).unwrap();
        assert_eq!(flat.display(), "[0, 3, 1, 2, 4]");

        let copy = call_array_method(&a, "copy", &[], Span::default()).unwrap();
        assert_eq!(copy.display(), Value::Array(Rc::clone(&a)).display());
        let combined = call_array_method(&a, "concat", &[num(7.0), copy], Span::default()).unwrap();
        assert_eq!(combined.display(), "[1, 2, 3, 7, 1, 2, 3]");
    }

    #[test]
    fn array_argument_type_errors() {
        let a = arr(vec![num(1.0)]);
        let err = call_array_method(&a, "insert", &[text("nope")], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        assert!(err.message.contains("insert"));
        let err = call_array_method(&a, "join", &[num(3.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_array_method(&a, "swap", &[num(0.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Arity);
        let err = call_array_method(&a, "insert", &[num(0.0)], Span::default()).unwrap_err();
        assert!(err.message.contains("insert needs a value") || err.message.contains("needs a value"));
    }

    #[test]
    fn string_case_and_affix_methods() {
        let s = "  hello world  ";
        assert_eq!(
            call_string_method(s, "startsWith", &[text("  hello")], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_string_method(s, "endsWith", &[text("world  ")], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_string_method("cat dog cat", "replace", &[text("cat"), text("fox")], Span::default())
                .unwrap()
                .display(),
            "fox dog cat"
        );
        assert_eq!(
            call_string_method("cat dog cat", "replaceAll", &[text("cat"), text("fox")], Span::default())
                .unwrap()
                .display(),
            "fox dog fox"
        );
        assert_eq!(
            call_string_method("hello WORLD", "capitalize", &[], Span::default())
                .unwrap()
                .display(),
            "Hello WORLD"
        );
        assert_eq!(
            call_string_method("hello   WORLD", "titleCase", &[], Span::default())
                .unwrap()
                .display(),
            "Hello   World"
        );
        assert_eq!(
            call_string_method("café", "reverse", &[], Span::default())
                .unwrap()
                .display(),
            "éfac"
        );
    }

    #[test]
    fn string_index_and_slice_methods() {
        let s = "héllo héllo";
        assert_eq!(
            call_string_method(s, "slice", &[num(1.0), num(4.0)], Span::default())
                .unwrap()
                .display(),
            "éll"
        );
        assert_eq!(
            call_string_method(s, "slice", &[num(-5.0)], Span::default())
                .unwrap()
                .display(),
            "héllo"
        );
        assert_eq!(
            call_string_method(s, "substring", &[num(4.0), num(1.0)], Span::default())
                .unwrap()
                .display(),
            "éll"
        );
        assert_eq!(
            call_string_method(s, "indexOf", &[text("héllo")], Span::default())
                .unwrap()
                .display(),
            "0"
        );
        assert_eq!(
            call_string_method(s, "lastIndexOf", &[text("héllo")], Span::default())
                .unwrap()
                .display(),
            "6"
        );
        assert_eq!(
            call_string_method(s, "charAt", &[num(1.0)], Span::default())
                .unwrap()
                .display(),
            "é"
        );
        assert_eq!(
            call_string_method(s, "charAt", &[num(-1.0)], Span::default())
                .unwrap()
                .display(),
            "o"
        );
        assert_eq!(
            call_string_method(s, "charAt", &[num(99.0)], Span::default())
                .unwrap()
                .display(),
            ""
        );
    }

    #[test]
    fn string_conversion_and_character_helpers() {
        assert_eq!(
            call_string_method(" 12.5 ", "toNumber", &[], Span::default())
                .unwrap()
                .display(),
            "12.5"
        );
        assert_eq!(
            call_string_method("abc", "toNumber", &[], Span::default())
                .unwrap()
                .display(),
            "null"
        );
        for spelling in ["NaN", "inf", "+inf", "-inf", "infinity"] {
            assert_eq!(
                call_string_method(spelling, "toNumber", &[], Span::default())
                    .unwrap()
                    .display(),
                "null",
                "{spelling} must not become a number"
            );
        }
        assert_eq!(
            call_string_method("123", "isDigit", &[], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_string_method("12a", "isDigit", &[], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_string_method("héllo", "isAlpha", &[], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_string_method("a b", "lines", &[], Span::default())
                .unwrap()
                .display(),
            "[\"a b\"]"
        );
        assert_eq!(
            call_string_method("first\r\nsecond\n", "lines", &[], Span::default())
                .unwrap()
                .display(),
            "[\"first\", \"second\"]"
        );
        assert_eq!(
            call_string_method("ab", "chars", &[], Span::default())
                .unwrap()
                .display(),
            "[\"a\", \"b\"]"
        );
    }

    #[test]
    fn string_repeat_and_padding_methods() {
        assert_eq!(
            call_string_method("ab", "repeat", &[num(3.0)], Span::default())
                .unwrap()
                .display(),
            "ababab"
        );
        assert_eq!(
            call_string_method("ab", "repeat", &[num(-1.0)], Span::default())
                .unwrap()
                .display(),
            ""
        );
        assert_eq!(
            call_string_method("7", "padLeft", &[num(3.0), text("0")], Span::default())
                .unwrap()
                .display(),
            "007"
        );
        assert_eq!(
            call_string_method("7", "padRight", &[num(4.0), text(".-")], Span::default())
                .unwrap()
                .display(),
            "7.-."
        );
        assert_eq!(
            call_string_method("abc", "padLeft", &[num(2.0)], Span::default())
                .unwrap()
                .display(),
            "abc"
        );
        let err = call_string_method("ab", "repeat", &[num(1.5)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_string_method("a", "padLeft", &[num(2.0), num(1.0)], Span::default())
            .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_string_method("a", "startsWith", &[], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Arity);
    }

    #[test]
    fn object_methods() {
        let mut fields = BTreeMap::new();
        fields.insert("a".to_string(), num(1.0));
        fields.insert("b".to_string(), text("two"));
        let o = Rc::new(RefCell::new(fields));

        assert_eq!(
            call_object_method(&o, "has", &[text("a")], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_object_method(&o, "get", &[text("missing"), text("fallback")], Span::default())
                .unwrap()
                .display(),
            "fallback"
        );
        assert_eq!(
            call_object_method(&o, "get", &[text("missing")], Span::default())
                .unwrap()
                .display(),
            "null"
        );
        call_object_method(&o, "set", &[text("c"), num(3.0)], Span::default()).unwrap();
        assert_eq!(
            call_object_method(&o, "length", &[], Span::default())
                .unwrap()
                .display(),
            "3"
        );
        assert_eq!(
            call_object_method(&o, "delete", &[text("a")], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_object_method(&o, "delete", &[text("a")], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_object_method(&o, "keys", &[], Span::default())
                .unwrap()
                .display(),
            "[\"b\", \"c\"]"
        );
        assert_eq!(
            call_object_method(&o, "isEmpty", &[], Span::default())
                .unwrap()
                .display(),
            "false"
        );
    }

    #[test]
    fn object_merge_copy_and_type_errors() {
        let o = Rc::new(RefCell::new(BTreeMap::new()));
        let mut fields = BTreeMap::new();
        fields.insert("x".to_string(), num(10.0));
        let other = Rc::new(RefCell::new(fields));
        call_object_method(&o, "merge", &[Value::Object(other)], Span::default()).unwrap();
        assert_eq!(
            call_object_method(&o, "get", &[text("x")], Span::default())
                .unwrap()
                .display(),
            "10"
        );
        // Merging an object into itself must not panic on a double borrow.
        call_object_method(&o, "merge", &[Value::Object(Rc::clone(&o))], Span::default()).unwrap();
        let copy = call_object_method(&o, "copy", &[], Span::default()).unwrap();
        assert!(matches!(copy, Value::Object(ref map) if !Rc::ptr_eq(map, &o)));
        assert_eq!(copy.display(), Value::Object(Rc::clone(&o)).display());
        assert_eq!(
            call_object_method(&o, "values", &[], Span::default())
                .unwrap()
                .display(),
            "[10]"
        );

        let err = call_object_method(&o, "merge", &[num(1.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_object_method(&o, "has", &[num(1.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_object_method(&o, "merge", &[], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Arity);
    }

    #[test]
    fn number_methods() {
        assert_eq!(
            call_number_method(4.0, "isEven", &[], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_number_method(4.5, "isEven", &[], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_number_method(-3.0, "isOdd", &[], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_number_method(3.0, "isInteger", &[], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_number_method(f64::INFINITY, "isFinite", &[], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_number_method(-2.7, "abs", &[], Span::default())
                .unwrap()
                .display(),
            "2.7"
        );
        assert_eq!(
            call_number_method(-2.2, "floor", &[], Span::default())
                .unwrap()
                .display(),
            "-3"
        );
        assert_eq!(
            call_number_method(2.2, "ceil", &[], Span::default())
                .unwrap()
                .display(),
            "3"
        );
        assert_eq!(
            call_number_method(2.5, "round", &[], Span::default())
                .unwrap()
                .display(),
            "3"
        );
        assert_eq!(
            call_number_method(-5.0, "sign", &[], Span::default())
                .unwrap()
                .display(),
            "-1"
        );
        assert_eq!(
            call_number_method(9.0, "sqrt", &[], Span::default())
                .unwrap()
                .display(),
            "3"
        );
    }

    #[test]
    fn number_formatting_and_clamp_methods() {
        assert_eq!(
            call_number_method(3.14159, "toFixed", &[num(2.0)], Span::default())
                .unwrap()
                .display(),
            "3.14"
        );
        assert_eq!(
            call_number_method(3.7, "toFixed", &[], Span::default())
                .unwrap()
                .display(),
            "4"
        );
        assert_eq!(
            call_number_method(15.0, "clamp", &[num(0.0), num(10.0)], Span::default())
                .unwrap()
                .display(),
            "10"
        );
        assert_eq!(
            call_number_method(255.0, "toHex", &[], Span::default())
                .unwrap()
                .display(),
            "ff"
        );
        assert_eq!(
            call_number_method(-5.0, "toBinary", &[], Span::default())
                .unwrap()
                .display(),
            "-101"
        );
        let err = call_number_method(-1.0, "sqrt", &[], Span::default()).unwrap_err();
        assert!(err.message.contains("sqrt"));
        let err = call_number_method(1.0, "clamp", &[num(1.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Arity);
        let err = call_number_method(1.0, "clamp", &[text("0"), num(1.0)], Span::default())
            .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);

        for method in ["toHex", "toBinary"] {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e30] {
                let err = call_number_method(value, method, &[], Span::default()).unwrap_err();
                assert!(
                    err.message.contains("finite"),
                    "{method}({value}) must be rejected"
                );
            }
        }
        let err = call_number_method(
            f64::NAN,
            "clamp",
            &[num(0.0), num(10.0)],
            Span::default(),
        )
        .unwrap_err();
        assert!(err.message.contains("finite"));
    }

    #[test]
    fn boolean_methods() {
        assert_eq!(
            call_bool_method(true, "toggle", &[], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_bool_method(false, "toString", &[], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_bool_method(true, "and", &[Value::Bool(false)], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_bool_method(false, "or", &[Value::Bool(true)], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_bool_method(true, "xor", &[Value::Bool(true)], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        let err = call_bool_method(true, "and", &[num(1.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_bool_method(true, "xor", &[], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Arity);
    }

    #[test]
    fn array_rotation_lookup_and_interleave_methods() {
        let a = arr(vec![num(1.0), num(2.0), num(3.0), num(4.0)]);
        assert_eq!(
            call_array_method(&a, "rotate", &[num(1.0)], Span::default())
                .unwrap()
                .display(),
            "[4, 1, 2, 3]"
        );
        assert_eq!(
            call_array_method(&a, "rotate", &[num(-1.0)], Span::default())
                .unwrap()
                .display(),
            "[2, 3, 4, 1]"
        );
        assert_eq!(
            call_array_method(&a, "rotate", &[num(5.0)], Span::default())
                .unwrap()
                .display(),
            "[4, 1, 2, 3]"
        );
        // Rotation returns a fresh array instead of mutating the receiver.
        assert_eq!(
            a.borrow().iter().map(Value::display).collect::<Vec<_>>(),
            ["1", "2", "3", "4"]
        );
        assert_eq!(
            call_array_method(&arr(vec![]), "rotate", &[num(2.0)], Span::default())
                .unwrap()
                .display(),
            "[]"
        );
        let err = call_array_method(&a, "rotate", &[num(1.5)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);

        let words = arr(vec![text("a"), text("b"), text("a")]);
        assert_eq!(
            call_array_method(&words, "indexesOf", &[text("a")], Span::default())
                .unwrap()
                .display(),
            "[0, 2]"
        );
        assert_eq!(
            call_array_method(&words, "indexesOf", &[text("z")], Span::default())
                .unwrap()
                .display(),
            "[]"
        );

        let left = arr(vec![num(1.0), num(2.0)]);
        let right = Value::new_array(vec![text("a"), text("b"), text("c")]);
        assert_eq!(
            call_array_method(&left, "interleave", &[right], Span::default())
                .unwrap()
                .display(),
            "[1, \"a\", 2, \"b\", \"c\"]"
        );
        let err = call_array_method(&left, "interleave", &[num(3.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_array_method(&left, "interleave", &[], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Arity);
    }

    #[test]
    fn object_pick_omit_and_entries_methods() {
        let mut fields = BTreeMap::new();
        fields.insert("a".to_string(), num(1.0));
        fields.insert("b".to_string(), num(2.0));
        fields.insert("c".to_string(), num(3.0));
        let o = Rc::new(RefCell::new(fields));

        let picked = call_object_method(
            &o,
            "pick",
            &[text("c"), text("a"), text("missing")],
            Span::default(),
        )
        .unwrap();
        assert_eq!(picked.display(), "{a: 1, c: 3}");
        let omitted = call_object_method(&o, "omit", &[text("b")], Span::default()).unwrap();
        assert_eq!(omitted.display(), "{a: 1, c: 3}");
        // Both helpers return a new object and leave the receiver alone.
        assert_eq!(o.borrow().len(), 3);
        assert_eq!(
            call_object_method(&o, "entries", &[], Span::default())
                .unwrap()
                .display(),
            "[[\"a\", 1], [\"b\", 2], [\"c\", 3]]"
        );
        let err = call_object_method(&o, "pick", &[num(1.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_object_method(&o, "omit", &[num(1.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
    }

    #[test]
    fn string_swap_case_blank_and_truncate_methods() {
        assert_eq!(
            call_string_method("Hello WORLD 42", "swapCase", &[], Span::default())
                .unwrap()
                .display(),
            "hELLO world 42"
        );
        assert_eq!(
            call_string_method("héllo", "swapCase", &[], Span::default())
                .unwrap()
                .display(),
            "HÉLLO"
        );
        for blank in ["", "   \t\n"] {
            assert_eq!(
                call_string_method(blank, "isBlank", &[], Span::default())
                    .unwrap()
                    .display(),
                "true"
            );
        }
        assert_eq!(
            call_string_method(" a ", "isBlank", &[], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_string_method("hello world", "truncate", &[num(8.0)], Span::default())
                .unwrap()
                .display(),
            "hello..."
        );
        assert_eq!(
            call_string_method(
                "hello world",
                "truncate",
                &[num(5.0), text("~")],
                Span::default(),
            )
            .unwrap()
            .display(),
            "hell~"
        );
        assert_eq!(
            call_string_method("hi", "truncate", &[num(10.0)], Span::default())
                .unwrap()
                .display(),
            "hi"
        );
        // A suffix at least as wide as the limit is clipped to the limit.
        assert_eq!(
            call_string_method("hello", "truncate", &[num(2.0)], Span::default())
                .unwrap()
                .display(),
            ".."
        );
        assert_eq!(
            call_string_method("hello", "truncate", &[num(0.0)], Span::default())
                .unwrap()
                .display(),
            ""
        );
        let err =
            call_string_method("hello", "truncate", &[num(1.5)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_string_method("hello", "truncate", &[], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Arity);
        let err =
            call_string_method("hello", "truncate", &[num(3.0), num(1.0)], Span::default())
                .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
    }

    #[test]
    fn number_octal_degree_and_reciprocal_methods() {
        assert_eq!(
            call_number_method(255.0, "toOctal", &[], Span::default())
                .unwrap()
                .display(),
            "377"
        );
        assert_eq!(
            call_number_method(-8.0, "toOctal", &[], Span::default())
                .unwrap()
                .display(),
            "-10"
        );
        for value in [f64::NAN, f64::INFINITY, 1e30] {
            let err = call_number_method(value, "toOctal", &[], Span::default()).unwrap_err();
            assert!(
                err.message.contains("finite"),
                "toOctal({value}) must be rejected"
            );
        }
        let degrees = call_number_method(std::f64::consts::PI, "toDegrees", &[], Span::default())
            .unwrap();
        assert!(matches!(degrees, Value::Number(n) if (n - 180.0).abs() < 1e-9));
        assert_eq!(
            call_number_method(4.0, "reciprocal", &[], Span::default())
                .unwrap()
                .display(),
            "0.25"
        );
        assert_eq!(
            call_number_method(-2.0, "reciprocal", &[], Span::default())
                .unwrap()
                .display(),
            "-0.5"
        );
        let err = call_number_method(0.0, "reciprocal", &[], Span::default()).unwrap_err();
        assert!(err.message.contains("zero"));
    }

    #[test]
    fn boolean_implication_and_nand_methods() {
        assert_eq!(
            call_bool_method(true, "implies", &[Value::Bool(true)], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_bool_method(true, "implies", &[Value::Bool(false)], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_bool_method(false, "implies", &[Value::Bool(false)], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_bool_method(true, "nand", &[Value::Bool(true)], Span::default())
                .unwrap()
                .display(),
            "false"
        );
        assert_eq!(
            call_bool_method(true, "nand", &[Value::Bool(false)], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        assert_eq!(
            call_bool_method(false, "nand", &[Value::Bool(true)], Span::default())
                .unwrap()
                .display(),
            "true"
        );
        // Operands are validated even when the receiver settles the result.
        let err = call_bool_method(false, "implies", &[num(1.0)], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Type);
        let err = call_bool_method(false, "nand", &[], Span::default()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Arity);
    }

    #[test]
    fn unknown_method_errors_list_all_available_methods() {
        let cases: Vec<(String, Vec<&str>)> = vec![
            (
                call_array_method(&arr(vec![]), "zzzz", &[], Span::default())
                    .unwrap_err()
                    .hint
                    .unwrap(),
                ARRAY_METHODS.to_vec(),
            ),
            (
                call_object_method(&Rc::new(RefCell::new(BTreeMap::new())), "zzzz", &[], Span::default())
                    .unwrap_err()
                    .hint
                    .unwrap(),
                OBJECT_METHODS.to_vec(),
            ),
            (
                call_string_method("x", "zzzz", &[], Span::default())
                    .unwrap_err()
                    .hint
                    .unwrap(),
                STRING_METHODS.to_vec(),
            ),
            (
                call_number_method(1.0, "zzzz", &[], Span::default())
                    .unwrap_err()
                    .hint
                    .unwrap(),
                NUMBER_METHODS.to_vec(),
            ),
            (
                call_bool_method(true, "zzzz", &[], Span::default())
                    .unwrap_err()
                    .hint
                    .unwrap(),
                BOOL_METHODS.to_vec(),
            ),
        ];
        for (hint, methods) in cases {
            for method in methods {
                assert!(hint.contains(method), "hint missing {method}: {hint}");
            }
        }
    }
}

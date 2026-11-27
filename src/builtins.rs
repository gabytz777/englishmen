//! Built-in functions available in every Englishmen scope.
//!
//! Both execution backends create their global environment through
//! [`crate::environment::Environment::new`], so registering a native function
//! here makes it available to the interpreter, the VM compiled programs and
//! the REPL at once.

use crate::runtime::native;
use crate::value::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn rng_state() -> &'static Mutex<u64> {
    static STATE: OnceLock<Mutex<u64>> = OnceLock::new();
    STATE.get_or_init(|| {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        Mutex::new(seed ^ 0xA0761D6478BD642F)
    })
}

fn next_rand() -> f64 {
    let mut guard = rng_state().lock().expect("rng lock poisoned");
    // xorshift64*
    let mut x = *guard;
    if x == 0 {
        x = 0x9E3779B97F4A7C15;
    }
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *guard = x;
    let v = x.wrapping_mul(0x2545F4914F6CDD1D);
    (v >> 11) as f64 / ((1u64 << 53) as f64)
}

fn arg(args: &[Value], idx: usize) -> &Value {
    args.get(idx).unwrap_or(&Value::Null)
}

fn expect_num(args: &[Value], idx: usize, ctx: &str) -> Result<f64, String> {
    match arg(args, idx) {
        Value::Number(n) => Ok(*n),
        other => Err(format!(
            "Type mismatch: expected a number {ctx}, found {}",
            other.type_name()
        )),
    }
}

fn expect_str(args: &[Value], idx: usize, ctx: &str) -> Result<String, String> {
    match arg(args, idx) {
        Value::String(s) => Ok(s.clone()),
        other => Err(format!(
            "Type mismatch: expected a string {ctx}, found {}",
            other.type_name()
        )),
    }
}

fn expect_array(args: &[Value], idx: usize, ctx: &str) -> Result<Vec<Value>, String> {
    match arg(args, idx) {
        Value::Array(a) => Ok(a.borrow().clone()),
        other => Err(format!(
            "Type mismatch: expected an array {ctx}, found {}",
            other.type_name()
        )),
    }
}

fn no_extra(args: &[Value], n: usize, name: &str) -> Result<(), String> {
    if args.len() > n {
        return Err(format!(
            "{name} expects at most {n} argument{}, found {}",
            if n == 1 { "" } else { "s" },
            args.len()
        ));
    }
    Ok(())
}

fn exact_args(args: &[Value], n: usize, name: &str) -> Result<(), String> {
    if args.len() != n {
        return Err(format!(
            "{name} expects {n} argument{}, found {}",
            if n == 1 { "" } else { "s" },
            args.len()
        ));
    }
    Ok(())
}

fn at_least_args(args: &[Value], n: usize, name: &str) -> Result<(), String> {
    if args.len() < n {
        return Err(format!(
            "{name} expects at least {n} argument{}, found {}",
            if n == 1 { "" } else { "s" },
            args.len()
        ));
    }
    Ok(())
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let code = c as u32;
                out.push_str(&format!("\\u{:04x}", code));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_encode(value: &Value) -> Result<String, String> {
    match value {
        Value::Null => Ok("null".to_string()),
        Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
        Value::Number(n) => {
            if n.is_finite() {
                Ok(format_number_json(*n))
            } else {
                Err("jsonEncode cannot encode non-finite numbers".to_string())
            }
        }
        Value::String(s) => Ok(json_escape(s)),
        Value::Array(a) => {
            let items = a.borrow();
            let mut parts = Vec::with_capacity(items.len());
            for item in items.iter() {
                parts.push(json_encode(item)?);
            }
            Ok(format!("[{}]", parts.join(",")))
        }
        Value::Object(o) => {
            let map = o.borrow();
            let mut parts = Vec::with_capacity(map.len());
            for (k, v) in map.iter() {
                parts.push(format!("{}:{}", json_escape(k), json_encode(v)?));
            }
            Ok(format!("{{{}}}", parts.join(",")))
        }
        Value::Function(_) | Value::Compiled(_) | Value::Native(_) => {
            Err("jsonEncode cannot encode functions".to_string())
        }
    }
}

fn format_number_json(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let mut s = format!("{n}");
        if !s.contains('.') && !s.contains('e') && !s.contains('E') {
            s.push_str(".0");
        }
        s
    }
}

struct JsonParser<'a> {
    chars: Vec<char>,
    pos: usize,
    _src: &'a str,
}

impl<'a> JsonParser<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            chars: src.chars().collect(),
            pos: 0,
            _src: src,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self.peek() {
            Some('n') => self.parse_null(),
            Some('t') => self.parse_true(),
            Some('f') => self.parse_false(),
            Some('"') => self.parse_string().map(Value::String),
            Some('[') => self.parse_array(),
            Some('{') => self.parse_object(),
            Some('-') | Some('0'..='9') => self.parse_number(),
            Some(c) => Err(format!("jsonDecode: unexpected character '{c}'")),
            None => Err("jsonDecode: unexpected end of input".to_string()),
        }
    }

    fn consume_literal(&mut self, lit: &str) -> Result<(), String> {
        for expected in lit.chars() {
            match self.next() {
                Some(c) if c == expected => {}
                _ => return Err(format!("jsonDecode: expected '{lit}'")),
            }
        }
        Ok(())
    }

    fn parse_null(&mut self) -> Result<Value, String> {
        self.consume_literal("null")?;
        Ok(Value::Null)
    }

    fn parse_true(&mut self) -> Result<Value, String> {
        self.consume_literal("true")?;
        Ok(Value::Bool(true))
    }

    fn parse_false(&mut self) -> Result<Value, String> {
        self.consume_literal("false")?;
        Ok(Value::Bool(false))
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.peek() {
            Some('0') => {
                self.pos += 1;
            }
            Some('1'..='9') => {
                while matches!(self.peek(), Some('0'..='9')) {
                    self.pos += 1;
                }
            }
            _ => return Err("jsonDecode: invalid number".to_string()),
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err("jsonDecode: invalid number".to_string());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+') | Some('-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err("jsonDecode: invalid number".to_string());
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.pos += 1;
            }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        let n: f64 = s
            .parse()
            .map_err(|_| format!("jsonDecode: invalid number '{s}'"))?;
        Ok(Value::Number(n))
    }

    fn parse_string(&mut self) -> Result<String, String> {
        if self.next() != Some('"') {
            return Err("jsonDecode: expected string".to_string());
        }
        let mut out = String::new();
        loop {
            match self.next() {
                Some('"') => return Ok(out),
                Some('\\') => {
                    let esc = self
                        .next()
                        .ok_or_else(|| "jsonDecode: unterminated string escape".to_string())?;
                    match esc {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let mut code: u32 = 0;
                            for _ in 0..4 {
                                let h = self.next().ok_or_else(|| {
                                    "jsonDecode: incomplete unicode escape".to_string()
                                })?;
                                code = code * 16
                                    + h.to_digit(16).ok_or_else(|| {
                                        "jsonDecode: invalid unicode escape".to_string()
                                    })?;
                            }
                            if (0xD800..=0xDBFF).contains(&code) {
                                // surrogate pair
                                if self.next() != Some('\\') || self.next() != Some('u') {
                                    return Err("jsonDecode: invalid unicode surrogate pair".to_string());
                                }
                                let mut low: u32 = 0;
                                for _ in 0..4 {
                                    let h = self.next().ok_or_else(|| {
                                        "jsonDecode: incomplete unicode escape".to_string()
                                    })?;
                                    low = low * 16
                                        + h.to_digit(16).ok_or_else(|| {
                                            "jsonDecode: invalid unicode escape".to_string()
                                        })?;
                                }
                                if !(0xDC00..=0xDFFF).contains(&low) {
                                    return Err("jsonDecode: invalid unicode surrogate pair".to_string());
                                }
                                let cp = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                let ch = char::from_u32(cp)
                                    .ok_or_else(|| "jsonDecode: invalid unicode codepoint".to_string())?;
                                out.push(ch);
                            } else {
                                let ch = char::from_u32(code)
                                    .ok_or_else(|| "jsonDecode: invalid unicode codepoint".to_string())?;
                                out.push(ch);
                            }
                        }
                        _ => return Err(format!("jsonDecode: invalid escape '\\{esc}'")),
                    }
                }
                Some(c) => {
                    if c.is_control() {
                        return Err("jsonDecode: control character in string".to_string());
                    }
                    out.push(c);
                }
                None => return Err("jsonDecode: unterminated string".to_string()),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        if self.next() != Some('[') {
            return Err("jsonDecode: expected array".to_string());
        }
        self.skip_ws();
        let mut items = Vec::new();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Value::new_array(items));
        }
        loop {
            let v = self.parse_value()?;
            items.push(v);
            self.skip_ws();
            match self.next() {
                Some(',') => {}
                Some(']') => break,
                _ => return Err("jsonDecode: expected ',' or ']' in array".to_string()),
            }
        }
        Ok(Value::new_array(items))
    }

    fn parse_object(&mut self) -> Result<Value, String> {
        if self.next() != Some('{') {
            return Err("jsonDecode: expected object".to_string());
        }
        self.skip_ws();
        let mut map = BTreeMap::new();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Value::new_object(map));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.next() != Some(':') {
                return Err("jsonDecode: expected ':' in object".to_string());
            }
            let val = self.parse_value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.next() {
                Some(',') => {}
                Some('}') => break,
                _ => return Err("jsonDecode: expected ',' or '}' in object".to_string()),
            }
        }
        Ok(Value::new_object(map))
    }
}

fn json_decode(src: &str) -> Result<Value, String> {
    let mut p = JsonParser::new(src);
    let val = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err("jsonDecode: trailing characters after JSON value".to_string());
    }
    Ok(val)
}

/// The global builtin table. Seeded into every new root environment.
pub fn globals() -> HashMap<String, Value> {
    let mut g = HashMap::new();

    // --- Conversions / introspection ---
    g.insert(
        "len".to_string(),
        native("len", |args| {
            exact_args(args, 1, "len")?;
            match arg(args, 0) {
                Value::String(s) => Ok(Value::Number(s.chars().count() as f64)),
                Value::Array(a) => Ok(Value::Number(a.borrow().len() as f64)),
                Value::Object(o) => Ok(Value::Number(o.borrow().len() as f64)),
                other => Err(format!(
                    "Type mismatch: len expects a string, array, or object, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    g.insert(
        "type".to_string(),
        native("type", |args| {
            exact_args(args, 1, "type")?;
            Ok(Value::String(arg(args, 0).type_name().to_string()))
        }),
    );

    g.insert(
        "toNumber".to_string(),
        native("toNumber", |args| {
            exact_args(args, 1, "toNumber")?;
            match arg(args, 0) {
                Value::Number(n) => Ok(Value::Number(*n)),
                Value::Bool(b) => Ok(Value::Number(if *b { 1.0 } else { 0.0 })),
                Value::Null => Ok(Value::Number(0.0)),
                Value::String(s) => {
                    let t = s.trim();
                    t.parse::<f64>().map(Value::Number).map_err(|_| {
                        format!("toNumber could not convert string \"{}\" to a number", s)
                    })
                }
                other => Err(format!(
                    "Type mismatch: toNumber expects a number, string, boolean, or null, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    g.insert(
        "toString".to_string(),
        native("toString", |args| {
            exact_args(args, 1, "toString")?;
            Ok(Value::String(arg(args, 0).display()))
        }),
    );

    g.insert(
        "toBool".to_string(),
        native("toBool", |args| {
            exact_args(args, 1, "toBool")?;
            Ok(Value::Bool(arg(args, 0).is_truthy()))
        }),
    );

    g.insert(
        "isNumber".to_string(),
        native("isNumber", |args| {
            exact_args(args, 1, "isNumber")?;
            Ok(Value::Bool(matches!(arg(args, 0), Value::Number(_))))
        }),
    );

    g.insert(
        "isString".to_string(),
        native("isString", |args| {
            exact_args(args, 1, "isString")?;
            Ok(Value::Bool(matches!(arg(args, 0), Value::String(_))))
        }),
    );

    g.insert(
        "isArray".to_string(),
        native("isArray", |args| {
            exact_args(args, 1, "isArray")?;
            Ok(Value::Bool(matches!(arg(args, 0), Value::Array(_))))
        }),
    );

    g.insert(
        "isObject".to_string(),
        native("isObject", |args| {
            exact_args(args, 1, "isObject")?;
            Ok(Value::Bool(matches!(arg(args, 0), Value::Object(_))))
        }),
    );

    g.insert(
        "isBool".to_string(),
        native("isBool", |args| {
            exact_args(args, 1, "isBool")?;
            Ok(Value::Bool(matches!(arg(args, 0), Value::Bool(_))))
        }),
    );

    g.insert(
        "isNull".to_string(),
        native("isNull", |args| {
            exact_args(args, 1, "isNull")?;
            Ok(Value::Bool(matches!(arg(args, 0), Value::Null)))
        }),
    );

    g.insert(
        "isFunction".to_string(),
        native("isFunction", |args| {
            exact_args(args, 1, "isFunction")?;
            Ok(Value::Bool(matches!(
                arg(args, 0),
                Value::Function(_) | Value::Compiled(_) | Value::Native(_)
            )))
        }),
    );

    // --- Collections ---
    g.insert(
        "range".to_string(),
        native("range", |args| {
            at_least_args(args, 1, "range")?;
            no_extra(args, 3, "range")?;
            let (start, end, step) = if args.len() == 1 {
                (0.0, expect_num(args, 0, "as the end for range")?, 1.0)
            } else if args.len() == 2 {
                (
                    expect_num(args, 0, "as the start for range")?,
                    expect_num(args, 1, "as the end for range")?,
                    1.0,
                )
            } else {
                (
                    expect_num(args, 0, "as the start for range")?,
                    expect_num(args, 1, "as the end for range")?,
                    expect_num(args, 2, "as the step for range")?,
                )
            };
            if step == 0.0 || !step.is_finite() {
                return Err("range step must be a non-zero finite number".to_string());
            }
            if !start.is_finite() || !end.is_finite() {
                return Err("range start and end must be finite numbers".to_string());
            }
            let mut out = Vec::new();
            let mut cur = start;
            if step > 0.0 {
                while cur < end {
                    out.push(Value::Number(cur));
                    cur += step;
                }
            } else {
                while cur > end {
                    out.push(Value::Number(cur));
                    cur += step;
                }
            }
            Ok(Value::new_array(out))
        }),
    );

    g.insert(
        "sum".to_string(),
        native("sum", |args| {
            exact_args(args, 1, "sum")?;
            let items = expect_array(args, 0, "for sum")?;
            let mut total = 0.0;
            for (i, v) in items.iter().enumerate() {
                match v {
                    Value::Number(n) => total += n,
                    other => {
                        return Err(format!(
                            "Type mismatch: sum expects an array of numbers, found {} at index {}",
                            other.type_name(),
                            i
                        ))
                    }
                }
            }
            Ok(Value::Number(total))
        }),
    );

    g.insert(
        "average".to_string(),
        native("average", |args| {
            exact_args(args, 1, "average")?;
            let items = expect_array(args, 0, "for average")?;
            if items.is_empty() {
                return Err("average cannot compute the average of an empty array".to_string());
            }
            let mut total = 0.0;
            for (i, v) in items.iter().enumerate() {
                match v {
                    Value::Number(n) => total += n,
                    other => {
                        return Err(format!(
                            "Type mismatch: average expects an array of numbers, found {} at index {}",
                            other.type_name(),
                            i
                        ))
                    }
                }
            }
            Ok(Value::Number(total / items.len() as f64))
        }),
    );

    g.insert(
        "min".to_string(),
        native("min", |args| {
            at_least_args(args, 1, "min")?;
            let values: Vec<Value> = if args.len() == 1 {
                expect_array(args, 0, "for min")?
            } else {
                args.to_vec()
            };
            if values.is_empty() {
                return Err("min expects at least one value".to_string());
            }
            let mut best = expect_num_value(&values[0], "for min")?;
            for v in values.iter().skip(1) {
                let n = expect_num_value(v, "for min")?;
                if n < best {
                    best = n;
                }
            }
            Ok(Value::Number(best))
        }),
    );

    g.insert(
        "max".to_string(),
        native("max", |args| {
            at_least_args(args, 1, "max")?;
            let values: Vec<Value> = if args.len() == 1 {
                expect_array(args, 0, "for max")?
            } else {
                args.to_vec()
            };
            if values.is_empty() {
                return Err("max expects at least one value".to_string());
            }
            let mut best = expect_num_value(&values[0], "for max")?;
            for v in values.iter().skip(1) {
                let n = expect_num_value(v, "for max")?;
                if n > best {
                    best = n;
                }
            }
            Ok(Value::Number(best))
        }),
    );

    g.insert(
        "abs".to_string(),
        native("abs", |args| {
            exact_args(args, 1, "abs")?;
            let n = expect_num(args, 0, "for abs")?;
            Ok(Value::Number(n.abs()))
        }),
    );

    g.insert(
        "clamp".to_string(),
        native("clamp", |args| {
            exact_args(args, 3, "clamp")?;
            let v = expect_num(args, 0, "as the value for clamp")?;
            let lo = expect_num(args, 1, "as the minimum for clamp")?;
            let hi = expect_num(args, 2, "as the maximum for clamp")?;
            if lo > hi {
                return Err("clamp expects min <= max".to_string());
            }
            Ok(Value::Number(v.max(lo).min(hi)))
        }),
    );

    g.insert(
        "sorted".to_string(),
        native("sorted", |args| {
            exact_args(args, 1, "sorted")?;
            let mut items = expect_array(args, 0, "for sorted")?;
            if items.is_empty() {
                return Ok(Value::new_array(items));
            }
            let all_numbers = items.iter().all(|v| matches!(v, Value::Number(_)));
            let all_strings = items.iter().all(|v| matches!(v, Value::String(_)));
            if all_numbers {
                items.sort_by(|a, b| {
                    let an = match a { Value::Number(n) => *n, _ => 0.0 };
                    let bn = match b { Value::Number(n) => *n, _ => 0.0 };
                    an.partial_cmp(&bn).unwrap_or(std::cmp::Ordering::Equal)
                });
            } else if all_strings {
                items.sort_by(|a, b| {
                    let as_ = match a { Value::String(s) => s.as_str(), _ => "" };
                    let bs_ = match b { Value::String(s) => s.as_str(), _ => "" };
                    as_.cmp(bs_)
                });
            } else {
                return Err("sorted expects an array of all numbers or all strings".to_string());
            }
            Ok(Value::new_array(items))
        }),
    );

    g.insert(
        "reversed".to_string(),
        native("reversed", |args| {
            exact_args(args, 1, "reversed")?;
            match arg(args, 0) {
                Value::Array(a) => {
                    let mut items = a.borrow().clone();
                    items.reverse();
                    Ok(Value::new_array(items))
                }
                Value::String(s) => {
                    let rev: String = s.chars().rev().collect();
                    Ok(Value::String(rev))
                }
                other => Err(format!(
                    "Type mismatch: reversed expects an array or string, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    g.insert(
        "zip".to_string(),
        native("zip", |args| {
            exact_args(args, 2, "zip")?;
            let a = expect_array(args, 0, "as the first array for zip")?;
            let b = expect_array(args, 1, "as the second array for zip")?;
            let len = a.len().min(b.len());
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                out.push(Value::new_array(vec![a[i].clone(), b[i].clone()]));
            }
            Ok(Value::new_array(out))
        }),
    );

    // --- Math ---
    g.insert(
        "pow".to_string(),
        native("pow", |args| {
            exact_args(args, 2, "pow")?;
            let base = expect_num(args, 0, "as the base for pow")?;
            let exp = expect_num(args, 1, "as the exponent for pow")?;
            Ok(Value::Number(base.powf(exp)))
        }),
    );

    g.insert(
        "sqrt".to_string(),
        native("sqrt", |args| {
            exact_args(args, 1, "sqrt")?;
            let n = expect_num(args, 0, "for sqrt")?;
            if n < 0.0 {
                return Err("sqrt cannot take the square root of a negative number".to_string());
            }
            Ok(Value::Number(n.sqrt()))
        }),
    );

    g.insert(
        "floor".to_string(),
        native("floor", |args| {
            exact_args(args, 1, "floor")?;
            Ok(Value::Number(expect_num(args, 0, "for floor")?.floor()))
        }),
    );

    g.insert(
        "ceil".to_string(),
        native("ceil", |args| {
            exact_args(args, 1, "ceil")?;
            Ok(Value::Number(expect_num(args, 0, "for ceil")?.ceil()))
        }),
    );

    g.insert(
        "round".to_string(),
        native("round", |args| {
            exact_args(args, 1, "round")?;
            Ok(Value::Number(expect_num(args, 0, "for round")?.round()))
        }),
    );

    g.insert(
        "sign".to_string(),
        native("sign", |args| {
            exact_args(args, 1, "sign")?;
            let n = expect_num(args, 0, "for sign")?;
            let s = if n > 0.0 { 1.0 } else if n < 0.0 { -1.0 } else { 0.0 };
            Ok(Value::Number(s))
        }),
    );

    g.insert(
        "random".to_string(),
        native("random", |args| {
            exact_args(args, 0, "random")?;
            Ok(Value::Number(next_rand()))
        }),
    );

    g.insert(
        "randomInt".to_string(),
        native("randomInt", |args| {
            at_least_args(args, 1, "randomInt")?;
            no_extra(args, 2, "randomInt")?;
            let (min, max) = if args.len() == 1 {
                (0.0, expect_num(args, 0, "as the max for randomInt")?)
            } else {
                (
                    expect_num(args, 0, "as the min for randomInt")?,
                    expect_num(args, 1, "as the max for randomInt")?,
                )
            };
            if !min.is_finite() || !max.is_finite() || min.fract() != 0.0 || max.fract() != 0.0 {
                return Err("randomInt expects finite integer arguments".to_string());
            }
            let min_i = min as i64;
            let max_i = max as i64;
            if min_i > max_i {
                return Err("randomInt expects min <= max".to_string());
            }
            let span = (max_i - min_i) as u64 + 1;
            let v = min_i + (next_rand_u64() % span) as i64;
            Ok(Value::Number(v as f64))
        }),
    );

    g.insert(
        "seed".to_string(),
        native("seed", |args| {
            exact_args(args, 1, "seed")?;
            let n = expect_num(args, 0, "for seed")?;
            if !n.is_finite() {
                return Err("seed expects a finite number".to_string());
            }
            let mut guard = rng_state().lock().expect("rng lock poisoned");
            let bits = n.to_bits();
            *guard = bits ^ 0xA0761D6478BD642F;
            if *guard == 0 {
                *guard = 0x9E3779B97F4A7C15;
            }
            Ok(Value::Null)
        }),
    );

    // --- Text ---
    g.insert(
        "input".to_string(),
        native("input", |args| {
            no_extra(args, 1, "input")?;
            if args.len() == 1 {
                let prompt = expect_str(args, 0, "as the prompt for input")?;
                print!("{}", prompt);
                io::stdout()
                    .flush()
                    .map_err(|e| format!("input could not flush stdout: {e}"))?;
            }
            let mut line = String::new();
            io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("input could not read from stdin: {e}"))?;
            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Ok(Value::String(line))
        }),
    );

    g.insert(
        "print".to_string(),
        native("print", |args| {
            let parts: Vec<String> = args.iter().map(|v| v.display()).collect();
            println!("{}", parts.join(" "));
            Ok(Value::Null)
        }),
    );

    g.insert(
        "format".to_string(),
        native("format", |args| {
            at_least_args(args, 1, "format")?;
            let template = expect_str(args, 0, "as the format template")?;
            let fmt_args = &args[1..];
            Ok(Value::String(format_string(&template, fmt_args)?))
        }),
    );

    g.insert(
        "trim".to_string(),
        native("trim", |args| {
            exact_args(args, 1, "trim")?;
            let s = expect_str(args, 0, "for trim")?;
            Ok(Value::String(s.trim().to_string()))
        }),
    );

    g.insert(
        "upper".to_string(),
        native("upper", |args| {
            exact_args(args, 1, "upper")?;
            let s = expect_str(args, 0, "for upper")?;
            Ok(Value::String(s.to_uppercase()))
        }),
    );

    g.insert(
        "lower".to_string(),
        native("lower", |args| {
            exact_args(args, 1, "lower")?;
            let s = expect_str(args, 0, "for lower")?;
            Ok(Value::String(s.to_lowercase()))
        }),
    );

    // --- Filesystem ---
    g.insert(
        "readFile".to_string(),
        native("readFile", |args| {
            exact_args(args, 1, "readFile")?;
            let path = expect_str(args, 0, "as the path for readFile")?;
            fs::read_to_string(&path)
                .map(Value::String)
                .map_err(|e| format!("readFile could not read '{}': {e}", path))
        }),
    );

    g.insert(
        "writeFile".to_string(),
        native("writeFile", |args| {
            exact_args(args, 2, "writeFile")?;
            let path = expect_str(args, 0, "as the path for writeFile")?;
            let content = expect_str(args, 1, "as the content for writeFile")?;
            fs::write(&path, content)
                .map_err(|e| format!("writeFile could not write '{}': {e}", path))?;
            Ok(Value::Null)
        }),
    );

    g.insert(
        "appendFile".to_string(),
        native("appendFile", |args| {
            exact_args(args, 2, "appendFile")?;
            let path = expect_str(args, 0, "as the path for appendFile")?;
            let content = expect_str(args, 1, "as the content for appendFile")?;
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| format!("appendFile could not open '{}': {e}", path))?;
            f.write_all(content.as_bytes())
                .map_err(|e| format!("appendFile could not write '{}': {e}", path))?;
            Ok(Value::Null)
        }),
    );

    g.insert(
        "fileExists".to_string(),
        native("fileExists", |args| {
            exact_args(args, 1, "fileExists")?;
            let path = expect_str(args, 0, "as the path for fileExists")?;
            Ok(Value::Bool(Path::new(&path).exists()))
        }),
    );

    g.insert(
        "deleteFile".to_string(),
        native("deleteFile", |args| {
            exact_args(args, 1, "deleteFile")?;
            let path = expect_str(args, 0, "as the path for deleteFile")?;
            let p = Path::new(&path);
            if p.is_dir() {
                fs::remove_dir_all(p).map_err(|e| {
                    format!("deleteFile could not delete directory '{}': {e}", path)
                })?;
            } else {
                fs::remove_file(p)
                    .map_err(|e| format!("deleteFile could not delete '{}': {e}", path))?;
            }
            Ok(Value::Null)
        }),
    );

    g.insert(
        "listFiles".to_string(),
        native("listFiles", |args| {
            exact_args(args, 1, "listFiles")?;
            let path = expect_str(args, 0, "as the path for listFiles")?;
            let entries = fs::read_dir(&path)
                .map_err(|e| format!("listFiles could not read directory '{}': {e}", path))?;
            let mut names = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|e| {
                    format!("listFiles could not read entry in '{}': {e}", path)
                })?;
                let name = entry.file_name().to_string_lossy().to_string();
                names.push(Value::String(name));
            }
            names.sort_by(|a, b| match (a, b) {
                (Value::String(a), Value::String(b)) => a.cmp(b),
                _ => std::cmp::Ordering::Equal,
            });
            Ok(Value::new_array(names))
        }),
    );

    g.insert(
        "makeDir".to_string(),
        native("makeDir", |args| {
            exact_args(args, 1, "makeDir")?;
            let path = expect_str(args, 0, "as the path for makeDir")?;
            fs::create_dir_all(&path)
                .map_err(|e| format!("makeDir could not create '{}': {e}", path))?;
            Ok(Value::Null)
        }),
    );

    // --- System ---
    g.insert(
        "clock".to_string(),
        native("clock", |args| {
            exact_args(args, 0, "clock")?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| format!("clock could not read system time: {e}"))?;
            Ok(Value::Number(now.as_secs_f64()))
        }),
    );

    g.insert(
        "timestamp".to_string(),
        native("timestamp", |args| {
            exact_args(args, 0, "timestamp")?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| format!("timestamp could not read system time: {e}"))?;
            Ok(Value::Number(now.as_millis() as f64))
        }),
    );

    g.insert(
        "env".to_string(),
        native("env", |args| {
            exact_args(args, 1, "env")?;
            let key = expect_str(args, 0, "as the variable name for env")?;
            match std::env::var(&key) {
                Ok(v) => Ok(Value::String(v)),
                Err(_) => Ok(Value::Null),
            }
        }),
    );

    g.insert(
        "args".to_string(),
        native("args", |args| {
            exact_args(args, 0, "args")?;
            let argv: Vec<Value> = std::env::args().map(Value::String).collect();
            Ok(Value::new_array(argv))
        }),
    );

    g.insert(
        "sleep".to_string(),
        native("sleep", |args| {
            exact_args(args, 1, "sleep")?;
            let ms = expect_num(args, 0, "as milliseconds for sleep")?;
            if !ms.is_finite() || ms < 0.0 {
                return Err("sleep expects a non-negative finite number of milliseconds".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            Ok(Value::Null)
        }),
    );

    g.insert(
        "assert".to_string(),
        native("assert", |args| {
            at_least_args(args, 1, "assert")?;
            no_extra(args, 2, "assert")?;
            let cond = arg(args, 0).is_truthy();
            if cond {
                return Ok(Value::Null);
            }
            let msg = if args.len() == 2 {
                expect_str(args, 1, "as the message for assert")?
            } else {
                "assertion failed".to_string()
            };
            Err(format!("Assertion failed: {msg}"))
        }),
    );

    // --- Serialization ---
    g.insert(
        "jsonEncode".to_string(),
        native("jsonEncode", |args| {
            exact_args(args, 1, "jsonEncode")?;
            json_encode(arg(args, 0)).map(Value::String)
        }),
    );

    g.insert(
        "jsonDecode".to_string(),
        native("jsonDecode", |args| {
            exact_args(args, 1, "jsonDecode")?;
            let src = expect_str(args, 0, "as the JSON string for jsonDecode")?;
            json_decode(&src)
        }),
    );

    g
}

fn expect_num_value(value: &Value, ctx: &str) -> Result<f64, String> {
    match value {
        Value::Number(n) => Ok(*n),
        other => Err(format!(
            "Type mismatch: expected a number {ctx}, found {}",
            other.type_name()
        )),
    }
}

fn next_rand_u64() -> u64 {
    let mut guard = rng_state().lock().expect("rng lock poisoned");
    let mut x = *guard;
    if x == 0 {
        x = 0x9E3779B97F4A7C15;
    }
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *guard = x;
    x.wrapping_mul(0x2545F4914F6CDD1D)
}

fn format_string(template: &str, args: &[Value]) -> Result<String, String> {
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    let mut auto_idx = 0usize;
    while i < chars.len() {
        if chars[i] == '{' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                out.push('{');
                i += 2;
                continue;
            }
            let mut j = i + 1;
            let mut idx_str = String::new();
            while j < chars.len() && chars[j] != '}' {
                idx_str.push(chars[j]);
                j += 1;
            }
            if j >= chars.len() {
                return Err("format: unmatched '{' in template".to_string());
            }
            let val = if idx_str.is_empty() {
                let v = args.get(auto_idx).cloned().ok_or_else(|| {
                    format!("format: missing argument for placeholder at index {auto_idx}")
                })?;
                auto_idx += 1;
                v
            } else {
                let idx: usize = idx_str.parse().map_err(|_| {
                    format!("format: invalid placeholder '{{{idx_str}}}'")
                })?;
                args.get(idx).cloned().ok_or_else(|| {
                    format!("format: missing argument for placeholder {{{idx_str}}}")
                })?
            };
            out.push_str(&val.display());
            i = j + 1;
        } else if chars[i] == '}' {
            if i + 1 < chars.len() && chars[i + 1] == '}' {
                out.push('}');
                i += 2;
                continue;
            }
            return Err("format: unmatched '}' in template".to_string());
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Ok(out)
}

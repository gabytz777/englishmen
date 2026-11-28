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
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
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

/// Recover a poisoned mutex instead of panicking in a user-callable builtin.
fn lock_rng() -> MutexGuard<'static, u64> {
    rng_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn next_rand() -> f64 {
    let mut guard = lock_rng();
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

/// Locate a value inside a string, array or object key, returning -1 when it
/// is not present. Strings search by substring; arrays use value equality.
fn index_of_value(haystack: &Value, needle: &Value) -> i64 {
    match (haystack, needle) {
        (Value::String(s), Value::String(part)) => s
            .find(part.as_str())
            // Match the runtime string methods: indices count characters,
            // not UTF-8 bytes.
            .map(|byte| s[..byte].chars().count() as i64)
            .unwrap_or(-1),
        (Value::Array(items), _) => items
            .borrow()
            .iter()
            .position(|v| v.equals(needle))
            .map(|i| i as i64)
            .unwrap_or(-1),
        (Value::Object(fields), Value::String(key)) => fields
            .borrow()
            .keys()
            .position(|k| k == key)
            .map(|i| i as i64)
            .unwrap_or(-1),
        _ => -1,
    }
}

fn contains_value(haystack: &Value, needle: &Value) -> bool {
    index_of_value(haystack, needle) >= 0
}

fn slice_index(raw: f64, len: usize, name: &str) -> Result<usize, String> {
    if !raw.is_finite() || raw.fract() != 0.0 {
        return Err(format!("{name} expects integer slice indexes"));
    }
    let index = raw as i64;
    let index = if index < 0 {
        len as i64 + index
    } else {
        index
    };
    Ok(index.clamp(0, len as i64) as usize)
}

/// Validate a non-negative, integer count or target length argument.
fn expect_count(args: &[Value], idx: usize, name: &str) -> Result<usize, String> {
    let n = expect_num(args, idx, &format!("as the count for {name}"))?;
    if !n.is_finite() || n.fract() != 0.0 || n < 0.0 {
        return Err(format!("{name} expects a non-negative integer count"));
    }
    if n >= usize::MAX as f64 {
        return Err(format!("{name} count is too large"));
    }
    Ok(n as usize)
}

/// Pad a string to a target length measured in characters, repeating `fill`.
fn pad_string(
    text: &str,
    target: usize,
    fill: &str,
    _name: &str,
    at_start: bool,
) -> Result<String, String> {
    // Match the string padding methods: a capped width and a space fallback.
    let target = target.min(100_000);
    let len = text.chars().count();
    if target <= len {
        return Ok(text.to_string());
    }
    let missing = target - len;
    let unit = if fill.is_empty() { " " } else { fill };
    let filler: Vec<char> = unit.chars().collect();
    let mut padding = String::new();
    for i in 0..missing {
        padding.push(filler[i % filler.len()]);
    }
    Ok(if at_start {
        format!("{padding}{text}")
    } else {
        format!("{text}{padding}")
    })
}

/// Take the first `count` items/chars, or drop them and keep the remainder.
fn take_or_drop(source: &Value, count: usize, drop: bool, name: &str) -> Result<Value, String> {
    match source {
        Value::Array(a) => {
            let items = a.borrow();
            let n = count.min(items.len());
            let range = if drop { n..items.len() } else { 0..n };
            Ok(Value::new_array(items[range].to_vec()))
        }
        Value::String(s) => {
            let chars: Vec<char> = s.chars().collect();
            let n = count.min(chars.len());
            let range = if drop { n..chars.len() } else { 0..n };
            Ok(Value::String(chars[range].iter().collect()))
        }
        other => Err(format!(
            "Type mismatch: {name} expects an array or string, found {}",
            other.type_name()
        )),
    }
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
                    match t.parse::<f64>() {
                        Ok(number) if number.is_finite() => Ok(Value::Number(number)),
                        _ => Err(format!(
                            "toNumber could not convert string \"{}\" to a number",
                            s
                        )),
                    }
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

    g.insert(
        "isNaN".to_string(),
        native("isNaN", |args| {
            exact_args(args, 1, "isNaN")?;
            Ok(Value::Bool(matches!(
                arg(args, 0),
                Value::Number(n) if n.is_nan()
            )))
        }),
    );

    g.insert(
        "parseInt".to_string(),
        native("parseInt", |args| {
            at_least_args(args, 1, "parseInt")?;
            no_extra(args, 2, "parseInt")?;
            let src = expect_str(args, 0, "for parseInt")?;
            let radix = if args.len() == 2 {
                expect_num(args, 1, "as the radix for parseInt")?
            } else {
                10.0
            };
            if !radix.is_finite() || radix.fract() != 0.0 || !(2.0..=36.0).contains(&radix) {
                return Err("parseInt radix must be an integer between 2 and 36".to_string());
            }
            let text = src.trim();
            let (negative, digits) = match text.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, text.strip_prefix('+').unwrap_or(text)),
            };
            if digits.is_empty() {
                return Err(format!("parseInt could not parse \"{src}\""));
            }
            let mut result = 0i128;
            for c in digits.chars() {
                let digit = c.to_digit(radix as u32).ok_or_else(|| {
                    format!("parseInt could not parse \"{src}\" as base {}", radix as u32)
                })?;
                result = result
                    .checked_mul(radix as i128)
                    .and_then(|v| v.checked_add(digit as i128))
                    .ok_or_else(|| format!("parseInt number is too large: \"{src}\""))?;
            }
            if negative {
                result = -result;
            }
            Ok(Value::Number(result as f64))
        }),
    );

    g.insert(
        "parseFloat".to_string(),
        native("parseFloat", |args| {
            exact_args(args, 1, "parseFloat")?;
            let src = expect_str(args, 0, "for parseFloat")?;
            src.trim()
                .parse::<f64>()
                .map(Value::Number)
                .map_err(|_| format!("parseFloat could not parse \"{src}\""))
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

    g.insert(
        "take".to_string(),
        native("take", |args| {
            exact_args(args, 2, "take")?;
            let count = expect_count(args, 1, "take")?;
            take_or_drop(arg(args, 0), count, false, "take")
        }),
    );

    g.insert(
        "drop".to_string(),
        native("drop", |args| {
            exact_args(args, 2, "drop")?;
            let count = expect_count(args, 1, "drop")?;
            take_or_drop(arg(args, 0), count, true, "drop")
        }),
    );

    g.insert(
        "keys".to_string(),
        native("keys", |args| {
            exact_args(args, 1, "keys")?;
            match arg(args, 0) {
                Value::Object(o) => Ok(Value::new_array(
                    o.borrow().keys().map(|k| Value::String(k.clone())).collect(),
                )),
                other => Err(format!(
                    "Type mismatch: keys expects an object, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    g.insert(
        "values".to_string(),
        native("values", |args| {
            exact_args(args, 1, "values")?;
            match arg(args, 0) {
                Value::Object(o) => Ok(Value::new_array(o.borrow().values().cloned().collect())),
                other => Err(format!(
                    "Type mismatch: values expects an object, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    g.insert(
        "entries".to_string(),
        native("entries", |args| {
            exact_args(args, 1, "entries")?;
            match arg(args, 0) {
                Value::Object(fields) => Ok(Value::new_array(
                    fields
                        .borrow()
                        .iter()
                        .map(|(k, v)| Value::new_array(vec![Value::String(k.clone()), v.clone()]))
                        .collect(),
                )),
                other => Err(format!(
                    "Type mismatch: entries expects an object, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    g.insert(
        "fromEntries".to_string(),
        native("fromEntries", |args| {
            exact_args(args, 1, "fromEntries")?;
            let entries = expect_array(args, 0, "for fromEntries")?;
            let mut fields = BTreeMap::new();
            for (i, entry) in entries.iter().enumerate() {
                let Value::Array(pair) = entry else {
                    return Err(format!(
                        "fromEntries expects an array of [key, value] pairs, found {} at index {i}",
                        entry.type_name()
                    ));
                };
                let pair = pair.borrow();
                if pair.len() != 2 {
                    return Err(format!(
                        "fromEntries expects exactly 2 values per entry, found {} at index {i}",
                        pair.len()
                    ));
                }
                let Value::String(key) = &pair[0] else {
                    return Err(format!(
                        "fromEntries expects string keys, found {} at index {i}",
                        pair[0].type_name()
                    ));
                };
                if fields.insert(key.clone(), pair[1].clone()).is_some() {
                    return Err(format!("fromEntries found duplicate key \"{key}\""));
                }
            }
            Ok(Value::new_object(fields))
        }),
    );

    g.insert(
        "first".to_string(),
        native("first", |args| {
            exact_args(args, 1, "first")?;
            match arg(args, 0) {
                Value::Array(a) => Ok(a.borrow().first().cloned().unwrap_or(Value::Null)),
                Value::String(s) => Ok(s
                    .chars()
                    .next()
                    .map(|c| Value::String(c.to_string()))
                    .unwrap_or(Value::Null)),
                other => Err(format!(
                    "Type mismatch: first expects an array or string, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    g.insert(
        "last".to_string(),
        native("last", |args| {
            exact_args(args, 1, "last")?;
            match arg(args, 0) {
                Value::Array(a) => Ok(a.borrow().last().cloned().unwrap_or(Value::Null)),
                Value::String(s) => Ok(s
                    .chars()
                    .next_back()
                    .map(|c| Value::String(c.to_string()))
                    .unwrap_or(Value::Null)),
                other => Err(format!(
                    "Type mismatch: last expects an array or string, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    g.insert(
        "indexOf".to_string(),
        native("indexOf", |args| {
            exact_args(args, 2, "indexOf")?;
            Ok(Value::Number(index_of_value(arg(args, 0), arg(args, 1)) as f64))
        }),
    );

    g.insert(
        "contains".to_string(),
        native("contains", |args| {
            exact_args(args, 2, "contains")?;
            Ok(Value::Bool(contains_value(arg(args, 0), arg(args, 1))))
        }),
    );

    g.insert(
        "slice".to_string(),
        native("slice", |args| {
            at_least_args(args, 2, "slice")?;
            no_extra(args, 3, "slice")?;
            let source = arg(args, 0).clone();
            let len = match &source {
                Value::Array(a) => a.borrow().len(),
                Value::String(s) => s.chars().count(),
                other => {
                    return Err(format!(
                        "Type mismatch: slice expects an array or string, found {}",
                        other.type_name()
                    ))
                }
            };
            let start = slice_index(expect_num(args, 1, "as the start for slice")?, len, "slice")?;
            let end = match args.get(2) {
                Some(v) => slice_index(expect_num_value(v, "as the end for slice")?, len, "slice")?,
                None => len,
            };
            if end < start {
                return Err("slice expects end >= start".to_string());
            }
            match source {
                Value::Array(a) => Ok(Value::new_array(
                    a.borrow()[start..end].to_vec(),
                )),
                Value::String(s) => Ok(Value::String(
                    s.chars().skip(start).take(end - start).collect(),
                )),
                _ => unreachable!(),
            }
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
        "median".to_string(),
        native("median", |args| {
            exact_args(args, 1, "median")?;
            let items = expect_array(args, 0, "for median")?;
            if items.is_empty() {
                return Err("median expects a non-empty array".to_string());
            }
            let mut numbers = Vec::with_capacity(items.len());
            for (i, value) in items.iter().enumerate() {
                let n = expect_num_value(value, &format!("at index {i} for median"))?;
                if !n.is_finite() {
                    return Err(format!(
                        "median expects finite numbers, found {n} at index {i}"
                    ));
                }
                numbers.push(n);
            }
            numbers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mid = numbers.len() / 2;
            let result = if numbers.len() % 2 == 1 {
                numbers[mid]
            } else {
                numbers[mid - 1] / 2.0 + numbers[mid] / 2.0
            };
            Ok(Value::Number(result))
        }),
    );

    g.insert(
        "log2".to_string(),
        native("log2", |args| {
            exact_args(args, 1, "log2")?;
            let n = expect_num(args, 0, "for log2")?;
            if !n.is_finite() || n <= 0.0 {
                return Err("log2 expects a finite positive number".to_string());
            }
            Ok(Value::Number(n.log2()))
        }),
    );

    g.insert(
        "log10".to_string(),
        native("log10", |args| {
            exact_args(args, 1, "log10")?;
            let n = expect_num(args, 0, "for log10")?;
            if !n.is_finite() || n <= 0.0 {
                return Err("log10 expects a finite positive number".to_string());
            }
            Ok(Value::Number(n.log10()))
        }),
    );

    g.insert(
        "toDegrees".to_string(),
        native("toDegrees", |args| {
            exact_args(args, 1, "toDegrees")?;
            let n = expect_num(args, 0, "for toDegrees")?;
            if !n.is_finite() {
                return Err("toDegrees expects a finite number".to_string());
            }
            Ok(Value::Number(n.to_degrees()))
        }),
    );

    g.insert(
        "toRadians".to_string(),
        native("toRadians", |args| {
            exact_args(args, 1, "toRadians")?;
            let n = expect_num(args, 0, "for toRadians")?;
            if !n.is_finite() {
                return Err("toRadians expects a finite number".to_string());
            }
            Ok(Value::Number(n.to_radians()))
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
            // Use i128 so the full i64 range (including MIN..MAX) is safe.
            let span = (max_i as i128 - min_i as i128 + 1) as u128;
            let v = min_i as i128 + (next_rand_u64() as u128 % span) as i128;
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
            let mut guard = lock_rng();
            let bits = n.to_bits();
            *guard = bits ^ 0xA0761D6478BD642F;
            if *guard == 0 {
                *guard = 0x9E3779B97F4A7C15;
            }
            Ok(Value::Null)
        }),
    );

    g.insert(
        "randomChoice".to_string(),
        native("randomChoice", |args| {
            exact_args(args, 1, "randomChoice")?;
            let items = expect_array(args, 0, "for randomChoice")?;
            if items.is_empty() {
                return Err("randomChoice expects a non-empty array".to_string());
            }
            let index = (next_rand_u64() % items.len() as u64) as usize;
            Ok(items[index].clone())
        }),
    );

    g.insert(
        "shuffle".to_string(),
        native("shuffle", |args| {
            exact_args(args, 1, "shuffle")?;
            let mut items = expect_array(args, 0, "for shuffle")?;
            for i in (1..items.len()).rev() {
                let j = (next_rand_u64() % (i as u64 + 1)) as usize;
                items.swap(i, j);
            }
            Ok(Value::new_array(items))
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

    g.insert(
        "split".to_string(),
        native("split", |args| {
            exact_args(args, 2, "split")?;
            let s = expect_str(args, 0, "for split")?;
            let separator = expect_str(args, 1, "as the separator for split")?;
            let parts: Vec<Value> = if separator.is_empty() {
                s.chars().map(|c| Value::String(c.to_string())).collect()
            } else {
                s.split(separator.as_str())
                    .map(|part| Value::String(part.to_string()))
                    .collect()
            };
            Ok(Value::new_array(parts))
        }),
    );

    g.insert(
        "join".to_string(),
        native("join", |args| {
            at_least_args(args, 1, "join")?;
            no_extra(args, 2, "join")?;
            let items = expect_array(args, 0, "for join")?;
            let separator = if args.len() == 2 {
                expect_str(args, 1, "as the separator for join")?
            } else {
                String::new()
            };
            let parts: Vec<String> = items.iter().map(|v| v.display()).collect();
            Ok(Value::String(parts.join(&separator)))
        }),
    );

    g.insert(
        "replace".to_string(),
        native("replace", |args| {
            exact_args(args, 3, "replace")?;
            let s = expect_str(args, 0, "for replace")?;
            let from = expect_str(args, 1, "as the text to replace for replace")?;
            let to = expect_str(args, 2, "as the replacement for replace")?;
            if from.is_empty() {
                return Ok(Value::String(s));
            }
            Ok(Value::String(s.replace(&from, &to)))
        }),
    );

    g.insert(
        "startsWith".to_string(),
        native("startsWith", |args| {
            exact_args(args, 2, "startsWith")?;
            let s = expect_str(args, 0, "for startsWith")?;
            let prefix = expect_str(args, 1, "as the prefix for startsWith")?;
            Ok(Value::Bool(s.starts_with(&prefix)))
        }),
    );

    g.insert(
        "endsWith".to_string(),
        native("endsWith", |args| {
            exact_args(args, 2, "endsWith")?;
            let s = expect_str(args, 0, "for endsWith")?;
            let suffix = expect_str(args, 1, "as the suffix for endsWith")?;
            Ok(Value::Bool(s.ends_with(&suffix)))
        }),
    );

    g.insert(
        "repeat".to_string(),
        native("repeat", |args| {
            exact_args(args, 2, "repeat")?;
            let s = expect_str(args, 0, "for repeat")?;
            let count = expect_num(args, 1, "as the count for repeat")?;
            if !count.is_finite() || count.fract() != 0.0 || count < 0.0 {
                return Err("repeat expects a non-negative integer count".to_string());
            }
            if count > usize::MAX as f64 {
                return Err("repeat count is too large".to_string());
            }
            let times = count as usize;
            let size = s
                .len()
                .checked_mul(times)
                .ok_or_else(|| "repeat output is too large".to_string())?;
            if size > 1_000_000 {
                return Err("repeat output would exceed 1000000 bytes".to_string());
            }
            Ok(Value::String(s.repeat(times)))
        }),
    );

    g.insert(
        "padStart".to_string(),
        native("padStart", |args| {
            at_least_args(args, 2, "padStart")?;
            no_extra(args, 3, "padStart")?;
            let text = expect_str(args, 0, "for padStart")?;
            let target = expect_count(args, 1, "padStart")?;
            let fill = if args.len() == 3 {
                expect_str(args, 2, "as the fill for padStart")?
            } else {
                " ".to_string()
            };
            Ok(Value::String(pad_string(
                &text, target, &fill, "padStart", true,
            )?))
        }),
    );

    g.insert(
        "padEnd".to_string(),
        native("padEnd", |args| {
            at_least_args(args, 2, "padEnd")?;
            no_extra(args, 3, "padEnd")?;
            let text = expect_str(args, 0, "for padEnd")?;
            let target = expect_count(args, 1, "padEnd")?;
            let fill = if args.len() == 3 {
                expect_str(args, 2, "as the fill for padEnd")?
            } else {
                " ".to_string()
            };
            Ok(Value::String(pad_string(
                &text, target, &fill, "padEnd", false,
            )?))
        }),
    );

    g.insert(
        "charCode".to_string(),
        native("charCode", |args| {
            exact_args(args, 1, "charCode")?;
            match arg(args, 0) {
                Value::String(s) => Ok(s
                    .chars()
                    .next()
                    .map(|c| Value::Number(c as u32 as f64))
                    .unwrap_or(Value::Null)),
                other => Err(format!(
                    "Type mismatch: charCode expects a string, found {}",
                    other.type_name()
                )),
            }
        }),
    );

    g.insert(
        "fromCharCode".to_string(),
        native("fromCharCode", |args| {
            exact_args(args, 1, "fromCharCode")?;
            let n = expect_num(args, 0, "for fromCharCode")?;
            if !n.is_finite() || n.fract() != 0.0 || n < 0.0 || n > 0x10FFFF as f64 {
                return Err(
                    "fromCharCode expects a Unicode code point from 0 to 1114111".to_string(),
                );
            }
            char::from_u32(n as u32)
                .map(|c| Value::String(c.to_string()))
                .ok_or_else(|| {
                    format!(
                        "fromCharCode: {} is not a valid Unicode code point",
                        n as u32
                    )
                })
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
        "writeBytes".to_string(),
        native("writeBytes", |args| {
            exact_args(args, 2, "writeBytes")?;
            let path = expect_str(args, 0, "as the path for writeBytes")?;
            let Value::Array(items) = &args[1] else {
                return Err(format!(
                    "Type mismatch: expected an array of bytes as the bytes for writeBytes, found {}",
                    args[1].type_name()
                ));
            };
            let items = items.borrow();
            let mut bytes = Vec::with_capacity(items.len());
            for item in items.iter() {
                match item {
                    Value::Number(n) if n.fract() == 0.0 && *n >= 0.0 && *n <= 255.0 => {
                        bytes.push(*n as u8)
                    }
                    other => {
                        return Err(format!(
                            "writeBytes: expected byte values from 0 to 255, found {}",
                            other.display()
                        ))
                    }
                }
            }
            fs::write(&path, bytes)
                .map_err(|e| format!("writeBytes could not write '{path}': {e}"))?;
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
    let mut guard = lock_rng();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile;
    use crate::interpreter::Interpreter;
    use crate::lexer::tokenize;
    use crate::parser::parse;
    use crate::vm::Vm;
    use std::rc::Rc;

    const EXPECTED_BUILTINS: &[&str] = &[
        "len",
        "type",
        "toNumber",
        "toString",
        "toBool",
        "isNumber",
        "isString",
        "isArray",
        "isObject",
        "isBool",
        "isNull",
        "isFunction",
        "isNaN",
        "parseInt",
        "parseFloat",
        "range",
        "sum",
        "average",
        "min",
        "max",
        "abs",
        "clamp",
        "sorted",
        "reversed",
        "zip",
        "take",
        "drop",
        "keys",
        "values",
        "entries",
        "fromEntries",
        "first",
        "last",
        "indexOf",
        "contains",
        "slice",
        "pow",
        "sqrt",
        "floor",
        "ceil",
        "round",
        "sign",
        "median",
        "log2",
        "log10",
        "toDegrees",
        "toRadians",
        "random",
        "randomInt",
        "seed",
        "randomChoice",
        "shuffle",
        "input",
        "print",
        "format",
        "trim",
        "upper",
        "lower",
        "split",
        "join",
        "replace",
        "startsWith",
        "endsWith",
        "repeat",
        "padStart",
        "padEnd",
        "charCode",
        "fromCharCode",
        "readFile",
        "writeFile",
        "writeBytes",
        "appendFile",
        "fileExists",
        "deleteFile",
        "listFiles",
        "makeDir",
        "clock",
        "timestamp",
        "env",
        "args",
        "sleep",
        "assert",
        "jsonEncode",
        "jsonDecode",
    ];

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        let table = globals();
        match table.get(name) {
            Some(Value::Native(f)) => (f.func)(args),
            Some(other) => panic!("{name} is not a native function: {}", other.display()),
            None => panic!("missing builtin: {name}"),
        }
    }

    fn ok(name: &str, args: &[Value]) -> Value {
        call(name, args).unwrap_or_else(|e| panic!("{name} failed unexpectedly: {e}"))
    }

    fn err(name: &str, args: &[Value]) -> String {
        call(name, args).expect_err("expected a builtin error")
    }

    fn number(value: Value) -> f64 {
        match value {
            Value::Number(n) => n,
            other => panic!("expected a number, found {}", other.display()),
        }
    }

    fn text(value: Value) -> String {
        match value {
            Value::String(s) => s,
            other => panic!("expected a string, found {}", other.display()),
        }
    }

    fn boolean(value: Value) -> bool {
        match value {
            Value::Bool(b) => b,
            other => panic!("expected a boolean, found {}", other.display()),
        }
    }

    fn items(value: Value) -> Vec<Value> {
        match value {
            Value::Array(a) => a.borrow().clone(),
            other => panic!("expected an array, found {}", other.display()),
        }
    }

    fn strings(values: &[&str]) -> Vec<Value> {
        values.iter().map(|s| Value::String((*s).to_string())).collect()
    }

    fn numbers(values: &[f64]) -> Vec<Value> {
        values.iter().map(|n| Value::Number(*n)).collect()
    }

    fn object(fields: &[(&str, Value)]) -> Value {
        Value::new_object(
            fields
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn registry_exposes_all_builtins_as_native_functions() {
        let table = globals();
        assert_eq!(
            table.len(),
            EXPECTED_BUILTINS.len(),
            "registry has {} builtins, expected list has {}",
            table.len(),
            EXPECTED_BUILTINS.len()
        );
        for name in EXPECTED_BUILTINS {
            match table.get(*name) {
                Some(Value::Native(f)) => {
                    assert_eq!(f.name, *name, "native function has the wrong name");
                }
                _ => panic!("missing builtin: {name}"),
            }
        }
    }

    #[test]
    fn conversions_and_type_checks() {
        assert_eq!(text(ok("type", &[Value::Null])), "null");
        assert_eq!(text(ok("type", &[Value::new_array(vec![])])), "array");
        assert_eq!(number(ok("toNumber", &[Value::String(" 3.5 ".into())])), 3.5);
        assert_eq!(number(ok("toNumber", &[Value::Bool(true)])), 1.0);
        assert_eq!(number(ok("toNumber", &[Value::Null])), 0.0);
        assert!(err("toNumber", &[Value::String("NaN".into())]).contains("could not convert"));
        assert!(err("toNumber", &[Value::String("inf".into())]).contains("could not convert"));
        assert_eq!(text(ok("toString", &[Value::Number(12.0)])), "12");
        assert!(boolean(ok("toBool", &[Value::String("x".into())])));
        assert!(!boolean(ok("toBool", &[Value::Number(0.0)])));
        assert_eq!(number(ok("parseInt", &[Value::String("ff".into()), Value::Number(16.0)])), 255.0);
        assert_eq!(number(ok("parseInt", &[Value::String("-101".into()), Value::Number(2.0)])), -5.0);
        assert_eq!(number(ok("parseFloat", &[Value::String("-2.25e2".into())])), -225.0);
        assert!(boolean(ok("isNumber", &[Value::Number(1.0)])));
        assert!(boolean(ok("isString", &[Value::String("".into())])));
        assert!(boolean(ok("isArray", &[Value::new_array(vec![])])));
        assert!(boolean(ok("isObject", &[object(&[])])));
        assert!(boolean(ok("isBool", &[Value::Bool(false)])));
        assert!(boolean(ok("isNull", &[Value::Null])));
        assert!(boolean(ok("isFunction", &[Value::Native(Rc::new(crate::value::NativeFunction {
            name: "x".to_string(),
            func: Box::new(|_| Ok(Value::Null)),
        }))])));
        assert!(err("parseInt", &[Value::String("12".into()), Value::Number(1.0)]).contains("2 and 36"));
    }

    #[test]
    fn string_builtins() {
        assert_eq!(text(ok("trim", &[Value::String("  hi  ".into())])), "hi");
        assert_eq!(text(ok("upper", &[Value::String("héllo".into())])), "HÉLLO");
        assert_eq!(text(ok("lower", &[Value::String("HÉLLO".into())])), "héllo");
        assert_eq!(text(ok("reversed", &[Value::String("abc".into())])), "cba");
        let split = items(ok("split", &[Value::String("a,b,c".into()), Value::String(",".into())]));
        assert_eq!(split.iter().map(|v| text(v.clone())).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(items(ok("split", &[Value::String("ab".into()), Value::String("".into())])).len(), 2);
        assert_eq!(text(ok("join", &[Value::new_array(strings(&["a", "b"])), Value::String("-".into())])), "a-b");
        assert_eq!(text(ok("join", &[Value::new_array(strings(&["a", "b"]))])), "ab");
        assert_eq!(text(ok("replace", &[Value::String("a-b-a".into()), Value::String("a".into()), Value::String("c".into())])), "c-b-c");
        assert!(boolean(ok("startsWith", &[Value::String("hello".into()), Value::String("he".into())])));
        assert!(boolean(ok("endsWith", &[Value::String("hello".into()), Value::String("lo".into())])));
        assert_eq!(text(ok("repeat", &[Value::String("ab".into()), Value::Number(3.0)])), "ababab");
        assert!(err("repeat", &[Value::String("x".into()), Value::Number(1e9)])
            .contains("exceed 1000000"));
        assert_eq!(text(ok("slice", &[Value::String("hello".into()), Value::Number(-3.0)])), "llo");
        assert_eq!(number(ok("indexOf", &[Value::String("hello".into()), Value::String("ll".into())])), 2.0);
        assert_eq!(number(ok("indexOf", &[Value::String("héllo".into()), Value::String("llo".into())])), 2.0);
        assert_eq!(number(ok("indexOf", &[Value::String("héllo".into()), Value::String("é".into())])), 1.0);
        assert!(boolean(ok("contains", &[Value::String("hello".into()), Value::String("ell".into())])));
        assert_eq!(text(ok("first", &[Value::String("abc".into())])), "a");
        assert_eq!(text(ok("last", &[Value::String("abc".into())])), "c");
        assert_eq!(text(ok("format", &[Value::String("{1}-{0} {{ok}}".into()), Value::String("a".into()), Value::String("b".into())])), "b-a {ok}");
        assert!(err("format", &[Value::String("{".into())]).contains("unmatched"));
    }

    #[test]
    fn collection_builtins() {
        let one_to_four = items(ok("range", &[Value::Number(1.0), Value::Number(5.0)]));
        assert_eq!(one_to_four.iter().map(|v| number(v.clone())).collect::<Vec<_>>(), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(items(ok("range", &[Value::Number(0.0), Value::Number(10.0), Value::Number(3.0)])).len(), 4);
        assert_eq!(number(ok("sum", &[Value::new_array(numbers(&[1.0, 2.0, 3.0]))])), 6.0);
        assert_eq!(number(ok("average", &[Value::new_array(numbers(&[2.0, 4.0]))])), 3.0);
        assert_eq!(number(ok("min", &[Value::new_array(numbers(&[3.0, -1.0, 2.0]))])), -1.0);
        assert_eq!(number(ok("max", &[Value::Number(4.0), Value::Number(9.0), Value::Number(2.0)])), 9.0);
        assert_eq!(items(ok("sorted", &[Value::new_array(numbers(&[3.0, 1.0, 2.0]))])).len(), 3);
        let sorted_strings = items(ok("sorted", &[Value::new_array(strings(&["b", "a"]))]));
        assert_eq!(
            sorted_strings.iter().map(|v| text(v.clone())).collect::<Vec<_>>(),
            ["a", "b"]
        );
        let reversed = items(ok("reversed", &[Value::new_array(numbers(&[1.0, 2.0]))]));
        assert_eq!(reversed.iter().map(|v| number(v.clone())).collect::<Vec<_>>(), [2.0, 1.0]);
        let pairs = items(ok("zip", &[Value::new_array(numbers(&[1.0, 2.0])), Value::new_array(strings(&["a"]))]));
        assert_eq!(pairs.len(), 1);
        let pair = items(pairs[0].clone());
        assert_eq!(number(pair[0].clone()), 1.0);
        assert_eq!(text(pair[1].clone()), "a");
        let object = object(&[("b", Value::Number(2.0)), ("a", Value::Number(1.0))]);
        let keys = items(ok("keys", &[object.clone()]));
        assert_eq!(keys.iter().map(|v| text(v.clone())).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(items(ok("values", &[object.clone()])).iter().map(|v| number(v.clone())).collect::<Vec<_>>(), [1.0, 2.0]);
        assert!(boolean(ok("contains", &[object.clone(), Value::String("a".into())])));
        assert_eq!(number(ok("indexOf", &[object, Value::String("b".into())])), 1.0);
        let array = Value::new_array(numbers(&[1.0, 2.0, 3.0, 4.0]));
        assert_eq!(number(ok("first", &[array.clone()])), 1.0);
        assert_eq!(number(ok("last", &[array.clone()])), 4.0);
        assert_eq!(items(ok("slice", &[array, Value::Number(1.0), Value::Number(3.0)])).iter().map(|v| number(v.clone())).collect::<Vec<_>>(), [2.0, 3.0]);
        assert!(err("range", &[Value::Number(1.0), Value::Number(2.0), Value::Number(0.0)]).contains("non-zero"));
        assert!(err("average", &[Value::new_array(vec![])]).contains("empty"));
    }

    #[test]
    fn math_builtins() {
        assert_eq!(number(ok("abs", &[Value::Number(-3.0)])), 3.0);
        assert_eq!(number(ok("pow", &[Value::Number(2.0), Value::Number(10.0)])), 1024.0);
        assert_eq!(number(ok("sqrt", &[Value::Number(9.0)])), 3.0);
        assert_eq!(number(ok("floor", &[Value::Number(1.9)])), 1.0);
        assert_eq!(number(ok("ceil", &[Value::Number(1.1)])), 2.0);
        assert_eq!(number(ok("round", &[Value::Number(1.5)])), 2.0);
        assert_eq!(number(ok("sign", &[Value::Number(-8.0)])), -1.0);
        assert_eq!(number(ok("clamp", &[Value::Number(12.0), Value::Number(0.0), Value::Number(10.0)])), 10.0);
        assert!(err("sqrt", &[Value::Number(-1.0)]).contains("negative"));
        assert!(err("clamp", &[Value::Number(1.0), Value::Number(5.0), Value::Number(0.0)]).contains("min <= max"));
    }

    #[test]
    fn padding_and_codepoint_builtins() {
        assert_eq!(
            text(ok("padStart", &[Value::String("7".into()), Value::Number(3.0)])),
            "  7"
        );
        assert_eq!(
            text(ok(
                "padStart",
                &[
                    Value::String("7".into()),
                    Value::Number(4.0),
                    Value::String("ab".into())
                ]
            )),
            "aba7"
        );
        assert_eq!(
            text(ok(
                "padEnd",
                &[
                    Value::String("é".into()),
                    Value::Number(3.0),
                    Value::String("-".into())
                ]
            )),
            "é--"
        );
        assert_eq!(
            text(ok("padEnd", &[Value::String("abcd".into()), Value::Number(2.0)])),
            "abcd"
        );
        assert_eq!(
            text(ok(
                "padStart",
                &[
                    Value::String("x".into()),
                    Value::Number(2.0),
                    Value::String("".into())
                ]
            )),
            " x",
            "an empty fill falls back to a space, matching string methods"
        );
        assert!(err("padEnd", &[Value::String("x".into()), Value::Number(-1.0)])
            .contains("non-negative integer"));

        assert_eq!(number(ok("charCode", &[Value::String("é".into())])), 233.0);
        assert!(matches!(ok("charCode", &[Value::String("".into())]), Value::Null));
        assert_eq!(text(ok("fromCharCode", &[Value::Number(233.0)])), "é");
        assert!(err("charCode", &[Value::Number(65.0)]).contains("expects a string"));
        assert!(err("fromCharCode", &[Value::Number(0xD800 as f64)])
            .contains("not a valid Unicode code point"));
        assert!(err("fromCharCode", &[Value::Number(0x110000 as f64)]).contains("0 to 1114111"));
    }

    #[test]
    fn object_entry_builtins() {
        let source = object(&[
            ("b", Value::Number(2.0)),
            ("a", Value::String("x".into())),
        ]);
        let pairs = items(ok("entries", &[source]));
        assert_eq!(pairs.len(), 2);
        let first = items(pairs[0].clone());
        assert_eq!(text(first[0].clone()), "a");
        assert_eq!(text(first[1].clone()), "x");
        let second = items(pairs[1].clone());
        assert_eq!(text(second[0].clone()), "b");
        assert_eq!(number(second[1].clone()), 2.0);

        assert_eq!(
            ok("fromEntries", &[Value::new_array(pairs)]).display(),
            "{a: \"x\", b: 2}"
        );
        assert_eq!(ok("entries", &[object(&[])]).display(), "[]");

        assert!(err("entries", &[Value::new_array(vec![])]).contains("expects an object"));
        assert!(err(
            "fromEntries",
            &[Value::new_array(vec![Value::Number(1.0)])]
        )
        .contains("pairs"));
        assert!(err(
            "fromEntries",
            &[Value::new_array(vec![Value::new_array(strings(&["a"]))])]
        )
        .contains("exactly 2"));
        assert!(err(
            "fromEntries",
            &[Value::new_array(vec![Value::new_array(vec![
                Value::Number(1.0),
                Value::Null
            ])])]
        )
        .contains("string keys"));
        assert!(err(
            "fromEntries",
            &[Value::new_array(vec![
                Value::new_array(vec![Value::String("a".into()), Value::Number(1.0)]),
                Value::new_array(vec![Value::String("a".into()), Value::Number(2.0)])
            ])]
        )
        .contains("duplicate key"));
    }

    #[test]
    fn take_and_drop_builtins() {
        let array = Value::new_array(numbers(&[1.0, 2.0, 3.0]));
        assert_eq!(
            items(ok("take", &[array.clone(), Value::Number(2.0)]))
                .iter()
                .map(|v| number(v.clone()))
                .collect::<Vec<_>>(),
            [1.0, 2.0]
        );
        assert_eq!(
            items(ok("drop", &[array.clone(), Value::Number(1.0)]))
                .iter()
                .map(|v| number(v.clone()))
                .collect::<Vec<_>>(),
            [2.0, 3.0]
        );
        assert_eq!(
            text(ok("take", &[Value::String("héllo".into()), Value::Number(2.0)])),
            "hé"
        );
        assert_eq!(
            text(ok("drop", &[Value::String("héllo".into()), Value::Number(2.0)])),
            "llo"
        );
        assert_eq!(items(ok("take", &[array.clone(), Value::Number(10.0)])).len(), 3);
        assert!(items(ok("drop", &[array, Value::Number(10.0)])).is_empty());
        assert!(err("take", &[Value::Number(1.0), Value::Number(1.0)])
            .contains("expects an array or string"));
        assert!(err("drop", &[Value::new_array(vec![]), Value::Number(1.5)])
            .contains("non-negative integer"));
    }

    #[test]
    fn extra_math_and_number_predicate_builtins() {
        assert_eq!(
            number(ok(
                "median",
                &[Value::new_array(numbers(&[3.0, 1.0, 2.0]))]
            )),
            2.0
        );
        assert_eq!(
            number(ok(
                "median",
                &[Value::new_array(numbers(&[4.0, 1.0, 3.0, 2.0]))]
            )),
            2.5
        );
        assert!((number(ok("log2", &[Value::Number(8.0)])) - 3.0).abs() < 1e-12);
        assert!((number(ok("log10", &[Value::Number(1000.0)])) - 3.0).abs() < 1e-12);
        assert!(
            (number(ok("toDegrees", &[Value::Number(std::f64::consts::PI)])) - 180.0).abs() < 1e-9
        );
        assert!(
            (number(ok("toRadians", &[Value::Number(180.0)])) - std::f64::consts::PI).abs() < 1e-12
        );
        assert!(boolean(ok("isNaN", &[Value::Number(f64::NAN)])));
        assert!(!boolean(ok("isNaN", &[Value::Number(1.0)])));

        assert!(err("median", &[Value::new_array(vec![])]).contains("non-empty"));
        assert!(err(
            "median",
            &[Value::new_array(vec![Value::Number(1.0), Value::String("x".into())])]
        )
        .contains("expected a number"));
        assert!(err("log2", &[Value::Number(0.0)]).contains("positive"));
        assert!(err("log10", &[Value::Number(-1.0)]).contains("positive"));
        assert!(err("toRadians", &[Value::Number(f64::INFINITY)]).contains("finite"));
        assert!(!boolean(ok("isNaN", &[Value::String("x".into())])));
    }

    #[test]
    fn random_builtins_respect_seed_and_bounds() {
        ok("seed", &[Value::Number(42.0)]);
        let first = number(ok("random", &[]));
        ok("seed", &[Value::Number(42.0)]);
        assert_eq!(number(ok("random", &[])), first);
        assert!((0.0..1.0).contains(&first));

        for _ in 0..50 {
            let value = number(ok("randomInt", &[Value::Number(-5.0), Value::Number(5.0)]));
            assert!((-5.0..=5.0).contains(&value) && value.fract() == 0.0);
        }
        let values = Value::new_array(numbers(&[1.0, 2.0, 3.0]));
        for _ in 0..20 {
            let chosen = number(ok("randomChoice", &[values.clone()]));
            assert!((1.0..=3.0).contains(&chosen));
            let mut shuffled = items(ok("shuffle", &[values.clone()]))
                .iter()
                .map(|v| number(v.clone()))
                .collect::<Vec<_>>();
            shuffled.sort_by(|a, b| a.partial_cmp(b).unwrap());
            assert_eq!(shuffled, [1.0, 2.0, 3.0]);
        }
        assert!(err("randomInt", &[Value::Number(5.0), Value::Number(1.0)]).contains("min <= max"));
        assert!(err("randomChoice", &[Value::new_array(vec![])]).contains("non-empty"));
        assert!(err("random", &[Value::Number(1.0)]).contains("expects 0 arguments"));
    }

    #[test]
    fn json_round_trip_and_validation() {
        let nested = object(&[
            ("name", Value::String("Ada\n\"the \"\"great\"\"\"".into())),
            ("scores", Value::new_array(vec![Value::Number(1.0), Value::Bool(true), Value::Null])),
        ]);
        let encoded = text(ok("jsonEncode", &[nested]));
        assert_eq!(
            encoded,
            r#"{"name":"Ada\n\"the \"\"great\"\"\"","scores":[1,true,null]}"#
        );
        let decoded = ok("jsonDecode", &[Value::String(encoded)]);
        let Value::Object(fields) = decoded else {
            panic!("expected an object");
        };
        assert_eq!(fields.borrow().get("name").unwrap().display(), "Ada\n\"the \"\"great\"\"\"");
        assert_eq!(text(ok("jsonEncode", &[ok("jsonDecode", &[Value::String("[1, 2, {\"a\": false}]".into())])])), r#"[1,2,{"a":false}]"#);
        assert!(err("jsonDecode", &[Value::String("{\"a\":}".into())]).contains("jsonDecode"));
        assert!(err("jsonDecode", &[Value::String("null extra".into())]).contains("trailing"));
        assert!(err("jsonEncode", &[Value::Number(f64::NAN)]).contains("non-finite"));
        assert!(err("jsonEncode", &[Value::Null, Value::Null]).contains("expects 1 argument"));
    }

    #[test]
    fn filesystem_builtins() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("em-builtins-{}-{nanos}", std::process::id()));
        let dir_name = dir.to_string_lossy().to_string();
        let file = dir.join("note.txt");
        let file_name = file.to_string_lossy().to_string();

        ok("makeDir", &[Value::String(dir_name.clone())]);
        assert!(matches!(
            ok("fileExists", &[Value::String(file_name.clone())]),
            Value::Bool(false)
        ));
        ok("writeFile", &[Value::String(file_name.clone()), Value::String("one".into())]);
        ok("appendFile", &[Value::String(file_name.clone()), Value::String("two".into())]);
        assert_eq!(text(ok("readFile", &[Value::String(file_name.clone())])), "onetwo");
        assert!(matches!(
            ok("fileExists", &[Value::String(file_name.clone())]),
            Value::Bool(true)
        ));
        let listed = items(ok("listFiles", &[Value::String(dir_name.clone())]));
        assert_eq!(listed.len(), 1);
        assert_eq!(text(listed[0].clone()), "note.txt");
        ok("deleteFile", &[Value::String(file_name.clone())]);
        assert!(matches!(
            ok("fileExists", &[Value::String(file_name)]),
            Value::Bool(false)
        ));
        ok("deleteFile", &[Value::String(dir_name)]);
        assert!(!dir.exists());
    }

    #[test]
    fn system_and_assertion_builtins() {
        assert!(number(ok("clock", &[])) > 1_000_000_000.0);
        assert!(number(ok("timestamp", &[])) > 1_000_000_000_000.0);
        assert!(!items(ok("args", &[])).is_empty());
        assert!(matches!(
            ok("env", &[Value::String("__EM_BUILTINS_TEST_UNSET__".into())]),
            Value::Null
        ));
        assert!(matches!(ok("assert", &[Value::Bool(true)]), Value::Null));
        assert!(err(
            "assert",
            &[Value::Bool(false), Value::String("broken".into())]
        )
        .contains("broken"));
        ok("sleep", &[Value::Number(0.0)]);
        assert!(err("sleep", &[Value::Number(-1.0)]).contains("non-negative"));
    }

    #[test]
    fn argument_validation_is_consistent() {
        assert!(err("len", &[]).contains("expects 1 argument"));
        assert!(err(
            "range",
            &[
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(1.0),
                Value::Number(1.0)
            ]
        )
        .contains("at most 3 arguments"));
        assert!(err("sqrt", &[Value::String("4".into())]).contains("expected a number"));
        assert!(err(
            "sorted",
            &[Value::new_array(vec![Value::Number(1.0), Value::String("a".into())])]
        )
        .contains("all numbers or all strings"));
        assert!(err("toNumber", &[Value::String("oops".into())]).contains("could not convert"));
        assert!(err("jsonDecode", &[Value::Number(1.0)]).contains("expected a string"));
        assert!(err("take", &[Value::Number(1.0)]).contains("expects 2 arguments"));
        assert!(
            err("padStart", &[Value::String("x".into())])
                .contains("expects at least 2 arguments")
        );
        assert!(err("isNaN", &[]).contains("expects 1 argument"));
    }

    #[test]
    fn builtins_are_reachable_from_both_backends() {
        let src = concat!(
            "a = len(range(1, 5))\n",
            "b = upper(\"hi\")\n",
            "c = jsonEncode({ name: \"Ada\", scores: [1, 2] })\n",
            "d = sum([1, 2, 3])\n",
            "parts = split(\"a,b,c\", \",\")\n",
            "e = parts[1]\n",
            "f = padStart(\"7\", 3, \"0\")\n",
            "g = entries({ b: 2, a: 1 })\n",
            "h = median([3, 1, 2])\n",
            "i = fromEntries(zip([\"x\"], [4]))\n",
            "j = take(\"héllo\", 2)\n",
            "k = drop([1, 2, 3], 1)\n",
            "l = charCode(\"A\")\n",
            "m = fromCharCode(66)\n",
            "n = log2(8)\n",
            "o = log10(100)\n",
            "p = isNaN(1)\n",
            "q = padEnd(\"ab\", 4, \"xy\")\n",
            "r = toDegrees(0)\n",
            "s = toRadians(0)\n"
        );
        let program = parse(tokenize(src).unwrap()).unwrap();

        let mut interpreter = Interpreter::new();
        interpreter.run(&program).unwrap();
        let mut vm = Vm::new();
        vm.run(compile(&program).unwrap()).unwrap();

        for (name, expected) in [
            ("a", "4"),
            ("b", "HI"),
            ("c", r#"{"name":"Ada","scores":[1,2]}"#),
            ("d", "6"),
            ("e", "b"),
            ("f", "007"),
            ("g", r#"[["a", 1], ["b", 2]]"#),
            ("h", "2"),
            ("i", "{x: 4}"),
            ("j", "hé"),
            ("k", "[2, 3]"),
            ("l", "65"),
            ("m", "B"),
            ("n", "3"),
            ("o", "2"),
            ("p", "false"),
            ("q", "abxy"),
            ("r", "0"),
            ("s", "0"),
        ] {
            let interpreted = interpreter.globals.get(name).unwrap().display();
            let compiled = vm.globals.get(name).unwrap().display();
            assert_eq!(interpreted, expected, "{name} in interpreter");
            assert_eq!(compiled, expected, "{name} in VM");
        }
    }
}

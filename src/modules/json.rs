//! A deliberately small, dependency-free JSON implementation for Englishmen.
//!
//! It covers the JSON data model supported by [`Value`] (numbers, strings,
//! booleans, null, arrays and objects) and is enough for configuration files
//! and simple APIs. Functions and compiled closures cannot be encoded.

#[cfg(test)]
use crate::modules::support::call_native;
use crate::runtime::{expect_string, native};
use crate::value::{format_number, Value};
use std::collections::BTreeMap;

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "encode".to_string(),
        native("json.encode", |args| {
            let value = args.get(0).unwrap_or(&Value::Null);
            Ok(Value::String(encode(value)?))
        }),
    );

    fields.insert(
        "decode".to_string(),
        native("json.decode", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for json.decode",
            )?;
            decode(&text)
        }),
    );

    fields.insert(
        "pretty".to_string(),
        native("json.pretty", |args| {
            let value = args.get(0).unwrap_or(&Value::Null);
            let mut out = String::new();
            write_pretty(value, 0, &mut out)?;
            Ok(Value::String(out))
        }),
    );

    fields.insert(
        "isValid".to_string(),
        native("json.isValid", |args| {
            let Ok(text) = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for json.isValid",
            ) else {
                return Ok(Value::Bool(false));
            };
            Ok(Value::Bool(decode(&text).is_ok()))
        }),
    );

    fields.insert(
        "get".to_string(),
        native("json.get", |args| {
            let root = args.get(0).unwrap_or(&Value::Null);
            let path = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the path for json.get",
            )?;
            Ok(get(root, &path))
        }),
    );

    fields.insert(
        "set".to_string(),
        native("json.set", |args| {
            let root = args.get(0).unwrap_or(&Value::Null).clone();
            let path = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the path for json.set",
            )?;
            let value = args.get(2).cloned().unwrap_or(Value::Null);
            set(root, &path, value)
        }),
    );

    Value::new_object(fields)
}

/// Serialize a runtime value as compact JSON text.
fn encode(value: &Value) -> Result<String, String> {
    let mut out = String::new();
    write_json(value, &mut out)?;
    Ok(out)
}

fn write_json(value: &Value, out: &mut String) -> Result<(), String> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if !n.is_finite() {
                return Err("Cannot encode a non-finite number as JSON".to_string());
            }
            out.push_str(&format_number(*n));
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.borrow().iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out)?;
            }
            out.push(']');
        }
        Value::Object(fields) => {
            out.push('{');
            for (i, (key, item)) in fields.borrow().iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_json(item, out)?;
            }
            out.push('}');
        }
        other => {
            return Err(format!(
                "Cannot encode a {} as JSON",
                other.type_name()
            ))
        }
    }
    Ok(())
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c < '\u{20}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Serialize with two-space indentation for human-readable output.
fn write_pretty(value: &Value, level: usize, out: &mut String) -> Result<(), String> {
    match value {
        Value::Array(items) => {
            let items = items.borrow();
            if items.is_empty() {
                out.push_str("[]");
                return Ok(());
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                write_indent(out, level + 1);
                write_pretty(item, level + 1, out)?;
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            write_indent(out, level);
            out.push(']');
            Ok(())
        }
        Value::Object(fields) => {
            let fields = fields.borrow();
            if fields.is_empty() {
                out.push_str("{}");
                return Ok(());
            }
            out.push_str("{\n");
            for (i, (key, item)) in fields.iter().enumerate() {
                write_indent(out, level + 1);
                write_string(key, out);
                out.push_str(": ");
                write_pretty(item, level + 1, out)?;
                if i + 1 < fields.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            write_indent(out, level);
            out.push('}');
            Ok(())
        }
        other => write_json(other, out),
    }
}

fn write_indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

/// Parse JSON text into a runtime value.
fn decode(text: &str) -> Result<Value, String> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        pos: 0,
    };
    parser.skip_whitespace();
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.pos != parser.chars.len() {
        return Err(format!(
            "Unexpected trailing JSON characters at position {}",
            parser.pos
        ));
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn skip_whitespace(&mut self) {
        while self.pos < self.chars.len()
            && matches!(self.chars[self.pos], ' ' | '\t' | '\n' | '\r')
        {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn consume_literal(&mut self, literal: &str) -> bool {
        let chars: Vec<char> = literal.chars().collect();
        if self.chars[self.pos..].starts_with(&chars) {
            self.pos += chars.len();
            true
        } else {
            false
        }
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some('"') => Ok(Value::String(self.parse_string()?)),
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('t') if self.consume_literal("true") => Ok(Value::Bool(true)),
            Some('f') if self.consume_literal("false") => Ok(Value::Bool(false)),
            Some('n') if self.consume_literal("null") => Ok(Value::Null),
            Some('-') | Some('0'..='9') => self.parse_number(),
            _ => Err(format!(
                "Unexpected JSON character at position {}",
                self.pos
            )),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err("Unterminated JSON string".to_string());
            };
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(escape) = self.peek() else {
                        return Err("Unterminated JSON escape sequence".to_string());
                    };
                    self.pos += 1;
                    match escape {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{08}'),
                        'f' => out.push('\u{0c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let mut code = self.parse_hex4()?;
                            if (0xd800..=0xdbff).contains(&code) {
                                if self.peek() != Some('\\')
                                    || self.chars.get(self.pos + 1) != Some(&'u')
                                {
                                    return Err("Expected a low surrogate in JSON string".to_string());
                                }
                                self.pos += 2;
                                let low = self.parse_hex4()?;
                                if !(0xdc00..=0xdfff).contains(&low) {
                                    return Err("Invalid low surrogate in JSON string".to_string());
                                }
                                code = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                            }
                            let decoded = char::from_u32(code)
                                .ok_or_else(|| "Invalid Unicode escape in JSON string".to_string())?;
                            out.push(decoded);
                        }
                        _ => return Err("Invalid JSON escape sequence".to_string()),
                    }
                }
                c if c < '\u{20}' => {
                    return Err("Control character in JSON string".to_string())
                }
                c => out.push(c),
            }
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, String> {
        if self.pos + 4 > self.chars.len() {
            return Err("Incomplete Unicode escape in JSON string".to_string());
        }
        let mut value = 0u32;
        for _ in 0..4 {
            let c = self.chars[self.pos];
            self.pos += 1;
            let digit = c.to_digit(16).ok_or_else(|| {
                "Invalid Unicode escape in JSON string".to_string()
            })?;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
        {
            self.pos += 1;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        if !is_valid_json_number(&text) {
            return Err(format!("Invalid JSON number '{text}'"));
        }
        let n = text
            .parse::<f64>()
            .map_err(|_| format!("Invalid JSON number '{text}'"))?;
        Ok(Value::Number(n))
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Value::new_array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.parse_value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(']') => {
                    self.pos += 1;
                    return Ok(Value::new_array(items));
                }
                _ => return Err(format!("Expected ',' or ']' at position {}", self.pos)),
            }
        }
    }

    fn parse_object(&mut self) -> Result<Value, String> {
        self.pos += 1; // '{'
        let mut fields = BTreeMap::new();
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Value::new_object(fields));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some('"') {
                return Err(format!("Expected a string key at position {}", self.pos));
            }
            let key = self.parse_string()?;
            self.skip_whitespace();
            if self.peek() != Some(':') {
                return Err(format!("Expected ':' at position {}", self.pos));
            }
            self.pos += 1;
            self.skip_whitespace();
            let value = self.parse_value()?;
            fields.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    return Ok(Value::new_object(fields));
                }
                _ => return Err(format!("Expected ',' or '}}' at position {}", self.pos)),
            }
        }
    }
}

fn is_valid_json_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'-') {
        i += 1;
    }
    if i >= bytes.len() {
        return false;
    }
    if bytes[i] == b'0' {
        i += 1;
        if i < bytes.len() && bytes[i].is_ascii_digit() {
            return false; // leading zeros are not JSON
        }
    } else if bytes[i].is_ascii_digit() {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    } else {
        return false;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        if i >= bytes.len() || !bytes[i].is_ascii_digit() {
            return false;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
        i += 1;
        if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
            i += 1;
        }
        if i >= bytes.len() || !bytes[i].is_ascii_digit() {
            return false;
        }
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    i == bytes.len()
}

/// Look up a dotted path such as `"user.name"` or `"items.0"`. Missing paths
/// resolve to `null`, just like reading a missing object field in Englishmen.
fn get(root: &Value, path: &str) -> Value {
    let mut current = root.clone();
    if path.is_empty() {
        return current;
    }
    for segment in path.split('.') {
        current = match &current {
            Value::Object(fields) => fields.borrow().get(segment).cloned().unwrap_or(Value::Null),
            Value::Array(items) => segment
                .parse::<usize>()
                .ok()
                .and_then(|index| items.borrow().get(index).cloned())
                .unwrap_or(Value::Null),
            _ => Value::Null,
        };
    }
    current
}

/// Store `value` at a dotted path, creating missing objects along the way.
/// Arrays must already have the target index. The original root is returned so
/// callers can encode it; mutations are visible through shared containers.
fn set(root: Value, path: &str, value: Value) -> Result<Value, String> {
    if path.is_empty() {
        return Ok(value);
    }
    let segments: Vec<&str> = path.split('.').collect();
    let root = match root {
        Value::Null => Value::new_object(BTreeMap::new()),
        Value::Object(_) | Value::Array(_) => root,
        other => {
            return Err(format!(
                "json.set expects an object, array or null root, found {}",
                other.type_name()
            ))
        }
    };
    set_at(&root, &segments, value)?;
    Ok(root)
}

fn set_at(container: &Value, segments: &[&str], value: Value) -> Result<(), String> {
    let segment = segments[0];
    let last = segments.len() == 1;
    match container {
        Value::Object(fields) => {
            if last {
                fields.borrow_mut().insert(segment.to_string(), value);
                return Ok(());
            }
            let next = fields.borrow().get(segment).cloned().unwrap_or(Value::Null);
            let child = match next {
                Value::Object(_) | Value::Array(_) => next,
                Value::Null => Value::new_object(BTreeMap::new()),
                other => {
                    return Err(format!(
                        "Cannot descend into a {} at '{}' for json.set",
                        other.type_name(), segment
                    ))
                }
            };
            // Re-link the child before recursing: a newly created object must
            // be reachable from the original root.
            fields.borrow_mut().insert(segment.to_string(), child.clone());
            set_at(&child, &segments[1..], value)
        }
        Value::Array(items) => {
            let index = segment.parse::<usize>().map_err(|_| {
                format!("Expected an array index in path, found '{segment}'")
            })?;
            if last {
                let mut items = items.borrow_mut();
                if index < items.len() {
                    items[index] = value;
                } else if index == items.len() {
                    items.push(value);
                } else {
                    return Err("json.set array index is past the end of the array".to_string());
                }
                return Ok(());
            }
            let child = items.borrow().get(index).cloned().ok_or_else(|| {
                "json.set array index is past the end of the array".to_string()
            })?;
            if !matches!(child, Value::Object(_) | Value::Array(_)) {
                return Err("Cannot descend into a non-container for json.set".to_string());
            }
            set_at(&child, &segments[1..], value)
        }
        other => Err(format!(
            "json.set cannot descend into a {} at '{}'",
            other.type_name(), segment
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        call_native(&module(), name, args)
    }

    #[test]
    fn encode_round_trips_all_json_types() {
        let mut fields = BTreeMap::new();
        fields.insert("z".to_string(), Value::Bool(true));
        fields.insert("a".to_string(), Value::Null);
        let source = Value::new_object(fields);
        let text = call("encode", &[source.clone()]).unwrap();
        assert_eq!(
            text.display(),
            "{\"a\":null,\"z\":true}"
        );
        assert_eq!(
            call("decode", &[text]).unwrap().display(),
            "{a: null, z: true}"
        );
        let function = crate::runtime::native("x", |_| Ok(Value::Null));
        assert!(call("encode", &[function]).is_err());
    }

    #[test]
    fn pretty_prints_indented_json() {
        let source = call(
            "decode",
            &[Value::String("{\"a\":[1,2]}".to_string())],
        )
        .unwrap();
        assert_eq!(
            call("pretty", &[source]).unwrap().display(),
            "{\n  \"a\": [\n    1,\n    2\n  ]\n}"
        );
    }

    #[test]
    fn escape_sequences_and_unicode() {
        let text = call("encode", &[Value::String("a\"\n\u{08}☃".to_string())]).unwrap();
        assert_eq!(text.display(), "\"a\\\"\\n\\b☃\"");
        let json = "\"\\u2603 \\uD83D\\uDE00\"";
        let Value::String(decoded) = call("decode", &[Value::String(json.to_string())]).unwrap() else {
            panic!("decoded value is not a string");
        };
        assert_eq!(decoded, "☃ 😀");
    }

    #[test]
    fn decode_rejects_invalid_json() {
        for bad in ["", "{", "[1,]", "{\"a\" 1}", "01", "tru", "[1] extra", "'x'"] {
            assert!(call("decode", &[Value::String(bad.to_string())]).is_err(), "{bad}");
            assert!(matches!(
                call("isValid", &[Value::String(bad.to_string())]).unwrap(),
                Value::Bool(false)
            ));
        }
        assert!(matches!(
            call("isValid", &[Value::String("{\"a\": [1, 2.5, true]}".to_string())]).unwrap(),
            Value::Bool(true)
        ));
    }

    #[test]
    fn get_and_set_dotted_paths() {
        let parsed = call(
            "decode",
            &[Value::String("{\"user\":{\"name\":\"Ada\"},\"items\":[1,2]}".to_string())],
        )
        .unwrap();
        assert_eq!(call("get", &[parsed.clone(), Value::String("user.name".to_string())]).unwrap().display(), "Ada");
        assert_eq!(call("get", &[parsed.clone(), Value::String("items.1".to_string())]).unwrap().display(), "2");
        assert!(matches!(call("get", &[parsed.clone(), Value::String("missing.path".to_string())]).unwrap(), Value::Null));
        let updated = call(
            "set",
            &[
                parsed.clone(),
                Value::String("user.age".to_string()),
                Value::Number(36.0),
            ],
        )
        .unwrap();
        assert_eq!(call("get", &[updated.clone(), Value::String("user.age".to_string())]).unwrap().display(), "36");
        assert_eq!(call("get", &[updated.clone(), Value::String("user.name".to_string())]).unwrap().display(), "Ada");
        assert!(call("set", &[Value::Null, Value::String("a.b".to_string()), Value::Bool(true)]).is_ok());
    }
}

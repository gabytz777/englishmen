//! Binary byte buffers for Englishmen.
//!
//! A buffer is an ordinary Englishmen array whose items are whole numbers in
//! `0..=255`. Conversion, packing and slicing functions return new arrays;
//! mutating helpers return the original buffer so calls can be chained.

#[cfg(test)]
use crate::modules::support::call_native;
use crate::modules::support::{array_items, expect_array, expect_path};
use crate::runtime::{expect_number, expect_string, native};
use crate::value::Value;
use std::collections::BTreeMap;

/// Build the `bytes` module object.
pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "fromString".to_string(),
        native("bytes.fromString", |args| {
            let text = string_arg(args, 0, "bytes.fromString", "text")?;
            Ok(make_buffer(text.as_bytes()))
        }),
    );

    fields.insert(
        "toString".to_string(),
        native("bytes.toString", |args| {
            let raw = buffer_bytes(
                arg(args, 0, "bytes.toString", "buffer")?,
                "for bytes.toString",
            )?;
            String::from_utf8(raw)
                .map(Value::String)
                .map_err(|_| "bytes.toString cannot decode the buffer as UTF-8".to_string())
        }),
    );

    fields.insert(
        "of".to_string(),
        native("bytes.of", |args| {
            let mut out = Vec::with_capacity(args.len());
            for (index, value) in args.iter().enumerate() {
                out.push(byte_value(
                    value,
                    &format!("as byte {} for bytes.of", index + 1),
                )?);
            }
            Ok(make_buffer(&out))
        }),
    );

    fields.insert(
        "length".to_string(),
        native("bytes.length", |args| {
            let raw = buffer_bytes(arg(args, 0, "bytes.length", "buffer")?, "for bytes.length")?;
            Ok(Value::Number(raw.len() as f64))
        }),
    );

    fields.insert(
        "get".to_string(),
        native("bytes.get", |args| {
            let raw = buffer_bytes(arg(args, 0, "bytes.get", "buffer")?, "for bytes.get")?;
            let index = number_arg(args, 1, "bytes.get", "index")?;
            let resolved = exact_index(raw.len(), index, "as the index for bytes.get")?;
            Ok(Value::Number(raw[resolved] as f64))
        }),
    );

    fields.insert(
        "set".to_string(),
        native("bytes.set", |args| {
            let buffer = arg(args, 0, "bytes.set", "buffer")?;
            // Validate the whole buffer first so a malformed input cannot be
            // silently repaired by a single `set`.
            let raw = buffer_bytes(buffer, "for bytes.set")?;
            let target = expect_array(buffer, "for bytes.set")?;
            let index = number_arg(args, 1, "bytes.set", "index")?;
            let resolved = exact_index(raw.len(), index, "as the index for bytes.set")?;
            let byte = byte_value(
                arg(args, 2, "bytes.set", "value")?,
                "as the byte for bytes.set",
            )?;
            target.borrow_mut()[resolved] = Value::Number(byte as f64);
            Ok(buffer.clone())
        }),
    );

    fields.insert(
        "concat".to_string(),
        native("bytes.concat", |args| {
            let mut out = Vec::new();
            for (index, value) in args.iter().enumerate() {
                out.extend(buffer_bytes(
                    value,
                    &format!("as buffer {} for bytes.concat", index + 1),
                )?);
            }
            Ok(make_buffer(&out))
        }),
    );

    fields.insert(
        "slice".to_string(),
        native("bytes.slice", |args| {
            let raw = buffer_bytes(arg(args, 0, "bytes.slice", "buffer")?, "for bytes.slice")?;
            let start = number_arg(args, 1, "bytes.slice", "start index")?;
            let from = slice_index(raw.len(), start, "as the start index for bytes.slice")?;
            let to = match args.get(2) {
                Some(end) => {
                    let end = expect_number(end, "as the end index for bytes.slice")?;
                    slice_index(raw.len(), end, "as the end index for bytes.slice")?
                }
                None => raw.len(),
            };
            if to < from {
                Ok(make_buffer(&[]))
            } else {
                Ok(make_buffer(&raw[from..to]))
            }
        }),
    );

    fields.insert(
        "copy".to_string(),
        native("bytes.copy", |args| {
            let buffer = arg(args, 0, "bytes.copy", "buffer")?;
            let raw = buffer_bytes(buffer, "for bytes.copy")?;
            Ok(make_buffer(&raw))
        }),
    );

    fields.insert(
        "fromHex".to_string(),
        native("bytes.fromHex", |args| {
            let text = string_arg(args, 0, "bytes.fromHex", "hex text")?;
            Ok(make_buffer(&parse_hex(&text)?))
        }),
    );

    fields.insert(
        "toHex".to_string(),
        native("bytes.toHex", |args| {
            let raw = buffer_bytes(arg(args, 0, "bytes.toHex", "buffer")?, "for bytes.toHex")?;
            let parts: Vec<String> = raw.iter().map(|byte| format!("{byte:02x}")).collect();
            Ok(Value::String(parts.join(" ")))
        }),
    );

    fields.insert(
        "u8".to_string(),
        native("bytes.u8", |args| {
            let value = unsigned_arg(args, "bytes.u8", 256.0, "255")?;
            Ok(make_buffer(&[value as u8]))
        }),
    );

    fields.insert(
        "u16".to_string(),
        native("bytes.u16", |args| {
            let value = unsigned_arg(args, "bytes.u16", 65_536.0, "65535")?;
            Ok(make_buffer(&(value as u16).to_le_bytes()))
        }),
    );

    fields.insert(
        "u32".to_string(),
        native("bytes.u32", |args| {
            let value = unsigned_arg(args, "bytes.u32", 4_294_967_296.0, "4294967295")?;
            Ok(make_buffer(&(value as u32).to_le_bytes()))
        }),
    );

    fields.insert(
        "u64".to_string(),
        native("bytes.u64", |args| {
            // `u64::MAX` rounds up to 2^64 as an f64, so the exclusive bound
            // keeps an out-of-range 2^64 from silently becoming `u64::MAX`.
            let value = unsigned_arg(
                args,
                "bytes.u64",
                18_446_744_073_709_551_616.0,
                "18446744073709551615",
            )?;
            Ok(make_buffer(&(value as u64).to_le_bytes()))
        }),
    );

    fields.insert(
        "i16".to_string(),
        native("bytes.i16", |args| {
            let value = signed_arg(args, "bytes.i16", -32_768.0, 32_768.0, "-32768", "32767")?;
            Ok(make_buffer(&(value as i16).to_le_bytes()))
        }),
    );

    fields.insert(
        "i32".to_string(),
        native("bytes.i32", |args| {
            let value = signed_arg(
                args,
                "bytes.i32",
                -2_147_483_648.0,
                2_147_483_648.0,
                "-2147483648",
                "2147483647",
            )?;
            Ok(make_buffer(&(value as i32).to_le_bytes()))
        }),
    );

    fields.insert(
        "i64".to_string(),
        native("bytes.i64", |args| {
            let value = signed_arg(
                args,
                "bytes.i64",
                -9_223_372_036_854_775_808.0,
                9_223_372_036_854_775_808.0,
                "-9223372036854775808",
                "9223372036854775807",
            )?;
            Ok(make_buffer(&(value as i64).to_le_bytes()))
        }),
    );

    fields.insert(
        "f64".to_string(),
        native("bytes.f64", |args| {
            let value = number_arg(args, 0, "bytes.f64", "number")?;
            Ok(make_buffer(&value.to_le_bytes()))
        }),
    );

    fields.insert(
        "read".to_string(),
        native("bytes.read", |args| {
            let path = expect_path(
                arg(args, 0, "bytes.read", "path")?,
                "as the path for bytes.read",
            )?;
            let raw = std::fs::read(&path)
                .map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
            Ok(make_buffer(&raw))
        }),
    );

    fields.insert(
        "write".to_string(),
        native("bytes.write", |args| {
            let path = expect_path(
                arg(args, 0, "bytes.write", "path")?,
                "as the path for bytes.write",
            )?;
            let raw = buffer_bytes(arg(args, 1, "bytes.write", "buffer")?, "for bytes.write")?;
            std::fs::write(&path, &raw)
                .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
            Ok(Value::Bool(true))
        }),
    );

    Value::new_object(fields)
}

/// Fetch a required argument with a helpful, Englishmen-style error.
fn arg<'a>(
    args: &'a [Value],
    index: usize,
    function: &str,
    description: &str,
) -> Result<&'a Value, String> {
    args.get(index)
        .ok_or_else(|| format!("{function} expects argument {} ({description})", index + 1))
}

fn string_arg(
    args: &[Value],
    index: usize,
    function: &str,
    description: &str,
) -> Result<String, String> {
    let context = format!("as the {description} for {function}");
    expect_string(arg(args, index, function, description)?, &context)
}

fn number_arg(
    args: &[Value],
    index: usize,
    function: &str,
    description: &str,
) -> Result<f64, String> {
    let context = format!("as the {description} for {function}");
    expect_number(arg(args, index, function, description)?, &context)
}

/// Validate a single byte value.
fn byte_value(value: &Value, context: &str) -> Result<u8, String> {
    let n = expect_number(value, context)?;
    if !n.is_finite() || n.fract() != 0.0 || !(0.0..=255.0).contains(&n) {
        return Err(format!(
            "Expected a whole number from 0 to 255 {context}, found {n}"
        ));
    }
    Ok(n as u8)
}

/// Validate and copy a buffer into plain bytes, without holding a `RefCell`
/// borrow across other operations.
fn buffer_bytes(value: &Value, context: &str) -> Result<Vec<u8>, String> {
    let items = array_items(value, context)?;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| byte_value(item, &format!("at index {index} {context}")))
        .collect()
}

fn make_buffer(raw: &[u8]) -> Value {
    Value::new_array(raw.iter().map(|byte| Value::Number(*byte as f64)).collect())
}

/// Resolve a get/set index, where negative numbers count from the end.
fn exact_index(len: usize, n: f64, context: &str) -> Result<usize, String> {
    if !n.is_finite() || n.fract() != 0.0 {
        return Err(format!("Expected a whole number {context}, found {n}"));
    }
    let resolved = if n < 0.0 {
        len.checked_sub((-n) as usize)
    } else if (n as usize) < len {
        Some(n as usize)
    } else {
        None
    };
    resolved.ok_or_else(|| format!("Index {n} is outside a buffer of {len} bytes"))
}

/// Resolve a slice boundary the same way array/string slicing does: negative
/// values count from the end and out-of-range values clamp to the buffer edges.
fn slice_index(len: usize, n: f64, context: &str) -> Result<usize, String> {
    if !n.is_finite() || n.fract() != 0.0 {
        return Err(format!("Expected a whole number {context}, found {n}"));
    }
    if n < 0.0 {
        Ok(len.saturating_sub((-n) as usize))
    } else {
        Ok((n as usize).min(len))
    }
}

/// Accept compact `aabbcc` or spaced `aa bb cc` hex, with optional whitespace.
fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let compact: Vec<char> = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if compact.len() % 2 != 0 {
        return Err("bytes.fromHex expects an even number of hex digits".to_string());
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    for pair in compact.chunks_exact(2) {
        let high = pair[0].to_digit(16).ok_or_else(|| {
            format!(
                "Invalid hex byte '{}{}' for bytes.fromHex",
                pair[0], pair[1]
            )
        })?;
        let low = pair[1].to_digit(16).ok_or_else(|| {
            format!(
                "Invalid hex byte '{}{}' for bytes.fromHex",
                pair[0], pair[1]
            )
        })?;
        out.push(((high << 4) | low) as u8);
    }
    Ok(out)
}

fn unsigned_arg(
    args: &[Value],
    function: &str,
    upper_exclusive: f64,
    max_label: &str,
) -> Result<u64, String> {
    let n = number_arg(args, 0, function, "number")?;
    if !n.is_finite() || n.fract() != 0.0 || n < 0.0 || n >= upper_exclusive {
        return Err(format!(
            "{function} expects a whole number from 0 to {max_label}, found {n}"
        ));
    }
    Ok(n as u64)
}

fn signed_arg(
    args: &[Value],
    function: &str,
    lower: f64,
    upper_exclusive: f64,
    min_label: &str,
    max_label: &str,
) -> Result<i64, String> {
    let n = number_arg(args, 0, function, "number")?;
    if !n.is_finite() || n.fract() != 0.0 || n < lower || n >= upper_exclusive {
        return Err(format!(
            "{function} expects a whole number from {min_label} to {max_label}, found {n}"
        ));
    }
    Ok(n as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        call_native(&module(), name, args)
    }

    fn buffer(raw: &[u8]) -> Value {
        make_buffer(raw)
    }

    fn raw(value: &Value) -> Vec<u8> {
        match value {
            Value::Array(items) => items
                .borrow()
                .iter()
                .map(|item| match item {
                    Value::Number(n) => *n as u8,
                    other => panic!(
                        "buffer contains a {}: {}",
                        other.type_name(),
                        other.display()
                    ),
                })
                .collect(),
            other => panic!("expected a buffer, found {}", other.type_name()),
        }
    }

    fn number(n: f64) -> Value {
        Value::Number(n)
    }

    fn text(s: &str) -> Value {
        Value::String(s.to_string())
    }

    #[test]
    fn string_conversion_uses_utf8_and_rejects_invalid_bytes() {
        let bytes = call("fromString", &[text("héllo ☃")]).unwrap();
        assert_eq!(raw(&bytes), "héllo ☃".as_bytes());
        assert_eq!(call("toString", &[bytes]).unwrap().display(), "héllo ☃");
        assert_eq!(raw(&call("fromString", &[text("")]).unwrap()), []);
        assert!(call("toString", &[buffer(&[0x66, 0xff, 0x80])]).is_err());
        assert!(call("fromString", &[number(1.0)]).is_err());
        assert!(call("toString", &[number(1.0)]).is_err());
        assert!(call("toString", &[]).is_err());
    }

    #[test]
    fn of_accepts_only_whole_numbers_from_zero_to_255() {
        let value = call("of", &[number(0.0), number(127.0), number(255.0)]).unwrap();
        assert_eq!(raw(&value), [0, 127, 255]);
        assert_eq!(raw(&call("of", &[]).unwrap()), []);
        for bad in [-1.0, 256.0, 1.5, f64::NAN, f64::INFINITY] {
            assert!(call("of", &[number(bad)]).is_err(), "{bad}");
        }
        assert!(call("of", &[text("1")]).is_err());
        assert!(call("of", &[buffer(&[1])]).is_err());
    }

    #[test]
    fn length_get_and_set_handle_negative_indexes_and_identity() {
        let value = buffer(&[10, 20, 30]);
        assert_eq!(call("length", &[value.clone()]).unwrap().display(), "3");
        assert_eq!(
            call("get", &[value.clone(), number(1.0)])
                .unwrap()
                .display(),
            "20"
        );
        assert_eq!(
            call("get", &[value.clone(), number(-1.0)])
                .unwrap()
                .display(),
            "30"
        );
        assert_eq!(
            call("get", &[value.clone(), number(-3.0)])
                .unwrap()
                .display(),
            "10"
        );

        let returned = call("set", &[value.clone(), number(-2.0), number(99.0)]).unwrap();
        let Value::Array(original) = &value else {
            unreachable!()
        };
        let Value::Array(returned_items) = &returned else {
            unreachable!()
        };
        assert!(Rc::ptr_eq(original, returned_items));
        assert_eq!(raw(&value), [10, 99, 30]);

        for index in [3.0, -4.0, 1.5, f64::NAN] {
            assert!(
                call("get", &[value.clone(), number(index)]).is_err(),
                "{index}"
            );
            assert!(
                call("set", &[value.clone(), number(index), number(1.0)]).is_err(),
                "{index}"
            );
        }
        assert!(call("set", &[value.clone(), number(0.0), number(-1.0)]).is_err());
        assert!(call("set", &[value.clone(), number(0.0), number(256.0)]).is_err());
        assert!(call("set", &[value.clone(), number(0.0), text("x")]).is_err());
        assert!(call("get", &[value]).is_err());
    }

    #[test]
    fn concat_slice_and_copy_return_independent_buffers() {
        let first = buffer(&[1, 2]);
        let second = buffer(&[3]);
        assert_eq!(
            raw(&call("concat", &[first.clone(), second.clone()]).unwrap()),
            [1, 2, 3]
        );
        assert_eq!(raw(&call("concat", &[]).unwrap()), []);

        let value = buffer(&[1, 2, 3, 4]);
        assert_eq!(
            raw(&call("slice", &[value.clone(), number(1.0)]).unwrap()),
            [2, 3, 4]
        );
        assert_eq!(
            raw(&call("slice", &[value.clone(), number(1.0), number(3.0)]).unwrap()),
            [2, 3]
        );
        assert_eq!(
            raw(&call("slice", &[value.clone(), number(-2.0)]).unwrap()),
            [3, 4]
        );
        assert_eq!(
            raw(&call("slice", &[value.clone(), number(0.0), number(-1.0)]).unwrap()),
            [1, 2, 3]
        );
        assert_eq!(
            raw(&call("slice", &[value.clone(), number(3.0), number(1.0)]).unwrap()),
            []
        );
        assert_eq!(
            raw(&call("slice", &[value.clone(), number(99.0)]).unwrap()),
            []
        );
        assert_eq!(
            raw(&call("slice", &[value.clone(), number(-99.0), number(4.0)]).unwrap()),
            [1, 2, 3, 4]
        );

        let copy = call("copy", &[value.clone()]).unwrap();
        assert_eq!(raw(&copy), [1, 2, 3, 4]);
        call("set", &[copy.clone(), number(0.0), number(9.0)]).unwrap();
        assert_eq!(raw(&copy), [9, 2, 3, 4]);
        assert_eq!(raw(&value), [1, 2, 3, 4]);

        assert!(call("slice", &[value.clone(), number(0.5)]).is_err());
        assert!(call("slice", &[value.clone(), number(1.0), text("2")]).is_err());
        assert!(call("copy", &[text("oops")]).is_err());
        assert!(call("concat", &[value, Value::new_array(vec![number(300.0)])]).is_err());
    }

    #[test]
    fn hex_conversion_accepts_spaces_and_compact_digits() {
        for source in ["aabbcc", "aa bb cc", "AA\tBB\nCC", "a ab bcc"] {
            assert_eq!(
                raw(&call("fromHex", &[text(source)]).unwrap()),
                [0xaa, 0xbb, 0xcc],
                "{source}"
            );
        }
        assert_eq!(raw(&call("fromHex", &[text("")]).unwrap()), []);
        assert_eq!(
            call("toHex", &[buffer(&[0x00, 0x0a, 0xff])])
                .unwrap()
                .display(),
            "00 0a ff"
        );
        assert_eq!(call("toHex", &[buffer(&[])]).unwrap().display(), "");
        for bad in ["a", "abc", "zz", "aa gg", "0x12"] {
            assert!(call("fromHex", &[text(bad)]).is_err(), "{bad}");
        }
        assert!(call("fromHex", &[number(1.0)]).is_err());
        assert!(call("toHex", &[text("aa")]).is_err());
        assert!(call("toHex", &[Value::new_array(vec![number(256.0)])]).is_err());
    }

    #[test]
    fn unsigned_packers_are_little_endian_and_range_checked() {
        assert_eq!(raw(&call("u8", &[number(255.0)]).unwrap()), [255]);
        assert_eq!(
            raw(&call("u16", &[number(0x1234u32 as f64)]).unwrap()),
            [0x34, 0x12]
        );
        assert_eq!(
            raw(&call("u32", &[number(0x1234_5678u32 as f64)]).unwrap()),
            [120, 86, 52, 18]
        );
        // Use an exactly representable f64 here: 0x...0708 rounds to ...0700.
        assert_eq!(
            raw(&call("u64", &[number(0x0102_0304_0506_0700u64 as f64)]).unwrap()),
            [0, 7, 6, 5, 4, 3, 2, 1]
        );
        for (name, bad) in [
            ("u8", 256.0),
            ("u16", -1.0),
            ("u32", 4_294_967_296.0),
            ("u64", 18_446_744_073_709_551_616.0),
        ] {
            assert!(call(name, &[number(bad)]).is_err(), "{name} {bad}");
        }
        for name in ["u8", "u16", "u32", "u64"] {
            assert!(call(name, &[number(1.5)]).is_err(), "{name}");
            assert!(call(name, &[text("1")]).is_err(), "{name}");
            assert!(call(name, &[]).is_err(), "{name}");
        }
    }

    #[test]
    fn signed_packers_use_twos_complement_little_endian() {
        assert_eq!(raw(&call("i16", &[number(-2.0)]).unwrap()), [0xfe, 0xff]);
        assert_eq!(
            raw(&call("i32", &[number(-2.0)]).unwrap()),
            [0xfe, 0xff, 0xff, 0xff]
        );
        assert_eq!(
            raw(&call("i64", &[number(-2.0)]).unwrap()),
            [0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
        assert_eq!(
            raw(&call("i16", &[number(-32_768.0)]).unwrap()),
            [0x00, 0x80]
        );
        assert_eq!(
            raw(&call("i16", &[number(32_767.0)]).unwrap()),
            [0xff, 0x7f]
        );
        for (name, bad) in [
            ("i16", 32_768.0),
            ("i16", -32_769.0),
            ("i32", -2_147_483_649.0),
            ("i64", 9_223_372_036_854_775_808.0),
        ] {
            assert!(call(name, &[number(bad)]).is_err(), "{name} {bad}");
        }
        for name in ["i16", "i32", "i64"] {
            assert!(call(name, &[number(0.5)]).is_err(), "{name}");
            assert!(call(name, &[text("0")]).is_err(), "{name}");
            assert!(call(name, &[]).is_err(), "{name}");
        }
    }

    #[test]
    fn f64_packs_ieee754_little_endian() {
        assert_eq!(
            raw(&call("f64", &[number(1.0)]).unwrap()),
            [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f]
        );
        assert_eq!(
            raw(&call("f64", &[number(-2.5)]).unwrap()),
            (-2.5f64).to_le_bytes()
        );
        assert_eq!(
            raw(&call("f64", &[number(f64::NAN)]).unwrap()),
            f64::NAN.to_le_bytes()
        );
        assert!(call("f64", &[text("1")]).is_err());
        assert!(call("f64", &[]).is_err());
    }

    #[test]
    fn binary_file_round_trip_preserves_all_byte_values() {
        let path = std::env::temp_dir().join(format!("em_bytes_module_{}.bin", std::process::id()));
        let name = path.display().to_string();
        let original = buffer(&[0, 65, 0xff, 0x80, 10]);
        let written = call("write", &[text(&name), original.clone()]).unwrap();
        assert!(matches!(written, Value::Bool(true)));
        let read = call("read", &[text(&name)]).unwrap();
        assert_eq!(raw(&read), [0, 65, 0xff, 0x80, 10]);
        assert!(call("toString", &[read]).is_err());

        assert!(call("read", &[number(1.0)]).is_err());
        assert!(call("write", &[number(1.0), original.clone()]).is_err());
        assert!(call("write", &[text(&name), text("not a buffer")]).is_err());
        assert!(call(
            "write",
            &[text(&name), Value::new_array(vec![number(256.0)])]
        )
        .is_err());
        assert!(call("read", &[text("/this/path/does/not/exist/em_bytes_test")]).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn all_functions_report_missing_arguments() {
        for name in [
            "fromString",
            "toString",
            "length",
            "get",
            "set",
            "copy",
            "fromHex",
            "toHex",
            "u8",
            "u16",
            "u32",
            "u64",
            "i16",
            "i32",
            "i64",
            "f64",
            "read",
            "write",
        ] {
            assert!(call(name, &[]).is_err(), "{name} accepted no arguments");
        }
        assert!(call("slice", &[buffer(&[1])]).is_err());
        assert!(call("concat", &[text("not a buffer")]).is_err());

        let malformed = Value::new_array(vec![text("x")]);
        assert!(call("length", &[malformed.clone()]).is_err());
        assert!(call("get", &[malformed.clone(), number(0.0)]).is_err());
        assert!(call("set", &[malformed.clone(), number(0.0), number(1.0)]).is_err());
        assert!(call("slice", &[malformed.clone(), number(0.0)]).is_err());
        assert!(call("copy", &[malformed]).is_err());
    }
}

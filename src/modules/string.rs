use crate::modules::support::{array_items, as_usize, call_native};
use crate::runtime::{expect_string, native};
use crate::value::Value;
use std::collections::BTreeMap;

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "upper".to_string(),
        native("string.upper", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for string.upper",
            )?;
            Ok(Value::String(text.to_uppercase()))
        }),
    );

    fields.insert(
        "lower".to_string(),
        native("string.lower", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for string.lower",
            )?;
            Ok(Value::String(text.to_lowercase()))
        }),
    );

    fields.insert(
        "trim".to_string(),
        native("string.trim", |args| {
            let text = expect_string(args.get(0).unwrap_or(&Value::Null), "for string.trim")?;
            Ok(Value::String(text.trim().to_string()))
        }),
    );

    fields.insert(
        "split".to_string(),
        native("string.split", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for string.split",
            )?;
            let separator = args
                .get(1)
                .map(|v| expect_string(v, "as the separator for string.split"))
                .transpose()?
                .unwrap_or_default();
            let parts: Vec<Value> = if separator.is_empty() {
                text.chars().map(|c| Value::String(c.to_string())).collect()
            } else {
                text.split(separator.as_str())
                    .map(|part| Value::String(part.to_string()))
                    .collect()
            };
            Ok(Value::new_array(parts))
        }),
    );

    fields.insert(
        "join".to_string(),
        native("string.join", |args| {
            let items = array_items(
                args.get(0).unwrap_or(&Value::Null),
                "as the array for string.join",
            )?;
            let separator = args
                .get(1)
                .map(|v| expect_string(v, "as the separator for string.join"))
                .transpose()?
                .unwrap_or_default();
            let parts: Vec<String> = items.iter().map(|v| v.display()).collect();
            Ok(Value::String(parts.join(&separator)))
        }),
    );

    fields.insert(
        "replace".to_string(),
        native("string.replace", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for string.replace",
            )?;
            let old = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the text to replace for string.replace",
            )?;
            let new = expect_string(
                args.get(2).unwrap_or(&Value::Null),
                "as the replacement for string.replace",
            )?;
            Ok(Value::String(text.replace(old.as_str(), new.as_str())))
        }),
    );

    fields.insert(
        "reverse".to_string(),
        native("string.reverse", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for string.reverse",
            )?;
            Ok(Value::String(text.chars().rev().collect()))
        }),
    );

    fields.insert(
        "repeat".to_string(),
        native("string.repeat", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for string.repeat",
            )?;
            let count = as_usize(
                args.get(1).unwrap_or(&Value::Null),
                "as the repeat count for string.repeat",
            )?;
            // Guard against accidentally allocating gigabytes from a typo.
            if count
                .checked_mul(text.len())
                .map_or(true, |len| len > 10_000_000)
            {
                return Err("string.repeat would produce a string that is too long".to_string());
            }
            Ok(Value::String(text.repeat(count)))
        }),
    );

    fields.insert(
        "pad".to_string(),
        native("string.pad", |args| {
            let text = expect_string(args.get(0).unwrap_or(&Value::Null), "for string.pad")?;
            let width = as_usize(
                args.get(1).unwrap_or(&Value::Null),
                "as the width for string.pad",
            )?;
            let fill = args
                .get(2)
                .map(|v| expect_string(v, "as the fill text for string.pad"))
                .transpose()?
                .unwrap_or_else(|| " ".to_string());
            let fill = fill.chars().next().unwrap_or(' ');
            let align = args
                .get(3)
                .map(|v| expect_string(v, "as the alignment for string.pad"))
                .transpose()?
                .unwrap_or_else(|| "right".to_string());
            let current = text.chars().count();
            if current >= width {
                return Ok(Value::String(text));
            }
            let missing = width - current;
            let left = match align.as_str() {
                "right" | "both" | "center" => missing / 2 + if missing % 2 == 1 && align != "right" { 1 } else { 0 },
                "left" => 0,
                _ => return Err("string.pad alignment must be 'left', 'right' or 'center'".to_string()),
            };
            let right = missing - left;
            Ok(Value::String(format!(
                "{}{}{}",
                fill.to_string().repeat(left),
                text,
                fill.to_string().repeat(right)
            )))
        }),
    );

    fields.insert(
        "contains".to_string(),
        native("string.contains", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for string.contains",
            )?;
            let needle = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "to search for with string.contains",
            )?;
            Ok(Value::Bool(text.contains(needle.as_str())))
        }),
    );

    fields.insert(
        "startsWith".to_string(),
        native("string.startsWith", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for string.startsWith",
            )?;
            let prefix = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the prefix for string.startsWith",
            )?;
            Ok(Value::Bool(text.starts_with(prefix.as_str())))
        }),
    );

    fields.insert(
        "endsWith".to_string(),
        native("string.endsWith", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "for string.endsWith",
            )?;
            let suffix = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the suffix for string.endsWith",
            )?;
            Ok(Value::Bool(text.ends_with(suffix.as_str())))
        }),
    );

    Value::new_object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        call_native(&module(), name, args)
    }

    #[test]
    fn case_trim_and_reverse() {
        assert_eq!(
            call("upper", &[Value::String("hello world".to_string())])
                .unwrap()
                .display(),
            "HELLO WORLD"
        );
        assert_eq!(
            call("lower", &[Value::String("HeLLo".to_string())])
                .unwrap()
                .display(),
            "hello"
        );
        assert_eq!(
            call("trim", &[Value::String("  hi \n".to_string())])
                .unwrap()
                .display(),
            "hi"
        );
        assert_eq!(
            call("reverse", &[Value::String("héllo".to_string())])
                .unwrap()
                .display(),
            "olléh"
        );
    }

    #[test]
    fn split_and_join() {
        let parts = call(
            "split",
            &[
                Value::String("a,b,c".to_string()),
                Value::String(",".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(parts.display(), "[\"a\", \"b\", \"c\"]");
        let chars = call("split", &[Value::String("ab".to_string())]).unwrap();
        assert_eq!(chars.display(), "[\"a\", \"b\"]");
        let joined = call(
            "join",
            &[
                parts,
                Value::String("-".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(joined.display(), "a-b-c");
    }

    #[test]
    fn replace_repeat_and_pad() {
        assert_eq!(
            call(
                "replace",
                &[
                    Value::String("a-b-a".to_string()),
                    Value::String("a".to_string()),
                    Value::String("z".to_string()),
                ]
            )
            .unwrap()
            .display(),
            "z-b-z"
        );
        assert_eq!(
            call(
                "repeat",
                &[Value::String("ab".to_string()), Value::Number(3.0)]
            )
            .unwrap()
            .display(),
            "ababab"
        );
        assert_eq!(
            call(
                "pad",
                &[
                    Value::String("hi".to_string()),
                    Value::Number(5.0),
                    Value::String("*".to_string()),
                ]
            )
            .unwrap()
            .display(),
            "hi***"
        );
        assert_eq!(
            call(
                "pad",
                &[
                    Value::String("hi".to_string()),
                    Value::Number(6.0),
                    Value::String("*".to_string()),
                    Value::String("center".to_string()),
                ]
            )
            .unwrap()
            .display(),
            "**hi**"
        );
    }

    #[test]
    fn predicates() {
        let args = [
            Value::String("Hello, world".to_string()),
            Value::String("Hello".to_string()),
        ];
        assert!(matches!(
            call("contains", &[args[0].clone(), Value::String("world".to_string())]).unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(call("startsWith", &args).unwrap(), Value::Bool(true)));
        assert!(matches!(
            call("endsWith", &[args[0].clone(), Value::String("world".to_string())]).unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(
            call("endsWith", &[args[0].clone(), Value::String("Hello".to_string())]).unwrap(),
            Value::Bool(false)
        ));
    }

    #[test]
    fn repeat_rejects_non_integers() {
        assert!(call(
            "repeat",
            &[Value::String("a".to_string()), Value::Number(-1.0)]
        )
        .is_err());
    }
}

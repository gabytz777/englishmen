//! Native `project` module: read and write `.emc` project manifests.
//!
//! This is the Englishmen-callable counterpart of [`crate::config`]. A parsed
//! manifest is an object with `root` and `sections` objects; values are
//! strings (the `.emc` format has no type inference on read). The writer
//! accepts strings, numbers, booleans and null, emitting bare values for the
//! last three.

use crate::modules::support::expect_path;
use crate::runtime::{expect_string, native};
use crate::value::{format_number, Value};
use std::collections::BTreeMap;

/// Build the `project` module object.
pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "parse".to_string(),
        native("project.parse", |args| {
            check_arity("parse", args, &[1])?;
            let text = expect_string(&args[0], "as the manifest text for project.parse")?;
            Ok(parse_config(&text))
        }),
    );

    fields.insert(
        "load".to_string(),
        native("project.load", |args| {
            check_arity("load", args, &[0, 1])?;
            let path = if args.is_empty() {
                let cwd = std::env::current_dir()
                    .map_err(|e| format!("Could not determine the current directory: {e}"))?;
                cwd.join("englishmen.emc")
            } else {
                expect_path(&args[0], "as the path for project.load")?
            };
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
            Ok(parse_config(&text))
        }),
    );

    fields.insert(
        "get".to_string(),
        native("project.get", |args| {
            check_arity("get", args, &[2])?;
            let (root, _) = config_parts(&args[0], "get")?;
            let key = expect_string(&args[1], "as the key for project.get")?;
            Ok(root.get(&key).cloned().unwrap_or(Value::Null))
        }),
    );

    fields.insert(
        "section".to_string(),
        native("project.section", |args| {
            check_arity("section", args, &[2])?;
            let (_, sections) = config_parts(&args[0], "section")?;
            let name = expect_string(&args[1], "as the section name for project.section")?;
            Ok(Value::new_object(
                sections.get(&name).cloned().unwrap_or_default(),
            ))
        }),
    );

    fields.insert("name".to_string(), shortcut("project.name", "name", "name"));
    fields.insert(
        "version".to_string(),
        shortcut("project.version", "version", "version"),
    );
    fields.insert("main".to_string(), shortcut("project.main", "main", "main"));

    fields.insert(
        "default".to_string(),
        native("project.default", |args| {
            check_arity("default", args, &[1])?;
            let name = expect_string(&args[0], "as the project name for project.default")?;
            Ok(Value::String(crate::config::default_emc(&name)))
        }),
    );

    fields.insert(
        "write".to_string(),
        native("project.write", |args| {
            check_arity("write", args, &[2])?;
            let path = expect_path(&args[0], "as the path for project.write")?;
            let (root, sections) = config_parts(&args[1], "write")?;
            let text = serialize_config(&root, &sections)?;
            std::fs::write(&path, text)
                .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
            Ok(Value::Bool(true))
        }),
    );

    Value::new_object(fields)
}

/// Reject wrong argument counts with an Englishmen-style error rather than
/// relying on `unwrap` or silently ignoring extra arguments.
fn check_arity(name: &str, args: &[Value], allowed: &[usize]) -> Result<(), String> {
    if allowed.contains(&args.len()) {
        return Ok(());
    }
    let expected = allowed
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(" or ");
    Err(format!(
        "project.{name} expects {expected} argument(s), found {}",
        args.len()
    ))
}

/// Shared implementation of `project.name/version/main`.
fn shortcut(native_name: &'static str, op: &'static str, key: &'static str) -> Value {
    native(native_name, move |args| {
        check_arity(op, args, &[1])?;
        let (root, _) = config_parts(&args[0], op)?;
        Ok(root.get(key).cloned().unwrap_or(Value::Null))
    })
}

/// Parse `.emc` text into `{root: object, sections: object}`.
fn parse_config(text: &str) -> Value {
    let mut root = BTreeMap::new();
    let mut sections: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    let mut current_section: Option<String> = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let name = line[1..line.len() - 1].trim().to_string();
            sections.entry(name.clone()).or_default();
            current_section = Some(name);
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = Value::String(parse_value(value.trim()));
            match &current_section {
                Some(section) => {
                    sections
                        .entry(section.clone())
                        .or_default()
                        .insert(key, value);
                }
                None => {
                    root.insert(key, value);
                }
            }
        }
    }

    let section_values = sections
        .into_iter()
        .map(|(name, fields)| (name, Value::new_object(fields)))
        .collect();
    let mut config = BTreeMap::new();
    config.insert("root".to_string(), Value::new_object(root));
    config.insert("sections".to_string(), Value::new_object(section_values));
    Value::new_object(config)
}

/// Trim, remove one pair of quotes, and decode the escapes emitted by
/// `write_quoted` so `project.write`/`project.parse` round-trip exactly.
fn parse_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        unescape(&trimmed[1..trimmed.len() - 1])
    } else {
        trimmed.to_string()
    }
}

/// Reverse `write_quoted`: `\"`, `\\`, `\n`, `\r`, `\t` and `\uXXXX`.
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let digits: String = chars.by_ref().take(4).collect();
                match u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(decoded) => out.push(decoded),
                    None => {
                        out.push_str("\\u");
                        out.push_str(&digits);
                    }
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Snapshot the two objects of a parsed config so native calls can inspect
/// them without holding a `RefCell` borrow across another native call.
fn config_parts(
    config: &Value,
    op: &str,
) -> Result<
    (
        BTreeMap<String, Value>,
        BTreeMap<String, BTreeMap<String, Value>>,
    ),
    String,
> {
    let Value::Object(fields) = config else {
        return Err(format!(
            "Type mismatch: expected a project config object for project.{op}, found {}",
            config.type_name()
        ));
    };
    let fields = fields.borrow();

    let root = match fields.get("root") {
        Some(Value::Object(map)) => map.borrow().clone(),
        Some(other) => {
            return Err(format!(
                "project.{op} expects 'root' to be an object, found {}",
                other.type_name()
            ))
        }
        None => {
            return Err(format!(
                "project.{op} expects a config with a 'root' object"
            ))
        }
    };

    let section_values = match fields.get("sections") {
        Some(Value::Object(map)) => map.borrow().clone(),
        Some(other) => {
            return Err(format!(
                "project.{op} expects 'sections' to be an object, found {}",
                other.type_name()
            ))
        }
        None => {
            return Err(format!(
                "project.{op} expects a config with a 'sections' object"
            ))
        }
    };

    let mut sections = BTreeMap::new();
    for (name, value) in section_values {
        match value {
            Value::Object(map) => {
                sections.insert(name, map.borrow().clone());
            }
            other => {
                return Err(format!(
                    "project.{op} expects section '{name}' to be an object, found {}",
                    other.type_name()
                ))
            }
        }
    }
    Ok((root, sections))
}

/// Serialize with `BTreeMap` ordering: root keys first, then `[section]`
/// headers with their keys.
fn serialize_config(
    root: &BTreeMap<String, Value>,
    sections: &BTreeMap<String, BTreeMap<String, Value>>,
) -> Result<String, String> {
    let mut out = String::new();
    for (key, value) in root {
        write_pair(key, value, &mut out)?;
    }
    for (name, fields) in sections {
        validate_name(name, "section name")?;
        out.push('[');
        out.push_str(name);
        out.push_str("]\n");
        for (key, value) in fields {
            write_pair(key, value, &mut out)?;
        }
    }
    Ok(out)
}

fn write_pair(key: &str, value: &Value, out: &mut String) -> Result<(), String> {
    validate_name(key, "manifest key")?;
    out.push_str(key);
    out.push_str(" = ");
    match value {
        Value::String(text) => write_quoted(text, out),
        Value::Number(n) if n.is_finite() => out.push_str(&format_number(*n)),
        Value::Number(_) => {
            return Err(format!("Cannot write a non-finite number for key '{key}'"))
        }
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Null => out.push_str("null"),
        other => {
            return Err(format!(
                "Cannot write a {} for key '{key}' in a manifest",
                other.type_name()
            ))
        }
    }
    out.push('\n');
    Ok(())
}

/// Keys and section names must stay on one line, must not collide with the
/// `[section]` or `key=value` syntax, and must survive the trimming and
/// comment rules of `parse_config` unchanged.
fn validate_name(name: &str, what: &str) -> Result<(), String> {
    if name.is_empty()
        || name.starts_with('#')
        || name
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '[' | ']' | '='))
    {
        return Err(format!("Invalid {what} '{name}' for project.write"));
    }
    Ok(())
}

/// Keep a string value on one line; `project.parse` removes the outer quotes.
fn write_quoted(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\u{20}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use crate::modules::support::call_native;
    use crate::runtime::native;
    use crate::value::Value;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;

    fn call(name: &str, args: &[Value]) -> Result<Value, String> {
        call_native(&module(), name, args)
    }

    fn config(text: &str) -> Value {
        call("parse", &[Value::String(text.to_string())]).unwrap()
    }

    fn temp_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "em_project_test_{}_{}_{nanos}",
            std::process::id(),
            name
        ))
    }

    fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
        Value::new_object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    #[test]
    fn parse_builds_root_and_sections() {
        let parsed = config(
            "# comment\n\nname = \"Demo\"\nversion = 2.1\n\n[installer]\ninstall_dir = \"C:/em\"\nprojects_enabled = \"true\"\n",
        );
        assert_eq!(
            call("get", &[parsed.clone(), Value::String("name".into())])
                .unwrap()
                .display(),
            "Demo"
        );
        assert_eq!(
            call("get", &[parsed.clone(), Value::String("version".into())])
                .unwrap()
                .display(),
            "2.1"
        );
        assert!(matches!(
            call("get", &[parsed.clone(), Value::String("missing".into())]).unwrap(),
            Value::Null
        ));
        let installer = call(
            "section",
            &[parsed.clone(), Value::String("installer".into())],
        )
        .unwrap();
        let Value::Object(fields) = installer else {
            panic!("section should be an object");
        };
        assert_eq!(
            fields.borrow().get("install_dir").unwrap().display(),
            "C:/em"
        );
        assert_eq!(
            fields.borrow().get("projects_enabled").unwrap().display(),
            "true"
        );
        assert_eq!(
            call("section", &[parsed, Value::String("missing".into())])
                .unwrap()
                .display(),
            "{}"
        );
    }

    #[test]
    fn parse_trims_and_strips_one_quote_pair() {
        let parsed = config("a = \"  spaced  \"\nb = 'single'\nc = bare\n");
        for (key, expected) in [("a", "  spaced  "), ("b", "'single'"), ("c", "bare")] {
            assert_eq!(
                call("get", &[parsed.clone(), Value::String(key.into())])
                    .unwrap()
                    .display(),
                expected
            );
        }
    }

    #[test]
    fn load_reads_an_explicit_path() {
        let path = temp_path("load.emc");
        std::fs::write(&path, "name = \"Loaded\"\n").unwrap();
        let loaded = call("load", &[Value::String(path.display().to_string())]).unwrap();
        assert_eq!(call("name", &[loaded]).unwrap().display(), "Loaded");
        let missing = call(
            "load",
            &[Value::String(
                temp_path("missing.emc").display().to_string(),
            )],
        );
        assert!(missing.unwrap_err().contains("Could not read"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_defaults_to_englishmen_emc_in_cwd() {
        static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = CWD_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let original = std::env::current_dir().unwrap();
        let dir = temp_path("cwd");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("englishmen.emc"), "name = \"DefaultPath\"\n").unwrap();

        struct RestoreCwd(PathBuf);
        impl Drop for RestoreCwd {
            fn drop(&mut self) {
                let _ = std::env::set_current_dir(&self.0);
            }
        }
        let _restore = RestoreCwd(original);
        std::env::set_current_dir(&dir).unwrap();

        let loaded = call("load", &[]).unwrap();
        assert_eq!(call("name", &[loaded]).unwrap().display(), "DefaultPath");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortcuts_return_root_keys() {
        let parsed = config("name = \"N\"\nversion = \"1.2.3\"\nmain = \"app.enm\"\n");
        for (function, expected) in [("name", "N"), ("version", "1.2.3"), ("main", "app.enm")] {
            assert_eq!(
                call(function, &[parsed.clone()]).unwrap().display(),
                expected
            );
        }
        let empty = config("name = \"N\"\n");
        assert!(matches!(call("version", &[empty]).unwrap(), Value::Null));
    }

    #[test]
    fn default_matches_the_rust_emc_template() {
        let text = call("default", &[Value::String("Demo".into())]).unwrap();
        assert_eq!(text.display(), crate::config::default_emc("Demo"));
        let config = call("parse", &[text]).unwrap();
        assert_eq!(call("name", &[config]).unwrap().display(), "Demo");
    }

    #[test]
    fn write_serializes_stable_keys_and_bare_values() {
        let parsed = config(
            "name = \"Demo\"\nversion = \"2.1\"\nmain = \"main.enm\"\n[notes]\nbody = \"hello\"\n",
        );
        let path = temp_path("written.emc");
        let name = path.display().to_string();
        assert!(matches!(
            call("write", &[Value::String(name.clone()), parsed]).unwrap(),
            Value::Bool(true)
        ));
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            written,
            "main = \"main.enm\"\nname = \"Demo\"\nversion = \"2.1\"\n[notes]\nbody = \"hello\"\n"
        );
        let loaded = call("load", &[Value::String(name)]).unwrap();
        assert_eq!(call("name", &[loaded.clone()]).unwrap().display(), "Demo");
        assert_eq!(
            call("get", &[loaded, Value::String("main".into())])
                .unwrap()
                .display(),
            "main.enm"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_round_trips_escaped_string_values() {
        let tricky = "quote \" slash \\ newline \n return \r tab \t control \u{1f}";
        let config = Value::new_object(BTreeMap::from([
            (
                "root".to_string(),
                object([("tricky", Value::String(tricky.into()))]),
            ),
            ("sections".to_string(), Value::new_object(BTreeMap::new())),
        ]));
        let path = temp_path("escaped.emc");
        let name = path.display().to_string();
        assert!(matches!(
            call("write", &[Value::String(name.clone()), config]).unwrap(),
            Value::Bool(true)
        ));
        let loaded = call("load", &[Value::String(name)]).unwrap();
        assert_eq!(
            call("get", &[loaded, Value::String("tricky".into())])
                .unwrap()
                .display(),
            tricky
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_rejects_names_that_parse_would_change() {
        for bad in ["#comment", " spaced", "trailing ", "two words", "a=b"] {
            let config = Value::new_object(BTreeMap::from([
                (
                    "root".to_string(),
                    object([(bad, Value::String("x".into()))]),
                ),
                ("sections".to_string(), Value::new_object(BTreeMap::new())),
            ]));
            let path = temp_path("badname.emc");
            let error = call(
                "write",
                &[Value::String(path.display().to_string()), config],
            )
            .unwrap_err();
            assert!(error.contains("Invalid manifest key"), "{bad}: {error}");
        }
        let config = Value::new_object(BTreeMap::from([
            ("root".to_string(), Value::new_object(BTreeMap::new())),
            (
                "sections".to_string(),
                Value::new_object(BTreeMap::from([(
                    " #section".to_string(),
                    Value::new_object(BTreeMap::new()),
                )])),
            ),
        ]));
        let path = temp_path("badsection.emc");
        let error = call(
            "write",
            &[Value::String(path.display().to_string()), config],
        )
        .unwrap_err();
        assert!(error.contains("Invalid section name"), "{error}");
    }

    #[test]
    fn write_formats_numbers_bools_and_null_as_bare_values() {
        let root = object([
            ("z", Value::Number(3.0)),
            ("name", Value::String("Demo".into())),
            ("enabled", Value::Bool(true)),
            ("disabled", Value::Bool(false)),
            ("nothing", Value::Null),
        ]);
        let sections = Value::new_object(BTreeMap::from([(
            "alpha".to_string(),
            object([
                ("b", Value::String("two".into())),
                ("a", Value::Number(1.0)),
            ]),
        )]));
        let config = Value::new_object(BTreeMap::from([
            ("root".to_string(), root),
            ("sections".to_string(), sections),
        ]));
        let path = temp_path("bare.emc");
        let name = path.display().to_string();
        call("write", &[Value::String(name.clone()), config]).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "disabled = false\nenabled = true\nname = \"Demo\"\nnothing = null\nz = 3\n[alpha]\na = 1\nb = \"two\"\n"
        );
        let loaded = call("load", &[Value::String(name)]).unwrap();
        assert_eq!(
            call("get", &[loaded, Value::String("enabled".into())])
                .unwrap()
                .display(),
            "true"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn validation_errors_are_returned() {
        assert!(call("parse", &[]).unwrap_err().contains("expects 1"));
        assert!(call("parse", &[Value::Number(1.0)])
            .unwrap_err()
            .contains("expected a string"));
        assert!(call("load", &[Value::String("a".into()), Value::Null])
            .unwrap_err()
            .contains("argument(s)"));
        let parsed = config("name = \"Demo\"\n");
        assert!(call("get", &[Value::Null, Value::String("name".into())]).is_err());
        assert!(call("get", &[parsed.clone(), Value::Bool(true)]).is_err());
        assert!(call("section", &[parsed.clone(), Value::Number(1.0)]).is_err());
        assert!(call("name", &[]).is_err());
        assert!(call("default", &[Value::Null]).is_err());
        assert!(call("write", &[Value::String("x".into())]).is_err());
        assert!(call("write", &[Value::Number(1.0), parsed.clone()]).is_err());

        let bad_config = object([("root", Value::Bool(true))]);
        assert!(call("write", &[Value::String("x".into()), bad_config])
            .unwrap_err()
            .contains("'root' to be an object"));

        fn writeable(root: Value, sections: Value) -> Value {
            Value::new_object(BTreeMap::from([
                ("root".to_string(), root),
                ("sections".to_string(), sections),
            ]))
        }
        let no_sections = Value::new_object(BTreeMap::new());

        let function = native("test", |_| Ok(Value::Null));
        let with_function = writeable(object([("bad", function)]), no_sections.clone());
        assert!(call("write", &[Value::String("x".into()), with_function])
            .unwrap_err()
            .contains("Cannot write a function"));

        let with_nan = writeable(
            object([("bad", Value::Number(f64::NAN))]),
            no_sections.clone(),
        );
        assert!(call("write", &[Value::String("x".into()), with_nan])
            .unwrap_err()
            .contains("non-finite"));

        let bad_key = writeable(
            object([("bad\nkey", Value::String("x".into()))]),
            no_sections.clone(),
        );
        assert!(call("write", &[Value::String("x".into()), bad_key])
            .unwrap_err()
            .contains("Invalid manifest key"));

        let bad_section = writeable(
            no_sections,
            Value::new_object(BTreeMap::from([("bad=name".to_string(), object([]))])),
        );
        assert!(call("write", &[Value::String("x".into()), bad_section])
            .unwrap_err()
            .contains("Invalid section name"));
    }
}

use crate::modules::support::call_native;
use crate::runtime::{expect_string, native};
use crate::value::Value;
use std::collections::BTreeMap;

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "name".to_string(),
        native("os.name", |_args| {
            Ok(Value::String(std::env::consts::OS.to_string()))
        }),
    );

    fields.insert(
        "arch".to_string(),
        native("os.arch", |_args| {
            Ok(Value::String(std::env::consts::ARCH.to_string()))
        }),
    );

    fields.insert(
        "args".to_string(),
        native("os.args", |_args| {
            let args: Vec<Value> = std::env::args()
                .skip(1)
                .map(|arg| Value::String(arg))
                .collect();
            Ok(Value::new_array(args))
        }),
    );

    fields.insert(
        "env".to_string(),
        native("os.env", |args| {
            let key = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as the variable name for os.env",
            )?;
            match std::env::var(&key) {
                Ok(value) => Ok(Value::String(value)),
                Err(_) => Ok(args.get(1).cloned().unwrap_or(Value::Null)),
            }
        }),
    );

    fields.insert(
        "cwd".to_string(),
        native("os.cwd", |_args| {
            let path = std::env::current_dir()
                .map_err(|e| format!("Could not determine the current directory: {e}"))?;
            Ok(Value::String(path.display().to_string()))
        }),
    );

    fields.insert(
        "tempDir".to_string(),
        native("os.tempDir", |_args| {
            Ok(Value::String(
                std::env::temp_dir().display().to_string(),
            ))
        }),
    );

    fields.insert(
        "platform".to_string(),
        native("os.platform", |_args| {
            Ok(Value::String(std::env::consts::FAMILY.to_string()))
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
    fn platform_identifiers_are_non_empty() {
        for name in ["name", "arch", "platform", "cwd", "tempDir"] {
            let value = call(name, &[]).unwrap();
            let Value::String(text) = value else {
                panic!("{name} should return a string");
            };
            assert!(!text.is_empty(), "{name} should not be empty");
        }
        assert_eq!(call("name", &[]).unwrap().display(), std::env::consts::OS);
        assert_eq!(call("arch", &[]).unwrap().display(), std::env::consts::ARCH);
    }

    #[test]
    fn env_reads_variables_and_uses_defaults() {
        let key = format!("EM_OS_MODULE_TEST_{}", std::process::id());
        std::env::set_var(&key, "englishmen");
        assert_eq!(
            call("env", &[Value::String(key.clone())])
                .unwrap()
                .display(),
            "englishmen"
        );
        std::env::remove_var(&key);
        assert!(matches!(
            call("env", &[Value::String(key.clone())]).unwrap(),
            Value::Null
        ));
        assert_eq!(
            call(
                "env",
                &[Value::String(key), Value::String("fallback".to_string())]
            )
            .unwrap()
            .display(),
            "fallback"
        );
        assert!(call("env", &[Value::Number(1.0)]).is_err());
    }

    #[test]
    fn args_is_an_array() {
        assert!(matches!(call("args", &[]).unwrap(), Value::Array(_)));
    }
}

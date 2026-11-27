use crate::modules::support::expect_path;
#[cfg(test)]
use crate::modules::support::call_native;
use crate::runtime::{expect_string, native};
use crate::value::Value;
use std::collections::BTreeMap;
use std::time::UNIX_EPOCH;

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "read".to_string(),
        native("fs.read", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.read",
            )?;
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("Could not read '{}': {e}", path.display()))?;
            Ok(Value::String(text))
        }),
    );

    fields.insert(
        "write".to_string(),
        native("fs.write", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.write",
            )?;
            let content = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the content for fs.write",
            )?;
            std::fs::write(&path, content)
                .map_err(|e| format!("Could not write '{}': {e}", path.display()))?;
            Ok(Value::Null)
        }),
    );

    fields.insert(
        "append".to_string(),
        native("fs.append", |args| {
            use std::io::Write;
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.append",
            )?;
            let content = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the content for fs.append",
            )?;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| format!("Could not open '{}': {e}", path.display()))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("Could not append to '{}': {e}", path.display()))?;
            Ok(Value::Null)
        }),
    );

    fields.insert(
        "exists".to_string(),
        native("fs.exists", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.exists",
            )?;
            Ok(Value::Bool(path.exists()))
        }),
    );

    fields.insert(
        "list".to_string(),
        native("fs.list", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the directory for fs.list",
            )?;
            let mut names: Vec<String> = std::fs::read_dir(&path)
                .map_err(|e| format!("Could not list '{}': {e}", path.display()))?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            Ok(Value::new_array(
                names.into_iter().map(Value::String).collect(),
            ))
        }),
    );

    fields.insert(
        "remove".to_string(),
        native("fs.remove", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.remove",
            )?;
            if !path.exists() {
                return Ok(Value::Bool(false));
            }
            if path.is_dir() {
                std::fs::remove_dir(&path)
                    .map_err(|e| format!("Could not remove '{}': {e}", path.display()))?;
            } else {
                std::fs::remove_file(&path)
                    .map_err(|e| format!("Could not remove '{}': {e}", path.display()))?;
            }
            Ok(Value::Bool(true))
        }),
    );

    fields.insert(
        "mkdir".to_string(),
        native("fs.mkdir", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.mkdir",
            )?;
            let existed = path.exists();
            std::fs::create_dir_all(&path)
                .map_err(|e| format!("Could not create '{}': {e}", path.display()))?;
            Ok(Value::Bool(!existed))
        }),
    );

    fields.insert(
        "copy".to_string(),
        native("fs.copy", |args| {
            let source = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the source for fs.copy",
            )?;
            let target = expect_path(
                args.get(1).unwrap_or(&Value::Null),
                "as the destination for fs.copy",
            )?;
            std::fs::copy(&source, &target).map_err(|e| {
                format!(
                    "Could not copy '{}' to '{}': {e}",
                    source.display(),
                    target.display()
                )
            })?;
            Ok(Value::Null)
        }),
    );

    fields.insert(
        "metadata".to_string(),
        native("fs.metadata", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.metadata",
            )?;
            let meta = std::fs::metadata(&path)
                .map_err(|e| format!("Could not stat '{}': {e}", path.display()))?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs_f64())
                .unwrap_or(0.0);
            let mut info = BTreeMap::new();
            info.insert("size".to_string(), Value::Number(meta.len() as f64));
            info.insert("isFile".to_string(), Value::Bool(meta.is_file()));
            info.insert("isDir".to_string(), Value::Bool(meta.is_dir()));
            info.insert("modified".to_string(), Value::Number(modified));
            Ok(Value::new_object(info))
        }),
    );

    fields.insert(
        "cwd".to_string(),
        native("fs.cwd", |_args| {
            let path = std::env::current_dir()
                .map_err(|e| format!("Could not determine the current directory: {e}"))?;
            Ok(Value::String(path.display().to_string()))
        }),
    );

    fields.insert(
        "move".to_string(),
        native("fs.move", |args| {
            let source = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the source for fs.move",
            )?;
            let target = expect_path(
                args.get(1).unwrap_or(&Value::Null),
                "as the destination for fs.move",
            )?;
            std::fs::rename(&source, &target).map_err(|e| {
                format!(
                    "Could not move '{}' to '{}': {e}",
                    source.display(),
                    target.display()
                )
            })?;
            Ok(Value::Null)
        }),
    );

    fields.insert(
        "isFile".to_string(),
        native("fs.isFile", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.isFile",
            )?;
            Ok(Value::Bool(path.is_file()))
        }),
    );

    fields.insert(
        "isDir".to_string(),
        native("fs.isDir", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.isDir",
            )?;
            Ok(Value::Bool(path.is_dir()))
        }),
    );

    fields.insert(
        "size".to_string(),
        native("fs.size", |args| {
            let path = expect_path(
                args.get(0).unwrap_or(&Value::Null),
                "as the path for fs.size",
            )?;
            let size = std::fs::metadata(&path)
                .map_err(|e| format!("Could not stat '{}': {e}", path.display()))?
                .len();
            Ok(Value::Number(size as f64))
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

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("em_fs_test_{}_{}", std::process::id(), name));
        path
    }

    #[test]
    fn write_read_append_and_remove() {
        let path = temp_path("text.txt");
        let _ = std::fs::remove_file(&path);
        let name = path.display().to_string();
        assert!(matches!(
            call("exists", &[Value::String(name.clone())]).unwrap(),
            Value::Bool(false)
        ));
        call("write", &[Value::String(name.clone()), Value::String("one".to_string())]).unwrap();
        call("append", &[Value::String(name.clone()), Value::String("two".to_string())]).unwrap();
        assert_eq!(
            call("read", &[Value::String(name.clone())]).unwrap().display(),
            "onetwo"
        );
        assert!(matches!(
            call("exists", &[Value::String(name.clone())]).unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(
            call("remove", &[Value::String(name.clone())]).unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(
            call("remove", &[Value::String(name)]).unwrap(),
            Value::Bool(false)
        ));
    }

    #[test]
    fn mkdir_list_copy_and_metadata() {
        let dir = temp_path("dir");
        let nested = temp_path("dir/nested");
        let copied = temp_path("copy.txt");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&copied);
        call("mkdir", &[Value::String(nested.display().to_string())]).unwrap();
        let original = temp_path("dir/nested/file.txt");
        call(
            "write",
            &[
                Value::String(original.display().to_string()),
                Value::String("hello".to_string()),
            ],
        )
        .unwrap();
        let names = call("list", &[Value::String(nested.display().to_string())]).unwrap();
        assert_eq!(names.display(), "[\"file.txt\"]");
        call(
            "copy",
            &[
                Value::String(original.display().to_string()),
                Value::String(copied.display().to_string()),
            ],
        )
        .unwrap();
        let meta = call("metadata", &[Value::String(copied.display().to_string())]).unwrap();
        let Value::Object(info) = meta else {
            panic!("metadata should be an object");
        };
        assert!(matches!(
            info.borrow().get("isFile"),
            Some(Value::Bool(true))
        ));
        assert!(matches!(
            info.borrow().get("size"),
            Some(Value::Number(n)) if *n == 5.0
        ));
        assert!(matches!(call("remove", &[Value::String(dir.display().to_string())]).is_err(), true));
        let _ = std::fs::remove_file(&copied);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cwd_is_an_absolute_path() {
        let cwd = call("cwd", &[]).unwrap();
        let Value::String(text) = cwd else {
            panic!("cwd should be a string");
        };
        assert!(std::path::Path::new(&text).is_absolute());
    }

    #[test]
    fn invalid_paths_report_errors() {
        assert!(call("read", &[Value::Number(1.0)]).is_err());
        assert!(call("read", &[Value::String(temp_path("missing").display().to_string())]).is_err());
    }

    #[test]
    fn move_classify_and_size() {
        let source = temp_path("move-source.txt");
        let target = temp_path("move-target.txt");
        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&target);
        call(
            "write",
            &[
                Value::String(source.display().to_string()),
                Value::String("abcd".to_string()),
            ],
        )
        .unwrap();
        assert!(matches!(
            call("isFile", &[Value::String(source.display().to_string())]).unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(
            call("isDir", &[Value::String(source.display().to_string())]).unwrap(),
            Value::Bool(false)
        ));
        assert!(matches!(
            call("size", &[Value::String(source.display().to_string())]).unwrap(),
            Value::Number(n) if n == 4.0
        ));
        call(
            "move",
            &[
                Value::String(source.display().to_string()),
                Value::String(target.display().to_string()),
            ],
        )
        .unwrap();
        assert!(matches!(
            call("exists", &[Value::String(target.display().to_string())]).unwrap(),
            Value::Bool(true)
        ));
        assert!(matches!(
            call("exists", &[Value::String(source.display().to_string())]).unwrap(),
            Value::Bool(false)
        ));
        let _ = std::fs::remove_file(&target);
    }
}

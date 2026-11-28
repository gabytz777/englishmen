//! Integration tests for the self-hosted Englishmen parser.
//!
//! The CLI entry point reads the source path from the process arguments (not
//! from `args[0]`, which contains the `em` executable), so these tests invoke
//! it exactly as a user would: `em run selfhost/parser.enm <file>`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn run_parser(source: &Path) -> std::process::Output {
    let mut command = Command::new(em());
    command
        .args(["run", "selfhost/parser.enm"])
        .arg(source)
        .current_dir(repo_root())
        .env("EM_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::null());
    command.output().expect("failed to run em")
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "parser CLI failed: {}",
        stderr(output)
    );
    assert!(
        stderr(output).is_empty(),
        "parser CLI wrote diagnostics: {}",
        stderr(output)
    );
}

fn assert_ast_object(text: &str) {
    assert!(
        text.starts_with("{\"statements\":["),
        "AST JSON must be an object with a statements array, got: {text}"
    );
    assert!(
        text.ends_with("],\"version\":1}"),
        "AST JSON must end with the version 1 field, got: {text}"
    );
    assert!(
        text.contains("\"kind\":"),
        "AST JSON must contain statement kinds, got: {text}"
    );
    assert_eq!(
        text.lines().count(),
        1,
        "parser CLI must print exactly one JSON line"
    );
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "em_selfhost_parser_{}_{}",
        name,
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn number_field(text: &str, key: &str) -> i64 {
    let needle = format!("\"{key}\":");
    let start = text
        .find(&needle)
        .unwrap_or_else(|| panic!("missing {key} in {text}"))
        + needle.len();
    let digits: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    digits
        .parse()
        .unwrap_or_else(|_| panic!("bad {key} in {text}"))
}

#[test]
fn parses_every_example() {
    let examples = repo_root().join("examples");
    let mut paths: Vec<PathBuf> = fs::read_dir(&examples)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("enm"))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no examples found in {}",
        examples.display()
    );

    for path in paths {
        let output = run_parser(&path);
        assert_success(&output);
        assert_ast_object(&stdout(&output));
    }
}

#[test]
fn reports_structured_parse_error() {
    let dir = scratch_dir("negative");
    let source = dir.join("bad.enm");
    fs::write(&source, "if true\n    say(1)\n").unwrap();

    let output = run_parser(&source);
    assert_success(&output);
    let text = stdout(&output);
    assert!(
        text.starts_with('{') && text.ends_with('}'),
        "error must be a JSON object, got: {text}"
    );
    assert!(
        text.contains("\"error\":"),
        "error object needs an error field, got: {text}"
    );
    let line = number_field(&text, "line");
    let col = number_field(&text, "col");
    assert!(line >= 1, "error line must be positive, got {line}");
    assert!(col >= 1, "error col must be positive, got {col}");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn covers_every_ast_variant() {
    let dir = scratch_dir("variants");
    let source = dir.join("all.enm");
    fs::write(
        &source,
        r#"import json
function f(a, b)
    if a
        return a + b
    else
        return 0
    end
end
x = 1
x += 2
y = -x
z = not true
w = ~1
arr = [1, 2]
obj = {a: 1, b: "s"}
p = obj.a
q = arr[0]
r = f(1, 2)
m = "abc".length()
t = true ? 1 : 2
c = null ?? 3
b1 = 1 & 2
b2 = 1 | 2
b3 = 1 ^ 2
b4 = 1 << 2
b5 = 5 >> 1
pow = 2 ** 3 ** 2
fd = 7 // 2
d1 = a - b
d2 = a * b
d3 = a / b
d4 = a % b
d5 = a == b
d6 = a != b
d7 = a > b
d8 = a < b
d9 = a >= b
d10 = a <= b
d11 = a and b
d12 = a or b
x += 1
x -= 1
x *= 2
x /= 2
x %= 2
x **= 2
x //= 2
x &= 1
x |= 1
x ^= 1
x <<= 1
x >>= 1
x++
x--
repeat 3
    break
end
while false
    continue
end
for v in arr
    say(v)
end
s = "value={x} done"
"#,
    )
    .unwrap();

    let output = run_parser(&source);
    assert_success(&output);
    let text = stdout(&output);
    assert_ast_object(&text);

    for kind in [
        "Assign",
        "CompoundAssign",
        "ExprStmt",
        "If",
        "FunctionDef",
        "Return",
        "Repeat",
        "While",
        "ForIn",
        "Break",
        "Continue",
        "Import",
        "Number",
        "String",
        "Bool",
        "Null",
        "Variable",
        "PropertyAccess",
        "Index",
        "Array",
        "Object",
        "Call",
        "MethodCall",
        "Binary",
        "Unary",
        "Ternary",
        "Postfix",
    ] {
        assert!(
            text.contains(&format!("\"kind\":\"{kind}\"")),
            "missing {kind} in AST: {text}"
        );
    }

    for op in [
        "+", "-", "*", "/", "%", "**", "//", "&", "|", "^", "<<", ">>", "==", "!=", ">", "<", ">=",
        "<=", "and", "or", "??",
    ] {
        assert!(
            text.contains(&format!("\"op\":\"{op}\"")),
            "missing operator {op} in AST: {text}"
        );
    }

    // Interpolated strings are lowered exactly like
    // `src/parser.rs::parse_interpolated`: a leading empty string, then `+`
    // chains for literals and embedded expressions.
    assert!(
        text.contains("\"value\":\"\""),
        "interpolation must start with an empty string literal"
    );
    assert!(
        text.contains("\"value\":\"value=\""),
        "interpolation literal was not preserved"
    );
    assert!(
        text.contains("\"value\":\" done\""),
        "trailing interpolation literal was not preserved"
    );

    fs::remove_dir_all(&dir).ok();
}

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

/// Keep `em`'s installer and update checks away from the real machine.
fn isolated_env(dir: &Path) -> [(&'static str, String); 3] {
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .envs(isolated_env(dir))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("em_selfhost_lexer_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A tiny Englishmen program that checks the CLI's output with `json` itself.
fn write_json_check(path: &Path) {
    std::fs::write(
        path,
        r#"import fs
import json
import os
argv = os.args()
text = fs.read(argv[argv.length() - 1])
if json.isValid(text)
    decoded = json.decode(text)
    if type(decoded) == "array"
        say("array")
    else
        say("not-array")
    end
else
    say("invalid")
end
"#,
    )
    .unwrap();
}

/// Print one compact signature per token so Rust can assert on token details
/// without adding a JSON dependency.
fn write_signature_check(path: &Path) {
    std::fs::write(
        path,
        r#"import fs
import json
import os
argv = os.args()
tokens = json.decode(fs.read(argv[argv.length() - 1]))
for tok in tokens
    if tok.type == "CompoundAssign"
        say(tok.type + "|" + json.encode(tok.op) + "|" + tok.line + ":" + tok.col)
    else
        say(tok.type + "|" + json.encode(tok.value) + "|" + tok.line + ":" + tok.col)
    end
end
"#,
    )
    .unwrap();
}

fn lex_to_file(dir: &Path, source: &Path, output: &Path) {
    let source = source.to_str().expect("non-UTF-8 source path");
    let lexer = "selfhost/lexer.enm";
    let result = run(dir, &["run", lexer, source]);
    assert!(
        result.status.success(),
        "lexing {source} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    let trimmed = stdout.trim();
    assert!(
        trimmed.starts_with('[') && trimmed.ends_with(']'),
        "expected a JSON token array for {source}, got: {trimmed}"
    );
    std::fs::write(output, trimmed).unwrap();
}

fn assert_json_array(dir: &Path, validator: &Path, json_file: &Path) {
    let validator = validator.to_str().expect("non-UTF-8 validator path");
    let json_file = json_file.to_str().expect("non-UTF-8 JSON path");
    let result = run(dir, &["run", validator, json_file]);
    assert!(
        result.status.success(),
        "json validation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "array");
}

#[test]
fn lexes_every_bundled_example_as_a_json_token_array() {
    let dir = scratch("examples");
    let validator = dir.join("json_check.enm");
    let json_file = dir.join("tokens.json");
    write_json_check(&validator);

    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&examples)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().map(|ext| ext == "enm").unwrap_or(false))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no bundled examples found in {examples:?}"
    );

    for path in paths {
        lex_to_file(&dir, &path, &json_file);
        assert_json_array(&dir, &validator, &json_file);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lexes_the_full_token_surface() {
    let dir = scratch("surface");
    let source = dir.join("surface.enm");
    let json_file = dir.join("tokens.json");
    let validator = dir.join("signatures.enm");
    std::fs::write(
        &source,
        r#"$- block comment -$ # line comment
n = 0xFF + 0b1010 + 1_000 + 2.5e-2 + 1e3
n += 7 ** 2 // 3
n -= 1
n **= 2
n //= 3
n *= 2
n /= 2
n %= 2
n &= 1
n |= 2
n ^= 3
n <<= 1
n >>= 1
n++
n--
message = "hi {name.titleCase()} \u{1F600} {{literal}} \n \t \r \0 \" \\ \{ \}"
say(message ?? "fallback")
// a comment, not floor division
flag = true and not false or null
other = !flag and 1 && 2 || 3
x = 7 div 2
x = ~1
x = flag ? 1 : 2
call(a.b[0]++, 1 != 2, 3 >= 4, 5 <= 6)
"#,
    )
    .unwrap();
    write_signature_check(&validator);
    let json_validator = dir.join("json_check.enm");
    write_json_check(&json_validator);
    lex_to_file(&dir, &source, &json_file);
    assert_json_array(&dir, &json_validator, &json_file);

    let result = run(
        &dir,
        &[
            "run",
            validator.to_str().unwrap(),
            json_file.to_str().unwrap(),
        ],
    );
    assert!(
        result.status.success(),
        "signature check failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let signatures = String::from_utf8_lossy(&result.stdout);
    for expected in [
        "Number|255|",
        "Number|10|",
        "Number|1000|",
        "Number|0.025|",
        "CompoundAssign|\"+\"|",
        "CompoundAssign|\"-\"|",
        "CompoundAssign|\"*\"|",
        "CompoundAssign|\"/\"|",
        "CompoundAssign|\"%\"|",
        "CompoundAssign|\"**\"|",
        "CompoundAssign|\"//\"|",
        "CompoundAssign|\"&\"|",
        "CompoundAssign|\"|\"|",
        "CompoundAssign|\"^\"|",
        "CompoundAssign|\"<<\"|",
        "CompoundAssign|\">>\"|",
        "PlusPlus|null|",
        "MinusMinus|null|",
        "Pow|null|",
        "FloorDiv|null|",
        "Div|null|",
        "Interpolated|",
        "NullCoalesce|null|",
        "Question|null|",
        "Colon|null|",
        "Bang|null|",
        "AndAlso|null|",
        "OrElse|null|",
        "BitNot|null|",
        "NotEq|null|",
        "GtEq|null|",
        "LtEq|null|",
    ] {
        assert!(
            signatures.contains(expected),
            "missing {expected:?} in signatures:\n{signatures}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

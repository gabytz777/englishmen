//! End-to-end parity test for the combined `selfhost/full.enm` pipeline.
//!
//! The script inlines the pure-Englishmen lexer, parser, AST→IR codegen, IR
//! encoder, VM, and runtime. For every example that does not import a native
//! module, its `output` array must match the lines printed by the native VM.

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const EXAMPLES: &[&str] = &[
    "hello.enm",
    "conditions.enm",
    "arrays.enm",
    "example.enm",
    "objects.enm",
    "objects-api.enm",
];

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_full_pipeline_env_{}", std::process::id()));
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(manifest_dir())
        .envs(isolated_env())
        .env_remove("EM_SELFTEST")
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em")
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Mirror `json_escape` in `src/builtins.rs`; `say` always prints strings.
fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Pull the `output` array out of the JSON summary printed by `full.enm`.
fn output_array(stdout: &str) -> &str {
    let start = stdout
        .find("\"output\":[")
        .expect("full.enm did not print an output array")
        + "\"output\":".len();
    let bytes = stdout.as_bytes();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, byte) in bytes[start..].iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && *byte == b'\\' {
            escaped = true;
            continue;
        }
        match *byte {
            b'"' => quoted = !quoted,
            b'[' if !quoted => depth += 1,
            b']' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return &stdout[start..start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated output array");
}

#[test]
fn combined_pipeline_matches_native_output_on_native_free_examples() {
    for name in EXAMPLES {
        let source = format!("examples/{name}");
        let native = run(&["run", &source]);
        assert!(
            native.status.success(),
            "native run failed for {name}: {}",
            String::from_utf8_lossy(&native.stderr)
        );

        let full = run(&["run", "selfhost/full.enm", &source]);
        assert!(
            full.status.success(),
            "self-hosted run failed for {name}: {}",
            String::from_utf8_lossy(&full.stderr)
        );

        let expected_lines: Vec<String> = stdout_text(&native).lines().map(json_escape).collect();
        let expected = format!("[{}]", expected_lines.join(","));
        assert_eq!(
            output_array(&stdout_text(&full)),
            expected,
            "output mismatch for {name}"
        );
    }
}

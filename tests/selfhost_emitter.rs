//! The schema-ordered Englishmen emitter must agree byte-for-byte with the
//! Rust AST JSON writer on every bundled example.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env("EM_NO_UPDATE_CHECK", "1")
        .env(
            "EM_HOME",
            std::env::temp_dir().join("em_selfhost_emitter_home"),
        )
        .stdin(std::process::Stdio::null())
        .output()
        .expect("failed to run em")
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "em failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string()
}

#[test]
fn emitter_matches_rust_ast_json_on_every_example() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut examples: Vec<PathBuf> = std::fs::read_dir(root.join("examples"))
        .expect("examples directory")
        .map(|entry| entry.expect("example entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("enm"))
        .collect();
    examples.sort();
    assert!(!examples.is_empty(), "no bundled examples found");

    for example in examples {
        let file = example.to_str().expect("utf-8 example path");
        let rust = stdout(&run(&["build", file, "--emit", "ast-json"]));
        let enm = stdout(&run(&["run", "selfhost/emit.enm", file]));
        assert_eq!(rust, enm, "emitter mismatch for {}", example.display());
    }
}

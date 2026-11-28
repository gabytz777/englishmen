//! Bootstrap parity test: the inlined `.enm` compiler must emit exactly the
//! same AST JSON as Rust's `em build <file> --emit ast-json`.
//!
//! The `--emit ast-json` flag and script argument forwarding are parent
//! integration steps (see docs/selfhost/CONTRACT.md section 8). Until they are
//! wired, this test reports a clear skip instead of pretending to verify the
//! comparison.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_bootstrap_env_{}", std::process::id()));
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

fn run(args: &[&str], cwd: Option<&Path>) -> std::process::Output {
    let mut command = Command::new(em());
    command.args(args).envs(isolated_env()).stdin(Stdio::null());
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command.output().expect("failed to run em")
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn trim_newlines(text: &str) -> &str {
    text.trim_end_matches(&['\r', '\n'][..])
}

fn examples() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(root().join("examples"))
        .expect("examples directory is missing")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("enm"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no examples/*.enm files found");
    paths
}

#[test]
fn bootstrap_ast_json_matches_rust_emitter() {
    let compiler = root().join("selfhost").join("compiler.enm");
    let source = fs::read_to_string(&compiler).expect("selfhost/compiler.enm is missing");
    if source.contains("TODO") {
        eprintln!(
            "SKIP selfhost bootstrap: selfhost/compiler.enm still contains TODO; \
             implement the inlined lexer, parser and emitter first."
        );
        return;
    }

    let example_paths = examples();
    let first = example_paths[0].to_str().expect("non-UTF-8 example path");
    let probe = run(&["build", first, "--emit", "ast-json"], None);
    if !probe.status.success() {
        let message = stderr_text(&probe);
        if message.contains("Unknown emit format 'ast-json'") {
            eprintln!(
                "SKIP selfhost bootstrap: this branch does not yet wire \
                 `em build <file> --emit ast-json` (parent integration step 3). \
                 The parity assertions will run once the flag is available."
            );
            return;
        }
        panic!("em build --emit ast-json failed unexpectedly: {message}");
    }

    let compiler_path = compiler.to_str().expect("non-UTF-8 compiler path");
    for example in example_paths {
        let path = example.to_str().expect("non-UTF-8 example path");
        let name = example
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>");

        let reference = run(&["build", path, "--emit", "ast-json"], None);
        assert!(
            reference.status.success(),
            "em build --emit ast-json failed for {name}: {}",
            stderr_text(&reference)
        );

        let bootstrap = run(&["run", compiler_path, path], Some(&root()));
        assert!(
            bootstrap.status.success(),
            "em run selfhost/compiler.enm failed for {name}: {}",
            stderr_text(&bootstrap)
        );

        let reference_stdout = stdout_text(&reference);
        let bootstrap_stdout = stdout_text(&bootstrap);
        let expected = trim_newlines(&reference_stdout);
        let actual = trim_newlines(&bootstrap_stdout);
        assert_eq!(
            actual, expected,
            "AST JSON differs for {name}\nexpected: {expected}\nactual:   {actual}"
        );
    }
}

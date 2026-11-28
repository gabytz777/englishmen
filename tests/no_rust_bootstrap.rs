//! Round-4 proof: the emitted compiler image must run without the Rust VM.
//!
//! This test implements the acceptance proof in `bootstrap-c/ABI.md`:
//!
//! 1. `make -C bootstrap-c` produces `bootstrap-c/emrun`.
//! 2. The regenerated `.enm` sources contain no native module imports outside
//!    stripped self-test guards.
//! 3. The Rust `em` binary is used once as an oracle: `full.enm` compiles
//!    `bootstrap_compile.enm` into a `.emb` image.
//! 4. `bootstrap-c/emrun <image.emb> examples/hello.enm` prints exactly the
//!    expected JSON summary line (see `EXPECTED`).
//! 5. The same image run with `em run-fixed` prints the identical line.
//!
//! Until `bootstrap-c/emrun` exists the proof test skips with an explicit
//! message; it never silently passes by substituting the Rust CLI for it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The exact output required by bootstrap-c/ABI.md. Do not relax this to a
/// JSON-shape or substring comparison: byte-for-byte equality is the proof.
const EXPECTED: &str =
    r#"{"astStatements":1,"encodedBytes":104,"irInstructions":4,"output":[]}"#;

const NATIVE_MODULES: &[&str] = &[
    "fs", "json", "os", "emb", "bytes", "project", "math", "cowsay", "string",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn emrun() -> PathBuf {
    root().join("bootstrap-c").join("emrun")
}

fn scratch() -> PathBuf {
    let base = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root().join("target"));
    let dir = base.join("em_no_rust_bootstrap").join(std::process::id().to_string());
    fs::create_dir_all(&dir).expect("could not create scratch directory");
    dir
}

fn isolated_env() -> [(&'static str, String); 5] {
    let dir = scratch();
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
        ("TMPDIR", dir.display().to_string()),
        ("EM_SELFTEST", "0".to_string()),
    ]
}

fn run(binary: &Path, args: &[&str]) -> Output {
    Command::new(binary)
        .args(args)
        .current_dir(root())
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", binary.display()))
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn summary_line(output: &Output, what: &str) -> String {
    assert!(
        output.status.success(),
        "{what} failed: {}",
        stderr_text(output)
    );
    let stdout = stdout_text(output);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "{what} must print exactly one line, got {lines:?}"
    );
    lines[0].to_string()
}

/// A native `import` is allowed only inside an `EM_SELFTEST` guard, where the
/// generated files strip it away. Indentation tracks guard nesting; Englishmen
/// blocks are delimited by `if`/`end`.
fn assert_no_unguarded_native_imports(relative: &str) {
    let path = root().join(relative);
    let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut guard_indents: Vec<usize> = Vec::new();
    for (lineno, line) in source.lines().enumerate() {
        let stripped = line.trim_start();
        let indent = line.len() - stripped.len();
        if !stripped.trim().is_empty() {
            while guard_indents.last().is_some_and(|open| indent <= *open) {
                guard_indents.pop();
            }
        }
        if let Some(rest) = stripped.strip_prefix("import ") {
            let name = rest.trim();
            if guard_indents.is_empty() && NATIVE_MODULES.contains(&name) {
                panic!(
                    "{}:{lineno}: unguarded native import '{name}' in {relative}",
                    path.display()
                );
            }
        }
        if stripped.starts_with("if ") && stripped.contains("EM_SELFTEST") {
            guard_indents.push(indent);
        }
    }
}

#[test]
fn generated_sources_have_no_unguarded_native_imports() {
    assert_no_unguarded_native_imports("selfhost/full.enm");
    assert_no_unguarded_native_imports("selfhost/bootstrap_compile.enm");
}

#[test]
fn no_rust_bootstrap_proof() {
    let runtime = emrun();
    if !runtime.is_file() {
        eprintln!(
            "SKIP no-rust-bootstrap: bootstrap-c/emrun is not built yet. \
             Build the independent C runtime with `make -C bootstrap-c` \
             (see bootstrap-c/ABI.md); this test will then emit the compiler \
             image with the Rust oracle and execute it with emrun."
        );
        return;
    }

    // Steps 2 and 3: prove the sources are import-free, then use the Rust
    // oracle exactly once to write the compiler image.
    assert_no_unguarded_native_imports("selfhost/full.enm");
    assert_no_unguarded_native_imports("selfhost/bootstrap_compile.enm");
    let image = scratch().join("bootstrap_compile.emb");
    let image_arg = image.to_str().expect("non-UTF-8 scratch path");
    let emitted = run(
        &em(),
        &[
            "run",
            "selfhost/full.enm",
            "selfhost/bootstrap_compile.enm",
            "--write-emb",
            image_arg,
        ],
    );
    assert!(
        emitted.status.success(),
        "Rust oracle could not emit the compiler image: {}",
        stderr_text(&emitted)
    );
    let bytes = fs::read(&image).expect("emitted .emb image is missing");
    assert!(
        bytes.starts_with(b"EMBC"),
        "emitted image has no EMBC magic header"
    );

    // Step 4: execute the image with the non-Rust runtime. This is the proof.
    let image_arg = image.to_str().expect("non-UTF-8 scratch path");
    let c_output = run(&runtime, &[image_arg, "examples/hello.enm"]);
    let c_summary = summary_line(&c_output, "bootstrap-c/emrun");
    assert_eq!(
        c_summary, EXPECTED,
        "emrun output differs from the required proof output\nexpected: {EXPECTED}\nactual:   {c_summary}"
    );

    // Step 5: the native VM must agree byte-for-byte on the same image.
    let rust_output = run(&em(), &["run-fixed", image_arg, "examples/hello.enm"]);
    let rust_summary = summary_line(&rust_output, "em run-fixed");
    assert_eq!(
        rust_summary, c_summary,
        "Rust and C runtimes disagree\nnative: {rust_summary}\nC:      {c_summary}"
    );
}

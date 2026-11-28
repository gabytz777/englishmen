//! Cross-backend coverage for `examples/bytes.enm`.
//!
//! The `bytes` module lives in a separate implementation branch. Once the
//! parent registers it in `src/modules/mod.rs`, both backends must print the
//! exact same deterministic output for the example program.
//!
//! TODO(parent): cherry-pick `sh-bytes-impl`, add `bytes` to `STD_MODULES` and
//! `load_module`, and update the CLI module catalogue. Until then this test
//! fails at `import bytes` with "Unknown module 'bytes'" by design.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

/// The output `examples/bytes.enm` is expected to print, in order.
const EXPECTED: &str = "\
[104, 195, 169, 108, 108, 111]
héllo
[10, 20, 30, 40]
4
10
40
[10, 99, 30, 40]
[1, 2, 3, 4, 5]
[2, 3, 4]
[3, 4, 5]
[1, 2, 3, 4, 5]
[99, 2, 3, 4, 5]
[170, 187, 204]
[10, 11, 12]
00 0f ff
[255]
[52, 18]
[120, 86, 52, 18]
[255, 255, 255, 255, 0, 0, 0, 0]
[254, 255]
[254, 255, 255, 255]
[254, 255, 255, 255, 255, 255, 255, 255]
[0, 0, 0, 0, 0, 0, 248, 63]
true
[0, 1, 254, 255]
00 01 fe ff
true
";

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

/// Keep the "prefer the configured install" logic and update checks away from
/// the real machine, as the other CLI tests do.
fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_bytes_env_{}", std::process::id()));
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

fn scratch(tag: &str) -> PathBuf {
    let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("em_bytes_{tag}_{}_{id}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(program: &Path, interpret: bool) -> std::process::Output {
    let mut command = Command::new(em());
    command.arg("run").arg(program);
    if interpret {
        command.arg("--interpret");
    }
    command
        .current_dir(program.parent().unwrap())
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em")
}

fn failure(label: &str, output: &std::process::Output) -> String {
    format!(
        "{label} failed with {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn bytes_example_prints_identical_output_on_both_backends() {
    let dir = scratch("example");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/bytes.enm");
    let program = dir.join("bytes.enm");
    fs::copy(&source, &program).unwrap();

    let compiled = run(&program, false);
    let interpreted = run(&program, true);

    assert!(compiled.status.success(), "{}", failure("VM", &compiled));
    assert!(
        interpreted.status.success(),
        "{}",
        failure("interpreter", &interpreted)
    );
    assert_eq!(
        compiled.stdout, interpreted.stdout,
        "backends disagree on examples/bytes.enm"
    );

    let stdout = String::from_utf8_lossy(&compiled.stdout);
    assert_eq!(stdout, EXPECTED, "unexpected example output");

    // The example must clean up its own binary scratch file.
    assert!(!dir.join("bytes-example.bin").exists());

    let _ = fs::remove_dir_all(&dir);
}

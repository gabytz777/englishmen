//! Guarded parity checks for `selfhost/pure_project_cowsay.enm`.
//!
//! The library is self-contained: it imports only `os` for the
//! `EM_SELFTEST` guard. Native `project` and `cowsay` are loaded inside that
//! guard and used strictly as reference oracles. File I/O goes through the
//! `host` module, which the guard aliases to the CLI's global builtins.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn library() -> String {
    fs::read_to_string(root().join("selfhost/pure_project_cowsay.enm"))
        .expect("selfhost/pure_project_cowsay.enm is missing")
}

fn scratch(tag: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "em_pure_project_cowsay_{tag}_{}_{id}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

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

fn run(script: &Path, cwd: &Path, selftest: bool, interpret: bool) -> Output {
    let mut command = Command::new(em());
    command
        .arg("run")
        .arg(script)
        .envs(isolated_env(cwd))
        .stdin(Stdio::null());
    if selftest {
        command.env("EM_SELFTEST", "1");
    } else {
        command.env_remove("EM_SELFTEST");
    }
    if interpret {
        command.arg("--interpret");
    }
    command.output().expect("failed to run em")
}

fn success(output: &Output, what: &str) -> String {
    assert!(
        output.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn source_is_formatted() {
    let output = Command::new(em())
        .arg("fmt")
        .arg(root().join("selfhost/pure_project_cowsay.enm"))
        .arg("--check")
        .envs(isolated_env(&scratch("fmt")))
        .output()
        .expect("failed to run em fmt --check");
    assert!(
        output.status.success(),
        "em fmt --check failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn inline_selftest_is_quiet_by_default_and_prints_ok_when_guarded() {
    let script = root().join("selfhost/pure_project_cowsay.enm");
    let dir = scratch("guard");
    for interpret in [false, true] {
        let quiet = run(&script, &dir, false, interpret);
        assert!(
            success(&quiet, "quiet library load").is_empty(),
            "library printed output without EM_SELFTEST (interpret={interpret})"
        );
        let guarded = run(&script, &dir, true, interpret);
        assert_eq!(
            success(&guarded, "inline parity selftest").trim_end(),
            "ok",
            "unexpected selftest output (interpret={interpret})"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn module_objects_expose_the_expected_api() {
    let dir = scratch("api");
    let mut source = String::from("import os\nhost = {readFile: readFile, writeFile: writeFile, cwd: os.cwd, tempDir: os.tempDir, pid: os.pid, env: os.env}\n");
    source.push_str(&library().split("if os.env(").next().unwrap());
    source.push_str(
        r#"
p = ppcProjectModule()
c = ppcCowModule()
assert(jsonEncode(sorted(keys(p))) == jsonEncode(["default", "get", "load", "main", "name", "parse", "section", "version", "write"]), "project API mismatch")
assert(jsonEncode(sorted(keys(c))) == jsonEncode(["render", "say"]), "cowsay API mismatch")
cfg = p.parse(p.default("Demo"))
assert(p.name(cfg) == "Demo", "name mismatch")
assert(p.version(cfg) == "1.0.0", "version mismatch")
assert(p.main(cfg) == "main.enm", "main mismatch")
assert(c.render("Hello").indexOf("^__^") >= 0, "cow art missing")
say("ok")
"#,
    );
    let script = dir.join("api.enm");
    fs::write(&script, source).unwrap();
    for interpret in [false, true] {
        let output = run(&script, &dir, false, interpret);
        assert_eq!(
            success(&output, "module API driver").trim_end(),
            "ok",
            "unexpected API output (interpret={interpret})"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

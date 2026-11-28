//! CLI coverage for the native `project` module.
//!
//! NOTE FOR THE PARENT INTEGRATOR: these tests can only pass after
//! `src/modules/project.rs` is registered in `src/modules/mod.rs` (both
//! `STD_MODULES` and `load_module`) and `project` is added to the CLI module
//! catalogue. Until then every program here fails at `import project` with
//! "Unknown module 'project'". The Rust-level unit tests in
//! `src/modules/project.rs` use `call_native` and need the same registration
//! before the bin test harness compiles them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

/// Keep the CLI from touching a real Englishmen install or checking for
/// updates, matching the isolation used by the other CLI test suites.
fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_project_env_{}", std::process::id()));
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

fn scratch(tag: &str) -> PathBuf {
    let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "em_project_module_{}_{}_{id}",
        tag,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Write `source` into a scratch program and require identical successful
/// output from the compiler/VM and the reference interpreter.
fn agree(tag: &str, source: &str, cwd: &Path) -> String {
    let file = cwd.join("program.enm");
    fs::write(&file, source).unwrap();
    let program = file.to_str().unwrap();

    let vm = run(&[program], Some(cwd));
    let interpreted = run(&[program, "--interpret"], Some(cwd));
    assert!(
        vm.status.success(),
        "VM failed for {tag}: {}",
        stderr_text(&vm)
    );
    assert!(
        interpreted.status.success(),
        "interpreter failed for {tag}: {}",
        stderr_text(&interpreted)
    );
    assert_eq!(vm.stdout, interpreted.stdout, "backends disagree for {tag}");
    assert!(
        stderr_text(&vm).is_empty(),
        "VM wrote diagnostics for {tag}: {}",
        stderr_text(&vm)
    );
    assert!(
        stderr_text(&interpreted).is_empty(),
        "interpreter wrote diagnostics for {tag}: {}",
        stderr_text(&interpreted)
    );
    String::from_utf8(vm.stdout).unwrap()
}

#[test]
fn parse_and_accessors_work_on_both_backends() {
    let dir = scratch("parse");
    let out = agree(
        "parse",
        r#"import project

config = project.parse("name = \"Demo\"\nversion = \"2.1\"\nmain = \"main.enm\"\n[installer]\ninstall_dir = \"C:/em\"\nprojects_enabled = \"true\"\n")
say(project.name(config))
say(project.version(config))
say(project.main(config))
say(project.get(config, "missing"))
say(project.section(config, "installer").install_dir)
say(project.section(config, "missing").length())
"#,
        &dir,
    );
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        ["Demo", "2.1", "main.enm", "null", "C:/em", "0"]
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn default_returns_the_standard_manifest_text() {
    let dir = scratch("default");
    let out = agree(
        "default",
        r#"import project
say(project.default("Demo"))
"#,
        &dir,
    );
    assert_eq!(
        out,
        "name = \"Demo\"\nversion = \"1.0.0\"\nmain = \"main.enm\"\n\n"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn load_uses_the_working_directory_by_default() {
    let dir = scratch("load");
    fs::write(
        dir.join("englishmen.emc"),
        "name = \"Loaded\"\nversion = \"3.0\"\n",
    )
    .unwrap();
    let out = agree(
        "load",
        r#"import project

default_config = project.load()
explicit_config = project.load("englishmen.emc")
say(project.name(default_config))
say(project.version(explicit_config))
"#,
        &dir,
    );
    assert_eq!(out.lines().collect::<Vec<_>>(), ["Loaded", "3.0"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_produces_stable_emc_text_and_round_trips() {
    let dir = scratch("write");
    let out = agree(
        "write",
        r#"import fs
import project

config = {root: {z: 3, name: "Demo", enabled: true, nothing: null}, sections: {alpha: {b: "two", a: 1}}}
say(project.write("written.emc", config))
say(fs.read("written.emc"))
loaded = project.load("written.emc")
say(project.name(loaded))
say(type(project.get(loaded, "enabled")))
"#,
        &dir,
    );
    assert_eq!(
        fs::read_to_string(dir.join("written.emc")).unwrap(),
        "enabled = true\nname = \"Demo\"\nnothing = null\nz = 3\n[alpha]\na = 1\nb = \"two\"\n"
    );
    let lines = out.lines().collect::<Vec<_>>();
    assert_eq!(lines.first(), Some(&"true"));
    assert_eq!(lines.last(), Some(&"string"));
    assert!(lines.contains(&"Demo"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn invalid_native_arguments_fail_with_helpful_errors() {
    let dir = scratch("errors");
    let file = dir.join("bad.enm");

    fs::write(&file, "import project\nproject.parse(42)\n").unwrap();
    let bad_text = run(&[file.to_str().unwrap()], Some(&dir));
    assert!(!bad_text.status.success());
    assert!(
        stderr_text(&bad_text).contains("expected a string"),
        "{}",
        stderr_text(&bad_text)
    );

    fs::write(&file, "import project\nproject.get(1, \"name\")\n").unwrap();
    let bad_config = run(&[file.to_str().unwrap(), "--interpret"], Some(&dir));
    assert!(!bad_config.status.success());
    assert!(
        stderr_text(&bad_config).contains("expected a project config object"),
        "{}",
        stderr_text(&bad_config)
    );
    let _ = fs::remove_dir_all(&dir);
}

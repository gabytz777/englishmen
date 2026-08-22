use std::path::PathBuf;
use std::process::{Command, Stdio};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

/// Every test runs with EM_HOME/EM_PROJECTS_DIR pointed at nonexistent scratch
/// paths so `em`'s "prefer the configured install" logic never touches the real
/// machine's Englishmen install, even if one happens to be present.
fn isolated_env() -> [(&'static str, String); 2] {
    let dir = std::env::temp_dir().join(format!("em_cli_isolated_env_{}", std::process::id()));
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
    ]
}

/// stdin is pinned to null so `em` never sees a real terminal here — this keeps any
/// interactive prompt (install confirmations, the "press Enter to continue" pause)
/// from ever blocking a test, regardless of what stdin the test harness itself has.
fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em")
}

#[test]
fn runs_the_required_example_program() {
    let dir = std::env::temp_dir().join(format!("em_cli_test_example_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("example.em");
    std::fs::write(
        &file,
        r#"$-
    Englishmen example
-$

person.name = "Alex"
person.age = 21
person.active = true

if person.age >= 18 and person.active == true
    say("Welcome, " + person.name)
else
    say("Access denied")
end
"#,
    )
    .unwrap();

    let output = run(&[file.to_str().unwrap()]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "Welcome, Alex");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reports_a_helpful_error_for_unknown_variables() {
    let dir = std::env::temp_dir().join(format!("em_cli_test_err_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bad.em");
    std::fs::write(&file, "age = 18\nsay(agee)\n").unwrap();

    let output = run(&[file.to_str().unwrap()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown variable"));
    assert!(stderr.contains("age"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn help_flag_lists_usage() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage"));
    assert!(stdout.contains("em new"));
    assert!(stdout.contains("em repair"));
    assert!(stdout.contains("em build"));
}

#[test]
fn version_flag_prints_version() {
    let output = run(&["--version"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Englishmen"));
}

#[test]
fn new_creates_a_project_with_emc_and_main() {
    let dir = std::env::temp_dir().join(format!("em_cli_test_new_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let output = Command::new(bin())
        .args(["new", "TestProject"])
        .current_dir(&dir)
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em new");
    assert!(output.status.success());

    let project_dir = dir.join("TestProject");
    assert!(project_dir.join("englishmen.emc").exists());
    assert!(project_dir.join("main.em").exists());

    let emc = std::fs::read_to_string(project_dir.join("englishmen.emc")).unwrap();
    assert!(emc.contains("TestProject"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_refuses_to_overwrite_an_existing_project() {
    let dir = std::env::temp_dir().join(format!("em_cli_test_new_dup_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("Dup")).unwrap();

    let output = Command::new(bin())
        .args(["new", "Dup"])
        .current_dir(&dir)
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em new");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already exists"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runs_all_bundled_examples_successfully() {
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    for entry in std::fs::read_dir(&examples_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "em").unwrap_or(false) {
            let output = run(&[path.to_str().unwrap()]);
            assert!(
                output.status.success(),
                "example {:?} failed: {}",
                path,
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

/// A scratch directory for one test, removed on the way out.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("em_cli_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const SAMPLE: &str = r#"function double(n)
    return n * 2
end

total = 0
for n in [1, 2, 3]
    total = total + double(n)
end
say("total is " + total)
"#;

#[test]
fn build_writes_bytecode_that_runs_the_same_as_the_source() {
    let dir = scratch("build");
    let source = dir.join("prog.em");
    std::fs::write(&source, SAMPLE).unwrap();

    let build = run(&["build", source.to_str().unwrap()]);
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let build_stdout = String::from_utf8_lossy(&build.stdout);
    assert!(build_stdout.contains("instructions"), "{build_stdout}");

    let bytecode = dir.join("prog.emb");
    assert!(bytecode.exists(), "em build should write prog.emb");

    let from_source = run(&[source.to_str().unwrap()]);
    let from_bytecode = run(&[bytecode.to_str().unwrap()]);
    assert!(from_bytecode.status.success());
    assert_eq!(
        String::from_utf8_lossy(&from_source.stdout),
        String::from_utf8_lossy(&from_bytecode.stdout)
    );
    assert!(String::from_utf8_lossy(&from_bytecode.stdout).contains("total is 12"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_honours_the_output_flag_and_can_run_immediately() {
    let dir = scratch("build_out");
    let source = dir.join("prog.em");
    std::fs::write(&source, SAMPLE).unwrap();
    let out = dir.join("custom.emb");

    let output = run(&[
        "build",
        source.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--run",
    ]);
    assert!(output.status.success());
    assert!(out.exists());
    assert!(!dir.join("prog.emb").exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("total is 12"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_emit_asm_prints_a_disassembly_without_writing_a_file() {
    let dir = scratch("build_asm");
    let source = dir.join("prog.em");
    std::fs::write(&source, SAMPLE).unwrap();

    let output = run(&["build", source.to_str().unwrap(), "--emit", "asm"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("== prog.em =="), "{stdout}");
    assert!(stdout.contains("MAKE_CLOSURE"), "{stdout}");
    assert!(stdout.contains("function double"), "{stdout}");
    assert!(!dir.join("prog.emb").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_with_no_file_uses_the_project_main() {
    let dir = scratch("build_project");
    std::fs::write(
        dir.join("englishmen.emc"),
        "name = \"Demo\"\nversion = \"1.0.0\"\nmain = \"main.em\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("main.em"), "say(\"from the project main\")\n").unwrap();

    let output = Command::new(bin())
        .args(["build", "--run"])
        .current_dir(&dir)
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em build");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.join("main.emb").exists());
    assert!(String::from_utf8_lossy(&output.stdout).contains("from the project main"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn running_a_file_that_is_not_bytecode_fails_clearly() {
    let dir = scratch("bad_emb");
    let fake = dir.join("fake.emb");
    std::fs::write(&fake, "say(\"hello\")\n").unwrap();

    let output = run(&[fake.to_str().unwrap()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Not an Englishmen bytecode file"),
        "{stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn both_backends_produce_the_same_output_for_every_example() {
    let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    for entry in std::fs::read_dir(&examples_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "em").unwrap_or(false) {
            let compiled = run(&[path.to_str().unwrap()]);
            let interpreted = run(&[path.to_str().unwrap(), "--interpret"]);
            assert_eq!(
                String::from_utf8_lossy(&compiled.stdout),
                String::from_utf8_lossy(&interpreted.stdout),
                "backends disagree on {path:?}"
            );
        }
    }
}

/// The platform's name for a standalone build of `stem`.
fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

#[test]
fn build_exe_produces_a_program_that_runs_on_its_own() {
    let dir = scratch("build_exe");
    let source = dir.join("prog.em");
    std::fs::write(&source, SAMPLE).unwrap();

    let build = run(&["build", source.to_str().unwrap(), "--exe"]);
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    assert!(String::from_utf8_lossy(&build.stdout).contains("runs on its own"));

    let program = dir.join(exe_name("prog"));
    assert!(
        program.exists(),
        "expected a standalone build at {program:?}"
    );

    // Run it as itself, with no arguments and no reference to `em`.
    let output = Command::new(&program)
        .current_dir(&dir)
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run the standalone program");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("total is 12"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_standalone_program_ignores_em_commands_and_stays_itself() {
    let dir = scratch("build_exe_args");
    let source = dir.join("prog.em");
    std::fs::write(&source, SAMPLE).unwrap();
    assert!(run(&["build", source.to_str().unwrap(), "--exe"])
        .status
        .success());

    // The bundled program is the program, so `--version` is not em's anymore.
    let output = Command::new(dir.join(exe_name("prog")))
        .arg("--version")
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run the standalone program");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("total is 12"), "{stdout}");
    assert!(!stdout.contains("Englishmen 1."), "{stdout}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn build_exe_refuses_to_overwrite_the_source() {
    let dir = scratch("build_exe_clobber");
    let source = dir.join("prog.em");
    std::fs::write(&source, SAMPLE).unwrap();

    let output = run(&[
        "build",
        source.to_str().unwrap(),
        "--exe",
        "-o",
        source.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("would overwrite"));
    assert_eq!(std::fs::read_to_string(&source).unwrap(), SAMPLE);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn em_itself_is_not_mistaken_for_a_standalone_program() {
    // The runtime must only take the bundled path when a payload is present.
    let output = run(&["--version"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Englishmen"));
}

#[test]
fn runs_a_file_saved_with_a_utf8_byte_order_mark() {
    let dir = scratch("bom");
    let file = dir.join("bom.em");
    // Notepad and PowerShell's Out-File write this BOM by default.
    let mut contents = String::from("\u{feff}");
    contents.push_str("say(\"hello from a BOM file\")\n");
    std::fs::write(&file, contents).unwrap();

    let output = run(&[file.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("hello from a BOM file"));

    // The compiler path must accept it too, not just the runner.
    let build = run(&["build", file.to_str().unwrap()]);
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

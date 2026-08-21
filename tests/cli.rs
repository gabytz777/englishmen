use std::path::PathBuf;
use std::process::Command;

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

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .envs(isolated_env())
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
    assert!(!stdout.contains("em build"));
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

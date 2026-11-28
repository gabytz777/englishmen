//! Parity tests for the pure-Englishmen `project` manifest module.
//!
//! `selfhost/lib_project.enm` is self-contained (no local `.enm` imports), so
//! the Rust harness concatenates it with a generated driver and drives both
//! the pure functions and the native `project` module on the same fixtures.
//! The native module is used strictly as an oracle.

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

fn lib_source() -> String {
    fs::read_to_string(root().join("selfhost/lib_project.enm"))
        .expect("selfhost/lib_project.enm is missing")
}

fn scratch(tag: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "em_selfhost_enm_modules_{tag}_{}_{id}",
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

fn run(script: &Path, cwd: &Path, selftest: bool) -> Output {
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

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Rust's debug string escaping is accepted by the Englishmen lexer (`\n`,
/// `\t`, `\r`, `\0`, `\u{...}`, `\"`, `\\`).
fn enm_string(text: &str) -> String {
    format!("{text:?}")
}

fn fixture_texts() -> Vec<String> {
    vec![
        String::new(),
        "name = \"Demo\"\nversion = \"1.0.0\"\nmain = \"main.enm\"\n".to_string(),
        "# leading comment\n\n   name = \"  spaced  \"   \nversion=2.1\nmain = bare\n"
            .to_string(),
        "name = \"Demo\"\r\n\tversion = \"2.1\"\r\n# comment\r\n[notes]\r\nbody = \"hello\"\r\n"
            .to_string(),
        "\u{a0}name = \"Wide\"\u{a0}\n\u{a0}[  section  ]\u{a0}\n\u{a0}key = \"value\"\u{a0}\n"
            .to_string(),
        format!(
            "{}\n",
            r#"a = "quote \" slash \\ newline \n return \r tab \t control \u001f""#
        ),
        format!(
            "{}\n",
            r#"a = "short \u41 plus \u+42 bad \uZZZZ surrogate \ud800 high \u{d800} low \u{110000}""#
        ),
        "[a]\nx = \"first\"\n[a]\nx = \"last\"\ny = \"a=b#c\"\n[ü]\nключ = \"значение\"\n"
            .to_string(),
        "dup = \"first\"\ndup = \"second\"\n[sec]\nq = \"# not a comment\"\n".to_string(),
        format!("x = \"{}\"\n", "\u{0001}\u{263a}"),
        format!("x = \"{}\"\n", "\0"),
        format!("{}\n", r#"q = "a"b""#),
        format!("{}\n", r#"z = "tail \\""#),
        "single = 'quoted'\nempty =\n[empty]\n".to_string(),
    ]
}

fn manifest_literals() -> Vec<String> {
    let tricky = "quote \" slash \\ newline \n return \r tab \t control \u{1f} unicode ☺ \0";
    vec![
        "{root: {}, sections: {}}".to_string(),
        format!(
            "{{root: {{z: 3, name: {}, enabled: true, nothing: null, big: 1e20, small: 1e-5, whole: 12.0, boundary: 1000000000000000.0, negative: -2.25, tricky: {}}}, sections: {{alpha: {{b: {}, a: 1}}, empty: {{}}}}}}",
            enm_string("Demo"),
            enm_string(tricky),
            enm_string("two")
        ),
        format!(
            "{{root: {{esc: {}}}, sections: {{}}}}",
            enm_string("line\n\"quote\"\\tab\t")
        ),
    ]
}

fn parity_driver(fixture_paths: &[PathBuf], write_path: &Path, manifests: &[String]) -> String {
    let mut source = lib_source();
    source.push_str("\nimport project\n");
    let sample_lines: Vec<String> = fixture_paths
        .iter()
        .map(|path| format!("    readFile({})", enm_string(path.to_str().unwrap())))
        .collect();
    source.push_str("samples = [\n");
    source.push_str(&sample_lines.join(",\n"));
    source.push_str("\n]\n");
    source.push_str(&format!(
        "path = {}\n",
        enm_string(write_path.to_str().unwrap())
    ));
    source.push_str(
        r#"
i = 0
while i < samples.length()
    text = samples[i]
    pure = pjParse(text)
    native = project.parse(text)
    assert(jsonEncode(pure) == jsonEncode(native), "pjParse mismatch for sample {i}: " + jsonEncode(pure) + " != " + jsonEncode(native))
    assert(jsonEncode(pjName(pure)) == jsonEncode(project.name(native)), "pjName mismatch for sample {i}")
    assert(jsonEncode(pjVersion(pure)) == jsonEncode(project.version(native)), "pjVersion mismatch for sample {i}")
    assert(jsonEncode(pjMain(pure)) == jsonEncode(project.main(native)), "pjMain mismatch for sample {i}")
    assert(project.write(path, native), "native write failed for sample {i}")
    written = readFile(path)
    assert(pjWrite(pure) == written, "pjWrite mismatch for sample {i}")
    assert(jsonEncode(pjParse(written)) == jsonEncode(project.parse(written)), "parse-after-write mismatch for sample {i}")
    i += 1
end
"#,
    );
    let manifest_lines: Vec<String> = manifests
        .iter()
        .map(|literal| format!("    {literal}"))
        .collect();
    source.push_str("manifests = [\n");
    source.push_str(&manifest_lines.join(",\n"));
    source.push_str("\n");
    source.push_str(
        r#"
]
i = 0
while i < manifests.length()
    config = manifests[i]
    assert(project.write(path, config), "native write failed for manifest {i}")
    expected = readFile(path)
    assert(pjWrite(config) == expected, "pjWrite mismatch for manifest {i}")
    assert(jsonEncode(pjParse(expected)) == jsonEncode(project.parse(expected)), "parse-after-write mismatch for manifest {i}")
    i += 1
end
say("ok")
"#,
    );
    source
}

fn first_error_line(output: &Output) -> String {
    stderr(output)
        .lines()
        .next()
        .unwrap_or("<no stderr>")
        .trim_end()
        .to_string()
}

fn expect_same_error(pure_source: &str, native_source: &str, dir: &Path, tag: &str) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let pure_path = dir.join(format!("{tag}_pure_{id}.enm"));
    let native_path = dir.join(format!("{tag}_native_{id}.enm"));
    fs::write(&pure_path, pure_source).unwrap();
    fs::write(&native_path, native_source).unwrap();

    let pure = run(&pure_path, dir, false);
    let native = run(&native_path, dir, false);
    assert!(!pure.status.success(), "{tag}: pure call unexpectedly succeeded");
    assert!(
        !native.status.success(),
        "{tag}: native call unexpectedly succeeded"
    );
    let pure_message = first_error_line(&pure)
        .trim_start_matches("Error: Assertion failed: ")
        .to_string();
    let native_message = first_error_line(&native)
        .trim_start_matches("Error: ")
        .to_string();
    assert_eq!(
        pure_message, native_message,
        "{tag}: error message differs\npure:   {pure_message}\nnative: {native_message}"
    );
}

#[test]
fn pure_manifest_api_matches_native_on_broad_fixtures() {
    let dir = scratch("parity");
    let fixtures = fixture_texts();
    let mut fixture_paths = Vec::new();
    for (index, text) in fixtures.iter().enumerate() {
        let path = dir.join(format!("fixture_{index}.emc"));
        fs::write(&path, text).unwrap();
        fixture_paths.push(path);
    }
    let write_path = dir.join("written.emc");
    let driver_path = dir.join("driver.enm");
    fs::write(
        &driver_path,
        parity_driver(&fixture_paths, &write_path, &manifest_literals()),
    )
    .unwrap();

    for backend in ["vm", "interpreter"] {
        let mut command = Command::new(em());
        command
            .arg("run")
            .arg(&driver_path)
            .envs(isolated_env(&dir))
            .stdin(Stdio::null())
            .env_remove("EM_SELFTEST");
        if backend == "interpreter" {
            command.arg("--interpret");
        }
        let output = command.output().expect("failed to run em");
        let stdout = success(&output, &format!("{backend} parity driver"));
        assert_eq!(
            stdout.trim_end(),
            "ok",
            "{backend} driver printed unexpected output: {stdout}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn validation_errors_match_native() {
    let dir = scratch("errors");
    let path_text = enm_string(dir.join("written.emc").to_str().unwrap());
    let library = lib_source();

    // Object literal keys must be identifiers in Englishmen, so invalid keys
    // and section names are installed at runtime with `set`.
    let cases = [
        "cfg.root.set(\"bad key\", \"x\")",
        "cfg.root.set(\"#tag\", \"x\")",
        "cfg.sections.set(\"bad=name\", {})",
        "cfg.root.set(\"bad\", [1])",
        "cfg.root.set(\"bad\", pow(10, 400))",
        "cfg.sections.set(\"s\", 2)",
        "cfg.root = 1",
    ];
    for (index, setup) in cases.iter().enumerate() {
        let pure_source = format!(
            "{library}\ncfg = {{root: {{}}, sections: {{}}}}\n{setup}\npjWrite(cfg)\n"
        );
        let native_source = format!(
            "import project\ncfg = {{root: {{}}, sections: {{}}}}\n{setup}\nproject.write({path_text}, cfg)\n"
        );
        expect_same_error(&pure_source, &native_source, &dir, &format!("write_{index}"));
    }

    expect_same_error(
        &format!("{library}\npjParse(42)\n"),
        "import project\nproject.parse(42)\n",
        &dir,
        "parse_type",
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn inline_selftest_guard_prints_ok_and_is_quiet_by_default() {
    let script = root().join("selfhost/lib_project.enm");
    let default_output = run(&script, &root(), false);
    assert!(
        success(&default_output, "quiet library load").is_empty(),
        "library printed output without EM_SELFTEST"
    );
    let selftest_output = run(&script, &root(), true);
    assert_eq!(success(&selftest_output, "inline selftest").trim_end(), "ok");
}

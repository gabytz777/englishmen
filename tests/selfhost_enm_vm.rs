//! CLI-level parity tests for `selfhost/vm_runtime.enm`.
//!
//! Each fixture is an EMB IR object (see `docs/selfhost/EMB_AND_MANIFEST.md`).
//! The native oracle writes the same object to a `.emb` image with `emb.write`
//! and executes it with `em run-fixed`, so stdout comes from `bytecode::Vm`.
//! The self-hosted runtime reads the IR as JSON and prints its collected
//! output array. The test compares that array with the native `Say` lines.
//!
//! No `serde_json` dependency is needed: `say` always prints strings, so the
//! expected JSON array is built from the native stdout lines directly.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Keep `em` isolated from a real install and off the update network, matching
/// the setup used by `tests/emb_ir.rs` and `tests/cli.rs`.
fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_vm_runtime_env_{}", std::process::id()));
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
        .envs(isolated_env())
        .env_remove("EM_SELFTEST")
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("em_vm_runtime_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Escape a host string for an Englishmen string literal.
fn enm_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Mirror `json_escape` in `src/builtins.rs` for the expected output array.
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

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

/// Write the IR as both a JSON file (for the self-hosted CLI) and a native
/// emitter that rebuilds the same object with `jsonDecode` + `emb.write`.
fn prepare(dir: &Path, tag: &str, ir: &str) -> (PathBuf, PathBuf) {
    let json_path = dir.join(format!("{tag}.json"));
    let image_path = dir.join(format!("{tag}.emb"));
    let emitter_path = dir.join(format!("{tag}_emit.enm"));
    std::fs::write(&json_path, ir).unwrap();

    let source = format!(
        r#"$-\ Native oracle: write the fixture IR as a .emb image. -$
import emb
import fs
ir = jsonDecode(fs.read("{json}"))
emb.write("{image}", ir)
"#,
        json = enm_string(json_path.to_str().expect("utf-8 path")),
        image = enm_string(image_path.to_str().expect("utf-8 path")),
    );
    std::fs::write(&emitter_path, source).unwrap();

    let emitted = run(&["run", emitter_path.to_str().unwrap()]);
    assert!(
        emitted.status.success(),
        "{tag}: emitter failed: {}",
        stderr_text(&emitted)
    );
    assert!(
        image_path.exists(),
        "{tag}: emb.write did not create the image"
    );
    (json_path, image_path)
}

/// Compare native VM stdout with the self-hosted runtime's output array.
fn check_fixture(tag: &str, ir: &str) {
    let dir = scratch(tag);
    let (json_path, image_path) = prepare(&dir, tag, ir);

    let native = run(&["run-fixed", image_path.to_str().unwrap()]);
    assert!(
        native.status.success(),
        "{tag}: native VM failed: {}",
        stderr_text(&native)
    );
    let lines: Vec<String> = String::from_utf8_lossy(&native.stdout)
        .trim()
        .lines()
        .map(|line| line.to_string())
        .collect();
    let expected = format!(
        "[{}]",
        lines
            .iter()
            .map(|line| json_escape(line))
            .collect::<Vec<_>>()
            .join(",")
    );

    let runtime_path = manifest_dir().join("selfhost/vm_runtime.enm");
    let hosted = run(&[
        "run",
        runtime_path.to_str().unwrap(),
        json_path.to_str().unwrap(),
    ]);
    assert!(
        hosted.status.success(),
        "{tag}: self-hosted runtime failed: {}",
        stderr_text(&hosted)
    );
    assert_eq!(
        stdout_text(&hosted),
        expected,
        "{tag}: self-hosted output array differs from native Say lines"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn selftest_prints_ok() {
    let runtime_path = manifest_dir().join("selfhost/vm_runtime.enm");
    let output = Command::new(bin())
        .arg("run")
        .arg(runtime_path)
        .envs(isolated_env())
        .env("EM_SELFTEST", "1")
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em");
    assert!(
        output.status.success(),
        "selftest failed: {}",
        stderr_text(&output)
    );
    assert_eq!(stdout_text(&output), "ok");
}

#[test]
fn source_is_formatted() {
    let runtime_path = manifest_dir().join("selfhost/vm_runtime.enm");
    let output = run(&["fmt", "--check", runtime_path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "em fmt --check failed: {}{}",
        stdout_text(&output),
        stderr_text(&output)
    );
}

#[test]
fn hello_and_arithmetic_match_the_native_vm() {
    check_fixture(
        "hello",
        r#"{"version":2,"constants":[{"tag":"string","value":"hello from rt"}],"functions":[],"code":[{"op":"LoadConst","a":0,"b":0,"line":1,"col":1},{"op":"Say","a":1,"b":0,"line":1,"col":1}]}"#,
    );
    check_fixture(
        "arithmetic",
        r#"{"version":2,"constants":[{"tag":"number","value":2},{"tag":"number","value":3}],"functions":[],"code":[{"op":"LoadConst","a":0,"b":0,"line":1,"col":1},{"op":"LoadConst","a":1,"b":0,"line":1,"col":1},{"op":"Add","a":0,"b":0,"line":1,"col":1},{"op":"Say","a":1,"b":0,"line":1,"col":1}]}"#,
    );
}

#[test]
fn variables_and_repeat_match_the_native_vm() {
    // n = 1
    // repeat 3
    //   say(n)
    //   n = n + 1
    // end
    check_fixture(
        "repeat",
        r#"{"version":2,"constants":[{"tag":"number","value":1},{"tag":"string","value":"n"},{"tag":"number","value":3}],"functions":[],"code":[{"op":"LoadConst","a":0,"b":0,"line":1,"col":1},{"op":"DefineVar","a":1,"b":0,"line":1,"col":1},{"op":"LoadConst","a":2,"b":0,"line":2,"col":1},{"op":"RepeatSetup","a":0,"b":0,"line":2,"col":1},{"op":"RepeatNext","a":13,"b":0,"line":2,"col":1},{"op":"GetVar","a":1,"b":0,"line":3,"col":7},{"op":"Say","a":1,"b":0,"line":3,"col":3},{"op":"Pop","a":0,"b":0,"line":3,"col":3},{"op":"GetVar","a":1,"b":0,"line":4,"col":7},{"op":"LoadConst","a":0,"b":0,"line":4,"col":11},{"op":"Add","a":0,"b":0,"line":4,"col":9},{"op":"SetVar","a":1,"b":0,"line":4,"col":3},{"op":"Jump","a":4,"b":0,"line":5,"col":1}]}"#,
    );
}

#[test]
fn conditionals_match_the_native_vm() {
    // age = 15; if age >= 18 -> adult, else if age >= 13 -> teen, else child.
    check_fixture(
        "conditional",
        r#"{"version":2,"constants":[{"tag":"number","value":15},{"tag":"string","value":"age"},{"tag":"number","value":18},{"tag":"string","value":"adult"},{"tag":"number","value":13},{"tag":"string","value":"teen"},{"tag":"string","value":"child"}],"functions":[],"code":[{"op":"LoadConst","a":0,"b":0,"line":1,"col":1},{"op":"DefineVar","a":1,"b":0,"line":1,"col":1},{"op":"GetVar","a":1,"b":0,"line":2,"col":4},{"op":"LoadConst","a":2,"b":0,"line":2,"col":11},{"op":"GtEq","a":0,"b":0,"line":2,"col":8},{"op":"JumpIfFalse","a":10,"b":0,"line":2,"col":1},{"op":"LoadConst","a":3,"b":0,"line":3,"col":6},{"op":"Say","a":1,"b":0,"line":3,"col":3},{"op":"Pop","a":0,"b":0,"line":3,"col":3},{"op":"Jump","a":21,"b":0,"line":3,"col":3},{"op":"GetVar","a":1,"b":0,"line":4,"col":12},{"op":"LoadConst","a":4,"b":0,"line":4,"col":19},{"op":"GtEq","a":0,"b":0,"line":4,"col":16},{"op":"JumpIfFalse","a":18,"b":0,"line":4,"col":9},{"op":"LoadConst","a":5,"b":0,"line":5,"col":6},{"op":"Say","a":1,"b":0,"line":5,"col":3},{"op":"Pop","a":0,"b":0,"line":5,"col":3},{"op":"Jump","a":21,"b":0,"line":5,"col":3},{"op":"LoadConst","a":6,"b":0,"line":7,"col":6},{"op":"Say","a":1,"b":0,"line":7,"col":3},{"op":"Pop","a":0,"b":0,"line":7,"col":3}]}"#,
    );
}

#[test]
fn closures_calls_and_methods_match_the_native_vm() {
    // function greet(name) say("Hi " + name) return name.length() end
    // say(greet("Ada"))
    check_fixture(
        "function",
        r#"{"version":2,"constants":[{"tag":"string","value":"greet"},{"tag":"string","value":"Ada"},{"tag":"string","value":"result"}],"functions":[{"name":"greet","params":["name"],"chunk":{"constants":[{"tag":"string","value":"Hi "},{"tag":"string","value":"name"},{"tag":"string","value":"length"}],"functions":[],"code":[{"op":"LoadConst","a":0,"b":0,"line":1,"col":9},{"op":"GetVar","a":1,"b":0,"line":1,"col":13},{"op":"Add","a":0,"b":0,"line":1,"col":11},{"op":"Say","a":1,"b":0,"line":1,"col":3},{"op":"Pop","a":0,"b":0,"line":1,"col":3},{"op":"GetVar","a":1,"b":0,"line":2,"col":10},{"op":"MethodCall","a":2,"b":0,"line":2,"col":14},{"op":"Return","a":0,"b":0,"line":2,"col":3}]}}],"code":[{"op":"MakeClosure","a":0,"b":0,"line":1,"col":1},{"op":"DefineVar","a":0,"b":0,"line":1,"col":1},{"op":"GetVar","a":0,"b":0,"line":4,"col":12},{"op":"LoadConst","a":1,"b":0,"line":4,"col":18},{"op":"Call","a":1,"b":0,"line":4,"col":12},{"op":"DefineVar","a":2,"b":0,"line":4,"col":1},{"op":"GetVar","a":2,"b":0,"line":5,"col":5},{"op":"Say","a":1,"b":0,"line":5,"col":1},{"op":"Pop","a":0,"b":0,"line":5,"col":1}]}"#,
    );
}

#[test]
fn runtime_methods_and_builtins_match_the_native_vm() {
    // say([3, 1, 2].sort().join("-"))
    // say({a: 1, b: 2}.length())
    // say("hi".upper())
    // say("héllo".length())
    check_fixture(
        "methods",
        r#"{"version":2,"constants":[{"tag":"number","value":3},{"tag":"number","value":1},{"tag":"number","value":2},{"tag":"string","value":"-"},{"tag":"string","value":"join"},{"tag":"string","value":"a"},{"tag":"number","value":1},{"tag":"string","value":"b"},{"tag":"number","value":2},{"tag":"string","value":"length"},{"tag":"string","value":"hi"},{"tag":"string","value":"héllo"},{"tag":"string","value":"upper"},{"tag":"string","value":"sort"}],"functions":[],"code":[{"op":"LoadConst","a":0,"b":0,"line":1,"col":5},{"op":"LoadConst","a":1,"b":0,"line":1,"col":8},{"op":"LoadConst","a":2,"b":0,"line":1,"col":11},{"op":"MakeArray","a":3,"b":0,"line":1,"col":4},{"op":"MethodCall","a":13,"b":0,"line":1,"col":16},{"op":"LoadConst","a":3,"b":0,"line":1,"col":25},{"op":"MethodCall","a":4,"b":1,"line":1,"col":21},{"op":"Say","a":1,"b":0,"line":1,"col":1},{"op":"Pop","a":0,"b":0,"line":1,"col":1},{"op":"LoadConst","a":5,"b":0,"line":2,"col":6},{"op":"LoadConst","a":6,"b":0,"line":2,"col":10},{"op":"LoadConst","a":7,"b":0,"line":2,"col":14},{"op":"LoadConst","a":8,"b":0,"line":2,"col":18},{"op":"MakeObject","a":2,"b":0,"line":2,"col":5},{"op":"MethodCall","a":9,"b":0,"line":2,"col":23},{"op":"Say","a":1,"b":0,"line":2,"col":1},{"op":"Pop","a":0,"b":0,"line":2,"col":1},{"op":"LoadConst","a":10,"b":0,"line":3,"col":6},{"op":"MethodCall","a":12,"b":0,"line":3,"col":8},{"op":"Say","a":1,"b":0,"line":3,"col":1},{"op":"Pop","a":0,"b":0,"line":3,"col":1},{"op":"LoadConst","a":11,"b":0,"line":4,"col":6},{"op":"MethodCall","a":9,"b":0,"line":4,"col":13},{"op":"Say","a":1,"b":0,"line":4,"col":1},{"op":"Pop","a":0,"b":0,"line":4,"col":1}]}"#,
    );
}

#[test]
fn multi_argument_say_matches_the_native_vm() {
    // say("a", 2, true, null)
    check_fixture(
        "multi_say",
        r#"{"version":2,"constants":[{"tag":"string","value":"a"},{"tag":"number","value":2},{"tag":"bool","value":true},{"tag":"null"}],"functions":[],"code":[{"op":"LoadConst","a":0,"b":0,"line":1,"col":5},{"op":"LoadConst","a":1,"b":0,"line":1,"col":9},{"op":"LoadConst","a":2,"b":0,"line":1,"col":12},{"op":"LoadConst","a":3,"b":0,"line":1,"col":17},{"op":"Say","a":4,"b":0,"line":1,"col":1}]}"#,
    );
}

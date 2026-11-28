//! CLI coverage for the `emb` module.
//!
//! This test can only pass after the parent registers `emb` in
//! `src/modules/mod.rs` (`STD_MODULES` and `load_module`) and in the CLI module
//! catalogue. It compiles in this branch but is expected to fail until that
//! integration step lands.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_emb_cli_env_{}", std::process::id()));
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

#[test]
fn emb_is_importable_and_writes_loadable_bytecode() {
    let dir = std::env::temp_dir().join(format!("em_emb_module_cli_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let program = dir.join("program.enm");
    let image = dir.join("out.emb");
    let _ = fs::remove_file(&image);

    let source = r#"
import emb

ir = {
  version: 2,
  constants: [
    {tag: "number", value: 1},
    {tag: "string", value: "hi"},
    {tag: "bool", value: true},
    {tag: "null"}
  ],
  functions: [
    {name: "f", params: ["a"], chunk: {constants: [], functions: [], code: []}}
  ],
  code: [{op: "LoadConst", a: 0, b: 0, line: 1, col: 1}]
}

say(emb.magic())
say(emb.version())
say(emb.encodeOp("LoadConst"))
say(emb.encodeOp("JumpIfNullPeek"))
say(len(emb.encode(ir)))
say(emb.write("IMAGE_PATH", ir))
"#
    .replace("IMAGE_PATH", &image.display().to_string());
    fs::write(&program, source).unwrap();

    // Header 6 + constant count 4 + constants 19 + function count 4 +
    // function 26 (name, param and its three empty counts) + instruction
    // count 4 + one instruction 17 = 80 bytes.
    let expected = "EMBC\n2\n0\n54\n80\ntrue";
    for extra in [&[][..], &["--interpret"][..]] {
        let output = Command::new(em())
            .arg("run")
            .arg(program.to_str().unwrap())
            .args(extra)
            .envs(isolated_env())
            .stdin(Stdio::null())
            .output()
            .expect("failed to run em");
        assert!(
            output.status.success(),
            "emb is not available from the CLI yet: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim_end(),
            expected,
            "stdout differed for args {extra:?}"
        );

        let bytes = fs::read(&image).unwrap();
        assert_eq!(&bytes[..6], b"EMBC\x02\x00");
        assert_eq!(bytes.len(), 80);
    }

    let _ = fs::remove_dir_all(&dir);
}

fn enm_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Write a one-off Englishmen program to a fresh scratch directory and run it
/// with the same isolated environment as the existing CLI test. Each call gets
/// its own directory because these tests run in parallel.
fn run_source(source: &str) -> std::process::Output {
    static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("em_emb_source_{}_{id}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let program = dir.join("program.enm");
    fs::write(&program, source).unwrap();
    let output = Command::new(em())
        .arg("run")
        .arg(program.to_str().unwrap())
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em");
    let _ = fs::remove_dir_all(&dir);
    output
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string()
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn write_bytes_preserves_all_256_byte_values_and_the_empty_array() {
    let dir = std::env::temp_dir().join(format!("em_emb_write_bytes_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let all = dir.join("all.bin");
    let empty = dir.join("empty.bin");

    let values: Vec<String> = (0..=255u8).map(|byte| byte.to_string()).collect();
    let source = format!(
        "import emb\n\
         all = [{}]\n\
         empty = []\n\
         say(emb.writeBytes(\"{}\", all))\n\
         say(emb.writeBytes(\"{}\", empty))\n",
        values.join(", "),
        enm_string(&all.display().to_string()),
        enm_string(&empty.display().to_string()),
    );
    let output = run_source(&source);
    assert!(
        output.status.success(),
        "writeBytes failed: {}",
        stderr_text(&output)
    );
    assert_eq!(stdout_text(&output), "true\ntrue");
    assert_eq!(
        fs::read(&all).unwrap(),
        (0..=255u8).collect::<Vec<u8>>(),
        "every byte value must survive unchanged"
    );
    assert_eq!(fs::read(&empty).unwrap(), [] as [u8; 0]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_bytes_output_loads_through_run_fixed() {
    let dir = std::env::temp_dir().join(format!("em_emb_write_bytes_load_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let image = dir.join("from_write_bytes.emb");
    let alias = dir.join("from_write.emb");

    let source = format!(
        r#"import emb

ir = {{
    version: 2,
    constants: [{{tag: "string", value: "hello from writeBytes"}}],
    functions: [],
    code: [
        {{op: "LoadConst", a: 0, b: 0, line: 1, col: 1}},
        {{op: "Say", a: 1, b: 0, line: 1, col: 1}}
    ]
}}

encoded = emb.encode(ir)
say(emb.writeBytes("{image}", encoded))
say(emb.write("{alias}", ir))
"#,
        image = enm_string(&image.display().to_string()),
        alias = enm_string(&alias.display().to_string()),
    );
    let written = run_source(&source);
    assert!(
        written.status.success(),
        "the writer failed: {}",
        stderr_text(&written)
    );
    assert_eq!(stdout_text(&written), "true\ntrue");

    let bytes = fs::read(&image).expect("writeBytes did not create the image");
    assert_eq!(&bytes[..6], b"EMBC\x02\x00");
    assert_eq!(
        bytes,
        fs::read(&alias).expect("emb.write did not create the image"),
        "writeBytes must be an alias for saving the encoded image"
    );

    let loaded = Command::new(em())
        .arg("run-fixed")
        .arg(image.to_str().unwrap())
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em");
    assert!(
        loaded.status.success(),
        "run-fixed rejected the image: {}",
        stderr_text(&loaded)
    );
    assert_eq!(stdout_text(&loaded), "hello from writeBytes");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn write_bytes_rejects_invalid_path_and_byte_arguments() {
    let dir = std::env::temp_dir().join(format!("em_emb_write_bytes_bad_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let target = dir.join("should_not_exist.bin");
    let path = enm_string(&target.display().to_string());

    let cases = [
        (
            "no arguments",
            "emb.writeBytes()",
            "expects argument 1 (path)",
        ),
        (
            "no buffer",
            &format!("emb.writeBytes(\"{path}\")"),
            "expects argument 2 (buffer)",
        ),
        (
            "path is not a string",
            "emb.writeBytes(1, [0])",
            "expected a string as the path for emb.writeBytes",
        ),
        (
            "unwritable path",
            "emb.writeBytes(\"/no/such/directory/out.bin\", [0])",
            "Could not write",
        ),
        (
            "buffer is not an array",
            &format!("emb.writeBytes(\"{path}\", \"abc\")"),
            "expected an array for emb.writeBytes",
        ),
        (
            "buffer item is not a number",
            &format!("emb.writeBytes(\"{path}\", [\"x\"])"),
            "expected a number at index 0 for emb.writeBytes",
        ),
        (
            "negative byte",
            &format!("emb.writeBytes(\"{path}\", [0, -1])"),
            "whole number from 0 to 255 at index 1",
        ),
        (
            "fractional byte",
            &format!("emb.writeBytes(\"{path}\", [1.5])"),
            "whole number from 0 to 255 at index 0",
        ),
        (
            "byte above 255",
            &format!("emb.writeBytes(\"{path}\", [256])"),
            "whole number from 0 to 255 at index 0",
        ),
    ];
    for (label, call, expected) in cases {
        let source = format!("import emb\n{call}\n");
        let output = run_source(&source);
        assert!(!output.status.success(), "{label} unexpectedly succeeded");
        assert!(
            stderr_text(&output).contains(expected),
            "{label}: expected {expected:?} in {}",
            stderr_text(&output)
        );
    }
    assert!(
        !target.exists(),
        "invalid bytes must be rejected before the file is touched"
    );

    let _ = fs::remove_dir_all(&dir);
}

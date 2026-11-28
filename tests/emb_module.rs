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

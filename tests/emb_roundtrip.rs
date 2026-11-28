//! End-to-end EMB round trip: an Englishmen program builds an IR, `emb.write`
//! serializes it, and the VM loads the result with `em run-fixed`.
//!
//! Both `emb` and the `run-fixed` CLI alias are parent-integration work (see
//! `docs/selfhost/EMB_AND_MANIFEST.md`). This test is written for the final
//! integrated tree and is expected to fail in this agent branch.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// An IR with every constant tag at the top level and a nested function that
/// adds two constants. The generated image must print two lines in the VM.
const GENERATOR: &str = r#"import emb
import os

out = os.env("EM_EMB_ROUNDTRIP_OUT")
ir = {
    version: emb.version(),
    constants: [
        {tag: "string", value: "EMB round-trip OK"},
        {tag: "number", value: 7},
        {tag: "bool", value: true},
        {tag: "null"}
    ],
    functions: [
        {
            name: "answer",
            params: [],
            chunk: {
                constants: [
                    {tag: "number", value: 40},
                    {tag: "number", value: 2}
                ],
                functions: [],
                code: [
                    {op: "LoadConst", a: 0},
                    {op: "LoadConst", a: 1},
                    {op: "Add"},
                    {op: "Return"}
                ]
            }
        }
    ],
    code: [
        {op: "LoadConst", a: 0},
        {op: "Say", a: 1},
        {op: "MakeClosure", a: 0},
        {op: "Call", a: 0, b: 0},
        {op: "Say", a: 1},
        {op: "ReturnNull"}
    ]
}
assert(emb.write(out, ir), "emb.write failed")
"#;

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_emb_roundtrip_env_{}", std::process::id()));
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

fn run(args: &[&str], env_out: &Path) -> std::process::Output {
    Command::new(em())
        .args(args)
        .envs(isolated_env())
        .env("EM_EMB_ROUNDTRIP_OUT", env_out)
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em")
}

#[test]
fn englishmen_writes_an_image_the_vm_can_load() {
    let dir = std::env::temp_dir().join(format!("em_emb_roundtrip_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let generator = dir.join("generate.enm");
    let image = dir.join("generated.emb");
    fs::write(&generator, GENERATOR).unwrap();

    let writer = run(&["run", generator.to_str().unwrap()], &image);
    assert!(
        writer.status.success(),
        "the generator failed: {}",
        String::from_utf8_lossy(&writer.stderr)
    );
    assert!(
        image.exists(),
        "emb.write did not create {}",
        image.display()
    );
    let bytes = fs::read(&image).unwrap();
    assert_eq!(&bytes[..4], b"EMBC", "wrong magic in the written image");
    assert_eq!(
        &bytes[4..6],
        &2u16.to_le_bytes(),
        "wrong version in the written image"
    );

    // `run-fixed` goes straight to bytecode::deserialize and the VM; no source
    // file is available, so the recorded spans must be enough on their own.
    let loaded = run(&["run-fixed", image.to_str().unwrap()], &image);
    assert!(
        loaded.status.success(),
        "the VM rejected the generated image: {}",
        String::from_utf8_lossy(&loaded.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&loaded.stdout),
        "EMB round-trip OK\n42\n"
    );

    let _ = fs::remove_dir_all(&dir);
}

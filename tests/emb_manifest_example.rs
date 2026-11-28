//! Runs the manifest/EMB example on both language backends.
//!
//! The example needs the `emb` and `project` modules registered by the parent
//! integrator; until then these tests fail at import time by design.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn em() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_emb_manifest_env_{}", std::process::id()));
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

fn run(example: &Path, out: &Path, interpret: bool) -> std::process::Output {
    let mut command = Command::new(em());
    command
        .arg("run")
        .arg(example)
        .envs(isolated_env())
        .env("EM_EMB_OUT", out)
        .stdin(Stdio::null());
    if interpret {
        command.arg("--interpret");
    }
    command.output().expect("failed to run em")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("em_emb_manifest_{tag}_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn manifest_example_agrees_across_backends_and_writes_a_valid_image() {
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("emb_manifest.enm");
    let dir = scratch("example");
    let vm_out = dir.join("vm.emb");
    let interpreted_out = dir.join("interpreted.emb");

    let vm = run(&example, &vm_out, false);
    let interpreted = run(&example, &interpreted_out, true);

    assert!(
        vm.status.success(),
        "VM backend failed: {}",
        String::from_utf8_lossy(&vm.stderr)
    );
    assert!(
        interpreted.status.success(),
        "interpreter backend failed: {}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    assert_eq!(
        vm.stdout, interpreted.stdout,
        "backends disagree on the manifest example"
    );

    let stdout = String::from_utf8_lossy(&vm.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines.len(), 2, "expected name and byte length: {stdout}");
    assert_eq!(lines[0], "ManifestDemo", "{stdout}");
    let printed_len: u64 = lines[1]
        .parse()
        .unwrap_or_else(|_| panic!("second line should be a byte length: {stdout}"));
    assert!(printed_len > 0, "the example wrote an empty file");

    for out in [&vm_out, &interpreted_out] {
        let bytes = fs::read(out).unwrap_or_else(|e| panic!("missing image {out:?}: {e}"));
        assert_eq!(bytes.len() as u64, printed_len, "{out:?} length mismatch");
        assert!(bytes.len() >= 6, "{out:?} is too short to hold a header");
        assert_eq!(&bytes[..4], b"EMBC", "{out:?} has the wrong magic");
        assert_eq!(
            &bytes[4..6],
            &2u16.to_le_bytes(),
            "{out:?} has the wrong version"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}

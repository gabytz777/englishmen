//! CLI-level coverage for the EMB IR described in `docs/selfhost/EMB_IR.md`.
//!
//! These tests drive `em` as a black box: a temporary `.enm` program builds an
//! IR object with `emb.encode`/`emb.write`, and then `em run-fixed` loads the
//! emitted image and executes it on the VM.
//!
//! TODO(parent integration): the `emb` module lives on branch `sh-emb-impl` and
//! is not present in this branch, and `em run-fixed` is not wired in `src/cli.rs`
//! yet. The parent must (a) register `emb` in `STD_MODULES`/`load_module` and
//! (b) add a `run-fixed <file.emb>` subcommand that routes to the existing
//! `cmd_run_bytecode` path (`bytecode::deserialize` + `Vm::run`). Until then
//! these tests fail at the emitter or `run-fixed` step by design. They are
//! written for the integrated tree and must not be skipped or weakened.
//!
//! The sibling test `tests/emb_roundtrip.rs` covers the Rust-side oracle
//! (`bytecode::deserialize(&emb.encode(ir))`); this file deliberately stays at
//! the CLI boundary.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

/// Keep `em` away from any real install and off the update network, matching
/// the setup used by `tests/cli.rs`.
fn isolated_env() -> [(&'static str, String); 3] {
    let dir = std::env::temp_dir().join(format!("em_emb_ir_env_{}", std::process::id()));
    [
        ("EM_HOME", dir.join("home").display().to_string()),
        (
            "EM_PROJECTS_DIR",
            dir.join("projects_unused").display().to_string(),
        ),
        ("EM_NO_UPDATE_CHECK", "1".to_string()),
    ]
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .envs(isolated_env())
        .stdin(Stdio::null())
        .output()
        .expect("failed to run em")
}

/// `run-fixed` executes a compiled image without recompiling source. Parent
/// integration wires it to `bytecode::deserialize` + `Vm::run`, which is the
/// same path `em <file.emb>` already uses.
fn run_fixed(path: &Path) -> std::process::Output {
    run(&["run-fixed", path.to_str().expect("utf-8 path")])
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("em_emb_ir_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Escape a host path for embedding in an Englishmen string literal.
fn enm_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// A self-hosted emitter for the smallest useful image: push a string with
/// `LoadConst`, then print it with `Say`. The expected canonical bytes and
/// helper values are explained in `docs/selfhost/EMB_IR.md`.
fn emitter_source(image: &Path) -> String {
    format!(
        r#"$-\ Build a canonical LoadConst + Say image. -$
import emb

ir = {{
    version: 2,
    constants: [
        {{tag: "string", value: "hello from emb"}}
    ],
    functions: [],
    code: [
        {{op: "LoadConst", a: 0, b: 0, line: 1, col: 1}},
        {{op: "Say", a: 1, b: 0, line: 1, col: 1}}
    ]
}}

encoded = emb.encode(ir)
emb.write("{image}", ir)

say(encoded.length)
say(emb.magic())
say(emb.version())
say(emb.encodeOp("LoadConst"))
say(emb.encodeOp("Say"))
"#,
        image = enm_string(image.to_str().expect("utf-8 path")),
    )
}

/// Write the emitter program into a fresh scratch dir and run it. Returns the
/// scratch dir (caller removes it), the image path, and the emitter's output.
fn write_image(tag: &str, flags: &[&str]) -> (PathBuf, PathBuf, std::process::Output) {
    let dir = scratch(tag);
    let source = dir.join("build_image.enm");
    let image = dir.join("hello.emb");
    std::fs::write(&source, emitter_source(&image)).unwrap();

    let mut args = vec!["run"];
    args.extend_from_slice(flags);
    args.push(source.to_str().unwrap());
    let output = run(&args);
    (dir, image, output)
}

const EMITTER_STDOUT: &str = "71\nEMBC\n2\n0\n16";
const IMAGE_SIZE: u64 = 71;

#[test]
fn emb_write_then_run_fixed_executes_loadconst_and_say() {
    let (dir, image, emitted) = write_image("roundtrip", &[]);
    assert!(
        emitted.status.success(),
        "emitter failed: {}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&emitted.stdout).trim(),
        EMITTER_STDOUT
    );

    // The file on disk is the canonical bytecode::serialize image: "EMBC",
    // version 2 little-endian, exactly 71 bytes (see the binary layout in the
    // spec).
    let bytes = std::fs::read(&image).expect("emb.write did not create the image");
    assert_eq!(bytes.len() as u64, IMAGE_SIZE);
    assert_eq!(&bytes[..4], b"EMBC");
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 2);

    // The oracle end to end: deserialize + VM execution must accept the bytes
    // a self-hosted program wrote.
    let executed = run_fixed(&image);
    assert!(
        executed.status.success(),
        "run-fixed rejected a self-hosted image: {}",
        String::from_utf8_lossy(&executed.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&executed.stdout).trim(),
        "hello from emb"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn emitter_and_image_are_backend_independent() {
    let (vm_dir, vm_image, vm) = write_image("vm", &[]);
    let (interp_dir, interp_image, interp) = write_image("interpret", &["--interpret"]);

    assert!(
        vm.status.success(),
        "compiler backend failed: {}",
        String::from_utf8_lossy(&vm.stderr)
    );
    assert!(
        interp.status.success(),
        "interpreter backend failed: {}",
        String::from_utf8_lossy(&interp.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&vm.stdout),
        String::from_utf8_lossy(&interp.stdout)
    );
    assert_eq!(
        std::fs::read(&vm_image).expect("compiler image"),
        std::fs::read(&interp_image).expect("interpreter image"),
        "both backends must emit the same canonical bytes"
    );

    for executed in [run_fixed(&vm_image), run_fixed(&interp_image)] {
        assert!(executed.status.success());
        assert_eq!(
            String::from_utf8_lossy(&executed.stdout).trim(),
            "hello from emb"
        );
    }

    let _ = std::fs::remove_dir_all(&vm_dir);
    let _ = std::fs::remove_dir_all(&interp_dir);
}

#[test]
fn run_fixed_rejects_an_unsupported_version() {
    let (dir, image, emitted) = write_image("bad_version", &[]);
    assert!(
        emitted.status.success(),
        "emitter failed: {}",
        String::from_utf8_lossy(&emitted.stderr)
    );

    let mut bytes = std::fs::read(&image).unwrap();
    bytes[4..6].copy_from_slice(&3u16.to_le_bytes());
    let bad = dir.join("bad_version.emb");
    std::fs::write(&bad, &bytes).unwrap();

    let output = run_fixed(&bad);
    assert!(
        !output.status.success(),
        "a version mismatch must be rejected by bytecode::deserialize"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("version"),
        "error should name the version mismatch: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_fixed_rejects_a_truncated_image() {
    let (dir, image, emitted) = write_image("truncated", &[]);
    assert!(
        emitted.status.success(),
        "emitter failed: {}",
        String::from_utf8_lossy(&emitted.stderr)
    );

    let bytes = std::fs::read(&image).unwrap();
    let bad = dir.join("truncated.emb");
    std::fs::write(&bad, &bytes[..10]).unwrap();

    let output = run_fixed(&bad);
    assert!(!output.status.success(), "truncated bytes must be rejected");
    assert!(
        !String::from_utf8_lossy(&output.stderr).trim().is_empty(),
        "a truncated image must produce a diagnostic"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn encode_op_tags_match_the_bytecode_table() {
    let (dir, _, emitted) = write_image("op_tags", &[]);
    assert!(
        emitted.status.success(),
        "emitter failed: {}",
        String::from_utf8_lossy(&emitted.stderr)
    );
    // The emitter prints encodeOp("LoadConst") then encodeOp("Say"); both
    // values are part of the canonical table in EMB_IR.md.
    let stdout = String::from_utf8_lossy(&emitted.stdout);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[3], "0", "LoadConst tag");
    assert_eq!(lines[4], "16", "Say tag");
    let _ = std::fs::remove_dir_all(&dir);
}

//! CLI coverage for the `bytes` standard module.
//!
//! These tests intentionally run a real `.enm` program on both backends. They
//! can only pass once the parent has registered `bytes` in
//! `src/modules/mod.rs` (both `STD_MODULES` and `load_module`).

use std::process::{Command, Stdio};

fn bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_em"))
}

const PROGRAM: &str = r#"import bytes
import json
import os

target = os.env("EM_BYTES_TEST_FILE")

hello = bytes.fromString("héllo")
say(bytes.length(hello))
say(bytes.toString(hello))
say(json.encode(bytes.of(0, 127, 255)))

buf = bytes.of(10, 20, 30)
say(bytes.get(buf, -1))
bytes.set(buf, 1, 9)
say(json.encode(buf))
say(json.encode(bytes.concat(bytes.of(1), bytes.of(2, 3))))
say(json.encode(bytes.slice(bytes.of(1, 2, 3, 4), 1, 3)))
say(json.encode(bytes.copy(buf)))
say(bytes.toHex(bytes.fromHex("aa bbcc")))

say(json.encode(bytes.u8(255)))
say(json.encode(bytes.u16(4660)))
say(json.encode(bytes.u32(305419896)))
say(json.encode(bytes.u64(72623859790382848)))
say(json.encode(bytes.i16(-2)))
say(json.encode(bytes.i32(-2)))
say(json.encode(bytes.i64(-2)))
say(json.encode(bytes.f64(1.0)))

say(bytes.write(target, bytes.of(0, 65, 255)))
say(json.encode(bytes.read(target)))
"#;

const EXPECTED: &str = "6
héllo
[0,127,255]
30
[10,9,30]
[1,2,3]
[2,3]
[10,9,30]
aa bb cc
[255]
[52,18]
[120,86,52,18]
[0,7,6,5,4,3,2,1]
[254,255]
[254,255,255,255]
[254,255,255,255,255,255,255,255]
[0,0,0,0,0,0,240,63]
true
[0,65,255]";

/// Importing an unregistered module is an error, so this test is expected to
/// fail until the parent wires `bytes` into `load_module`.
#[test]
fn bytes_module_is_available_from_the_cli_on_both_backends() {
    let dir = std::env::temp_dir().join(format!("em_bytes_cli_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("bytes_module.enm");
    let binary = dir.join("bytes.bin");
    std::fs::write(&script, PROGRAM).unwrap();
    let _ = std::fs::remove_file(&binary);

    for extra in [&["--interpret"][..], &[][..]] {
        let mut args = vec!["run", script.to_str().unwrap()];
        args.extend_from_slice(extra);
        let output = Command::new(bin())
            .args(&args)
            .env("EM_BYTES_TEST_FILE", &binary)
            .env("EM_HOME", dir.join("home"))
            .env("EM_PROJECTS_DIR", dir.join("projects_unused"))
            .env("EM_NO_UPDATE_CHECK", "1")
            .stdin(Stdio::null())
            .output()
            .expect("failed to run em");
        assert!(
            output.status.success(),
            "args: {args:?}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim_end(),
            EXPECTED,
            "args: {args:?}"
        );
        assert_eq!(std::fs::read(&binary).unwrap(), vec![0, 65, 255]);
    }

    let _ = std::fs::remove_file(&binary);
    let _ = std::fs::remove_file(&script);
    let _ = std::fs::remove_dir(&dir);
}

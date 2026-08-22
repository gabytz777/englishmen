//! Standalone executables: the `em` runtime with a compiled program glued to
//! the end of it.
//!
//! A Windows `.exe` (and an ELF binary on Unix) is loaded by the OS using the
//! headers at the *front* of the file, so trailing bytes are ignored. That
//! makes it possible to produce a single self-contained program without a
//! native code generator: copy the runtime, append the bytecode, and record
//! where it starts.
//!
//! ```text
//! [ em runtime ][ bytecode ][ length: u64 ][ magic: 8 bytes ]
//!                           \___________ 16-byte trailer ___/
//! ```
//!
//! At startup `em` looks at its own file for that trailer. If it is there, the
//! embedded program runs instead of the usual command line.

use crate::bytecode::{self, Chunk};
use crate::error::EmError;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Marks a bundled executable. The trailing NUL keeps it eight bytes wide.
const BUNDLE_MAGIC: &[u8; 8] = b"EMBUNDL\0";
/// Length prefix plus magic.
const TRAILER_LEN: u64 = 16;

/// The program embedded in this executable, if it is a bundle.
///
/// Reads the 16-byte trailer only, so a plain `em` pays for one open and one
/// short read at startup and nothing more.
pub fn embedded_program() -> Option<Result<Chunk, EmError>> {
    let path = std::env::current_exe().ok()?;
    let mut file = File::open(&path).ok()?;
    let total = file.metadata().ok()?.len();
    if total < TRAILER_LEN {
        return None;
    }

    file.seek(SeekFrom::End(-(TRAILER_LEN as i64))).ok()?;
    let mut trailer = [0u8; TRAILER_LEN as usize];
    file.read_exact(&mut trailer).ok()?;
    if &trailer[8..16] != BUNDLE_MAGIC {
        return None;
    }

    // From here on the file claims to be a bundle, so problems are reported
    // rather than silently falling back to the command line. A damaged
    // trailer can claim any length, so the arithmetic is all checked.
    let payload_len = u64::from_le_bytes(trailer[0..8].try_into().expect("8 bytes"));
    let Some(start) = payload_len
        .checked_add(TRAILER_LEN)
        .and_then(|end| total.checked_sub(end))
    else {
        return Some(Err(corrupt()));
    };

    Some(read_payload(&mut file, start, payload_len).unwrap_or_else(|| Err(corrupt())))
}

fn read_payload(file: &mut File, start: u64, len: u64) -> Option<Result<Chunk, EmError>> {
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut payload = vec![0u8; usize::try_from(len).ok()?];
    file.read_exact(&mut payload).ok()?;
    Some(bytecode::deserialize(&payload))
}

fn corrupt() -> EmError {
    EmError::new("This program's embedded bytecode is damaged").with_hint(
        "The executable was modified after it was built. Rebuild it with 'em build --exe'.",
    )
}

/// Write a standalone executable: this `em` runtime plus `chunk`.
/// Returns the size of the file produced.
pub fn create(chunk: &Chunk, out: &Path) -> Result<u64, EmError> {
    let runtime = std::env::current_exe()
        .map_err(|e| EmError::new(format!("Could not locate the em runtime: {e}")))?;

    let mut bytes = std::fs::read(&runtime).map_err(|e| {
        EmError::new(format!(
            "Could not read the em runtime at '{}': {e}",
            runtime.display()
        ))
    })?;

    // Building from an already-bundled `em` would otherwise nest one payload
    // inside another, so drop any payload the runtime is carrying.
    if let Some(start) = payload_start(&bytes) {
        bytes.truncate(start);
    }

    let payload = bytecode::serialize(chunk);
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(BUNDLE_MAGIC);

    std::fs::write(out, &bytes)
        .map_err(|e| EmError::new(format!("Could not write '{}': {e}", out.display())))?;
    make_executable(out);

    Ok(bytes.len() as u64)
}

/// Where the payload begins in an in-memory image, if it is a bundle.
fn payload_start(bytes: &[u8]) -> Option<usize> {
    let total = bytes.len();
    if total < TRAILER_LEN as usize {
        return None;
    }
    let trailer = &bytes[total - TRAILER_LEN as usize..];
    if &trailer[8..16] != BUNDLE_MAGIC {
        return None;
    }
    let payload_len = u64::from_le_bytes(trailer[0..8].try_into().expect("8 bytes"));
    usize::try_from(payload_len)
        .ok()?
        .checked_add(TRAILER_LEN as usize)
        .and_then(|end| total.checked_sub(end))
}

/// The conventional name for a standalone build of `source` on this platform.
pub fn default_output_path(source: &Path) -> PathBuf {
    if cfg!(windows) {
        source.with_extension("exe")
    } else {
        source.with_extension("")
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut perms = metadata.permissions();
        perms.set_mode(perms.mode() | 0o111);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn chunk() -> Chunk {
        compile(&parse(tokenize("say(\"hi\")\n").unwrap()).unwrap()).unwrap()
    }

    /// Build the byte image a bundle would have, without touching the disk.
    fn image(runtime: &[u8], chunk: &Chunk) -> Vec<u8> {
        let payload = bytecode::serialize(chunk);
        let mut bytes = runtime.to_vec();
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(BUNDLE_MAGIC);
        bytes
    }

    #[test]
    fn finds_the_payload_after_the_runtime() {
        let runtime = vec![0x4du8; 4096];
        let bytes = image(&runtime, &chunk());
        assert_eq!(payload_start(&bytes), Some(runtime.len()));
    }

    #[test]
    fn a_plain_runtime_has_no_payload() {
        assert_eq!(payload_start(&[0x4du8; 4096]), None);
        assert_eq!(payload_start(b"tiny"), None);
    }

    #[test]
    fn rebundling_replaces_the_previous_payload() {
        let runtime = vec![0x4du8; 4096];
        let once = image(&runtime, &chunk());
        // Bundling the already-bundled image must not nest payloads.
        let mut stripped = once.clone();
        stripped.truncate(payload_start(&once).unwrap());
        let twice = image(&stripped, &chunk());
        assert_eq!(once.len(), twice.len());
        assert_eq!(payload_start(&twice), Some(runtime.len()));
    }

    #[test]
    fn a_truncated_payload_is_detected() {
        let mut bytes = image(&[0x4du8; 64], &chunk());
        let len = bytes.len();
        // Claim a payload larger than the whole file.
        bytes[len - 16..len - 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(payload_start(&bytes), None);
    }

    #[test]
    fn the_output_name_drops_the_em_extension() {
        let out = default_output_path(Path::new("some/dir/hello.em"));
        assert_eq!(out.file_stem().unwrap(), "hello");
        assert_ne!(out.extension().map(|e| e.to_str().unwrap()), Some("em"));
    }
}

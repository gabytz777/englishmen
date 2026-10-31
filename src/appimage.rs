//! Linux AppImage packaging for standalone builds.
//!
//! An AppImage is an ELF runtime with a filesystem image appended to it. The
//! runtime mounts (or extracts) that image and runs `AppRun` from the top of
//! the AppDir. Rather than reimplementing SquashFS, this module builds a
//! temporary AppDir and asks the official `appimagetool` to package it.
//!
//! `em build hello.em --exe --appimage` therefore does the usual standalone
//! build, puts the result in `usr/bin/`, adds a desktop entry and an icon, and
//! leaves a single portable `hello-x86_64.AppImage` behind.

use crate::bytecode::Chunk;
use crate::bundle;
use crate::error::EmError;
use image::{Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Environment variable that points at a specific `appimagetool` binary.
const TOOL_ENV: &str = "APPIMAGETOOL";

/// Name of the file produced when the user does not pass `-o`.
pub fn default_output_path(source: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "program".to_string());
    source.with_file_name(format!("{}-{}.AppImage", app_name(&stem), appimage_arch()))
}

/// The program name used inside the AppDir. Keep it to characters that are
/// safe in desktop entries and shell scripts.
pub fn app_name(stem: &str) -> String {
    let sanitized: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let sanitized = sanitized.trim_matches('-');
    if sanitized.is_empty() {
        "program".to_string()
    } else {
        sanitized.to_string()
    }
}

/// The architecture string AppImage tooling expects.
fn appimage_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86" => "i686",
        "arm" => "armhf",
        other => other,
    }
}

fn desktop_entry(name: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Exec={name}\n\
         Icon={name}\n\
         Terminal=true\n\
         Categories=Utility;\n\
         Comment=A program built with Englishmen\n"
    )
}

fn launcher(name: &str) -> String {
    format!(
        "#!/bin/sh\n\
         # AppRun is started by the AppImage runtime with APPDIR pointing at\n\
         # the mounted filesystem.\n\
         exec \"$APPDIR/usr/bin/{name}\" \"$@\"\n"
    )
}

/// A small indigo icon with a white "E", drawn without a font.
fn icon() -> RgbaImage {
    fn in_rect(x: u32, y: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> bool {
        (x0..x1).contains(&x) && (y0..y1).contains(&y)
    }

    RgbaImage::from_fn(256, 256, |x, y| {
        let letter = in_rect(x, y, 70, 58, 104, 198)
            || in_rect(x, y, 70, 58, 188, 90)
            || in_rect(x, y, 70, 111, 188, 143)
            || in_rect(x, y, 70, 166, 188, 198);
        if letter {
            return Rgba([255, 255, 255, 255]);
        }

        // Diagonal gradient from indigo to near-black.
        let t = (x + y) as f32 / 510.0;
        let mix = |from: f32, to: f32| (from + (to - from) * t) as u8;
        Rgba([mix(67.0, 17.0), mix(56.0, 24.0), mix(202.0, 39.0), 255])
    })
}

/// A temporary directory that cleans itself up, including on early returns.
struct Workspace(PathBuf);

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn workspace() -> Result<Workspace, EmError> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!(
        "em-appimage-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| {
        EmError::new(format!(
            "Could not create a temporary AppDir at '{}': {e}",
            dir.display()
        ))
    })?;
    Ok(Workspace(dir))
}

fn tool() -> PathBuf {
    if let Some(path) = std::env::var_os(TOOL_ENV) {
        if Path::new(&path).exists() {
            return PathBuf::from(path);
        }
    }
    // `em install --optional` places appimagetool under the install directory.
    if let Some(path) = crate::installer::optional_tool("appimagetool") {
        return path;
    }
    PathBuf::from("appimagetool")
}

fn missing_tool(e: &std::io::Error) -> EmError {
    EmError::new(format!("Could not run appimagetool: {e}")).with_hint(
        "Install appimagetool from https://github.com/AppImage/appimagetool/releases \
         or set APPIMAGETOOL to its path, then rebuild.",
    )
}

/// Package `chunk` as an AppImage at `out`.
///
/// Returns the size of the finished AppImage.
pub fn create(chunk: &Chunk, stem: &str, out: &Path) -> Result<u64, EmError> {
    let name = app_name(stem);
    let work = workspace()?;
    let appdir = work.0.join("AppDir");
    let bin_dir = appdir.join("usr").join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| {
        EmError::new(format!(
            "Could not create the AppDir at '{}': {e}",
            bin_dir.display()
        ))
    })?;

    // The standalone build is the payload the AppImage runs.
    let binary = bin_dir.join(&name);
    bundle::create(chunk, &binary)?;

    let files = [
        (appdir.join("AppRun"), launcher(&name)),
        (appdir.join(format!("{name}.desktop")), desktop_entry(&name)),
    ];
    for (path, contents) in files {
        std::fs::write(&path, contents)
            .map_err(|e| EmError::new(format!("Could not write '{}': {e}", path.display())))?;
    }
    make_executable(&appdir.join("AppRun"));

    let icon_path = appdir.join(format!("{name}.png"));
    icon()
        .save(&icon_path)
        .map_err(|e| EmError::new(format!("Could not write '{}': {e}", icon_path.display())))?;

    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| EmError::new(format!("Could not create '{}': {e}", parent.display())))?;
    }

    let mut command = Command::new(tool());
    command
        .arg("--no-appstream")
        .arg(&appdir)
        .arg(out)
        .env("ARCH", appimage_arch())
        // Run appimagetool itself without FUSE, which is often unavailable in
        // containers and CI.
        .env("APPIMAGE_EXTRACT_AND_RUN", "1");

    let output = command.output().map_err(|e| missing_tool(&e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr
            .lines()
            .filter(|line| !line.trim().is_empty())
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(EmError::new(format!(
            "appimagetool failed to build '{}'",
            out.display()
        ))
        .with_hint(detail));
    }

    make_executable(out);
    let size = std::fs::metadata(out)
        .map_err(|e| EmError::new(format!("Could not read '{}': {e}", out.display())))?
        .len();
    Ok(size)
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

    #[test]
    fn default_output_keeps_the_directory_and_adds_the_arch() {
        let out = default_output_path(Path::new("some/dir/hello.em"));
        let name = out.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("hello-"), "{name}");
        assert!(name.ends_with(".AppImage"), "{name}");
        assert_eq!(out.parent().unwrap(), Path::new("some/dir"));
    }

    #[test]
    fn names_are_safe_for_desktop_entries_and_scripts() {
        assert_eq!(app_name("my cool app!"), "my-cool-app");
        assert_eq!(app_name("hello.em"), "hello-em");
        assert_eq!(app_name("!!!"), "program");
    }

    #[test]
    fn desktop_entry_points_at_the_binary_and_icon() {
        let entry = desktop_entry("hello");
        assert!(entry.contains("Type=Application"));
        assert!(entry.contains("Exec=hello"));
        assert!(entry.contains("Icon=hello"));
    }

    #[test]
    fn launcher_uses_the_mounted_appdir() {
        let script = launcher("hello");
        assert!(script.starts_with("#!/bin/sh"));
        assert!(script.contains("\"$APPDIR/usr/bin/hello\""));
        assert!(script.contains("\"$@\""));
    }

    #[test]
    fn the_icon_is_a_valid_256_png() {
        let mut bytes = Vec::new();
        icon()
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 256);
        assert_eq!(decoded.height(), 256);
    }
}

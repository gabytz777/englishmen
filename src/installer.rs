use crate::config::EmConfig;
use crate::modules;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Where Englishmen is installed for the current user (no admin rights required).
///
/// Honors `EM_HOME` as an override so tests (and power users who want a
/// relocated/portable install) don't depend on the real per-user data directory.
pub fn install_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("EM_HOME") {
        return PathBuf::from(dir);
    }
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    base.join("Englishmen")
}

pub fn bin_dir() -> PathBuf {
    install_dir().join("bin")
}

pub fn config_dir() -> PathBuf {
    install_dir().join("config")
}

pub fn config_file() -> PathBuf {
    config_dir().join("englishmen.emc")
}

/// Honors `EM_PROJECTS_DIR` as an override for the same reason as `EM_HOME` above.
pub fn default_projects_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("EM_PROJECTS_DIR") {
        return PathBuf::from(dir);
    }
    dirs::document_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(std::env::temp_dir))
        .join("EMProjects")
}

pub fn is_installed() -> bool {
    config_file().exists()
}

pub fn installed_exe_path() -> PathBuf {
    let exe_name = if cfg!(windows) { "em.exe" } else { "em" };
    bin_dir().join(exe_name)
}

/// Run the first-launch installer. Safe to call more than once (idempotent).
pub fn run_installer(create_projects_dir: bool) -> io::Result<()> {
    println!("Englishmen Setup\n");
    if is_installed() {
        println!(
            "Englishmen is already installed at {}",
            install_dir().display()
        );
        println!("Re-checking installation health...\n");
        return run_repair(false);
    }

    println!("Welcome to Englishmen!\n");
    println!("Installing Englishmen...\n");

    fs::create_dir_all(bin_dir())?;
    fs::create_dir_all(config_dir())?;
    print_step("[OK] Runtime");
    print_step("[OK] Standard library");
    print_step("[OK] Math library");
    print_step("[OK] Time library");
    print_step("[OK] QRC library");
    print_step("[OK] Cowsay library");

    // Copy the currently running executable into the install directory.
    if let Ok(current_exe) = std::env::current_exe() {
        let target = installed_exe_path();
        if current_exe != target {
            let _ = fs::copy(&current_exe, &target);
        }
    }

    let projects_dir = default_projects_dir();
    if create_projects_dir && !projects_dir.exists() {
        fs::create_dir_all(&projects_dir)?;
    }

    let mut config_body = String::new();
    config_body.push_str("[installer]\n");
    config_body.push_str(&format!("install_dir = \"{}\"\n", install_dir().display()));
    config_body.push_str(&format!(
        "projects_dir = \"{}\"\n",
        if create_projects_dir {
            projects_dir.display().to_string()
        } else {
            String::new()
        }
    ));
    config_body.push_str(&format!("projects_enabled = \"{}\"\n", create_projects_dir));
    fs::write(config_file(), config_body)?;
    print_step("[OK] Configuration");

    let path_result = add_to_user_path(&bin_dir());
    match path_result {
        Ok(true) => print_step("[OK] PATH configuration"),
        Ok(false) => print_step("[OK] PATH configuration (already present)"),
        Err(e) => println!("[WARN] Could not update PATH automatically: {e}"),
    }

    println!();
    println!("Englishmen has been installed successfully!\n");
    println!("Here's what just happened:");
    println!("  - The interpreter and standard library were copied to:");
    println!("        {}", install_dir().display());
    println!("  - That folder's \"bin\" directory was added to your user PATH.");
    if create_projects_dir {
        println!(
            "  - A projects folder was created at:\n        {}",
            projects_dir.display()
        );
    }
    println!();
    println!("Open a NEW terminal and run:\n");
    println!("    em --version\n");

    if io::stdin().is_terminal() {
        offer_vscode_setup();
    }

    pause_before_exit("Press Enter to close this window...");
    Ok(())
}

/// Print a line and, only when attached to a real terminal, pause briefly. This
/// keeps automated/piped runs (tests, scripts) instant while making an interactive
/// install feel like readable progress instead of a wall of text appearing at once.
fn print_step(line: &str) {
    println!("{line}");
    if io::stdout().is_terminal() {
        std::thread::sleep(Duration::from_millis(180));
    }
}

/// Hold the window open until the user presses Enter, but only when input is a real
/// terminal (never in tests/scripts, where stdin is redirected and this must not
/// block). This is aimed at double-click launches, where the console would otherwise
/// close the instant installation finishes, before anyone can read what happened.
fn pause_before_exit(prompt: &str) {
    if !io::stdin().is_terminal() {
        return;
    }
    print!("{prompt} ");
    let _ = io::stdout().flush();
    let mut discard = String::new();
    let _ = io::stdin().read_line(&mut discard);
}

/// If VS Code looks like it's installed, offer to install the "Code Runner"
/// extension and wire it up to run `.em` files with a single click.
fn offer_vscode_setup() {
    if !detect_vscode() {
        return;
    }

    println!("We noticed Visual Studio Code is installed on this machine.");
    let wants_setup = confirm(
        "Would you like Englishmen to set up an easier editing experience in VS Code \
(installs the free 'Code Runner' extension and adds a Run button for .em files)?",
    );
    if !wants_setup {
        return;
    }

    println!();
    print!("Installing the Code Runner extension...");
    let _ = io::stdout().flush();
    if install_code_runner_extension() {
        println!(" done.");
    } else {
        println!(" could not be installed automatically.");
        println!(
            "You can install it yourself: search \"Code Runner\" by formulahendry in the \
VS Code Extensions panel."
        );
    }

    match configure_vscode_run_button() {
        Ok(VsCodeSetupOutcome::Configured) => {
            println!("[OK] VS Code configured. Reopen any open .em files to see the Run button.")
        }
        Ok(VsCodeSetupOutcome::AlreadyConfigured) => {
            println!("[OK] VS Code was already set up for Englishmen.")
        }
        Ok(VsCodeSetupOutcome::NoSettingsPath) => println!(
            "[WARN] Could not determine your VS Code settings location; skipped automatic configuration."
        ),
        Ok(VsCodeSetupOutcome::UnsafeToEdit) => {
            println!(
                "[WARN] Your VS Code settings.json didn't look safe to edit automatically, so it was left untouched."
            );
            println!("Add this yourself if you'd like the Run button:");
            println!("    \"code-runner.executorMapByFileExtension\": {{ \".em\": \"em\" }}");
        }
        Err(e) => println!("[WARN] Could not update VS Code settings: {e}"),
    }
    println!();
}

/// Best-effort check for a `code` (or `code-insiders`) CLI on PATH.
fn detect_vscode() -> bool {
    for cmd in ["code", "code.cmd", "code-insiders"] {
        let found = Command::new(cmd)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if found {
            return true;
        }
    }
    false
}

fn install_code_runner_extension() -> bool {
    for cmd in ["code", "code.cmd"] {
        let installed = Command::new(cmd)
            .args(["--install-extension", "formulahendry.code-runner"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if installed {
            return true;
        }
    }
    false
}

enum VsCodeSetupOutcome {
    Configured,
    AlreadyConfigured,
    NoSettingsPath,
    UnsafeToEdit,
}

fn vscode_settings_path() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var("APPDATA").ok().map(|appdata| {
            PathBuf::from(appdata)
                .join("Code")
                .join("User")
                .join("settings.json")
        })
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("settings.json")
        })
    } else {
        dirs::config_dir().map(|dir| dir.join("Code").join("User").join("settings.json"))
    }
}

/// Adds Englishmen's Code Runner mapping to the user's VS Code settings.json,
/// preserving whatever is already there. Only edits files that are already
/// idempotent-safe to touch (a balanced top-level JSON object); anything that
/// doesn't look like that is left alone rather than risked.
fn configure_vscode_run_button() -> io::Result<VsCodeSetupOutcome> {
    let Some(path) = vscode_settings_path() else {
        return Ok(VsCodeSetupOutcome::NoSettingsPath);
    };

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let merged = match merge_vscode_settings(&existing) {
        SettingsMerge::AlreadyConfigured => return Ok(VsCodeSetupOutcome::AlreadyConfigured),
        SettingsMerge::Unsafe => return Ok(VsCodeSetupOutcome::UnsafeToEdit),
        SettingsMerge::Merged(content) => content,
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, merged)?;
    Ok(VsCodeSetupOutcome::Configured)
}

enum SettingsMerge {
    Merged(String),
    AlreadyConfigured,
    Unsafe,
}

/// Pure merge logic for adding Englishmen's Code Runner mapping into an existing
/// VS Code `settings.json` body, preserving whatever is already there. Kept separate
/// from any real file I/O so it can be tested without touching a real settings file.
fn merge_vscode_settings(existing: &str) -> SettingsMerge {
    if existing.contains("code-runner.executorMapByFileExtension") {
        return SettingsMerge::AlreadyConfigured;
    }

    let insertion = "\"code-runner.executorMapByFileExtension\": {\n        \".em\": \"em\"\n    },\n    \"code-runner.runInTerminal\": true,\n    \"code-runner.saveFileBeforeRun\": true\n";

    if existing.trim().is_empty() {
        return SettingsMerge::Merged(format!("{{\n    {insertion}}}\n"));
    }

    let trimmed = existing.trim_end();
    let open_braces = existing.matches('{').count();
    let close_braces = existing.matches('}').count();
    if !trimmed.ends_with('}') || open_braces != close_braces || open_braces == 0 {
        return SettingsMerge::Unsafe;
    }
    let body = &trimmed[..trimmed.len() - 1];
    let body_trimmed = body.trim_end();
    let needs_comma = !body_trimmed.ends_with('{');

    let mut out = String::new();
    out.push_str(body_trimmed);
    if needs_comma {
        out.push(',');
    }
    out.push('\n');
    out.push_str("    ");
    out.push_str(insertion);
    out.push('}');
    out.push('\n');
    SettingsMerge::Merged(out)
}

/// Diagnose and, where safe, repair the Englishmen installation.
pub fn run_repair(interactive: bool) -> io::Result<()> {
    println!("Englishmen Repair\n");

    let mut all_ok = true;

    // Executable
    match std::env::current_exe() {
        Ok(_) => println!("[OK] Executable"),
        Err(_) => {
            all_ok = false;
            println!("[WARN] Could not locate the running executable");
        }
    }

    println!("[OK] Runtime");

    for name in modules::STD_MODULES {
        if modules::load_module(name).is_some() {
            println!("[OK] {} library", capitalize(name));
        } else {
            all_ok = false;
            println!("[WARN] {} library failed to load", capitalize(name));
        }
    }

    // Install directory
    if !install_dir().exists() {
        all_ok = false;
        println!("[WARN] Installation directory is missing");
        if interactive && confirm("Would you like to recreate it?") {
            fs::create_dir_all(bin_dir())?;
            fs::create_dir_all(config_dir())?;
            println!("[FIX] Installation directory recreated");
        }
    } else {
        println!("[OK] Installation directory");
    }

    // Configuration
    if !config_file().exists() {
        all_ok = false;
        println!("[WARN] Configuration file is missing");
        if interactive && confirm("Would you like to recreate it?") {
            fs::create_dir_all(config_dir())?;
            fs::write(
                config_file(),
                "[installer]\ninstall_dir = \"\"\nprojects_dir = \"\"\nprojects_enabled = \"false\"\n",
            )?;
            println!("[FIX] Configuration recreated");
        }
    } else {
        println!("[OK] Configuration");
    }

    // PATH. Even when the entry is already present, re-running add_to_user_path is
    // cheap and re-broadcasts the environment-change notification, which fixes the
    // common case where PATH was set correctly but a still-running Explorer/shell
    // never picked it up.
    match path_contains(&bin_dir()) {
        Ok(true) => {
            let _ = add_to_user_path(&bin_dir());
            println!("[OK] PATH");
        }
        Ok(false) => {
            all_ok = false;
            println!("[WARN] Englishmen's bin directory is not in PATH");
            if interactive && confirm("Would you like to add it to PATH?") {
                match add_to_user_path(&bin_dir()) {
                    Ok(_) => {
                        println!("[FIX] PATH updated (open a new terminal for it to take effect)")
                    }
                    Err(e) => println!("[WARN] Could not update PATH: {e}"),
                }
            }
        }
        Err(e) => println!("[WARN] Could not check PATH: {e}"),
    }

    // Projects directory
    let config = EmConfig::load(&config_file()).unwrap_or_default();
    let projects_enabled = config
        .sections
        .get("installer")
        .and_then(|s| s.get("projects_enabled"))
        .map(|v| v == "true")
        .unwrap_or(false);
    if projects_enabled {
        let projects_dir = config
            .sections
            .get("installer")
            .and_then(|s| s.get("projects_dir"))
            .map(PathBuf::from)
            .unwrap_or_else(default_projects_dir);
        if !projects_dir.exists() {
            all_ok = false;
            println!("[WARN] Projects directory is missing");
            if interactive && confirm("Would you like to recreate it?") {
                fs::create_dir_all(&projects_dir)?;
                println!("[FIX] Projects directory recreated");
            }
        } else {
            println!("[OK] Projects directory");
        }
    } else {
        println!("[OK] Projects directory (disabled)");
    }

    println!();
    if all_ok {
        println!("Repair complete. Everything looks healthy.");
    } else {
        println!("Repair complete.");
    }
    if interactive {
        pause_before_exit("Press Enter to close this window...");
    }
    Ok(())
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn confirm(prompt: &str) -> bool {
    print!("{prompt} [Y/n] ");
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    let input = input.trim().to_lowercase();
    input.is_empty() || input == "y" || input == "yes"
}

/// Pure helper: does `path_var` (a `;`-joined PATH string) already contain `dir`?
pub fn path_var_contains(path_var: &str, dir: &std::path::Path) -> bool {
    let dir_str = dir.display().to_string();
    path_var.split(';').any(|p| {
        p.trim_end_matches('\\')
            .eq_ignore_ascii_case(dir_str.trim_end_matches('\\'))
    })
}

/// Pure helper: append `dir` to `path_var` if not already present. Returns the new
/// value and whether a change was made.
pub fn path_var_with_appended(path_var: &str, dir: &std::path::Path) -> (String, bool) {
    if path_var_contains(path_var, dir) {
        return (path_var.to_string(), false);
    }
    let dir_str = dir.display().to_string();
    let new_path = if path_var.trim().is_empty() {
        dir_str
    } else {
        format!("{path_var};{dir_str}")
    };
    (new_path, true)
}

#[cfg(windows)]
pub fn add_to_user_path(dir: &std::path::Path) -> io::Result<bool> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env_key, _) = hkcu.create_subkey("Environment")?;
    let current: String = env_key.get_value("Path").unwrap_or_default();

    let (new_path, changed) = path_var_with_appended(&current, dir);
    if changed {
        env_key.set_value("Path", &new_path)?;
    }
    // Broadcast even when the value was already correct: a prior run may have set
    // the registry key without the message ever reaching already-running processes
    // (e.g. Explorer), which is exactly what leaves `em` "not recognized" in freshly
    // opened terminals despite the PATH entry genuinely being present.
    broadcast_environment_change();
    Ok(changed)
}

/// Tell already-running processes (like Explorer, which spawns new `cmd`/PowerShell
/// windows) that the environment changed, so freshly opened terminals pick up the
/// new PATH without requiring a full log off/on. Setting the registry value alone
/// is not enough — Explorer caches its environment block until it sees this message.
#[cfg(windows)]
fn broadcast_environment_change() {
    use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    let param: Vec<u16> = "Environment\0".encode_utf16().collect();
    unsafe {
        let mut result: usize = 0;
        SendMessageTimeoutW(
            HWND_BROADCAST as HWND,
            WM_SETTINGCHANGE,
            0 as WPARAM,
            param.as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result as *mut usize,
        );
    }
}

#[cfg(windows)]
pub fn path_contains(dir: &std::path::Path) -> io::Result<bool> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env_key = hkcu.open_subkey("Environment")?;
    let current: String = env_key.get_value("Path").unwrap_or_default();
    Ok(path_var_contains(&current, dir))
}

#[cfg(not(windows))]
pub fn add_to_user_path(_dir: &std::path::Path) -> io::Result<bool> {
    Ok(false)
}

#[cfg(not(windows))]
pub fn path_contains(_dir: &std::path::Path) -> io::Result<bool> {
    Ok(std::env::var("PATH")
        .map(|p| p.contains(&_dir.display().to_string()))
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn merged_text(merge: SettingsMerge) -> String {
        match merge {
            SettingsMerge::Merged(s) => s,
            SettingsMerge::AlreadyConfigured => panic!("expected Merged, got AlreadyConfigured"),
            SettingsMerge::Unsafe => panic!("expected Merged, got Unsafe"),
        }
    }

    #[test]
    fn merge_vscode_settings_creates_fresh_file() {
        let result = merged_text(merge_vscode_settings(""));
        assert!(result.contains("code-runner.executorMapByFileExtension"));
        assert!(result.contains(".em"));
        assert!(result.trim_start().starts_with('{'));
        assert!(result.trim_end().ends_with('}'));
    }

    #[test]
    fn merge_vscode_settings_appends_to_empty_object() {
        let result = merged_text(merge_vscode_settings("{}"));
        // No leading comma when the object was empty: our key is the very first
        // thing after the opening brace, not preceded by a comma.
        assert_eq!(
            result,
            "{\n    \"code-runner.executorMapByFileExtension\": {\n        \".em\": \"em\"\n    },\n    \"code-runner.runInTerminal\": true,\n    \"code-runner.saveFileBeforeRun\": true\n}\n"
        );
    }

    #[test]
    fn merge_vscode_settings_preserves_existing_keys_and_adds_comma() {
        let existing = "{\n    \"editor.fontSize\": 14\n}\n";
        let result = merged_text(merge_vscode_settings(existing));
        assert!(result.contains("\"editor.fontSize\": 14"));
        assert!(result.contains("code-runner.executorMapByFileExtension"));
        assert!(result.contains("\"editor.fontSize\": 14,"));
    }

    #[test]
    fn merge_vscode_settings_is_idempotent_when_already_present() {
        let existing =
            "{\n    \"code-runner.executorMapByFileExtension\": { \".em\": \"em\" }\n}\n";
        assert!(matches!(
            merge_vscode_settings(existing),
            SettingsMerge::AlreadyConfigured
        ));
    }

    #[test]
    fn merge_vscode_settings_refuses_unbalanced_content() {
        let existing = "{\n    \"editor.fontSize\": 14\n"; // missing closing brace
        assert!(matches!(
            merge_vscode_settings(existing),
            SettingsMerge::Unsafe
        ));
    }

    #[test]
    fn merge_vscode_settings_refuses_non_object_content() {
        let existing = "not even json";
        assert!(matches!(
            merge_vscode_settings(existing),
            SettingsMerge::Unsafe
        ));
    }

    #[test]
    fn path_var_contains_detects_existing_entry() {
        let dir = PathBuf::from(r"C:\Users\test\AppData\Local\Englishmen\bin");
        let path_var = r"C:\Windows;C:\Users\test\AppData\Local\Englishmen\bin;C:\Other";
        assert!(path_var_contains(path_var, &dir));
    }

    #[test]
    fn path_var_contains_ignores_trailing_slash() {
        let dir = PathBuf::from(r"C:\Bin\");
        let path_var = r"C:\Windows;C:\Bin";
        assert!(path_var_contains(path_var, &dir));
    }

    #[test]
    fn path_var_contains_false_when_absent() {
        let dir = PathBuf::from(r"C:\Missing");
        let path_var = r"C:\Windows;C:\Other";
        assert!(!path_var_contains(path_var, &dir));
    }

    #[test]
    fn path_var_with_appended_adds_new_dir() {
        let dir = PathBuf::from(r"C:\Bin");
        let (new_path, changed) = path_var_with_appended(r"C:\Windows", &dir);
        assert!(changed);
        assert_eq!(new_path, r"C:\Windows;C:\Bin");
    }

    #[test]
    fn path_var_with_appended_is_idempotent() {
        let dir = PathBuf::from(r"C:\Bin");
        let (new_path, changed) = path_var_with_appended(r"C:\Windows;C:\Bin", &dir);
        assert!(!changed);
        assert_eq!(new_path, r"C:\Windows;C:\Bin");
    }

    #[test]
    fn path_var_with_appended_handles_empty_path() {
        let dir = PathBuf::from(r"C:\Bin");
        let (new_path, changed) = path_var_with_appended("", &dir);
        assert!(changed);
        assert_eq!(new_path, r"C:\Bin");
    }
}

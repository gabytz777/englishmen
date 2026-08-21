use crate::config::EmConfig;
use crate::modules;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

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
    println!("[OK] Runtime");
    println!("[OK] Standard library");
    println!("[OK] Math library");
    println!("[OK] Time library");
    println!("[OK] QRC library");
    println!("[OK] Cowsay library");

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
    println!("[OK] Configuration");

    let path_result = add_to_user_path(&bin_dir());
    match path_result {
        Ok(true) => println!("[OK] PATH configuration"),
        Ok(false) => println!("[OK] PATH configuration (already present)"),
        Err(e) => println!("[WARN] Could not update PATH automatically: {e}"),
    }

    println!();
    println!("Englishmen has been installed successfully!\n");
    println!("Open a new terminal and run:\n");
    println!("    em --version\n");
    if create_projects_dir {
        println!(
            "Your projects directory is ready at:\n    {}\n",
            projects_dir.display()
        );
    }
    Ok(())
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

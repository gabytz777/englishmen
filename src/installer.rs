use crate::config::EmConfig;
use crate::modules;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
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

/// Directory for optional tools that `em install --optional` adds.
pub fn tools_dir() -> PathBuf {
    install_dir().join("tools")
}

/// An optional tool installed by Englishmen itself, if it is present.
pub fn optional_tool(name: &str) -> Option<PathBuf> {
    let path = tools_dir().join(name);
    path.is_file().then_some(path)
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

/// The directory that must be on `PATH` for the `em` command to work.
///
/// On Windows that is the install `bin` directory itself. On Unix the runtime
/// stays in the install directory and a symlink is placed in the conventional
/// `~/.local/bin`, so upgrades replace one executable and PATH never points
/// into a version-specific folder.
pub fn user_bin_dir() -> PathBuf {
    #[cfg(not(windows))]
    if let Ok(dir) = std::env::var("EM_BIN_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        bin_dir()
    }
    #[cfg(not(windows))]
    {
        dirs::home_dir()
            .unwrap_or_else(|| install_dir())
            .join(".local")
            .join("bin")
    }
}

pub fn path_entry_dir() -> PathBuf {
    user_bin_dir()
}

/// Copy the running executable into the install directory. Re-copying an
/// identical file is harmless and keeps `em install` working as an upgrade
/// command.
fn copy_running_exe() -> io::Result<()> {
    let Ok(current_exe) = std::env::current_exe() else {
        return Ok(());
    };
    let target = installed_exe_path();
    if current_exe == target {
        return Ok(());
    }
    fs::copy(&current_exe, &target)?;
    Ok(())
}

/// An extra tool that is useful, but not required to run Englishmen.
struct OptionalPackage {
    name: &'static str,
    size: &'static str,
    purpose: &'static str,
    url: fn() -> Option<String>,
}

/// The optional packages available on this platform.
fn optional_packages() -> Vec<OptionalPackage> {
    let mut packages = Vec::new();
    if cfg!(target_os = "linux") {
        packages.push(OptionalPackage {
            name: "appimagetool",
            size: "~15 MB",
            purpose: "building portable Linux apps with 'em build --appimage'",
            url: appimagetool_url,
        });
    }
    packages
}

fn appimagetool_url() -> Option<String> {
    if let Ok(url) = std::env::var("EM_APPIMAGETOOL_URL") {
        if !url.trim().is_empty() {
            return Some(url);
        }
    }
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        "arm" => "armhf",
        "x86" => "i686",
        _ => return None,
    };
    Some(format!(
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-{arch}.AppImage"
    ))
}

fn print_optional_packages(packages: &[OptionalPackage]) {
    println!("Optional packages\n");
    for package in packages {
        println!("  {} ({})", package.name, package.size);
        println!("      Needed for: {}\n", package.purpose);
    }
}

/// Offer the optional packages during installation.
///
/// `choice` comes from `--optional` / `--no-optional`; `None` asks when a real
/// terminal is attached and skips otherwise, so scripted installs never block
/// and never download anything unexpectedly.
pub fn offer_optional_packages(choice: Option<bool>, interactive: bool) -> io::Result<()> {
    let packages = optional_packages();
    if packages.is_empty() {
        return Ok(());
    }
    // Re-running `em install` should not ask again when everything is present.
    if choice.is_none() && packages.iter().all(|package| optional_tool(package.name).is_some()) {
        return Ok(());
    }

    let install = match choice {
        Some(value) => value,
        None if interactive && io::stdin().is_terminal() => {
            print_optional_packages(&packages);
            confirm_default_no("Install optional packages?")
        }
        None => {
            let names = packages
                .iter()
                .map(|package| format!("{} ({})", package.name, package.size))
                .collect::<Vec<_>>()
                .join(", ");
            println!("Skipped optional packages: {names}.");
            println!("Run 'em install --optional' to add them later.\n");
            return Ok(());
        }
    };

    if !install {
        println!("Skipped optional packages. Run 'em install --optional' to add them later.\n");
        return Ok(());
    }

    if choice.is_some() {
        print_optional_packages(&packages);
    }
    for package in &packages {
        install_optional_package(package);
    }
    Ok(())
}

fn install_optional_package(package: &OptionalPackage) -> bool {
    let dest = tools_dir().join(package.name);
    if dest.is_file() {
        println!("[OK] {} (already installed)", package.name);
        return true;
    }

    let Some(url) = (package.url)() else {
        println!(
            "[WARN] {} is not available for this CPU architecture",
            package.name
        );
        return false;
    };

    print!("Installing {} ({})... ", package.name, package.size);
    let _ = io::stdout().flush();
    match download(&url, &dest) {
        Ok(()) => {
            make_file_executable(&dest);
            match fs::metadata(&dest) {
                Ok(metadata) => println!(
                    "done ({}).",
                    human_size(metadata.len())
                ),
                Err(_) => println!("done."),
            }
            println!();
            true
        }
        Err(e) => {
            println!("failed.");
            println!("[WARN] Could not install {}: {e}", package.name);
            println!("       Download it from https://github.com/AppImage/appimagetool/releases");
            println!("       and set APPIMAGETOOL=/path/to/appimagetool.\n");
            false
        }
    }
}

/// Download `url` to `dest` with curl or wget. Kept dependency-free on purpose:
/// both tools already exist on the systems the one-line installer supports.
fn download(url: &str, dest: &std::path::Path) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = dest.with_extension("download");
    let _ = fs::remove_file(&tmp);

    let mut last_error: Option<String> = None;
    for tool in ["curl", "wget"] {
        let mut command = Command::new(tool);
        if tool == "curl" {
            command
                .args(["-fsSL", "--retry", "2", "-o"])
                .arg(&tmp)
                .arg(url);
        } else {
            command.args(["-q", "-O"]).arg(&tmp).arg(url);
        }

        match command.stdin(Stdio::null()).status() {
            Ok(status) if status.success() => {
                fs::rename(&tmp, dest)?;
                return Ok(());
            }
            Ok(status) => last_error = Some(format!("{tool} exited with {status}")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => last_error = Some(format!("could not run {tool}: {e}")),
        }
    }

    let _ = fs::remove_file(&tmp);
    Err(io::Error::other(last_error.unwrap_or_else(|| {
        "neither curl nor wget is available".to_string()
    })))
}

fn human_size(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 0.1 {
        format!("{mb:.1} MB")
    } else {
        format!("{bytes} bytes")
    }
}

fn make_file_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(permissions.mode() | 0o111);
            let _ = fs::set_permissions(path, permissions);
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Run the first-launch installer. Safe to call more than once (idempotent).
///
/// `interactive` controls prompts and the final "press Enter" pause. Scripted
/// installs (`em install --yes`, or `install.sh`) pass `false`.
///
/// `optional` is the user's `--optional` / `--no-optional` choice. `None` asks
/// interactively and skips in scripted runs.
pub fn run_installer(
    create_projects_dir: bool,
    interactive: bool,
    optional: Option<bool>,
) -> io::Result<()> {
    println!("Englishmen Setup\n");
    if is_installed() {
        println!(
            "Englishmen is already installed at {}",
            install_dir().display()
        );
        println!("Re-checking installation health...\n");
        // Re-copy so `em install` also upgrades an existing installation, and
        // refresh the launcher/PATH even when health checks then run quietly.
        copy_running_exe()?;
        let path_dir = path_entry_dir();
        if let Err(e) = add_to_user_path(&path_dir) {
            println!("[WARN] Could not update PATH automatically: {e}");
        }
        // Only revisit optional packages when asked to (or when a person runs
        // the installer themselves); `em repair` stays a read-only check.
        if optional.is_some() || (interactive && io::stdin().is_terminal()) {
            offer_optional_packages(optional, interactive)?;
        }
        return run_repair(interactive);
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
    print_step("[OK] HTML library");

    copy_running_exe()?;

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

    let path_dir = path_entry_dir();
    match add_to_user_path(&path_dir) {
        Ok(true) => print_step("[OK] PATH configuration"),
        Ok(false) => print_step("[OK] PATH configuration (already present)"),
        Err(e) => println!("[WARN] Could not update PATH automatically: {e}"),
    }

    offer_optional_packages(optional, interactive)?;

    println!();
    println!("Englishmen has been installed successfully!\n");
    println!("Here's what just happened:");
    println!("  - The runtime (compiler, VM, and standard library) was copied to:");
    println!("        {}", install_dir().display());
    #[cfg(windows)]
    println!("  - That folder's \"bin\" directory was added to your user PATH.");
    #[cfg(not(windows))]
    {
        println!("  - The 'em' command was linked into:");
        println!("        {}", path_dir.display());
        println!("  - That folder was added to your PATH (in your shell profile).");
    }
    if create_projects_dir {
        println!(
            "  - A projects folder was created at:\n        {}",
            projects_dir.display()
        );
    }
    println!();
    println!("Open a NEW terminal and run:\n");
    println!("    em --version\n");

    if interactive && io::stdin().is_terminal() {
        offer_vscode_setup();
    }

    if interactive {
        pause_before_exit("Press Enter to close this window...");
    }
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
            println!("[OK] {} library", module_display_name(name));
        } else {
            all_ok = false;
            println!(
                "[WARN] {} library failed to load",
                module_display_name(name)
            );
        }
    }

    // Optional tools are reported, but their absence is not a problem: they are
    // only needed for specific build targets.
    for package in optional_packages() {
        match optional_tool(package.name) {
            Some(_) => println!("[OK] {} (optional)", package.name),
            None => println!(
                "[--] {} (optional, not installed; 'em install --optional' adds it)",
                package.name
            ),
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
    // never picked it up. On Unix it also recreates a missing ~/.local/bin/em link.
    let path_dir = path_entry_dir();
    match path_is_configured(&path_dir) {
        Ok(true) => {
            if let Err(e) = add_to_user_path(&path_dir) {
                println!("[WARN] Could not refresh the 'em' launcher: {e}");
            }
            println!("[OK] PATH");
        }
        Ok(false) => {
            all_ok = false;
            println!("[WARN] Englishmen's bin directory is not in PATH");
            if interactive && confirm("Would you like to add it to PATH?") {
                match add_to_user_path(&path_dir) {
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

/// Remove Englishmen from this machine.
///
/// Projects are kept unless `purge` is true. The running binary may be inside
/// the directory being removed; that is fine on Unix, and on Windows locked
/// files are reported instead of aborting the rest of the cleanup.
pub fn run_uninstall(assume_yes: bool, purge: bool) -> io::Result<()> {
    println!("Englishmen Uninstall\n");

    let dir = install_dir();
    let launcher = path_entry_dir().join(installed_exe_name());
    let launcher_is_separate = launcher != installed_exe_path();
    let launcher_owned = launcher_is_separate && launcher.exists() && launcher_is_ours(&launcher);

    let profile = uninstall_profile_path();
    let has_block = profile
        .as_ref()
        .map(|path| {
            fs::read_to_string(path)
                .map(|text| text.contains(PATH_BLOCK_START))
                .unwrap_or(false)
        })
        .unwrap_or(false);

    let config = EmConfig::load(&config_file()).unwrap_or_default();
    let projects_dir = config
        .sections
        .get("installer")
        .and_then(|section| section.get("projects_dir"))
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(default_projects_dir);
    let projects_exist = projects_dir.exists();

    if !dir.exists() && !launcher_owned && !has_block {
        println!("Englishmen does not appear to be installed on this machine.");
        return Ok(());
    }

    println!("This will remove:");
    if dir.exists() {
        println!("  - Runtime, configuration, and tools: {}", dir.display());
    }
    if launcher_owned {
        println!("  - The 'em' launcher: {}", launcher.display());
    }
    if let Some(path) = &profile {
        if has_block {
            println!("  - The Englishmen PATH entry in: {}", path.display());
        }
    }
    if projects_exist {
        if purge {
            println!("  - Your projects folder: {}", projects_dir.display());
        } else {
            println!(
                "  - (Your projects folder is kept: {})",
                projects_dir.display()
            );
        }
    }
    #[cfg(windows)]
    println!("  - The Englishmen entry in your user PATH");
    println!();

    if !assume_yes && !confirm_default_no("Remove Englishmen?") {
        println!("Uninstall cancelled.");
        return Ok(());
    }

    // Deleting projects is destructive and off by default, so ask separately.
    let remove_projects = projects_exist
        && (purge
            || (!assume_yes
                && confirm_default_no(&format!(
                    "Also delete your projects folder at '{}'?",
                    projects_dir.display()
                ))));

    let mut warnings: Vec<String> = Vec::new();

    if let Some(path) = &profile {
        if has_block {
            match strip_profile_block(path) {
                Ok(true) => println!("[OK] Removed PATH entry from {}", path.display()),
                Ok(false) => {}
                Err(e) => warnings.push(format!("Could not update '{}': {e}", path.display())),
            }
        }
    }

    #[cfg(windows)]
    match remove_user_path_entry(&bin_dir()) {
        Ok(true) => println!("[OK] Removed the Englishmen PATH entry"),
        Ok(false) => {}
        Err(e) => warnings.push(format!("Could not update the user PATH: {e}")),
    }

    if launcher_owned {
        match fs::remove_file(&launcher) {
            Ok(()) => println!("[OK] Removed launcher {}", launcher.display()),
            Err(e) => warnings.push(format!("Could not remove '{}': {e}", launcher.display())),
        }
    }

    // Remove known contents first so an unrelated file under a custom EM_HOME
    // is never deleted. `bin` goes last because it holds the running binary.
    for (name, label) in [
        ("tools", "optional tools"),
        ("config", "configuration"),
        ("bin", "runtime"),
    ] {
        let path = dir.join(name);
        if !path.exists() {
            continue;
        }
        match fs::remove_dir_all(&path) {
            Ok(()) => println!("[OK] Removed {label} ({})", path.display()),
            Err(e) => warnings.push(format!(
                "Could not remove '{}': {e}\n      Close any running Englishmen programs and delete it manually.",
                path.display()
            )),
        }
    }

    if dir.exists() {
        match fs::remove_dir(&dir) {
            Ok(()) => {}
            Err(_) if warnings.is_empty() => {
                println!("[--] Kept {} because it contains other files", dir.display());
            }
            Err(_) => {}
        }
    }

    if remove_projects {
        if projects_dir.exists() {
            match remove_projects_dir(&projects_dir) {
                Ok(()) => println!("[OK] Removed projects folder {}", projects_dir.display()),
                Err(e) => warnings.push(format!(
                    "Could not remove projects folder '{}': {e}",
                    projects_dir.display()
                )),
            }
        }
    } else if projects_exist {
        println!("[--] Kept your projects folder at {}", projects_dir.display());
    }

    println!();
    if warnings.is_empty() {
        println!("Englishmen has been uninstalled.");
    } else {
        println!("Englishmen was mostly uninstalled:");
        for warning in &warnings {
            println!("[WARN] {warning}");
        }
    }
    #[cfg(not(windows))]
    println!("Open a new terminal so the PATH change takes effect.");
    println!();
    Ok(())
}

fn installed_exe_name() -> &'static str {
    if cfg!(windows) {
        "em.exe"
    } else {
        "em"
    }
}

/// Only remove a launcher that points into the Englishmen install directory, or
/// an old copied runtime whose bytes match the installed one. Anything else may
/// belong to the user.
fn launcher_is_ours(launcher: &Path) -> bool {
    if let Ok(target) = fs::read_link(launcher) {
        return target.starts_with(install_dir());
    }
    match (fs::read(launcher), fs::read(installed_exe_path())) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn remove_projects_dir(path: &Path) -> io::Result<()> {
    // Never delete a home directory, a filesystem root, or something shallow.
    let forbidden = [
        dirs::home_dir(),
        dirs::document_dir(),
        Some(PathBuf::from("/")),
        Some(PathBuf::from("C:\\")),
    ];
    if forbidden.iter().flatten().any(|bad| bad == path) || path.components().count() < 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to delete a path that is too broad",
        ));
    }
    fs::remove_dir_all(path)
}

#[cfg(not(windows))]
fn uninstall_profile_path() -> Option<PathBuf> {
    shell_profile_path().ok()
}

#[cfg(windows)]
fn uninstall_profile_path() -> Option<PathBuf> {
    None
}

#[cfg(not(windows))]
fn strip_profile_block(path: &Path) -> io::Result<bool> {
    let existing = fs::read_to_string(path)?;
    let (updated, changed) = strip_path_block(&existing);
    if changed {
        fs::write(path, updated)?;
    }
    Ok(changed)
}

/// Windows configures PATH through the registry, not a shell profile.
#[cfg(windows)]
fn strip_profile_block(_path: &Path) -> io::Result<bool> {
    Ok(false)
}

/// Remove the guarded PATH block from a profile body, leaving everything else
/// untouched. Returns the new text and whether the block was present.
#[cfg(not(windows))]
fn strip_path_block(text: &str) -> (String, bool) {
    let Some(start) = text.find(PATH_BLOCK_START) else {
        return (text.to_string(), false);
    };
    let Some(relative_end) = text[start..].find(PATH_BLOCK_END) else {
        return (text.to_string(), false);
    };
    let end = start + relative_end + PATH_BLOCK_END.len();

    let mut before = text[..start].to_string();
    let mut after = text[end..].to_string();
    if after.starts_with('\n') {
        after.remove(0);
    }
    // The installer separates the block from earlier content with a blank line.
    if before.ends_with("\n\n") && (after.is_empty() || after.starts_with('\n')) {
        before.pop();
    }
    (before + &after, true)
}

#[cfg(windows)]
fn remove_user_path_entry(dir: &Path) -> io::Result<bool> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (env_key, _) = hkcu.create_subkey("Environment")?;
    let current: String = env_key.get_value("Path").unwrap_or_default();
    let target = dir.display().to_string().to_lowercase();
    let parts: Vec<&str> = current
        .split(';')
        .filter(|part| {
            !part.trim().is_empty() && part.trim().to_lowercase() != target
        })
        .collect();
    let updated = parts.join(";");
    if updated == current {
        return Ok(false);
    }
    env_key.set_value("Path", &updated)?;
    broadcast_environment_change();
    Ok(true)
}

fn module_display_name(name: &str) -> String {
    if name.eq_ignore_ascii_case("html") {
        "HTML".to_string()
    } else {
        capitalize(name)
    }
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

/// Like `confirm`, but an empty answer means "no". Used for optional extras so
/// pressing Enter does not download anything by accident.
fn confirm_default_no(prompt: &str) -> bool {
    print!("{prompt} [y/N] ");
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    let input = input.trim().to_lowercase();
    input == "y" || input == "yes"
}

/// Pure helper: does `path_var` (a `;`-joined PATH string) already contain `dir`?
#[cfg(any(windows, test))]
pub fn path_var_contains(path_var: &str, dir: &std::path::Path) -> bool {
    let dir_str = dir.display().to_string();
    path_var.split(';').any(|p| {
        p.trim_end_matches('\\')
            .eq_ignore_ascii_case(dir_str.trim_end_matches('\\'))
    })
}

/// Pure helper: append `dir` to `path_var` if not already present. Returns the new
/// value and whether a change was made.
#[cfg(any(windows, test))]
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

const PATH_BLOCK_START: &str = "# >>> Englishmen PATH >>>";
const PATH_BLOCK_END: &str = "# <<< Englishmen PATH <<<";

/// Add `em` to the user's PATH without requiring admin rights.
///
/// On Unix the runtime lives in the install directory, so a symlink is placed
/// in `~/.local/bin` and a small guarded block is appended to the user's shell
/// profile when that directory is not already on PATH. Running this repeatedly
/// is safe: an existing correct link and an existing profile block are left
/// untouched.
#[cfg(not(windows))]
pub fn add_to_user_path(_dir: &Path) -> io::Result<bool> {
    let link_dir = path_entry_dir();
    fs::create_dir_all(&link_dir)?;

    let target = installed_exe_path();
    if !target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "the installed runtime was not found at '{}'",
                target.display()
            ),
        ));
    }

    let link = link_dir.join("em");
    let mut changed = false;
    // If EM_BIN_DIR *is* the install bin directory there is nothing to link;
    // creating a link would mean replacing the real binary with a self-link.
    if link != target {
        let already_linked = fs::read_link(&link)
            .map(|existing| existing == target)
            .unwrap_or(false);
        if !already_linked {
            if fs::symlink_metadata(&link).is_ok() {
                if link.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("'{}' is a directory", link.display()),
                    ));
                }
                // Replace a stale link or an old copied binary. The path is
                // owned by the Englishmen installer, so this is an upgrade.
                fs::remove_file(&link)?;
            }
            std::os::unix::fs::symlink(&target, &link)?;
            changed = true;
        }
    }

    changed |= ensure_path_in_shell_profile(&link_dir)?;
    Ok(changed)
}

/// Append a guarded PATH block to the profile of the login shell, unless one is
/// already there.
///
/// This deliberately does not skip when the directory happens to be on the
/// current PATH: `em uninstall` removes the block, and a reinstall started from
/// a terminal that still has the old PATH must restore it for future shells.
/// The block guards against duplicate entries itself, so writing it is harmless.
#[cfg(not(windows))]
fn ensure_path_in_shell_profile(dir: &Path) -> io::Result<bool> {
    let profile = shell_profile_path()?;
    let existing = fs::read_to_string(&profile).unwrap_or_default();
    if existing.contains(PATH_BLOCK_START) {
        // Already configured, perhaps for a different bin directory. Do not
        // stack another block; new shells will read the existing one.
        return Ok(false);
    }

    if let Some(parent) = profile.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }

    let dir = shell_single_quote(&dir.display().to_string());
    let block = if is_fish_shell() {
        format!(
            "{PATH_BLOCK_START}\n\
             # Added by the Englishmen installer.\n\
             fish_add_path -g {dir}\n\
             {PATH_BLOCK_END}\n"
        )
    } else {
        format!(
            "{PATH_BLOCK_START}\n\
             # Added by the Englishmen installer.\n\
             ENGLISHMEN_BIN={dir}\n\
             case \":$PATH:\" in\n\
                 *\":$ENGLISHMEN_BIN:\"*) ;;\n\
                 *) export PATH=\"$ENGLISHMEN_BIN:$PATH\" ;;\n\
             esac\n\
             unset ENGLISHMEN_BIN\n\
             {PATH_BLOCK_END}\n"
        )
    };

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&profile)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(file)?;
    }
    if !existing.is_empty() {
        writeln!(file)?;
    }
    file.write_all(block.as_bytes())?;
    Ok(true)
}

#[cfg(not(windows))]
fn shell_profile_path() -> io::Result<PathBuf> {
    if let Ok(path) = std::env::var("EM_SHELL_PROFILE") {
        return Ok(PathBuf::from(path));
    }

    let home = dirs::home_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not determine your home directory",
        )
    })?;
    let shell = std::env::var("SHELL").unwrap_or_default();
    let shell = Path::new(&shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    Ok(match shell {
        "zsh" => home.join(".zshrc"),
        "bash" => home.join(".bashrc"),
        "fish" => home.join(".config").join("fish").join("config.fish"),
        _ => home.join(".profile"),
    })
}

#[cfg(not(windows))]
fn is_fish_shell() -> bool {
    std::env::var("SHELL")
        .ok()
        .and_then(|shell| {
            Path::new(&shell)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name == "fish")
        })
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(not(windows))]
pub fn path_contains(dir: &Path) -> io::Result<bool> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    Ok(std::env::split_paths(&current).any(|entry| entry == dir))
}

/// Whether `dir` is on the current PATH or already configured in the shell
/// profile for new shells. The latter matters right after an install: the
/// running shell cannot pick up the profile change until it is restarted.
#[cfg(not(windows))]
pub fn path_is_configured(dir: &Path) -> io::Result<bool> {
    if path_contains(dir)? {
        return Ok(true);
    }
    let profile = shell_profile_path()?;
    let text = fs::read_to_string(profile).unwrap_or_default();
    Ok(text.contains(PATH_BLOCK_START) && text.contains(&dir.display().to_string()))
}

#[cfg(windows)]
pub fn path_is_configured(dir: &std::path::Path) -> io::Result<bool> {
    path_contains(dir)
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
    fn optional_packages_describe_their_size_and_purpose() {
        for package in optional_packages() {
            assert!(!package.name.is_empty());
            assert!(package.size.contains("MB"), "{}", package.size);
            assert!(!package.purpose.is_empty());
            assert!((package.url)().is_some());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimagetool_is_the_linux_optional_package() {
        let names: Vec<_> = optional_packages().iter().map(|p| p.name).collect();
        assert!(names.contains(&"appimagetool"));
    }

    #[test]
    fn human_size_switches_units_at_a_tenth_of_a_megabyte() {
        assert_eq!(human_size(512), "512 bytes");
        assert_eq!(human_size(15 * 1024 * 1024), "15.0 MB");
    }

    #[cfg(not(windows))]
    #[test]
    fn strip_path_block_removes_only_the_guarded_section() {
        let text = "export FOO=1\n\n\
# >>> Englishmen PATH >>>\n\
# Added by the Englishmen installer.\n\
export PATH=\"$HOME/.local/bin:$PATH\"\n\
# <<< Englishmen PATH <<<\n\
export BAR=2\n";
        let (updated, changed) = strip_path_block(text);
        assert!(changed);
        assert!(updated.contains("export FOO=1"));
        assert!(updated.contains("export BAR=2"));
        assert!(!updated.contains("Englishmen PATH"));
    }

    #[cfg(not(windows))]
    #[test]
    fn strip_path_block_leaves_profiles_without_a_block_alone() {
        let text = "export FOO=1\n";
        let (updated, changed) = strip_path_block(text);
        assert!(!changed);
        assert_eq!(updated, text);
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

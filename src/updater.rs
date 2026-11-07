use crate::config::EmConfig;
use crate::installer;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Seconds a manual (or automatic) release check may take before giving up.
const CHECK_TIMEOUT_SECS: u64 = 15;
/// The automatic startup check gets a tighter budget so it never delays a
/// program run by more than a few seconds once a day.
const AUTO_CHECK_TIMEOUT_SECS: u64 = 5;
/// One day, in seconds — the automatic check's cooldown.
const AUTO_CHECK_COOLDOWN_SECS: u64 = 24 * 60 * 60;

const STATE_SECTION: &str = "updates";

fn state_file() -> PathBuf {
    installer::config_dir().join("updates.emc")
}

#[derive(Debug, Default, Clone)]
pub struct UpdateState {
    /// User preference: `Some("off")` silences the automatic startup notice.
    /// Manual checks and installs never change it.
    pub notifications: Option<String>,
    /// Unix timestamp (seconds) of the last successful check.
    pub last_check: Option<u64>,
    /// Latest version seen on GitHub, e.g. `v1.6.0`.
    pub latest_version: Option<String>,
}

fn load_state() -> UpdateState {
    let mut state = UpdateState::default();
    if let Ok(config) = EmConfig::load(&state_file()) {
        if let Some(section) = config.sections.get(STATE_SECTION) {
            state.notifications = section.get("notifications").cloned();
            state.last_check = section.get("last_check").and_then(|v| v.parse().ok());
            state.latest_version = section.get("latest_version").cloned();
        }
    }
    state
}

fn save_state(state: &UpdateState) -> io::Result<()> {
    fs::create_dir_all(installer::config_dir())?;
    let mut body = String::from("[updates]\n");
    body.push_str(&format!(
        "notifications = \"{}\"\n",
        state.notifications.as_deref().unwrap_or("on")
    ));
    if let Some(last) = state.last_check {
        body.push_str(&format!("last_check = \"{last}\"\n"));
    }
    if let Some(version) = &state.latest_version {
        body.push_str(&format!("latest_version = \"{version}\"\n"));
    }
    fs::write(state_file(), body)
}

/// Record a successful check without touching the notification preference.
fn record_check(latest_version: &str) {
    let mut state = load_state();
    state.last_check = Some(now_unix());
    state.latest_version = Some(latest_version.to_string());
    let _ = save_state(&state);
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn releases_api_url() -> String {
    if let Ok(url) = std::env::var("EM_RELEASES_API_URL") {
        if !url.trim().is_empty() {
            return url;
        }
    }
    "https://api.github.com/repos/gabytz777/englishmen/releases/latest".to_string()
}

/// Download the release API response with curl or wget and pull out the tag.
fn fetch_latest_version(timeout_secs: u64) -> Result<String, String> {
    let url = releases_api_url();
    let tmp = std::env::temp_dir().join(format!("em-release-check-{}.json", std::process::id()));
    let _ = fs::remove_file(&tmp);

    let mut last_error: Option<String> = None;
    for tool in ["curl", "wget"] {
        let mut command = Command::new(tool);
        if tool == "curl" {
            command
                .args(["-fsSL", "--retry", "2", "--max-time"])
                .arg(timeout_secs.to_string())
                .args([
                    "-H",
                    "User-Agent: englishmen-updater",
                    "-H",
                    "Accept: application/vnd.github+json",
                ])
                .arg("-o")
                .arg(&tmp)
                .arg(&url);
        } else {
            command
                .args(["-q", "--timeout"])
                .arg(timeout_secs.to_string())
                .args(["--tries", "2", "-U", "englishmen-updater", "-O"])
                .arg(&tmp)
                .arg(&url);
        }

        match command.stdin(Stdio::null()).output() {
            Ok(output) if output.status.success() => {
                let tag = fs::read_to_string(&tmp)
                    .ok()
                    .and_then(|text| parse_tag_name(&text));
                let _ = fs::remove_file(&tmp);
                return tag.ok_or_else(|| "release response had no tag_name".to_string());
            }
            Ok(output) => {
                let _ = fs::remove_file(&tmp);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let detail = stderr.lines().next().unwrap_or_default();
                last_error = Some(if detail.is_empty() {
                    format!("{tool} exited with {}", output.status)
                } else {
                    format!("{tool}: {detail}")
                });
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => last_error = Some(format!("could not run {tool}: {e}")),
        }
    }

    Err(last_error.unwrap_or_else(|| "neither curl nor wget is available".to_string()))
}

/// Pull the `"tag_name"` string out of a GitHub release API response without a
/// JSON library — the field always sits at the top level of the object.
fn parse_tag_name(json: &str) -> Option<String> {
    let key = "\"tag_name\"";
    let rest = &json[json.find(key)? + key.len()..];
    let rest = &rest[rest.find(':')? + 1..];
    let rest = rest.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    let tag = &rest[..end];
    (!tag.is_empty()).then(|| tag.to_string())
}

/// One component of a version string. Numeric components compare by value so
/// `1.10.0` sorts after `1.9.0`; a `1-beta` style component is that number as
/// a pre-release. Anything else keeps its raw text.
#[derive(Debug, PartialEq, Eq)]
enum VersionPart {
    Number { value: u64, prerelease: bool },
    Text(String),
}

impl VersionPart {
    fn number(&self) -> u64 {
        match self {
            VersionPart::Number { value, .. } => *value,
            VersionPart::Text(_) => 0,
        }
    }

    fn is_prerelease(&self) -> bool {
        matches!(self, VersionPart::Number { prerelease: true, .. })
    }
}

fn version_components(version: &str) -> Vec<VersionPart> {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|part| {
            let (core, prerelease) = match part.split_once('-') {
                Some((core, _suffix)) => (core, true),
                None => (part, false),
            };
            match core.parse::<u64>() {
                Ok(value) => VersionPart::Number { value, prerelease },
                Err(_) => VersionPart::Text(part.to_string()),
            }
        })
        .collect()
}

/// True when `latest` is strictly newer than the running version.
fn is_newer(latest: &str, current: &str) -> bool {
    let latest = version_components(latest);
    let current = version_components(current);
    // Compare the release cores first, ignoring pre-release markers, so
    // 1.5.1-beta counts as 1.5.1: newer than 1.5.0 but older than 1.5.1.
    for i in 0..latest.len().max(current.len()) {
        match (latest.get(i), current.get(i)) {
            (Some(a), Some(b)) => {
                if let (VersionPart::Text(a), VersionPart::Text(b)) = (a, b) {
                    if a != b {
                        return a > b;
                    }
                } else {
                    let (a, b) = (a.number(), b.number());
                    if a != b {
                        return a > b;
                    }
                }
            }
            (Some(_), None) => return true,
            (None, Some(_)) => return false,
            (None, None) => break,
        }
    }
    // Equal release cores: the final release beats any pre-release of it.
    !latest.iter().any(VersionPart::is_prerelease)
        && current.iter().any(VersionPart::is_prerelease)
}

/// Display a version with the leading `v` the release tags use.
fn display_version(version: &str) -> String {
    if version.starts_with('v') || version.starts_with('V') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

enum CheckOutcome {
    UpToDate,
    UpdateAvailable(String),
}

/// Fetch the latest release and print the standard status block. Always hits
/// the network — the cooldown only applies to the automatic startup check —
/// and refreshes the cached version on success.
fn perform_check() -> Result<CheckOutcome, ()> {
    println!("Checking for Englishmen updates...");
    let latest = match fetch_latest_version(CHECK_TIMEOUT_SECS) {
        Ok(tag) => tag,
        Err(_) => {
            eprintln!();
            eprintln!("Unable to check for updates.");
            eprintln!("Please check your internet connection and try again.");
            return Err(());
        }
    };
    record_check(&latest);
    if is_newer(&latest, VERSION) {
        Ok(CheckOutcome::UpdateAvailable(latest))
    } else {
        Ok(CheckOutcome::UpToDate)
    }
}

fn print_update_available(latest: &str) {
    println!();
    println!("Update available!");
    println!();
    println!("Installed: {}", display_version(VERSION));
    println!("Latest:    {}", display_version(latest));
    println!();
    println!("Run 'em update' to install.");
}

/// `em update --check`: report the latest version; never download or install.
pub fn run_manual_check() -> ExitCode {
    match perform_check() {
        Ok(CheckOutcome::UpToDate) => {
            println!();
            println!("You are running the latest version!");
            ExitCode::SUCCESS
        }
        Ok(CheckOutcome::UpdateAvailable(latest)) => {
            print_update_available(&latest);
            ExitCode::SUCCESS
        }
        Err(()) => ExitCode::FAILURE,
    }
}

/// `em update`: check, then download and install the newer release binary.
pub fn run_install() -> ExitCode {
    let latest = match perform_check() {
        Ok(CheckOutcome::UpToDate) => {
            println!();
            println!("You are running the latest version!");
            return ExitCode::SUCCESS;
        }
        Ok(CheckOutcome::UpdateAvailable(latest)) => latest,
        Err(()) => return ExitCode::FAILURE,
    };

    if !installer::is_installed() {
        eprintln!("Error: Englishmen is not installed. Run 'em install' first.");
        return ExitCode::FAILURE;
    }

    let asset = match release_asset_name() {
        Some(asset) => asset,
        None => {
            eprintln!(
                "Error: No prebuilt Englishmen binary is available for {} ({}).",
                std::env::consts::OS,
                std::env::consts::ARCH
            );
            return ExitCode::FAILURE;
        }
    };

    println!("Downloading {}...", display_version(&latest));
    let downloaded = std::env::temp_dir().join(format!(
        "em-update-{}{}",
        std::process::id(),
        std::env::consts::EXE_SUFFIX
    ));
    let _ = fs::remove_file(&downloaded);
    if let Err(e) = installer::download(&asset_download_url(asset), &downloaded) {
        eprintln!("Error: Could not download the update: {e}");
        let _ = fs::remove_file(&downloaded);
        return ExitCode::FAILURE;
    }

    let target = installer::installed_exe_path();
    if let Err(e) = replace_installed_binary(&downloaded, &target) {
        eprintln!("Error: Could not replace '{}': {e}", target.display());
        let _ = fs::remove_file(&downloaded);
        return ExitCode::FAILURE;
    }
    let _ = fs::remove_file(&downloaded);

    println!("[OK] Updated to {}!", display_version(&latest));
    ExitCode::SUCCESS
}

/// Asset names produced by .github/workflows/release.yml.
fn release_asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("em-linux-x86_64"),
        ("windows", "x86_64") => Some("em-windows-x86_64.exe"),
        _ => None,
    }
}

fn asset_download_url(asset: &str) -> String {
    if let Ok(url) = std::env::var("EM_RELEASE_ASSET_URL") {
        if !url.trim().is_empty() {
            return url;
        }
    }
    format!(
        "https://github.com/gabytz777/englishmen/releases/latest/download/{asset}"
    )
}

/// Move the freshly downloaded binary over the installed one. On Windows a
/// running executable cannot be overwritten, but it can be renamed out of the
/// way first; the leftover is removed on a later update.
fn replace_installed_binary(new_binary: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let old = target.with_file_name(format!(
            "{}.old",
            target
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("em")
        ));
        let _ = fs::remove_file(&old);
        if target.exists() {
            fs::rename(target, &old)?;
        }
    }
    fs::rename(new_binary, target)?;
    installer::make_file_executable(target);
    Ok(())
}

/// `em update --notifications on|off`
pub fn set_notifications(enabled: bool) -> ExitCode {
    let mut state = load_state();
    state.notifications = Some(if enabled { "on" } else { "off" }.to_string());
    match save_state(&state) {
        Ok(()) => {
            println!(
                "[OK] Automatic update notifications {}.",
                if enabled { "enabled" } else { "disabled" }
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: Could not save the notification preference: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The automatic check behind normal command runs: at most once every 24
/// hours, silent when the user turned notifications off, silent on failure,
/// and never installing anything.
///
/// Runs only when stdout is a terminal so scripted and CI output stays clean;
/// tests (and curious users) can force it on with EM_UPDATE_CHECK_FORCE=1.
pub fn auto_check() {
    if std::env::var("EM_NO_UPDATE_CHECK").as_deref() == Ok("1") {
        return;
    }
    if std::env::var("EM_UPDATE_CHECK_FORCE").as_deref() != Ok("1")
        && !io::stdout().is_terminal()
    {
        return;
    }
    if !installer::is_installed() {
        return;
    }
    let state = load_state();
    if state.notifications.as_deref() == Some("off") {
        return;
    }
    if let Some(last) = state.last_check {
        if now_unix().saturating_sub(last) < AUTO_CHECK_COOLDOWN_SECS {
            return;
        }
    }

    let Ok(latest) = fetch_latest_version(AUTO_CHECK_TIMEOUT_SECS) else {
        return;
    };
    record_check(&latest);
    if is_newer(&latest, VERSION) {
        println!(
            "Update available: {} (you have {}). Run 'em update' to install.",
            display_version(&latest),
            display_version(VERSION)
        );
        println!();
        let _ = io::stdout().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tag_name_reads_the_release_field() {
        let json = r#"{"url": "https://api.github.com/x", "tag_name": "v1.6.0", "name": "1.6.0"}"#;
        assert_eq!(parse_tag_name(json), Some("v1.6.0".to_string()));
        assert_eq!(parse_tag_name("{\"nope\": 1}"), None);
        assert_eq!(parse_tag_name("{\"tag_name\": \"\"}"), None);
    }

    #[test]
    fn version_comparison_is_numeric() {
        assert!(is_newer("v1.3.0", "1.2.0"));
        assert!(is_newer("1.10.0", "v1.9.9"));
        assert!(is_newer("v2.0", "1.9.9"));
        assert!(!is_newer("v1.2.0", "1.2.0"));
        assert!(!is_newer("v1.1.0", "1.2.0"));
    }

    #[test]
    fn a_prerelease_is_older_than_its_release() {
        assert!(!is_newer("v1.5.1-beta", "1.5.1"));
        assert!(is_newer("v1.5.1-beta", "1.5.0"));
        assert!(is_newer("v2.0.0-beta", "1.9.9"));
    }

    #[test]
    fn display_version_adds_the_v_prefix() {
        assert_eq!(display_version("1.2.0"), "v1.2.0");
        assert_eq!(display_version("v1.2.0"), "v1.2.0");
    }
}

//! Auto-update, queries GitHub releases on startup, prompts the user if a
//! newer version is available, downloads the platform installer, and hands
//! off to it so the app can relaunch on the new version.
//!
//! Flow:
//!   1. `check_latest_release()`, async HTTP GET to GitHub releases/latest
//!   2. UI compares `tag_name` against `env!("CARGO_PKG_VERSION")`; if newer
//!      and not in `skipped_version`, shows a modal with 3 options:
//!        - **Skip this version** → persists tag into vault `settings` table
//!        - **Remind me later** → dismisses, asks next launch
//!        - **Update now** → triggers `download_installer` + `launch_installer_and_exit`
//!   3. During download, the UI shows a progress bar via streaming bytes.

use std::path::PathBuf;

/// Hard-coded release repo, kept in one place so publishing the app to a
/// fork or mirror requires a single edit.
pub const RELEASE_REPO: &str = "wilsonglasser/oryxis";

/// The release stream the auto-updater follows. Persisted as the
/// `update_channel` setting (`"stable"` / `"nightly"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateChannel {
    #[default]
    Stable,
    Nightly,
}

impl UpdateChannel {
    pub fn from_setting(s: &str) -> Self {
        match s {
            "nightly" => Self::Nightly,
            _ => Self::Stable,
        }
    }

    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Nightly => "nightly",
        }
    }
}

/// Selectable channels for the settings picker, in display order.
pub const UPDATE_CHANNELS: [UpdateChannel; 2] = [UpdateChannel::Stable, UpdateChannel::Nightly];

// `pick_list` requires its option type to implement `Display` even when a
// mapper closure handles the visible label, so provide a plain fallback.
// The settings picker maps through i18n; this is only the default.
impl std::fmt::Display for UpdateChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Stable => "Stable",
            Self::Nightly => "Nightly",
        })
    }
}

/// Channel this binary was built for, baked in by `build.rs`. Stable for
/// tagged releases and local builds; nightly only for the rolling CI
/// build. Used so a user who flips back to the stable channel from a
/// nightly binary is offered a clean stable build instead of being
/// stranded (the nightly's `CARGO_PKG_VERSION` would read as "not newer").
pub fn build_channel() -> UpdateChannel {
    UpdateChannel::from_setting(env!("ORYXIS_CHANNEL"))
}

/// How an update is applied once downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateArtifact {
    /// A platform installer (NSIS / AppImage / tarball) handed off to the
    /// OS. The stable channel's mechanism.
    Installer,
    /// A bare executable that replaces the running binary in place. The
    /// nightly channel's mechanism, no installer is published for it.
    Binary,
}

/// Why an update check failed, kept separate from "no update available"
/// so the UI can report the truth instead of claiming up-to-date while
/// the network is down or firewalled (issue #38).
#[derive(Debug, Clone)]
pub enum UpdateError {
    /// DNS / connect / timeout / TLS failure, with a concise root cause.
    Network(String),
    /// Non-2xx HTTP status from the GitHub API.
    Http(u16),
    /// Payload didn't contain the expected fields.
    Parse,
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateError::Network(cause) => write!(f, "{cause}"),
            UpdateError::Http(status) => write!(f, "HTTP {status}"),
            UpdateError::Parse => write!(f, "unexpected API response"),
        }
    }
}

/// Settings > About status line for the manual update check. An enum
/// (not a pre-rendered string) so the view picks color + i18n at render
/// time and language switches don't strand a stale English string.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    Checking,
    UpToDate,
    Failed(String),
}

/// Boil a reqwest error chain down to its root cause, the part the user
/// can act on ("failed to lookup address", "connection refused", ...).
fn concise_cause(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        return "timeout".to_string();
    }
    let mut src: &dyn std::error::Error = e;
    while let Some(inner) = src.source() {
        src = inner;
    }
    src.to_string()
}

/// Release metadata extracted from the GitHub API payload.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// Version without the leading `v` (e.g. `0.3.2`), or `nightly
    /// (<sha>)` for the nightly channel.
    pub version: String,
    /// HTML page for the release (for "What's new").
    pub html_url: String,
    /// Release notes body (markdown), preview shown in the modal.
    pub body: String,
    /// Download URL for the installer asset matching this platform.
    pub installer_url: Option<String>,
    /// Installer file name (used when saving to temp).
    pub installer_name: Option<String>,
    /// Whether to launch an installer or swap the binary in place.
    pub artifact: UpdateArtifact,
}

/// Query the GitHub API for an available update on the given channel.
/// `Ok(None)` means genuinely up to date; failures (network, HTTP,
/// parse) come back as `Err` so callers can distinguish. The silent
/// boot check logs and ignores errors; the manual check surfaces them.
pub async fn check_latest_release(
    channel: UpdateChannel,
) -> Result<Option<UpdateInfo>, UpdateError> {
    match channel {
        UpdateChannel::Stable => check_stable().await,
        UpdateChannel::Nightly => check_nightly().await,
    }
}

/// Fetch a release JSON payload from a `releases/...` API path.
async fn fetch_release(path: &str) -> Result<serde_json::Value, UpdateError> {
    let url = format!("https://api.github.com/repos/{RELEASE_REPO}/{path}");
    let client = reqwest::Client::builder()
        .user_agent(concat!("Oryxis/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .https_only(true)
        .build()
        .map_err(|e| UpdateError::Network(concise_cause(&e)))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| UpdateError::Network(concise_cause(&e)))?;
    if !resp.status().is_success() {
        return Err(UpdateError::Http(resp.status().as_u16()));
    }
    resp.json().await.map_err(|_| UpdateError::Parse)
}

/// Stable channel: the newest tagged release. Normally only offered when
/// strictly newer than the running version, but a binary built on the
/// nightly channel always gets offered the latest stable so flipping the
/// channel toggle back actually lands the user on a stable build.
async fn check_stable() -> Result<Option<UpdateInfo>, UpdateError> {
    let json = fetch_release("releases/latest").await?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or(UpdateError::Parse)?
        .trim_start_matches('v')
        .to_string();
    let running_nightly = build_channel() == UpdateChannel::Nightly;
    if !running_nightly && !is_newer(&tag, env!("CARGO_PKG_VERSION")) {
        return Ok(None);
    }
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .ok_or(UpdateError::Parse)?
        .to_string();
    let body = json.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let (installer_url, installer_name) = pick_asset(&json, UpdateChannel::Stable);
    Ok(Some(UpdateInfo {
        version: tag,
        html_url,
        body,
        installer_url,
        installer_name,
        artifact: UpdateArtifact::Installer,
    }))
}

/// Nightly channel: the rolling `nightly` prerelease. Version numbers
/// don't move between nightlies, so "newer" means a different target
/// commit than the one baked into this binary. `/releases/latest` skips
/// prereleases, hence the explicit tag lookup.
async fn check_nightly() -> Result<Option<UpdateInfo>, UpdateError> {
    let json = fetch_release("releases/tags/nightly").await?;
    let remote_sha = nightly_commit(&json).ok_or(UpdateError::Parse)?;
    let local_sha = env!("ORYXIS_GIT_SHA");
    // Dev build with no embedded SHA: can't compare, so never nag.
    if local_sha == "unknown" || commit_eq(&remote_sha, local_sha) {
        return Ok(None);
    }
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .ok_or(UpdateError::Parse)?
        .to_string();
    let body = json.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let (installer_url, installer_name) = pick_asset(&json, UpdateChannel::Nightly);
    let short: String = remote_sha.chars().take(8).collect();
    Ok(Some(UpdateInfo {
        version: format!("nightly ({short})"),
        html_url,
        body,
        installer_url,
        installer_name,
        artifact: UpdateArtifact::Binary,
    }))
}

/// Extract the commit the `nightly` release points at. The publish job
/// creates the tag with `--target <full-sha>`, so `target_commitish`
/// usually carries it; fall back to the short SHA in the release title
/// (`Nightly (abcdef12)`).
fn nightly_commit(json: &serde_json::Value) -> Option<String> {
    if let Some(tc) = json.get("target_commitish").and_then(|v| v.as_str())
        && tc.len() >= 7
        && tc.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Some(tc.to_string());
    }
    let name = json.get("name").and_then(|v| v.as_str())?;
    let start = name.find('(')? + 1;
    let end = name[start..].find(')')? + start;
    let sha = &name[start..end];
    (sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit())).then(|| sha.to_string())
}

/// Compare two commit SHAs by their common-length prefix, so a short SHA
/// (8 hex from a title) matches the full 40-hex form.
fn commit_eq(a: &str, b: &str) -> bool {
    let n = a.len().min(b.len()).min(40);
    n >= 7 && a[..n].eq_ignore_ascii_case(&b[..n])
}

/// Strict "lhs > rhs" comparison over semantic-ish versions (major.minor.patch,
/// extra segments ignored). Returns false on parse failure so we never
/// prompt for a broken tag.
fn is_newer(lhs: &str, rhs: &str) -> bool {
    fn parse(s: &str) -> [u32; 3] {
        let mut out = [0u32; 3];
        for (i, seg) in s.split('.').take(3).enumerate() {
            let num: u32 = seg
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0);
            out[i] = num;
        }
        out
    }
    parse(lhs) > parse(rhs)
}

fn pick_asset(
    json: &serde_json::Value,
    channel: UpdateChannel,
) -> (Option<String>, Option<String>) {
    let assets = match json.get("assets").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return (None, None),
    };
    let (want, exclude) = match channel {
        UpdateChannel::Stable => (platform_asset_fragment(), platform_asset_exclude()),
        UpdateChannel::Nightly => (nightly_asset_fragment(), vec![]),
    };
    for a in assets {
        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let lname = name.to_lowercase();
        if !want.iter().all(|w| lname.contains(w)) {
            continue;
        }
        if exclude.iter().any(|w| lname.contains(w)) {
            continue;
        }
        let url = a.get("browser_download_url").and_then(|v| v.as_str()).map(|s| s.to_string());
        return (url, Some(name.to_string()));
    }
    (None, None)
}

/// On Windows we ship two installers: `oryxis-setup-x86_64.exe` (system,
/// `Program Files`, requires UAC) and `oryxis-user-setup-x86_64.exe`
/// (per-user, `%LOCALAPPDATA%`, no UAC). Pick the one matching the
/// running install so the auto-update preserves scope. On other
/// platforms the function returns `false` (no per-user concept).
#[cfg(target_os = "windows")]
fn is_per_user_install() -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let local = match std::env::var_os("LOCALAPPDATA") {
        Some(v) => std::path::PathBuf::from(v),
        None => return false,
    };
    let exe_lc = exe.to_string_lossy().to_lowercase();
    let local_lc = local.to_string_lossy().to_lowercase();
    exe_lc.starts_with(&local_lc)
}

/// Substrings we expect inside the asset filename for the current
/// platform. The release pipeline emits, per architecture:
///   • Windows x64:    `oryxis-setup-x86_64.exe` (NSIS, system / UAC)
///                     `oryxis-user-setup-x86_64.exe` (NSIS, per-user)
///   • Windows arm64:  `oryxis-setup-aarch64.exe` (NSIS, system / UAC)
///                     `oryxis-user-setup-aarch64.exe` (NSIS, per-user)
///                     `oryxis-windows-aarch64.zip` (portable fallback)
///   • macOS arm64:    `oryxis-macos-aarch64.tar.gz`
///   • Linux x64:      `oryxis-linux-x86_64.AppImage`
///   • Linux arm64:    `oryxis-linux-aarch64.AppImage`
///
/// We match by the most discriminating combination per platform, so a
/// future asset rename in only one of those slots doesn't silently
/// break the rest. Returns the empty list for platforms we don't ship
/// a per-arch installer for, the caller surfaces "no installer
/// asset for this platform" so the user falls back to manual install.
fn platform_asset_fragment() -> Vec<&'static str> {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "x86_64") {
            #[cfg(target_os = "windows")]
            {
                if is_per_user_install() {
                    return vec!["user-setup", "x86_64", ".exe"];
                }
            }
            vec!["setup", "x86_64", ".exe"]
        } else if cfg!(target_arch = "aarch64") {
            #[cfg(target_os = "windows")]
            {
                if is_per_user_install() {
                    return vec!["user-setup", "aarch64", ".exe"];
                }
            }
            vec!["setup", "aarch64", ".exe"]
        } else {
            vec![]
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            vec!["macos", "aarch64", ".tar.gz"]
        } else {
            vec![]
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "x86_64") {
            vec!["linux", "x86_64", ".appimage"]
        } else if cfg!(target_arch = "aarch64") {
            vec!["linux", "aarch64", ".appimage"]
        } else {
            vec![]
        }
    } else {
        vec![]
    }
}

/// Substrings that disqualify an otherwise matching asset. Used to keep
/// the Windows system fragment (`["setup", "<arch>", ".exe"]`) from
/// accidentally picking up `oryxis-user-setup-<arch>.exe`, which
/// satisfies all three substrings. Only the system path needs an
/// exclude rule, `user-setup` is already specific enough on its own.
fn platform_asset_exclude() -> Vec<&'static str> {
    if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            if is_per_user_install() {
                return vec![];
            }
        }
        return vec!["user-setup"];
    }
    vec![]
}

/// Substrings identifying this platform's bare-binary nightly asset. The
/// nightly workflow publishes, per platform:
///   • Linux:    `oryxis-nightly-linux-<arch>.bin`
///   • macOS:    `oryxis-nightly-macos-aarch64.bin`
///   • Windows:  `oryxis-nightly-windows-<arch>.exe`
/// The `.bin` / `.exe` suffix keeps the matcher from grabbing the
/// `.tar.gz` / `.zip` archives published under the same name stem.
fn nightly_asset_fragment() -> Vec<&'static str> {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "x86_64") {
            vec!["nightly", "windows", "x86_64", ".exe"]
        } else if cfg!(target_arch = "aarch64") {
            vec!["nightly", "windows", "aarch64", ".exe"]
        } else {
            vec![]
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            vec!["nightly", "macos", "aarch64", ".bin"]
        } else {
            vec![]
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "x86_64") {
            vec!["nightly", "linux", "x86_64", ".bin"]
        } else if cfg!(target_arch = "aarch64") {
            vec!["nightly", "linux", "aarch64", ".bin"]
        } else {
            vec![]
        }
    } else {
        vec![]
    }
}

/// Download the installer to a temp file. Reads the full response body into
/// memory first, simpler than streaming chunks, fine for our installer
/// sizes (~80 MB). The progress closure is accepted for API symmetry but
/// only fires once (0.0 then 1.0) in this implementation.
///
/// Before anything is written to disk the artifact's detached Ed25519
/// signature (the sibling `<asset>.sig` release asset, published by the
/// release/nightly workflows) is fetched and checked against the same
/// trust anchors the plugin pipeline uses. A missing or invalid
/// signature aborts the update: TLS alone is not the trust boundary
/// for code we are about to execute.
pub async fn download_installer(
    url: &str,
    file_name: &str,
    mut progress: impl FnMut(f32) + Send,
) -> Result<PathBuf, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("Oryxis/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(600))
        .https_only(true)
        .build()
        .map_err(|e| e.to_string())?;

    progress(0.0);
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;

    let sig_url = format!("{url}.sig");
    let sig_resp = client.get(&sig_url).send().await.map_err(|e| e.to_string())?;
    if !sig_resp.status().is_success() {
        return Err(format!(
            "update signature missing ({} on {file_name}.sig)",
            sig_resp.status()
        ));
    }
    let sig_b64 = sig_resp.text().await.map_err(|e| e.to_string())?;
    crate::plugins::verify::verify(&bytes, sig_b64.trim())
        .map_err(|e| format!("update signature verification failed: {e}"))?;

    let dest = std::env::temp_dir().join(file_name);
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    progress(1.0);
    Ok(dest)
}

/// Launch the platform installer and spawn-detach so it keeps running
/// after we exit. On Windows we go through `ShellExecuteW` so the
/// installer's manifest controls elevation: the system NSIS asks for
/// UAC, the per-user one runs as the current user without a prompt.
/// On macOS we open the mounted image; on Linux we open the file
/// manager so the user can run it.
pub fn launch_installer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let mut file: Vec<u16> = path.as_os_str().encode_wide().collect();
        file.push(0);

        let hinst = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                std::ptr::null(),
                file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW returns an HINSTANCE-shaped sentinel: values > 32
        // mean success, anything else is one of the documented error
        // codes (SE_ERR_ACCESSDENIED = 5 when the user declines UAC, etc).
        if (hinst as isize) <= 32 {
            return Err(format!("Failed to launch installer (ShellExecute={})", hinst as isize));
        }
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to open installer: {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Linux, best-effort: xdg-open file manager. install.sh expects the
        // user to run it manually.
        let _ = std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or_else(|| std::path::Path::new("/tmp")))
            .spawn();
    }
    Ok(())
}

/// Apply a downloaded nightly: replace the running executable with the
/// freshly downloaded bare binary and relaunch. The nightly channel
/// ships no installer, so there's nothing to hand off, we swap in place.
/// Returns once the new process is spawned; the caller then closes the
/// window so the old process exits and releases the file.
pub fn apply_binary_update(downloaded: &std::path::Path) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| format!("locate current exe: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Stage next to the target so the rename is same-filesystem and
        // atomic. Overwriting the running binary's path is fine on Unix:
        // the old inode stays alive for the still-running process.
        let staged = current.with_extension("new");
        std::fs::copy(downloaded, &staged).map_err(|e| format!("stage binary: {e}"))?;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("set exec bit: {e}"))?;
        std::fs::rename(&staged, &current).map_err(|e| format!("swap binary: {e}"))?;
        std::process::Command::new(&current)
            .spawn()
            .map_err(|e| format!("relaunch: {e}"))?;
    }

    #[cfg(windows)]
    {
        // A running .exe can't be overwritten, but it can be renamed.
        // Move ourselves aside, drop the new binary in place, relaunch.
        // `sweep_stale_binary` clears the `.old.exe` on the next boot.
        let old = current.with_extension("old.exe");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&current, &old).map_err(|e| format!("rename running exe: {e}"))?;
        if let Err(e) = std::fs::copy(downloaded, &current) {
            // Roll back so the user isn't left without a binary.
            let _ = std::fs::rename(&old, &current);
            return Err(format!("install new exe: {e}"));
        }
        std::process::Command::new(&current)
            .spawn()
            .map_err(|e| format!("relaunch: {e}"))?;
    }

    Ok(())
}

/// Delete the `.old.exe` left behind by a Windows nightly self-update.
/// Best-effort and a no-op everywhere else, called once on boot.
pub fn sweep_stale_binary() {
    #[cfg(windows)]
    {
        if let Ok(current) = std::env::current_exe() {
            let _ = std::fs::remove_file(current.with_extension("old.exe"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_eq_matches_full_and_short_prefixes() {
        let full = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
        // Identical full SHAs.
        assert!(commit_eq(full, full));
        // Short (8-hex title form) vs full: compare on the common prefix.
        assert!(commit_eq("a1b2c3d4", full));
        assert!(commit_eq(full, "A1B2C3D4")); // case-insensitive
        // Different commits.
        assert!(!commit_eq("a1b2c3d4", "ffffffff0000"));
        // Too short to trust (< 7 hex) never matches, guards against
        // accidental "everything is up to date" on a garbage value.
        assert!(!commit_eq("a1b", "a1b2c3d4"));
        assert!(!commit_eq("", full));
    }

    #[test]
    fn nightly_commit_prefers_hex_target_commitish() {
        let json = serde_json::json!({
            "target_commitish": "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
            "name": "Nightly (deadbeef)",
        });
        // A real hex commitish wins over the title.
        assert_eq!(
            nightly_commit(&json).as_deref(),
            Some("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678"),
        );
    }

    #[test]
    fn nightly_commit_falls_back_to_title_when_commitish_is_a_branch() {
        // GitHub often returns the branch name, not a SHA, in
        // target_commitish; parse the short SHA out of the title instead.
        let json = serde_json::json!({
            "target_commitish": "main",
            "name": "Nightly (deadbeef)",
        });
        assert_eq!(nightly_commit(&json).as_deref(), Some("deadbeef"));
    }

    #[test]
    fn nightly_commit_none_when_unparseable() {
        let json = serde_json::json!({
            "target_commitish": "main",
            "name": "Nightly build",
        });
        assert!(nightly_commit(&json).is_none());
    }
}

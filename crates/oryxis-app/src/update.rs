//! Auto-update — queries GitHub releases on startup, prompts the user if a
//! newer version is available, downloads the platform installer, and hands
//! off to it so the app can relaunch on the new version.
//!
//! Flow:
//!   1. `check_latest_release()` — async HTTP GET to GitHub releases/latest
//!   2. UI compares `tag_name` against `env!("CARGO_PKG_VERSION")`; if newer
//!      and not in `skipped_version`, shows a modal with 3 options:
//!        - **Skip this version** → persists tag into vault `settings` table
//!        - **Remind me later** → dismisses, asks next launch
//!        - **Update now** → triggers `download_installer` + `launch_installer_and_exit`
//!   3. During download, the UI shows a progress bar via streaming bytes.

use std::path::PathBuf;

/// Hard-coded release repo — kept in one place so publishing the app to a
/// fork or mirror requires a single edit.
pub const RELEASE_REPO: &str = "wilsonglasser/oryxis";

/// Release metadata extracted from the GitHub API payload.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    /// Version without the leading `v` (e.g. `0.3.2`).
    pub version: String,
    /// HTML page for the release (for "What's new").
    pub html_url: String,
    /// Release notes body (markdown), preview shown in the modal.
    pub body: String,
    /// Download URL for the installer asset matching this platform.
    pub installer_url: Option<String>,
    /// Installer file name (used when saving to temp).
    pub installer_name: Option<String>,
}

/// Query the GitHub API for the latest release. Returns `None` if the
/// remote version is not strictly newer than the compile-time package
/// version. Any network / parse error also returns `None` — update
/// notifications are best-effort, never break startup.
pub async fn check_latest_release() -> Option<UpdateInfo> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", RELEASE_REPO);
    let client = reqwest::Client::builder()
        .user_agent(concat!("Oryxis/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;

    let tag = json.get("tag_name")?.as_str()?.trim_start_matches('v').to_string();
    let current = env!("CARGO_PKG_VERSION");
    if !is_newer(&tag, current) {
        return None;
    }

    let html_url = json.get("html_url")?.as_str()?.to_string();
    let body = json.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Pick the asset that matches our platform.
    let (installer_url, installer_name) = pick_asset(&json);

    Some(UpdateInfo { version: tag, html_url, body, installer_url, installer_name })
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

fn pick_asset(json: &serde_json::Value) -> (Option<String>, Option<String>) {
    let assets = match json.get("assets").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return (None, None),
    };
    let want = platform_asset_fragment();
    for a in assets {
        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let lname = name.to_lowercase();
        if want.iter().all(|w| lname.contains(w)) {
            let url = a.get("browser_download_url").and_then(|v| v.as_str()).map(|s| s.to_string());
            return (url, Some(name.to_string()));
        }
    }
    (None, None)
}

/// Substrings we expect inside the asset filename for the current platform.
/// NSIS `.exe` installer for Windows, `.dmg` for macOS, `.AppImage`/`.deb`/
/// tarball for Linux depending on the release pipeline.
fn platform_asset_fragment() -> Vec<&'static str> {
    if cfg!(target_os = "windows") {
        vec!["windows", ".exe"]
    } else if cfg!(target_os = "macos") {
        vec!["macos", ".dmg"]
    } else {
        // Linux — prefer AppImage. install.sh typically consumes the tarball.
        vec!["linux"]
    }
}

/// Download the installer to a temp file. Reads the full response body into
/// memory first — simpler than streaming chunks, fine for our installer
/// sizes (~80 MB). The progress closure is accepted for API symmetry but
/// only fires once (0.0 then 1.0) in this implementation.
pub async fn download_installer(
    url: &str,
    file_name: &str,
    mut progress: impl FnMut(f32) + Send,
) -> Result<PathBuf, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("Oryxis/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    progress(0.0);
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let dest = std::env::temp_dir().join(file_name);
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    progress(1.0);
    Ok(dest)
}

/// Launch the platform installer and spawn-detach so it keeps running
/// after we exit. On Windows the NSIS `.exe` installer is invoked
/// directly (UAC prompt handled by it); on macOS we open the mounted
/// image; on Linux we open the file manager so the user can run it.
pub fn launch_installer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new(path)
            .spawn()
            .map_err(|e| format!("Failed to launch installer: {e}"))?;
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
        // Linux — best-effort: xdg-open file manager. install.sh expects the
        // user to run it manually.
        let _ = std::process::Command::new("xdg-open")
            .arg(path.parent().unwrap_or_else(|| std::path::Path::new("/tmp")))
            .spawn();
    }
    Ok(())
}

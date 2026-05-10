//! Locate the AWS `session-manager-plugin` binary.
//!
//! The plugin is a single Go binary (~25 MB) that AWS distributes
//! out-of-band from the SDK, installable via `apt`, `brew`, the
//! AWS docs RPM, or a manual download. We don't bundle it; we just
//! find it on PATH (or in a couple of well-known fallback paths)
//! and surface a clear error when it's missing so the user can
//! install it without guessing.
//!
//! No auto-download by design: it's a system-level binary the user
//! needs on their machine. We point them at the AWS docs install
//! page and they take it from there.

use std::path::PathBuf;

use oryxis_cloud::CloudError;

/// AWS official docs landing page for the install instructions. Same
/// page lists `apt` / `brew` / `rpm` / Windows installer commands.
pub const AWS_DOCS_INSTALL_URL: &str =
    "https://docs.aws.amazon.com/systems-manager/latest/userguide/session-manager-working-with-install-plugin.html";

/// Structured info about a missing plugin so the UI can render a
/// proper modal (title + multi-line body + "Open AWS docs" link)
/// instead of dumping a flat string into a log line.
#[derive(Debug, Clone)]
pub struct PluginMissing {
    pub title: String,
    pub body: String,
    pub docs_url: String,
}

/// Resolve the absolute path to `session-manager-plugin`. Errors with
/// install instructions when nothing matches.
pub fn find_plugin() -> Result<PathBuf, CloudError> {
    // 1. PATH lookup is the canonical install location for `apt`,
    //    `brew`, `dnf`, and the AWS RPM/DEB packages.
    if let Some(p) = which("session-manager-plugin") {
        return Ok(p);
    }

    // 2. macOS Homebrew with the canonical formula puts it at
    //    `/opt/homebrew/bin` (Apple Silicon) or `/usr/local/bin`
    //    (Intel). Both are usually on PATH but `which` in stripped
    //    GUI environments sometimes misses them.
    #[cfg(target_os = "macos")]
    {
        for candidate in [
            "/opt/homebrew/bin/session-manager-plugin",
            "/usr/local/bin/session-manager-plugin",
            "/opt/local/bin/session-manager-plugin",
        ] {
            let p = PathBuf::from(candidate);
            if p.exists() {
                return Ok(p);
            }
        }
    }

    // 3. AWS's official Linux packages drop the binary directly in
    //    `/usr/local/sessionmanagerplugin/bin/` (an unfortunate
    //    non-PATH location, a leftover from when AWS shipped the
    //    plugin as a tarball before the deb / rpm existed).
    #[cfg(target_os = "linux")]
    {
        for candidate in [
            "/usr/local/sessionmanagerplugin/bin/session-manager-plugin",
            "/usr/local/bin/session-manager-plugin",
        ] {
            let p = PathBuf::from(candidate);
            if p.exists() {
                return Ok(p);
            }
        }
    }

    // 4. Windows, the AWS installer drops it under Program Files.
    #[cfg(target_os = "windows")]
    {
        for candidate in [
            r"C:\Program Files\Amazon\SessionManagerPlugin\bin\session-manager-plugin.exe",
            r"C:\Program Files (x86)\Amazon\SessionManagerPlugin\bin\session-manager-plugin.exe",
        ] {
            let p = PathBuf::from(candidate);
            if p.exists() {
                return Ok(p);
            }
        }
    }

    Err(CloudError::Other(missing_info().body))
}

/// Structured per-OS install hint. Used by the UI to populate the
/// "plugin missing" modal. The body is intentionally short, the
/// docs link does the heavy lifting and stays evergreen even when
/// AWS changes their package URLs.
pub fn missing_info() -> PluginMissing {
    let title = "AWS Session Manager plugin not found".to_string();
    #[cfg(target_os = "linux")]
    let body = "ECS Exec and SSM Session need the AWS session-manager-plugin binary.\n\n\
        Install it on your system via apt / dnf / rpm following the AWS docs, then try again.\n\n\
        Quick install (Debian/Ubuntu): apt install session-manager-plugin\n\
        Quick install (Fedora/RHEL): dnf install session-manager-plugin"
        .to_string();
    #[cfg(target_os = "macos")]
    let body = "ECS Exec and SSM Session need the AWS session-manager-plugin binary.\n\n\
        Install it on your system following the AWS docs, then try again.\n\n\
        Quick install: brew install --cask session-manager-plugin"
        .to_string();
    #[cfg(target_os = "windows")]
    let body = "ECS Exec and SSM Session need the AWS SessionManagerPlugin.\n\n\
        Install it on your system from the AWS docs page below, then try again."
        .to_string();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let body = "ECS Exec and SSM Session need the AWS session-manager-plugin binary.\n\n\
        Install it on your system following the AWS docs, then try again."
        .to_string();

    PluginMissing { title, body, docs_url: AWS_DOCS_INSTALL_URL.to_string() }
}

/// Walk `PATH` looking for an executable. Returns the first hit. We
/// roll this by hand instead of pulling in `which` so the crate keeps
/// its dependency footprint tight (one less crate to audit).
fn which(name: &str) -> Option<PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        // Windows binaries usually carry an `.exe` suffix.
        #[cfg(target_os = "windows")]
        {
            let with_ext = dir.join(format!("{name}.exe"));
            if is_executable(&with_ext) {
                return Some(with_ext);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}

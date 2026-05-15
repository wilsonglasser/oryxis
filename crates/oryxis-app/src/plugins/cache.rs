//! On-disk plugin cache.
//!
//! Layout, under the same `~/.oryxis` root the vault uses (the plan
//! sketched `~/.local/share/oryxis`, but staying consistent with the
//! vault's `dirs::home_dir().join(".oryxis")` keeps everything in one
//! place and works identically across platforms):
//!
//! ```text
//! ~/.oryxis/plugins/
//!   aws/
//!     0.3.1/oryxis-cloud-aws-plugin
//!     0.4.2/oryxis-cloud-aws-plugin
//!     current                       <- text file: "0.4.2"
//!     manifest.json                 <- last seen, for offline use
//! ```
//!
//! `current` is a plain text file holding the active version string
//! rather than a symlink, Windows symlinks need a privilege the app
//! can't assume, and a one-line file is trivially atomic to swap.
//!
//! Retention: keep the last two versions per provider so a rollback
//! is always one step away; prune the rest.

use std::path::PathBuf;

use super::PluginError;

/// Root of the plugin cache: `~/.oryxis/plugins/`. Created on demand
/// by the callers that write into it.
pub fn cache_root() -> Result<PathBuf, PluginError> {
    let home = dirs::home_dir()
        .ok_or_else(|| PluginError::Io(std::io::Error::other("no home directory")))?;
    Ok(home.join(".oryxis").join("plugins"))
}

/// Per-provider directory: `~/.oryxis/plugins/<provider>/`.
pub fn provider_dir(provider_id: &str) -> Result<PathBuf, PluginError> {
    Ok(cache_root()?.join(provider_id))
}

/// Per-version directory: `~/.oryxis/plugins/<provider>/<version>/`.
pub fn version_dir(provider_id: &str, version: &str) -> Result<PathBuf, PluginError> {
    Ok(provider_dir(provider_id)?.join(version))
}

/// Conventional binary file name inside a version directory. The
/// `.exe` suffix on Windows is what `Command::spawn` needs to find
/// the executable.
pub fn binary_name(provider_id: &str) -> String {
    let base = format!("oryxis-cloud-{provider_id}-plugin");
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base
    }
}

/// Absolute path to a specific provider version's binary, whether or
/// not it actually exists on disk yet.
pub fn binary_path(provider_id: &str, version: &str) -> Result<PathBuf, PluginError> {
    Ok(version_dir(provider_id, version)?.join(binary_name(provider_id)))
}

/// Path to the `current` pointer file.
fn current_file(provider_id: &str) -> Result<PathBuf, PluginError> {
    Ok(provider_dir(provider_id)?.join("current"))
}

/// Path to the cached `manifest.json` (last seen copy, used offline).
pub fn manifest_path(provider_id: &str) -> Result<PathBuf, PluginError> {
    Ok(provider_dir(provider_id)?.join("manifest.json"))
}

/// Read the version string the `current` pointer names, or `None`
/// when no version has been activated yet.
pub fn current_version(provider_id: &str) -> Result<Option<String>, PluginError> {
    let path = current_file(provider_id)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let v = s.trim().to_string();
            Ok((!v.is_empty()).then_some(v))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(PluginError::Io(e)),
    }
}

/// Point `current` at `version`. The version directory must already
/// hold a binary, callers download + verify first, then flip the
/// pointer so a half-written install is never made active.
pub fn set_current(provider_id: &str, version: &str) -> Result<(), PluginError> {
    let bin = binary_path(provider_id, version)?;
    if !bin.exists() {
        return Err(PluginError::BinaryNotFound(bin));
    }
    let dir = provider_dir(provider_id)?;
    std::fs::create_dir_all(&dir)?;
    // Write to a temp sibling then rename, so a crash mid-write can't
    // leave `current` pointing at a truncated version string.
    let tmp = dir.join("current.tmp");
    std::fs::write(&tmp, version)?;
    std::fs::rename(&tmp, current_file(provider_id)?)?;
    Ok(())
}

/// Absolute path to the *active* plugin binary, or `None` when no
/// version is installed / activated.
pub fn current_binary(provider_id: &str) -> Result<Option<PathBuf>, PluginError> {
    match current_version(provider_id)? {
        Some(v) => {
            let path = binary_path(provider_id, &v)?;
            Ok(path.exists().then_some(path))
        }
        None => Ok(None),
    }
}

/// Every installed version of a provider, sorted ascending by
/// semver-ish ordering. A directory only counts when it actually
/// holds the binary, a bare directory from an interrupted download
/// is ignored.
pub fn installed_versions(provider_id: &str) -> Result<Vec<String>, PluginError> {
    let dir = provider_dir(provider_id)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(PluginError::Io(e)),
    };
    let mut versions: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if binary_path(provider_id, &name)?.exists() {
            versions.push(name);
        }
    }
    versions.sort_by_key(|v| super::manifest::version_key(v));
    Ok(versions)
}

/// Prune installed versions down to the most recent two, never
/// removing whatever `current` points at even if it's older (a
/// pinned rollback shouldn't get garbage-collected out from under
/// the user).
pub fn cleanup_keep_last_two(provider_id: &str) -> Result<(), PluginError> {
    let versions = installed_versions(provider_id)?;
    if versions.len() <= 2 {
        return Ok(());
    }
    let active = current_version(provider_id)?;
    // `versions` is ascending; the last two are the keepers.
    let keep: Vec<&String> = versions.iter().rev().take(2).collect();
    for v in &versions {
        if keep.contains(&v) || active.as_ref() == Some(v) {
            continue;
        }
        let dir = version_dir(provider_id, v)?;
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            // A failed prune is not fatal, log and keep going so one
            // locked file doesn't block reclaiming the rest.
            tracing::warn!(
                target = "oryxis::plugins",
                provider = %provider_id,
                version = %v,
                error = %e,
                "failed to prune old plugin version"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_name_has_platform_suffix() {
        let name = binary_name("aws");
        if cfg!(windows) {
            assert_eq!(name, "oryxis-cloud-aws-plugin.exe");
        } else {
            assert_eq!(name, "oryxis-cloud-aws-plugin");
        }
    }

    #[test]
    fn paths_nest_under_provider() {
        // Don't assert the home prefix (varies per machine), just the
        // tail structure the rest of the module relies on.
        let vdir = version_dir("aws", "0.4.2").unwrap();
        assert!(vdir.ends_with("plugins/aws/0.4.2") || vdir.ends_with("plugins\\aws\\0.4.2"));
        let bin = binary_path("aws", "0.4.2").unwrap();
        assert!(bin.starts_with(vdir));
    }
}

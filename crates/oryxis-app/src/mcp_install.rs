//! Plugin-managed install layer for `oryxis-mcp`.
//!
//! MCP differs from cloud plugins: external clients (Claude Desktop,
//! Claude Code, Cursor) are the ones that spawn `oryxis-mcp`, not the
//! app. The app's plugin subsystem still owns download / verify /
//! cache, but the binary then has to live at a *stable* path the
//! external client can hardcode in its config and not have invalidated
//! every time the plugin updates.
//!
//! Layout:
//!
//! ```text
//! ~/.oryxis/plugins/mcp/0.1.0/oryxis-mcp     (versioned cache, plugin infra writes this)
//! ~/.oryxis/bin/oryxis-mcp                   (stable launcher, this module manages)
//! ```
//!
//! Install flow: plugin code downloads + verifies + writes the
//! versioned binary, then [`sync_launcher_from_cache`] copies the
//! active version into the stable launcher path. External clients
//! always spawn the launcher path.

use std::path::PathBuf;

use crate::plugins::{cache, PluginError};

/// Stable launcher directory: `~/.oryxis/bin/`.
pub(crate) fn launcher_dir() -> Result<PathBuf, PluginError> {
    oryxis_core::paths::oryxis_dir()
        .ok_or_else(|| PluginError::Io(std::io::Error::other("no home directory")))
        .map(|dir| dir.join("bin"))
}

/// Stable launcher path: `~/.oryxis/bin/oryxis-mcp[.exe]`. External
/// MCP clients spawn this; the actual binary behind it is rotated
/// whenever a new plugin version installs.
pub(crate) fn launcher_path() -> Result<PathBuf, PluginError> {
    Ok(launcher_dir()?.join(cache::binary_name("mcp")))
}

/// `true` when the stable launcher exists on disk. Doesn't validate
/// that it matches the cached version; if a sync failed mid-way we'd
/// rather keep the old launcher reachable than report "not installed".
pub(crate) fn is_installed() -> bool {
    launcher_path().map(|p| p.exists()).unwrap_or(false)
}

/// Copy the currently-active cached MCP binary into the stable
/// launcher path, atomically. Call this after a successful plugin
/// install / update.
///
/// Windows can't overwrite a running `.exe` (sharing violation), so
/// if the launcher is held open by a live Claude Desktop process we
/// rename the old one to `oryxis-mcp.old.exe` first and let
/// [`sweep_stale_launcher`] reap it next boot. On Unix the rename
/// just overwrites.
pub(crate) fn sync_launcher_from_cache() -> Result<PathBuf, PluginError> {
    let dest = launcher_path()?;
    let source = cache::current_binary("mcp")?
        .ok_or_else(|| PluginError::BinaryNotFound(dest.clone()))?;
    let dir = launcher_dir()?;
    std::fs::create_dir_all(&dir)?;

    // Write to a `.tmp` sibling first so a half-finished copy can't
    // shadow the working launcher even if the process crashes mid-way.
    let tmp = dir.join(format!("{}.tmp", cache::binary_name("mcp")));
    std::fs::copy(&source, &tmp)?;
    set_executable(&tmp)?;

    if cfg!(windows) && dest.exists() {
        // The plugin may still be running under an external client.
        // Move the live binary aside instead of trying to replace it.
        let stale = dir.join(format!("{}.old", cache::binary_name("mcp")));
        let _ = std::fs::remove_file(&stale);
        if let Err(e) = std::fs::rename(&dest, &stale) {
            tracing::warn!(
                target = "oryxis::mcp",
                error = %e,
                "could not move stale MCP launcher aside; install may be incomplete until external client closes"
            );
            // Fall through; the rename below will fail with a clearer
            // error if the file is genuinely locked.
        }
    }

    std::fs::rename(&tmp, &dest)?;
    Ok(dest)
}

/// Called from the plugin install completion handler when `mcp`
/// finishes. Refreshes the stable launcher from the freshly-activated
/// cached version and rewrites the `oryxis` entry in every client
/// config that already carries one (on this machine and, on Windows,
/// inside the WSL distro), so each `command` points at the launcher
/// path the new version landed at. A legacy `~/.claude/.mcp.json`
/// entry older releases wrote counts as installed and is migrated to
/// the file Claude Code reads. `vault_pw` carries the master password
/// when the user opted in to embedding it, so the refresh doesn't
/// strip it from the configs. Best-effort: failures are logged but
/// don't roll back the install.
pub(crate) fn post_install_refresh(token: &str, vault_pw: Option<&str>) {
    if let Err(e) = sync_launcher_from_cache() {
        tracing::warn!(
            target = "oryxis::mcp",
            error = %e,
            "failed to refresh stable MCP launcher after install"
        );
        return;
    }
    if !crate::mcp::mcp_config_installed() {
        return;
    }
    // The legacy dead-letter entry has no live config to refresh in
    // place; migrating it means writing Claude Code's real file once.
    // Only when the LEGACY file holds one: another client's entry is not
    // a reason to write a file Claude Code's user never asked for (with
    // the vault password embedded, that would be a credential nobody
    // guards; Claude Code's folder guard cannot catch it, its config
    // sits in the home directory itself).
    if crate::mcp::legacy_config_has_entry()
        && !crate::mcp::client_has_entry(crate::mcp_clients::McpClient::ClaudeCode)
        && let Err(msg) = crate::mcp::install_mcp_config_to_file(
            crate::mcp_clients::McpClient::ClaudeCode,
            token,
            vault_pw,
        )
    {
        tracing::warn!(
            target = "oryxis::mcp",
            error = %msg,
            "failed to migrate the legacy MCP entry after install"
        );
    }
    crate::mcp::refresh_installed_clients(token, vault_pw);
}

/// One-shot migration / first-install: fetch the manifest, pick the
/// best compatible version, download + verify it. Used on boot when
/// `mcp_server_enabled` was already set but no plugin binary is
/// present, typically a v0.6 user upgrading to the plugin-managed
/// layout. Returns the version string so the standard
/// `PluginInstallDone` handler can flip `current` and refresh the
/// launcher.
pub(crate) async fn migrate_install() -> Result<String, String> {
    let manifest = crate::plugins::download::fetch_manifest("mcp")
        .await
        .map_err(|e| e.to_string())?;
    let best = manifest
        .best(
            env!("CARGO_PKG_VERSION"),
            oryxis_plugin_protocol::SUPPORTED_PROTOCOL_VERSIONS,
        )
        .ok_or_else(|| "no compatible mcp version".to_string())?
        .clone();
    crate::plugins::download::download_and_install("mcp", &best, |_, _| {})
        .await
        .map_err(|e| e.to_string())?;
    Ok(best.version)
}

/// Boot-time cleanup of the `.old` launcher [`sync_launcher_from_cache`]
/// left behind on Windows when it couldn't overwrite the live `.exe`.
/// No-op on Unix.
pub(crate) fn sweep_stale_launcher() {
    if !cfg!(windows) {
        return;
    }
    let Ok(dir) = launcher_dir() else { return };
    let stale = dir.join(format!("{}.old", cache::binary_name("mcp")));
    if stale.exists()
        && let Err(e) = std::fs::remove_file(&stale)
    {
        tracing::debug!(
            target = "oryxis::mcp",
            error = %e,
            "old MCP launcher still locked; will retry next boot"
        );
    }
}

/// Mark the launcher executable on Unix. No-op on Windows where the
/// `.exe` extension implies executability.
fn set_executable(path: &std::path::Path) -> Result<(), PluginError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_path_ends_with_oryxis_mcp() {
        let p = launcher_path().unwrap();
        let name = p.file_name().unwrap().to_string_lossy();
        if cfg!(windows) {
            assert_eq!(name, "oryxis-mcp.exe");
        } else {
            assert_eq!(name, "oryxis-mcp");
        }
        let parent = p.parent().unwrap();
        assert!(
            parent.ends_with(".oryxis/bin") || parent.ends_with(".oryxis\\bin"),
            "unexpected launcher dir: {}",
            parent.display()
        );
    }
}

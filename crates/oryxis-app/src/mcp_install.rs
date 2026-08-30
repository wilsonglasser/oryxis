//! Plugin-managed install layer for `oryxis-mcp`.
//!
//! MCP differs from other plugins: external clients (Claude Desktop,
//! Claude Code, Cursor) are the ones that spawn `oryxis-mcp`, not the
//! app. The app performs no network fetches anymore — whatever sits in
//! the local cache (or next to the app executable as a dev build) is
//! what runs — but the binary still has to live at a *stable* path the
//! external client can hardcode in its config and not have invalidated
//! every time the plugin updates.
//!
//! Layout:
//!
//! ```text
//! ~/.oryxis/plugins/mcp/0.1.0/oryxis-mcp     (versioned cache, populated out of band)
//! ~/.oryxis/plugins/mcp/manifest.json        (last seen manifest: sha256 + signature)
//! ~/.oryxis/bin/oryxis-mcp                   (stable launcher, this module manages)
//! ```
//!
//! Install flow: [`verify_cached_binary`] re-checks the cached binary
//! against the cached manifest (SHA-256 match, then Ed25519 signature
//! against this build's trust anchors — production key always, dev key
//! only in debug builds), and only then does
//! [`sync_launcher_from_cache`] copy the active version into the stable
//! launcher path. External clients always spawn the launcher path, so
//! nothing reaches it unsigned: release builds fail closed on a cache
//! whose contents do not match a signed manifest entry.

use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::plugins::{cache, manifest, verify, PluginError};

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

/// Re-verify the cached MCP binary against the cached manifest before
/// anything copies it to the launcher path external clients spawn.
///
/// The download path that used to gate this is gone; the cache is now
/// populated out of band (release installers, a dev build, a manual
/// copy), which makes the copy into `~/.oryxis/bin/` the trust
/// boundary: a same-user process can write the cache, but an unsigned
/// or manifest-mismatched binary must never become the launcher that
/// Claude Desktop spawns with the MCP token (and, if the user opted
/// in, the vault master password) in reach.
///
/// Matching is by SHA-256 across the version's manifest entries rather
/// than by os/arch: the cache only ever holds the one binary for this
/// machine, and a hash match is both stricter and immune to a
/// platform-tagging mistake. Debug builds additionally honor the dev
/// signing key (see [`verify`]), so locally built binaries install
/// after a `oryxis-plugin-signer --dev` signature; release builds
/// trust the production key only and fail closed.
fn verify_cached_binary(source: &std::path::Path, version: &str) -> Result<Vec<u8>, PluginError> {
    let manifest_path = cache::manifest_path("mcp")?;
    let manifest_json = std::fs::read_to_string(&manifest_path).map_err(|e| {
        PluginError::Integrity(format!(
            "no cached manifest at {} to verify the MCP binary against \
             (populate the cache with a signed release, or in a debug build \
             sign it with `oryxis-plugin-signer --dev` and record it in \
             manifest.json): {e}",
            manifest_path.display()
        ))
    })?;
    let parsed = manifest::PluginManifest::parse(&manifest_json)?;
    let entry = parsed.find_version(version).ok_or_else(|| {
        PluginError::Integrity(format!(
            "cached manifest has no entry for MCP version {version}"
        ))
    })?;

    let bytes = std::fs::read(source)?;
    // Returned to the caller so the bytes that land in the launcher are
    // the exact bytes verified here — a re-read of the path would reopen
    // the verify/copy race for no benefit (the buffer is already in
    // memory). sha2 0.11 returns a `hybrid_array::Array` with no LowerHex
    // impl; format the bytes directly (same as the font pin check).
    let digest: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    let matched = entry
        .binaries
        .iter()
        .find(|b| b.sha256.eq_ignore_ascii_case(&digest))
        .ok_or_else(|| {
            PluginError::Integrity(format!(
                "cached MCP binary does not match any signed manifest entry \
                 for version {version} (sha256 {digest}); refusing to install it \
                 as the launcher"
            ))
        })?;
    verify::verify(&bytes, &matched.signature).map_err(|e| {
        PluginError::Integrity(format!(
            "MCP binary failed its Ed25519 signature check; refusing to \
             install it as the launcher: {e}"
        ))
    })?;
    Ok(bytes)
}

/// Copy the currently-active cached MCP binary into the stable
/// launcher path, atomically, after verifying it against the cached
/// manifest. Call this after a successful plugin install / update.
///
/// Windows can't overwrite a running `.exe` (sharing violation), so
/// if the launcher is held open by a live Claude Desktop process we
/// rename the old one to `oryxis-mcp.old.exe` first and let
/// [`sweep_stale_launcher`] reap it next boot. On Unix the rename
/// just overwrites.
pub(crate) fn sync_launcher_from_cache() -> Result<PathBuf, PluginError> {
    let dest = launcher_path()?;
    let version = cache::current_version("mcp")?
        .ok_or_else(|| PluginError::BinaryNotFound(dest.clone()))?;
    let source = cache::current_binary("mcp")?
        .ok_or_else(|| PluginError::BinaryNotFound(dest.clone()))?;
    let verified = verify_cached_binary(&source, &version)?;
    let dir = launcher_dir()?;
    std::fs::create_dir_all(&dir)?;

    // Write the VERIFIED in-memory bytes to a `.tmp` sibling first so a
    // half-finished copy can't shadow the working launcher even if the
    // process crashes mid-way — and so what lands is by construction the
    // buffer that passed the hash + signature gates, never a fresh
    // (racy) re-read of the cache path.
    let tmp = dir.join(format!("{}.tmp", cache::binary_name("mcp")));
    std::fs::write(&tmp, &verified)?;
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

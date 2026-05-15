//! Fetch a plugin manifest and download + install a plugin binary.
//!
//! The install path is gated twice before a binary is made
//! reachable: the SHA-256 from the manifest must match the bytes on
//! the wire, *and* the Ed25519 signature must validate against a
//! baked-in trust anchor (see [`super::verify`]). Only then are the
//! bytes written, and the write is atomic, into a sibling `.tmp`
//! file that's renamed into place, so a half-finished download is
//! never visible as an installed version.

use std::path::PathBuf;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};

use super::manifest::{ManifestEntry, PlatformBinary, PluginManifest};
use super::{cache, verify, PluginError};

/// Fetch and parse a provider's manifest JSON from its hosted URL.
pub async fn fetch_manifest(url: &str) -> Result<PluginManifest, PluginError> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("Oryxis/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| PluginError::Download(e.to_string()))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| PluginError::Download(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(PluginError::Manifest(format!(
            "manifest fetch returned HTTP {}",
            resp.status()
        )));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| PluginError::Download(e.to_string()))?;
    PluginManifest::parse(&body)
}

/// Download the binary for `entry` on the current platform, verify
/// it, and install it into the version cache. Returns the absolute
/// path to the installed binary.
///
/// `progress` is called as bytes arrive with `(downloaded, total)`;
/// `total` is `0` when the server doesn't send a `Content-Length`.
/// Installing does *not* flip the `current` pointer, the caller
/// decides when a freshly installed version becomes active.
pub async fn download_and_install(
    provider_id: &str,
    entry: &ManifestEntry,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, PluginError> {
    let binary = entry
        .binary_for_current_platform()
        .ok_or_else(|| {
            PluginError::Manifest(format!(
                "{provider_id} {} has no binary for this platform",
                entry.version
            ))
        })?;

    let bytes = download_bytes(binary, &mut progress).await?;

    // Gate 1: SHA-256. Cheap, catches a corrupted / truncated
    // transfer before the more expensive signature check.
    let digest = to_hex(&Sha256::digest(&bytes));
    if !digest.eq_ignore_ascii_case(&binary.sha256) {
        return Err(PluginError::Integrity(format!(
            "sha256 mismatch: manifest says {}, downloaded bytes hash to {digest}",
            binary.sha256
        )));
    }

    // Gate 2: Ed25519 signature against a baked-in trust anchor.
    verify::verify(&bytes, &binary.signature)?;

    // Both gates passed, write atomically into the version dir.
    let dir = cache::version_dir(provider_id, &entry.version)?;
    std::fs::create_dir_all(&dir)?;
    let final_path = cache::binary_path(provider_id, &entry.version)?;
    let tmp_path = dir.join(format!("{}.tmp", cache::binary_name(provider_id)));
    std::fs::write(&tmp_path, &bytes)?;
    set_executable(&tmp_path)?;
    std::fs::rename(&tmp_path, &final_path)?;

    // Best-effort retention prune, a failure here doesn't invalidate
    // the install that just succeeded.
    if let Err(e) = cache::cleanup_keep_last_two(provider_id) {
        tracing::warn!(
            target = "oryxis::plugins",
            provider = %provider_id,
            error = %e,
            "plugin cache prune failed after install"
        );
    }

    Ok(final_path)
}

/// Stream the binary into memory, firing `progress` per chunk. Held
/// fully in memory (~25 MB for AWS) because the Ed25519 gate needs
/// every byte anyway, the same trade-off `update.rs` already makes
/// for the ~80 MB app installers.
async fn download_bytes(
    binary: &PlatformBinary,
    progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<u8>, PluginError> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("Oryxis/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| PluginError::Download(e.to_string()))?;
    let resp = client
        .get(&binary.url)
        .send()
        .await
        .map_err(|e| PluginError::Download(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(PluginError::Download(format!("HTTP {}", resp.status())));
    }

    // Prefer the server's Content-Length, fall back to the manifest
    // size so the UI still has something to draw a bar against.
    let total = resp.content_length().unwrap_or(binary.size);
    let mut buf: Vec<u8> = Vec::with_capacity(total as usize);
    let mut stream = resp.bytes_stream();
    progress(0, total);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| PluginError::Download(e.to_string()))?;
        buf.extend_from_slice(&chunk);
        progress(buf.len() as u64, total.max(buf.len() as u64));
    }
    Ok(buf)
}

/// Lowercase hex encoding. Rolled by hand rather than pulling a
/// `hex` crate for one helper, consistent with the rest of the
/// codebase's "tiny helper over a dependency" preference.
fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Mark a freshly written plugin binary executable. No-op on
/// Windows, where executability is decided by file extension.
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
    fn to_hex_matches_known_vectors() {
        assert_eq!(to_hex(&[0x00, 0xff, 0x10]), "00ff10");
        assert_eq!(to_hex(&[]), "");
        // SHA-256 of the empty input, the canonical sanity vector.
        assert_eq!(
            to_hex(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}

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

use super::manifest::{self, ManifestEntry, PlatformBinary, PluginManifest};
use super::{cache, verify, PluginError, RELEASE_REPO};

/// Find the latest `<provider>-v*` release on GitHub and download
/// the `<provider>.json` manifest from its assets.
///
/// The plugin release workflow uploads both the binaries and a
/// matching `aws.json` (or whatever the provider is) to the same
/// GitHub Release. There's no separate manifest host: the release
/// IS the manifest source. The app finds the right release by
/// listing the repo's releases, filtering by tag prefix, and picking
/// the highest version that actually carries a manifest asset.
pub async fn fetch_manifest(provider_id: &str) -> Result<PluginManifest, PluginError> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("Oryxis/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| PluginError::Download(e.to_string()))?;

    // Step 1: list releases. 30 entries covers years of plugin
    // releases without paginating.
    let releases_url =
        format!("https://api.github.com/repos/{RELEASE_REPO}/releases?per_page=30");
    let resp = client
        .get(&releases_url)
        .send()
        .await
        .map_err(|e| PluginError::Download(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(PluginError::Manifest(format!(
            "github releases api returned HTTP {} for {RELEASE_REPO}",
            resp.status()
        )));
    }
    let releases: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| PluginError::Download(format!("parse releases json: {e}")))?;

    // Step 2: filter by `<provider>-v` tag, require a manifest asset,
    // pick the highest version.
    let tag_prefix = format!("{provider_id}-v");
    let manifest_asset = format!("{provider_id}.json");
    let mut candidates: Vec<(&serde_json::Value, [u32; 4])> = releases
        .iter()
        .filter_map(|r| {
            let tag = r.get("tag_name")?.as_str()?;
            let version = tag.strip_prefix(&tag_prefix)?;
            // Skip releases that don't carry the manifest asset.
            let has_manifest = r
                .get("assets")
                .and_then(|a| a.as_array())
                .map(|assets| {
                    assets.iter().any(|asset| {
                        asset.get("name").and_then(|n| n.as_str())
                            == Some(manifest_asset.as_str())
                    })
                })
                .unwrap_or(false);
            has_manifest.then(|| (r, manifest::version_key(version)))
        })
        .collect();
    candidates.sort_by_key(|(_, key)| std::cmp::Reverse(*key));
    let (release, _) = candidates.first().ok_or_else(|| {
        PluginError::Manifest(format!(
            "no `{tag_prefix}*` release with a `{manifest_asset}` asset found in {RELEASE_REPO}"
        ))
    })?;

    // Step 3: download the manifest asset itself.
    let download_url = release
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|assets| {
            assets.iter().find(|asset| {
                asset.get("name").and_then(|n| n.as_str())
                    == Some(manifest_asset.as_str())
            })
        })
        .and_then(|asset| asset.get("browser_download_url"))
        .and_then(|u| u.as_str())
        .ok_or_else(|| {
            PluginError::Manifest("asset url missing on release payload".into())
        })?;

    let body = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| PluginError::Download(e.to_string()))?
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

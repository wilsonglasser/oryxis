//! Plugin manifest, the hosted JSON that lists every released
//! version of a provider and where to download it.
//!
//! One manifest per provider, served from a dev-controlled URL (e.g.
//! `plugins.oryxis.io/aws.json`). The app fetches it to discover
//! updates, then filters to versions this build can actually run,
//! `min_app` not in the future and at least one protocol version in
//! common, and picks the highest.
//!
//! Version comparison is a hand-rolled `major.minor.patch` tuple
//! (see [`version_key`]) rather than a `semver` dependency: every
//! version string in play is a plain three-part release tag, and the
//! codebase already prefers rolling tiny helpers over pulling crates
//! for one function (cf. `session_manager_plugin::which`).

use serde::{Deserialize, Serialize};

use super::PluginError;

/// Parsed contents of a provider's manifest JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Stable provider id, matches `CloudProvider::id()`.
    pub provider_id: String,
    /// Every published version, newest-last is *not* assumed, the
    /// app sorts explicitly.
    pub versions: Vec<ManifestEntry>,
}

/// One released version of a provider plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Release version, `major.minor.patch` (`"0.4.2"`).
    pub version: String,
    /// Protocol versions this build of the plugin speaks. The app
    /// runs it only when this intersects its own supported set.
    pub protocol_versions: Vec<u32>,
    /// Minimum app version required. A plugin release that needs a
    /// newer app stays dormant ("update the app first") instead of
    /// being offered and then failing.
    pub min_app: String,
    /// Human-readable release notes, shown in the install / update
    /// UI. Optional, an entry without notes is still installable.
    #[serde(default)]
    pub changelog: Option<String>,
    /// Per-platform binaries. A platform with no entry simply isn't
    /// offered there.
    pub binaries: Vec<PlatformBinary>,
}

/// A downloadable binary for one OS/arch pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformBinary {
    /// `"linux"`, `"macos"`, `"windows"`, matches
    /// [`current_os`].
    pub os: String,
    /// `"x86_64"`, `"aarch64"`, matches [`current_arch`].
    pub arch: String,
    /// Direct download URL for the binary.
    pub url: String,
    /// Lowercase hex SHA-256 of the binary, checked before the
    /// signature gate.
    pub sha256: String,
    /// Base64 Ed25519 signature over the raw binary bytes, verified
    /// against the baked-in public key (see [`super::verify`]).
    pub signature: String,
    /// Size in bytes, used only to render a download size in the UI.
    #[serde(default)]
    pub size: u64,
}

impl PluginManifest {
    /// Parse manifest JSON. Wraps serde errors in
    /// [`PluginError::Manifest`] so the caller surfaces one error
    /// type.
    pub fn parse(json: &str) -> Result<Self, PluginError> {
        serde_json::from_str(json)
            .map_err(|e| PluginError::Manifest(format!("invalid manifest json: {e}")))
    }

    /// Serialize back to pretty JSON, used to write the offline
    /// `manifest.json` copy into the cache.
    pub fn to_json(&self) -> Result<String, PluginError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| PluginError::Manifest(format!("serialize manifest: {e}")))
    }

    /// Versions this app build can run, highest first.
    ///
    /// An entry qualifies when all three hold:
    /// 1. `min_app <= app_version`
    /// 2. its `protocol_versions` intersects `supported_protocols`
    /// 3. it ships a binary for the current platform
    pub fn compatible(
        &self,
        app_version: &str,
        supported_protocols: &[u32],
    ) -> Vec<&ManifestEntry> {
        let app_key = version_key(app_version);
        let mut out: Vec<&ManifestEntry> = self
            .versions
            .iter()
            .filter(|e| version_key(&e.min_app) <= app_key)
            .filter(|e| {
                e.protocol_versions
                    .iter()
                    .any(|p| supported_protocols.contains(p))
            })
            .filter(|e| e.binary_for_current_platform().is_some())
            .collect();
        // Descending: highest compatible version first.
        out.sort_by_key(|e| std::cmp::Reverse(version_key(&e.version)));
        out
    }

    /// Highest version this app build can run, or `None` when the
    /// manifest carries nothing compatible (app too old, protocol
    /// drift, or no platform binary).
    pub fn best(
        &self,
        app_version: &str,
        supported_protocols: &[u32],
    ) -> Option<&ManifestEntry> {
        self.compatible(app_version, supported_protocols)
            .into_iter()
            .next()
    }

    /// Look up an exact version by string, used to honour a
    /// user-pinned version.
    pub fn find_version(&self, version: &str) -> Option<&ManifestEntry> {
        self.versions.iter().find(|e| e.version == version)
    }
}

impl ManifestEntry {
    /// The binary entry matching the running platform, if the
    /// release ships one.
    pub fn binary_for_current_platform(&self) -> Option<&PlatformBinary> {
        let (os, arch) = (current_os(), current_arch());
        self.binaries
            .iter()
            .find(|b| b.os == os && b.arch == arch)
    }
}

/// Canonical OS string used in manifests. Matches the values the
/// release pipeline writes.
pub fn current_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// Canonical architecture string used in manifests.
pub fn current_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    }
}

/// Parse a `major.minor.patch` version into a comparable key. Extra
/// segments past the fourth are dropped; non-numeric junk in a
/// segment parses as 0. Mirrors `update.rs::is_newer`'s tolerance so
/// a malformed tag sorts low instead of panicking.
pub fn version_key(v: &str) -> [u32; 4] {
    let mut out = [0u32; 4];
    for (i, seg) in v.trim_start_matches('v').split('.').take(4).enumerate() {
        out[i] = seg
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> String {
        let os = current_os();
        let arch = current_arch();
        format!(
            r#"{{
              "provider_id": "aws",
              "versions": [
                {{
                  "version": "0.3.1",
                  "protocol_versions": [1],
                  "min_app": "0.8.0",
                  "binaries": [
                    {{"os":"{os}","arch":"{arch}","url":"https://x/0.3.1","sha256":"aa","signature":"bb","size":10}}
                  ]
                }},
                {{
                  "version": "0.4.2",
                  "protocol_versions": [1, 2],
                  "min_app": "0.9.0",
                  "changelog": "faster discovery",
                  "binaries": [
                    {{"os":"{os}","arch":"{arch}","url":"https://x/0.4.2","sha256":"cc","signature":"dd","size":20}}
                  ]
                }},
                {{
                  "version": "0.5.0",
                  "protocol_versions": [3],
                  "min_app": "0.9.0",
                  "binaries": [
                    {{"os":"{os}","arch":"{arch}","url":"https://x/0.5.0","sha256":"ee","signature":"ff","size":30}}
                  ]
                }}
              ]
            }}"#
        )
    }

    #[test]
    fn version_key_orders_correctly() {
        assert!(version_key("0.4.2") > version_key("0.3.9"));
        assert!(version_key("0.10.0") > version_key("0.9.0"));
        assert!(version_key("v1.0.0") == version_key("1.0.0"));
        assert!(version_key("garbage") == [0, 0, 0, 0]);
    }

    #[test]
    fn best_respects_min_app_and_protocol() {
        let m = PluginManifest::parse(&sample_json()).unwrap();
        // App 0.9.0, protocol 1: 0.5.0 needs protocol 3 (out), 0.4.2
        // and 0.3.1 both qualify, highest is 0.4.2.
        let best = m.best("0.9.0", &[1]).unwrap();
        assert_eq!(best.version, "0.4.2");
        // App 0.8.5: 0.4.2/0.5.0 need min_app 0.9.0 (out), only
        // 0.3.1 remains.
        let best = m.best("0.8.5", &[1, 2, 3]).unwrap();
        assert_eq!(best.version, "0.3.1");
        // Protocol 2 only: 0.4.2 is the single match.
        let best = m.best("0.9.0", &[2]).unwrap();
        assert_eq!(best.version, "0.4.2");
        // No common protocol at all.
        assert!(m.best("0.9.0", &[99]).is_none());
    }

    #[test]
    fn find_version_exact_match() {
        let m = PluginManifest::parse(&sample_json()).unwrap();
        assert_eq!(m.find_version("0.3.1").map(|e| e.version.as_str()), Some("0.3.1"));
        assert!(m.find_version("9.9.9").is_none());
    }

    #[test]
    fn json_roundtrips() {
        let m = PluginManifest::parse(&sample_json()).unwrap();
        let back = PluginManifest::parse(&m.to_json().unwrap()).unwrap();
        assert_eq!(back.versions.len(), 3);
        assert_eq!(back.provider_id, "aws");
    }
}

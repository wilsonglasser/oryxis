//! Cloud-provider plugin subsystem.
//!
//! Cloud providers (`aws`, future `gcp` / `azure` / `k8s`) ship as
//! standalone binaries the app downloads on demand and spawns as
//! subprocesses, speaking line-delimited JSON-RPC 2.0 over stdio (the
//! contract lives in `oryxis-plugin-protocol`).
//!
//! Module map:
//!
//! - [`host`]: `PluginHost`, spawn, JSON-RPC multiplexer, lifecycle
//!   (idle teardown, restart on crash).
//! - [`manifest`]: parse the hosted manifest JSON, filter by
//!   protocol + `min_app`, pick the best version.
//! - [`verify`]: Ed25519 verify-only over downloaded binaries.
//! - [`download`]: reqwest GET, SHA-256 + signature gate, atomic
//!   write into the version cache.
//! - [`cache`]: on-disk layout under `~/.oryxis/plugins/`, keep the
//!   last two versions per provider.
//!
//! This whole subsystem is inert until the cloud dispatch path is
//! rewired onto it (`PluginProvider`, a later PR). It compiles and is
//! unit-tested here but nothing calls it yet.

pub mod cache;
pub mod download;
pub mod host;
pub mod manifest;
pub mod provider;
pub mod verify;

pub use host::PluginHost;
pub use manifest::{ManifestEntry, PlatformBinary, PluginManifest};
pub use provider::PluginProvider;

use std::path::PathBuf;

use oryxis_cloud::CloudError;

/// Base URL for hosted plugin manifests, one JSON file per provider
/// (`<base>/<provider>.json`). Default points at the repo's GitHub
/// Pages, which `.github/workflows/update-manifest.yml` publishes
/// to after each plugin release. Swap to a custom domain (e.g.
/// `https://plugins.oryxis.io`) when one is configured.
pub const MANIFEST_BASE_URL: &str = "https://wilsonglasser.github.io/oryxis";

/// Manifest URL for a given provider id.
pub fn manifest_url(provider_id: &str) -> String {
    format!("{MANIFEST_BASE_URL}/{provider_id}.json")
}

/// Unified error for every step of the plugin lifecycle, spawn,
/// JSON-RPC, manifest parsing, download, integrity. Kept as one enum
/// (rather than one per submodule) so call sites, and the eventual
/// `PluginProvider`, match on a single type.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// The plugin binary isn't present at the expected cache path.
    #[error("plugin binary not found: {0}")]
    BinaryNotFound(PathBuf),

    /// `Command::spawn` itself failed (missing exec bit, bad
    /// architecture, ENOEXEC, ...).
    #[error("failed to spawn plugin process: {0}")]
    Spawn(String),

    /// The plugin process exited or its stdout closed while a call
    /// was in flight. The host tears the connection down; the next
    /// call respawns.
    #[error("plugin process exited unexpectedly")]
    ProcessGone,

    /// A call didn't get a response within the call timeout.
    #[error("plugin call timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// Malformed JSON-RPC, an unparseable params/result payload, or
    /// a failed `initialize` handshake.
    #[error("protocol error: {0}")]
    Protocol(String),

    /// Host and plugin share no common protocol version. The UI
    /// should tell the user to update one side or the other.
    #[error("no common protocol version (host {host:?}, plugin {plugin:?})")]
    VersionMismatch { host: Vec<u32>, plugin: Vec<u32> },

    /// The call reached the provider and the provider returned a
    /// `CloudError`. Carried through verbatim from the wire.
    #[error("provider error: {0}")]
    Provider(#[from] CloudError),

    /// The manifest JSON was missing, unreachable, or malformed, or
    /// carried no version compatible with this app build.
    #[error("manifest error: {0}")]
    Manifest(String),

    /// The binary download failed (HTTP error, connection dropped).
    #[error("download failed: {0}")]
    Download(String),

    /// SHA-256 mismatch or Ed25519 signature rejection on a
    /// downloaded binary.
    #[error("integrity check failed: {0}")]
    Integrity(String),

    /// Filesystem error working with the cache directory.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

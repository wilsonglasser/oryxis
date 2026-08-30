//! Plugin subsystem.
//!
//! Plugins (GIF export, MCP, ...) ship as standalone binaries
//! speaking line-delimited JSON-RPC 2.0 over stdio (the contract
//! lives in `oryxis-plugin-protocol`). The app performs no network
//! fetches: whatever sits in the local cache (or next to the app
//! executable as a dev build) is what runs.
//!
//! Module map:
//!
//! - [`manifest`]: parse the manifest JSON, filter by protocol +
//!   `min_app`, pick the best version.
//! - [`verify`]: Ed25519 verify-only over cached binaries.
//! - [`cache`]: on-disk layout under `~/.oryxis/plugins/`, keep the
//!   last two versions per provider.

pub mod cache;
pub mod manifest;
pub mod verify;

pub use manifest::{ManifestEntry, PlatformBinary, PluginManifest};

use std::path::PathBuf;

/// Unified error for every step of the plugin lifecycle, spawn,
/// JSON-RPC, manifest parsing, download, integrity. Kept as one enum
/// (rather than one per submodule) so call sites match on a single
/// type.
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

    /// The manifest JSON was missing, unreachable, or malformed, or
    /// carried no version compatible with this app build.
    #[error("manifest error: {0}")]
    Manifest(String),

    /// SHA-256 mismatch or Ed25519 signature rejection on a
    /// cached binary.
    #[error("integrity check failed: {0}")]
    Integrity(String),

    /// Filesystem error working with the cache directory.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl PluginError {
    /// Stable i18n key for showing this error in the UI. The raw
    /// `Display` text is detailed (file paths, byte counts, HTTP
    /// codes, signature bytes) and goes through `tracing` for the
    /// log file; what the user sees is a short translated phrase
    /// per variant. Keep these aligned with the keys defined in
    /// `crate::i18n` for every language.
    pub fn i18n_key(&self) -> &'static str {
        match self {
            Self::BinaryNotFound(_) => "plugin_err_binary_not_found",
            Self::Spawn(_) => "plugin_err_spawn",
            Self::ProcessGone => "plugin_err_process_gone",
            Self::Timeout(_) => "plugin_err_timeout",
            Self::Protocol(_) => "plugin_err_protocol",
            Self::VersionMismatch { .. } => "plugin_err_version_mismatch",
            Self::Manifest(_) => "plugin_err_manifest",
            Self::Integrity(_) => "plugin_err_integrity",
            Self::Io(_) => "plugin_err_io",
        }
    }
}

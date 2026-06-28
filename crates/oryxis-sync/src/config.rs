use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SyncMode {
    Auto,
    #[default]
    Manual,
}

/// How a device exchanges sync state with the rest of its group. This is
/// the **transport** axis and is orthogonal to [`SyncMode`] (the
/// **cadence**): either transport can run Auto or Manual. A device is on
/// one transport at a time, the two don't bridge (a `P2p` device and an
/// `Sftp` device never meet).
///
/// - `P2p` negotiates a delta over a live QUIC/relay session with paired
///   peers (mDNS + signaling + relay).
/// - `Sftp` reconciles against a single encrypted snapshot file on an
///   SFTP host that all group members read/modify/write. The "group" is
///   defined by sharing the same file and the same passphrase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SyncTransport {
    #[default]
    P2p,
    Sftp,
}

#[derive(Debug, Clone)]
pub struct SyncConfig {
    pub enabled: bool,
    pub mode: SyncMode,
    pub relay_url: Option<String>,
    /// Cloudflare Workers signaling endpoint. `Some` enables the
    /// periodic STUN + register heartbeat and the signaling lookup
    /// branch of cross-network pairing; `None` keeps sync LAN-only.
    pub signaling_url: Option<String>,
    /// Bearer token for the signaling endpoint. Required when
    /// `signaling_url` is `Some`; ignored when it's `None`.
    pub signaling_token: Option<String>,
    pub listen_port: u16,
    pub auto_interval_secs: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        // `option_env!` is the non-panicking counterpart to `env!`: a
        // release build that sets the env vars ships the hosted Worker
        // URL out of the box; a dev / fork build without them just
        // returns `None` and the engine starts LAN-only.
        Self {
            enabled: false,
            mode: SyncMode::Manual,
            relay_url: None,
            signaling_url: option_env!("ORYXIS_SIGNALING_URL").map(str::to_string),
            signaling_token: option_env!("ORYXIS_SIGNALING_TOKEN").map(str::to_string),
            listen_port: 0,
            auto_interval_secs: 300,
        }
    }
}

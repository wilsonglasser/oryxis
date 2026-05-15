use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SyncMode {
    Auto,
    #[default]
    Manual,
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

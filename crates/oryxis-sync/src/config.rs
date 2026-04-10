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
    pub signaling_url: String,
    pub signaling_token: String,
    pub listen_port: u16,
    pub auto_interval_secs: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: SyncMode::Manual,
            relay_url: None,
            signaling_url: env!("ORYXIS_SIGNALING_URL").into(),
            signaling_token: env!("ORYXIS_SIGNALING_TOKEN").into(),
            listen_port: 0,
            auto_interval_secs: 300,
        }
    }
}

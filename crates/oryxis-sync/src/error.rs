use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    #[error("Sync failed: {0}")]
    SyncFailed(String),

    #[error("Transport error: {0}")]
    Transport(String),

    /// The relay endpoint reported a permanent condition (HTTP 404, 410,
    /// 501, ...) that retrying can't recover from: wrong URL, deprecated
    /// API version, recipient slot deleted server-side, etc. Callers
    /// should stop polling instead of looping forever burning network +
    /// battery, and log enough context for the user to know why sync
    /// isn't connecting through the relay anymore.
    #[error("Relay unavailable: {0}")]
    RelayUnavailable(String),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("Pairing failed: {0}")]
    PairingFailed(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Discovery error: {0}")]
    Discovery(String),

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("Timeout")]
    Timeout,
}

impl From<oryxis_vault::VaultError> for SyncError {
    fn from(e: oryxis_vault::VaultError) -> Self {
        SyncError::Vault(e.to_string())
    }
}

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    #[error("Sync failed: {0}")]
    SyncFailed(String),
}

/// P2P sync engine — will wrap iroh for decentralized folder sync.
pub struct SyncEngine;

impl Default for SyncEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncEngine {
    pub fn new() -> Self {
        Self
    }
}

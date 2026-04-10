use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Information about a paired peer device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPeer {
    pub peer_id: Uuid,
    pub device_name: String,
    pub public_key: Vec<u8>,
    pub shared_secret: Option<Vec<u8>>,
    pub last_known_ip: Option<String>,
    pub last_known_port: Option<u16>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub paired_at: DateTime<Utc>,
    pub is_active: bool,
}

/// Runtime status of a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerStatus {
    Online,
    Offline,
    Syncing,
    Error(String),
}

/// Peer info for UI display.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub peer_id: Uuid,
    pub device_name: String,
    pub status: PeerStatus,
    pub last_synced_at: Option<DateTime<Utc>>,
}

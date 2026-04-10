use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Protocol version for wire compatibility.
pub const PROTOCOL_VERSION: u32 = 1;

/// Entity types that can be synced.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EntityType {
    Connection,
    SshKey,
    Identity,
    Group,
    Snippet,
    KnownHost,
}

impl std::fmt::Display for EntityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connection => write!(f, "connection"),
            Self::SshKey => write!(f, "key"),
            Self::Identity => write!(f, "identity"),
            Self::Group => write!(f, "group"),
            Self::Snippet => write!(f, "snippet"),
            Self::KnownHost => write!(f, "known_host"),
        }
    }
}

/// Messages exchanged over QUIC streams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMessage {
    // Handshake
    Hello {
        device_id: Uuid,
        protocol_version: u32,
    },
    HelloAck {
        device_id: Uuid,
        protocol_version: u32,
    },

    // Pairing (first connection only)
    PairingRequest {
        device_name: String,
        public_key: Vec<u8>,
        pairing_code: String,
    },
    PairingAccepted {
        device_name: String,
        public_key: Vec<u8>,
    },
    PairingRejected {
        reason: String,
    },

    // Sync
    ManifestRequest,
    Manifest {
        entries: Vec<ManifestEntry>,
    },
    DeltaRequest {
        needed: Vec<DeltaRef>,
    },
    DeltaResponse {
        records: Vec<SyncRecord>,
    },
    DeltaPush {
        records: Vec<SyncRecord>,
    },
    DeltaAck {
        accepted: Vec<Uuid>,
    },

    // Housekeeping
    Ping,
    Pong,
    Bye,
}

/// A single entry in a sync manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub entity_type: EntityType,
    pub entity_id: Uuid,
    pub updated_at: DateTime<Utc>,
    pub is_deleted: bool,
}

/// Reference to a record needed from the peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaRef {
    pub entity_type: EntityType,
    pub entity_id: Uuid,
}

/// A complete record for syncing, with E2E encrypted payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRecord {
    pub entity_type: EntityType,
    pub entity_id: Uuid,
    pub updated_at: DateTime<Utc>,
    pub is_deleted: bool,
    /// E2E encrypted JSON payload (encrypted with shared secret).
    pub payload: Vec<u8>,
}

/// Frame header for length-prefixed messages over QUIC streams.
/// Format: [length: 4 bytes LE] [bincode data]
pub fn encode_message(msg: &SyncMessage) -> Result<Vec<u8>, bincode::Error> {
    let data = bincode::serialize(msg)?;
    let len = (data.len() as u32).to_le_bytes();
    let mut frame = Vec::with_capacity(4 + data.len());
    frame.extend_from_slice(&len);
    frame.extend_from_slice(&data);
    Ok(frame)
}

pub fn decode_message(data: &[u8]) -> Result<SyncMessage, bincode::Error> {
    bincode::deserialize(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let msg = SyncMessage::Hello {
            device_id: Uuid::new_v4(),
            protocol_version: PROTOCOL_VERSION,
        };
        let encoded = encode_message(&msg).unwrap();
        let len = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]) as usize;
        let decoded = decode_message(&encoded[4..4 + len]).unwrap();
        match decoded {
            SyncMessage::Hello { protocol_version, .. } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn manifest_entry_serialization() {
        let entry = ManifestEntry {
            entity_type: EntityType::Connection,
            entity_id: Uuid::new_v4(),
            updated_at: Utc::now(),
            is_deleted: false,
        };
        let msg = SyncMessage::Manifest {
            entries: vec![entry],
        };
        let encoded = encode_message(&msg).unwrap();
        let len = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]) as usize;
        let decoded = decode_message(&encoded[4..4 + len]).unwrap();
        match decoded {
            SyncMessage::Manifest { entries } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].entity_type, EntityType::Connection);
            }
            _ => panic!("Wrong message type"),
        }
    }
}

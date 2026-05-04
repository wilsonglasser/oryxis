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
    /// Saved proxy configurations referenced from `Connection.proxy_identity_id`.
    /// The associated password is included in the wire payload only when the
    /// peer's `sync_passwords` setting is on (off by default); older peers
    /// silently drop the extra fields.
    ProxyIdentity,
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
            Self::ProxyIdentity => write!(f, "proxy_identity"),
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

// ---------------------------------------------------------------------------
// Sync payload wrappers (transparent to wire JSON for connections /
// identities / proxy identities — the inner model is `#[serde(flatten)]`
// so older nodes that send a bare `Connection` still deserialize, and
// older nodes that receive these wrappers ignore the extra password
// fields. Passwords are only ever included when the local
// `sync_passwords` setting is on.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConnection {
    #[serde(flatten)]
    pub connection: oryxis_core::models::Connection,
    /// Main connection password — sent when `sync_passwords` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Inline-proxy password (separate encrypted column on disk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncIdentity {
    #[serde(flatten)]
    pub identity: oryxis_core::models::Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProxyIdentity {
    #[serde(flatten)]
    pub proxy_identity: oryxis_core::models::ProxyIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
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

    /// New `SyncConnection` wrappers must accept old-format payloads
    /// (bare `Connection` JSON) without losing fields. The optional
    /// password fields default to `None`.
    #[test]
    fn sync_connection_accepts_legacy_payload() {
        let conn = oryxis_core::models::Connection::new("legacy", "10.0.0.1");
        let bare = serde_json::to_vec(&conn).unwrap();
        let wrapped: SyncConnection = serde_json::from_slice(&bare).unwrap();
        assert_eq!(wrapped.connection.label, "legacy");
        assert!(wrapped.password.is_none());
        assert!(wrapped.proxy_password.is_none());
    }

    #[test]
    fn sync_connection_round_trip_with_passwords() {
        let conn = oryxis_core::models::Connection::new("modern", "10.0.0.1");
        let wrapper = SyncConnection {
            connection: conn,
            password: Some("conn-pw".into()),
            proxy_password: Some("proxy-pw".into()),
        };
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        let back: SyncConnection = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.password.as_deref(), Some("conn-pw"));
        assert_eq!(back.proxy_password.as_deref(), Some("proxy-pw"));
    }

    /// When no password is set we must NOT emit empty fields — keeps
    /// the wire payload byte-identical to the legacy format so older
    /// receivers don't see noise.
    #[test]
    fn sync_connection_omits_password_when_none() {
        let conn = oryxis_core::models::Connection::new("no-pw", "10.0.0.1");
        let wrapper = SyncConnection {
            connection: conn,
            password: None,
            proxy_password: None,
        };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert!(
            !json.contains("\"password\""),
            "password field leaked into JSON: {json}"
        );
        assert!(
            !json.contains("\"proxy_password\""),
            "proxy_password field leaked into JSON: {json}"
        );
    }

    #[test]
    fn sync_identity_round_trip() {
        let ident = oryxis_core::models::Identity::new("ident");
        let wrapper = SyncIdentity {
            identity: ident,
            password: Some("ident-pw".into()),
        };
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        let back: SyncIdentity = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.password.as_deref(), Some("ident-pw"));
    }

    #[test]
    fn sync_proxy_identity_round_trip() {
        let pi = oryxis_core::models::ProxyIdentity::new("pi");
        let wrapper = SyncProxyIdentity {
            proxy_identity: pi,
            password: Some("pi-pw".into()),
        };
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        let back: SyncProxyIdentity = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.password.as_deref(), Some("pi-pw"));
    }
}

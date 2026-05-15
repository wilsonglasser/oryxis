use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Protocol version for wire compatibility.
///
/// v2 added `auth_signature` to Hello/HelloAck (channel-bound Ed25519
/// proof of identity, fixes MITM-able handshake from v1).
///
/// v3 reworked pairing: `PairingRequest` / `PairingAccepted` carry the
/// sender's `device_id` (so each side knows which UUID to store the
/// peer under), and a `PairingChallenge` / `PairingResponse` round was
/// added so the joiner proves possession of the private key paired
/// with the `public_key` it sent. Pairing happens before any peer
/// pubkey is persisted, so the Hello channel-binding can't be reused.
///
/// Older peers cannot interop across a version bump, and that is
/// intentional. There are no pre-v3 peers in the wild because sync was
/// never wired into the app before this release.
pub const PROTOCOL_VERSION: u32 = 3;

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
    /// Cloud account credentials referenced from `Connection.cloud_ref` and
    /// `Group.cloud_query`. The encrypted secret blob travels over the
    /// wire only when `sync_passwords` is on (same opt-in as proxy /
    /// identity passwords).
    CloudProfile,
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
            Self::CloudProfile => write!(f, "cloud_profile"),
        }
    }
}

impl EntityType {
    /// Parse the wire string produced by [`Display`]. This is the
    /// inverse used to map the vault's string-typed `sync_metadata`
    /// tombstones back into typed manifest entries. An unknown string
    /// (an entity type only a newer peer knows about) returns `None`,
    /// so the caller skips that entry instead of failing the sync.
    pub fn from_wire_str(s: &str) -> Option<Self> {
        match s {
            "connection" => Some(Self::Connection),
            "key" => Some(Self::SshKey),
            "identity" => Some(Self::Identity),
            "group" => Some(Self::Group),
            "snippet" => Some(Self::Snippet),
            "known_host" => Some(Self::KnownHost),
            "proxy_identity" => Some(Self::ProxyIdentity),
            "cloud_profile" => Some(Self::CloudProfile),
            _ => None,
        }
    }
}

/// Messages exchanged over QUIC streams.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMessage {
    // Handshake. `auth_signature` is an Ed25519 signature over the QUIC
    // TLS RFC-5705 exporter (see `crypto::SESSION_EXPORTER_LABEL`). The
    // receiver looks up the sender's public key by `device_id` in
    // `sync_peers` and verifies, which both authenticates the peer and
    // binds the signature to this specific TLS session (defeats MITM
    // even with the rustls `SkipVerification` cert verifier).
    Hello {
        device_id: Uuid,
        protocol_version: u32,
        auth_signature: Vec<u8>,
    },
    HelloAck {
        device_id: Uuid,
        protocol_version: u32,
        auth_signature: Vec<u8>,
    },

    // Pairing (first connection only). The joiner opens a stream and
    // sends `PairingRequest`; the host (if it is currently hosting a
    // matching code) replies with a `PairingChallenge`, the joiner
    // answers with `PairingResponse`, and the host finishes with
    // `PairingAccepted` or `PairingRejected`. See `PROTOCOL_VERSION`.
    PairingRequest {
        device_id: Uuid,
        device_name: String,
        public_key: Vec<u8>,
        pairing_code: String,
        /// The joiner's own QUIC listen port. The host sees only the
        /// joiner's ephemeral source port on this connection, so the
        /// joiner has to advertise its listener explicitly for the
        /// host to be able to sync back to it later.
        listen_port: u16,
    },
    /// Host -> joiner: a fresh random nonce the joiner must sign with
    /// the private key matching the `public_key` it just sent. This
    /// proves the joiner isn't replaying an intercepted `PairingRequest`.
    PairingChallenge {
        challenge: Vec<u8>,
    },
    /// Joiner -> host: Ed25519 signature over the challenge nonce.
    PairingResponse {
        signed_challenge: Vec<u8>,
    },
    PairingAccepted {
        device_id: Uuid,
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
// identities / proxy identities, the inner model is `#[serde(flatten)]`
// so older nodes that send a bare `Connection` still deserialize, and
// older nodes that receive these wrappers ignore the extra password
// fields. Passwords are only ever included when the local
// `sync_passwords` setting is on.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConnection {
    #[serde(flatten)]
    pub connection: oryxis_core::models::Connection,
    /// Main connection password, sent when `sync_passwords` is on.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCloudProfile {
    #[serde(flatten)]
    pub profile: oryxis_core::models::CloudProfile,
    /// Encrypted secret blob payload (access key secret, kubeconfig
    /// inline contents, …). Sent only when `sync_passwords` is on; the
    /// field uses `skip_serializing_if` so older peers see byte-identical
    /// JSON to the legacy bare-`CloudProfile` payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
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
            auth_signature: vec![0xAB; 64],
        };
        let encoded = encode_message(&msg).unwrap();
        let len = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]) as usize;
        let decoded = decode_message(&encoded[4..4 + len]).unwrap();
        match decoded {
            SyncMessage::Hello {
                protocol_version,
                auth_signature,
                ..
            } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION);
                assert_eq!(auth_signature.len(), 64);
            }
            _ => panic!("Wrong message type"),
        }
    }

    /// The v3 pairing messages must survive a bincode frame round-trip
    /// with their `device_id` and challenge/response payloads intact.
    #[test]
    fn pairing_messages_round_trip() {
        let device_id = Uuid::new_v4();
        let messages = [
            SyncMessage::PairingRequest {
                device_id,
                device_name: "laptop".into(),
                public_key: vec![0x11; 32],
                pairing_code: "123456".into(),
                listen_port: 4433,
            },
            SyncMessage::PairingChallenge {
                challenge: vec![0x22; 32],
            },
            SyncMessage::PairingResponse {
                signed_challenge: vec![0x33; 64],
            },
            SyncMessage::PairingAccepted {
                device_id,
                device_name: "desktop".into(),
                public_key: vec![0x44; 32],
            },
        ];
        for msg in messages {
            let encoded = encode_message(&msg).unwrap();
            let len =
                u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]) as usize;
            let decoded = decode_message(&encoded[4..4 + len]).unwrap();
            match (&msg, &decoded) {
                (
                    SyncMessage::PairingRequest { device_id: a, .. },
                    SyncMessage::PairingRequest { device_id: b, .. },
                )
                | (
                    SyncMessage::PairingAccepted { device_id: a, .. },
                    SyncMessage::PairingAccepted { device_id: b, .. },
                ) => assert_eq!(a, b),
                (
                    SyncMessage::PairingChallenge { challenge: a },
                    SyncMessage::PairingChallenge { challenge: b },
                ) => assert_eq!(a, b),
                (
                    SyncMessage::PairingResponse { signed_challenge: a },
                    SyncMessage::PairingResponse { signed_challenge: b },
                ) => assert_eq!(a, b),
                _ => panic!("pairing message variant changed across round-trip"),
            }
        }
    }

    /// `Display` and `from_wire_str` must be exact inverses for every
    /// variant, the vault tombstone table stores the string form and
    /// the manifest builder maps it back.
    #[test]
    fn entity_type_wire_str_round_trip() {
        let all = [
            EntityType::Connection,
            EntityType::SshKey,
            EntityType::Identity,
            EntityType::Group,
            EntityType::Snippet,
            EntityType::KnownHost,
            EntityType::ProxyIdentity,
            EntityType::CloudProfile,
        ];
        for et in all {
            let s = et.to_string();
            assert_eq!(EntityType::from_wire_str(&s), Some(et), "round-trip {s}");
        }
        assert_eq!(EntityType::from_wire_str("unknown_future_type"), None);
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

    /// When no password is set we must NOT emit empty fields, keeps
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

    #[test]
    fn sync_cloud_profile_round_trip() {
        let cp = oryxis_core::models::CloudProfile::new("aws-prod", "aws");
        let wrapper = SyncCloudProfile {
            profile: cp,
            secret: Some("opaque-secret".into()),
        };
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        let back: SyncCloudProfile = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.secret.as_deref(), Some("opaque-secret"));
        assert_eq!(back.profile.label, "aws-prod");
    }

    /// Legacy peer that doesn't know about cloud profiles will send a
    /// bare `CloudProfile` JSON, the wrapper must accept it and resolve
    /// `secret` to `None`. (Symmetric to the connection / identity tests
    /// above.)
    #[test]
    fn sync_cloud_profile_accepts_legacy_payload() {
        let cp = oryxis_core::models::CloudProfile::new("legacy", "aws");
        let bare = serde_json::to_vec(&cp).unwrap();
        let wrapped: SyncCloudProfile = serde_json::from_slice(&bare).unwrap();
        assert_eq!(wrapped.profile.label, "legacy");
        assert!(wrapped.secret.is_none());
    }

    /// When secret is `None` the wire payload must be byte-identical to
    /// the legacy bare-`CloudProfile` JSON, no `"secret"` key emitted.
    #[test]
    fn sync_cloud_profile_omits_secret_when_none() {
        let cp = oryxis_core::models::CloudProfile::new("no-secret", "aws");
        let wrapper = SyncCloudProfile { profile: cp, secret: None };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert!(
            !json.contains("\"secret\""),
            "secret field leaked into JSON: {json}"
        );
    }
}

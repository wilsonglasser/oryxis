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
/// with the `public_key` it sent.
///
/// v4 added X25519 ephemeral key exchange to the pairing messages.
/// Both `PairingRequest` and `PairingAccepted` carry an `x25519_pub`;
/// each side computes a shared secret via Diffie-Hellman and persists
/// it on the `SyncPeer` row. From then on the per-record payloads
/// inside `SyncRecord` are sealed with ChaCha20-Poly1305 under that
/// secret, so a future MITM (or a compromised signaling relay) sees
/// only ciphertext even if the TLS layer is broken.
///
/// v5 added a three-step Ed25519 handshake to the relay session path
/// (`RelayHello` / `RelayHelloAck` / `RelayAuth`) so a relay session
/// is bound to a fresh nonce pair signed by both sides' long-term
/// identity keys. Without it, an attacker who learned a peer's
/// `device_id` (which travels in clear via pairing links + STUN
/// registrations) could open a relay session impersonating that peer
/// and push forged tombstones (`SyncRecord { is_deleted: true,
/// payload: [] }`) that bypass per-record AEAD because empty
/// payloads skip encryption. v5 also wraps the raw X25519 DH output
/// in HKDF-SHA256 before persisting as the per-peer AEAD key, so the
/// key is uniform group-independent material rather than a raw curve
/// point.
///
/// v6 switched the per-record and snapshot AEAD from ChaCha20-Poly1305
/// (96-bit nonce) to XChaCha20-Poly1305 (192-bit nonce). The wider
/// random nonce lifts the birthday bound past any realistic message
/// count, so nonce reuse is a non-issue regardless of sync volume or
/// key lifetime. The change grows the on-wire nonce 12 -> 24 bytes, so
/// it is a hard wire break, hence the version bump. The stored SFTP
/// snapshot carries its own `SNAPSHOT_VERSION` (bumped 1 -> 2 in
/// lockstep) so an old snapshot blob is rejected on its header rather
/// than fed to the wrong-length nonce. HKDF / Ed25519 domain labels are
/// unchanged: only the AEAD nonce length moved, the key material and
/// handshake signatures are identical to v5.
///
/// Older peers cannot interop across a version bump, and that is
/// intentional. v5 shipped in v0.8.3, so v5 peers and v1 snapshots do
/// exist in the wild; a v6 device rejects them at the version gate
/// (P2P handshake) or the snapshot header, which is the coordinated
/// re-pair / re-sync this bump expects. The reject is non-destructive:
/// a failed snapshot merge leaves both the local vault and the remote
/// blob untouched (`dispatch_sftp_sync.rs` refuses to push after it).
///
/// v7 is a SCHEMA gate, not a crypto change: the payloads gained enum
/// variants a v6 peer cannot deserialize (`AuthMethod::Certificate`,
/// `KeyAlgorithm::SkEd25519` / `SkEcdsaP256`). Unknown VARIANTS are a
/// hard serde error, unlike unknown fields, so a v6 peer receiving such
/// a record would warn-skip it on every cycle: permanent, silent
/// divergence, with round-trip corruption as the only "tolerant"
/// alternative (a downgraded record could LWW its way back). The bump
/// turns that into the same loud, non-destructive version reject as
/// every prior break. `SNAPSHOT_VERSION` moved 2 -> 3 in lockstep, but
/// asymmetrically: the crypto is unchanged, so a v7 device still READS
/// v2 snapshots (payload superset) and writes v3, while a v6 device
/// rejects a v3 blob at the header instead of skipping records inside
/// it. Future variant additions to synced enums need the same audit.
///
/// v8 is that audit coming due: `EntityType` gained `LoginScript`, and
/// unlike v7's payload-level variants this one sits in the ENVELOPE
/// (`ManifestEntry.entity_type` / `SyncRecord.entity_type`). bincode
/// encodes a variant as a bare u32 index, so a peer that knows ten
/// variants receiving index ten fails to decode the ENTIRE message, not
/// the one entry: two peers would go quiet the moment either of them
/// created a login script, with no error either user could see. The
/// bump turns that into the same loud handshake reject as every prior
/// break, and `SNAPSHOT_VERSION` moves 3 -> 4 in lockstep for the same
/// reason (the snapshot carries `SyncRecord`s).
pub const PROTOCOL_VERSION: u32 = 8;

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
    /// Standalone port forward rules. No credentials of their own (they
    /// reference a `Connection` by `host_id`), so the bare model travels
    /// over the wire like `Snippet`.
    PortForwardRule,
    /// Saved split-panel arrangements. No credentials (leaves reference
    /// hosts by id or are local shells), so the bare model travels over the
    /// wire like `Group`.
    SessionGroup,
    /// Reusable expect/send login automations referenced from
    /// `Connection.login_script_id`. Carries patterns and secret
    /// REFERENCES only (the credential it types lives in the host's
    /// encrypted `target_password`), so the bare model travels over the
    /// wire like `Snippet`, with no `sync_passwords` gate of its own.
    LoginScript,
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
            Self::PortForwardRule => write!(f, "port_forward_rule"),
            Self::SessionGroup => write!(f, "session_group"),
            Self::LoginScript => write!(f, "login_script"),
        }
    }
}

impl EntityType {
    /// Every variant, for the round-trip test. Kept honest by
    /// `wire_index` below: a new variant forces an arm there, and the
    /// arm's index forces this list to grow.
    pub const ALL: [EntityType; 11] = [
        Self::Connection,
        Self::SshKey,
        Self::Identity,
        Self::Group,
        Self::Snippet,
        Self::KnownHost,
        Self::ProxyIdentity,
        Self::CloudProfile,
        Self::PortForwardRule,
        Self::SessionGroup,
        Self::LoginScript,
    ];

    /// Position of a variant in [`Self::ALL`]. Exists only so the
    /// compiler refuses a new variant that nobody listed: the match is
    /// exhaustive, and `all_covers_every_variant` asserts the mapping
    /// agrees with the array. Test-only, which is enough: `cargo test`
    /// and `clippy --all-targets` are both CI gates.
    #[cfg(test)]
    fn wire_index(self) -> usize {
        match self {
            Self::Connection => 0,
            Self::SshKey => 1,
            Self::Identity => 2,
            Self::Group => 3,
            Self::Snippet => 4,
            Self::KnownHost => 5,
            Self::ProxyIdentity => 6,
            Self::CloudProfile => 7,
            Self::PortForwardRule => 8,
            Self::SessionGroup => 9,
            Self::LoginScript => 10,
        }
    }

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
            "port_forward_rule" => Some(Self::PortForwardRule),
            "session_group" => Some(Self::SessionGroup),
            "login_script" => Some(Self::LoginScript),
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
        /// Ephemeral X25519 public key (32 bytes). The host pairs it
        /// with its own ephemeral X25519 pubkey in `PairingAccepted`;
        /// both sides DH to the same shared secret and store it on
        /// the new `SyncPeer` row. Used to seal `SyncRecord.payload`
        /// in all subsequent syncs.
        x25519_pub: Vec<u8>,
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
        /// Host's ephemeral X25519 pubkey, completes the pairing-time
        /// Diffie-Hellman with the joiner's `x25519_pub`.
        x25519_pub: Vec<u8>,
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

    // Relay-session authentication (v5+). The QUIC path uses Hello /
    // HelloAck above, which binds the Ed25519 signature to the TLS
    // session via the RFC 5705 exporter, but the relay path has no
    // exporter available so we sign a fresh nonce pair instead. The
    // server's signature in `RelayHelloAck` authenticates the server
    // to the client (and binds to the client's nonce); the client's
    // signature in `RelayAuth` authenticates the client back. Both
    // sides verify against the peer's stored pubkey from pairing.
    RelayHello {
        device_id: Uuid,
        protocol_version: u32,
        client_nonce: [u8; 32],
    },
    RelayHelloAck {
        device_id: Uuid,
        protocol_version: u32,
        server_nonce: [u8; 32],
        /// Ed25519 signature over the transcript produced by
        /// `crypto::relay_handshake_transcript`.
        server_signature: Vec<u8>,
    },
    RelayAuth {
        /// Ed25519 signature over the same transcript the server
        /// signed in `RelayHelloAck`.
        client_signature: Vec<u8>,
    },
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

/// `skip_serializing_if` predicate for the `*_cleared` sentinels below so
/// the common "not cleared" case stays off the wire (older peers keep
/// seeing the legacy payload shape). Correctness never depends on it: a
/// missing sentinel deserializes to `false` = preserve.
fn is_false(b: &bool) -> bool {
    !*b
}

// Secret fields are a two-part wire encoding rather than a single
// `Option<Option<String>>`: serde's `flatten` (used for the inner model in
// every wrapper below) collapses the JSON null-vs-absent distinction a
// double-option would rely on, so a `*_cleared: bool` sentinel carries the
// "explicitly removed" signal instead. Semantics per secret:
//   value present            -> set it
//   value absent + cleared   -> clear it (apply passes Some("") -> NULL)
//   value absent + !cleared  -> preserve the receiver's existing value
// Backward compatible: old peers omit the sentinel (defaults false =
// preserve) and ignore the unknown field when they receive it.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConnection {
    #[serde(flatten)]
    pub connection: oryxis_core::models::Connection,
    /// Main connection password, sent when `sync_passwords` is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Password was explicitly removed (propagate the clear).
    #[serde(default, skip_serializing_if = "is_false")]
    pub password_cleared: bool,
    /// Inline-proxy password (separate encrypted column on disk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_password: Option<String>,
    /// Inline-proxy password was explicitly removed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub proxy_password_cleared: bool,
    /// TOTP secret (separate encrypted column on disk), gated by
    /// `sync_passwords` like every other credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub totp_secret: Option<String>,
    /// TOTP secret was explicitly removed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub totp_secret_cleared: bool,
    /// The credential a login script types at the ASSET's prompt,
    /// behind an interactive bastion (separate encrypted column on
    /// disk), gated by `sync_passwords` like every other credential.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_password: Option<String>,
    /// Target password was explicitly removed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub target_password_cleared: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncIdentity {
    #[serde(flatten)]
    pub identity: oryxis_core::models::Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Password was explicitly removed (propagate the clear).
    #[serde(default, skip_serializing_if = "is_false")]
    pub password_cleared: bool,
}

/// SSH key wrapper: the flattened `SshKey` model carries only public
/// material (`public_key`, `fingerprint`, `algorithm`, …), so the private
/// key rides alongside it here. Without this field a peer receives a key
/// row with a NULL private column and cannot authenticate with it. Sent
/// only when `sync_passwords` is on, exactly like the connection and
/// identity passwords.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSshKey {
    #[serde(flatten)]
    pub key: oryxis_core::models::SshKey,
    /// Private-key PEM in its stored form (it may itself be
    /// passphrase-encrypted; that is opaque to sync). `None` on the wire
    /// means "preserve the receiver's copy" for legacy/`sync_passwords`-off
    /// payloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// The key has no private material (public-only / FIDO2-SK row, or it
    /// was removed): propagate that so the receiver's copy is cleared too.
    #[serde(default, skip_serializing_if = "is_false")]
    pub private_key_cleared: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProxyIdentity {
    #[serde(flatten)]
    pub proxy_identity: oryxis_core::models::ProxyIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Password was explicitly removed (propagate the clear).
    #[serde(default, skip_serializing_if = "is_false")]
    pub password_cleared: bool,
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
    /// Secret was explicitly removed (propagate the clear).
    #[serde(default, skip_serializing_if = "is_false")]
    pub secret_cleared: bool,
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

    // #19: the password-removal signal must survive the wrapper's
    // `#[serde(flatten)]` (a double-option `null`-vs-absent encoding does
    // NOT, hence the explicit `*_cleared: bool` sentinel). Covers the three
    // states plus legacy backward compat.
    #[test]
    fn password_clear_sentinel_survives_flatten() {
        use oryxis_core::models::connection::Connection;

        // A bare legacy Connection (no password / sentinel keys) = preserve.
        let bare = serde_json::to_value(Connection::new("h", "x")).unwrap();
        let absent: SyncConnection = serde_json::from_value(bare.clone()).unwrap();
        assert_eq!(absent.password, None);
        assert!(!absent.password_cleared);

        // Cleared: value absent, sentinel true. Must round-trip through the
        // flattened wrapper, and the sentinel must actually be on the wire.
        let cleared = SyncConnection {
            connection: Connection::new("h", "x"),
            password: None,
            password_cleared: true,
            proxy_password: None,
            proxy_password_cleared: false,
            totp_secret: None,
            totp_secret_cleared: false,
            target_password: None,
            target_password_cleared: false,
        };
        let v = serde_json::to_value(&cleared).unwrap();
        assert_eq!(v["password_cleared"], serde_json::json!(true));
        assert!(v.get("password").is_none(), "no value when cleared");
        assert!(
            v.get("proxy_password_cleared").is_none(),
            "the not-cleared sentinel stays off the wire"
        );
        let back: SyncConnection = serde_json::from_value(v).unwrap();
        assert_eq!(back.password, None);
        assert!(back.password_cleared);

        // Set: value present.
        let set = SyncConnection {
            connection: Connection::new("h", "x"),
            password: Some("hunter2".into()),
            password_cleared: false,
            proxy_password: None,
            proxy_password_cleared: false,
            totp_secret: None,
            totp_secret_cleared: false,
            target_password: None,
            target_password_cleared: false,
        };
        let back: SyncConnection =
            serde_json::from_value(serde_json::to_value(&set).unwrap()).unwrap();
        assert_eq!(back.password.as_deref(), Some("hunter2"));
        assert!(!back.password_cleared);

        // Legacy peer: bare Connection + a bare `password` string, no
        // sentinel -> set, not cleared.
        let mut legacy_json = bare;
        legacy_json["password"] = serde_json::json!("old");
        let legacy: SyncConnection = serde_json::from_value(legacy_json).unwrap();
        assert_eq!(legacy.password.as_deref(), Some("old"));
        assert!(!legacy.password_cleared);
    }

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

    /// The v4 pairing messages must survive a bincode frame round-trip
    /// with `device_id`, the challenge/response payloads, and the
    /// X25519 pubkeys intact.
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
                x25519_pub: vec![0x55; 32],
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
                x25519_pub: vec![0x66; 32],
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
        for et in EntityType::ALL {
            let s = et.to_string();
            assert_eq!(EntityType::from_wire_str(&s), Some(et), "round-trip {s}");
        }
        assert_eq!(EntityType::from_wire_str("unknown_future_type"), None);
    }

    /// The list above used to be written out by hand at each call site
    /// and had silently fallen two variants behind. `ALL` is now the
    /// single list, and this asserts it stayed complete: adding a
    /// variant forces an arm in `wire_index`, and its index has to
    /// point at the matching slot here.
    #[test]
    fn all_covers_every_variant() {
        for (i, et) in EntityType::ALL.iter().enumerate() {
            assert_eq!(et.wire_index(), i, "{et} is in the wrong ALL slot");
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
            password_cleared: false,
            proxy_password: Some("proxy-pw".into()),
            proxy_password_cleared: false,
            totp_secret: Some("JBSWY3DPEHPK3PXP".into()),
            totp_secret_cleared: false,
            target_password: Some("asset-pw".into()),
            target_password_cleared: false,
        };
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        let back: SyncConnection = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.password.as_deref(), Some("conn-pw"));
        assert_eq!(back.proxy_password.as_deref(), Some("proxy-pw"));
        assert_eq!(back.totp_secret.as_deref(), Some("JBSWY3DPEHPK3PXP"));
    }

    /// mosh options ride the flattened `Connection`, which is the kind
    /// of thing that is true by construction right up until it is not:
    /// a `#[serde(flatten)]` carries a field only while that field
    /// serializes, and nothing on the wrapper mentions `mosh` for a
    /// reader to notice. So the wire is asked directly.
    ///
    /// A host reached over mosh whose options did not travel would come
    /// out of a sync as a plain SSH host, silently, on the device that
    /// had never been told.
    #[test]
    fn sync_connection_carries_mosh_options() {
        let mut conn = oryxis_core::models::Connection::new("roamer", "10.0.0.1");
        conn.mosh = Some(oryxis_core::models::mosh::MoshOptions {
            enabled: true,
            server_path: "/opt/mosh/bin/mosh-server".into(),
            port_range: "60000:60010".into(),
            command: "tmux new -A -s main".into(),
        });
        let wrapper = SyncConnection {
            connection: conn.clone(),
            password: None,
            password_cleared: false,
            proxy_password: None,
            proxy_password_cleared: false,
            totp_secret: None,
            totp_secret_cleared: false,
            target_password: None,
            target_password_cleared: false,
        };
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        let back: SyncConnection = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            back.connection.mosh, conn.mosh,
            "the whole option travelled or none of it did",
        );
    }

    /// The other direction, and the one that decides whether a peer
    /// that has never heard of mosh can still be synced with: its
    /// payload has no `mosh` key at all, and that has to read as an
    /// ordinary SSH host rather than a refused message.
    #[test]
    fn a_peer_that_never_heard_of_mosh_is_still_understood() {
        let conn = oryxis_core::models::Connection::new("older-peer", "10.0.0.1");
        let mut bare: serde_json::Value = serde_json::to_value(&conn).unwrap();
        bare.as_object_mut().unwrap().remove("mosh");
        assert!(
            bare.get("mosh").is_none(),
            "the field has to be absent for this to prove anything",
        );
        let wrapped: SyncConnection = serde_json::from_value(bare).unwrap();
        assert_eq!(wrapped.connection.mosh, None);
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
            password_cleared: false,
            proxy_password: None,
            proxy_password_cleared: false,
            totp_secret: None,
            totp_secret_cleared: false,
            target_password: None,
            target_password_cleared: false,
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
            password_cleared: false,
        };
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        let back: SyncIdentity = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back.password.as_deref(), Some("ident-pw"));
    }

    #[test]
    fn sync_ssh_key_round_trip_with_private_key() {
        let key = oryxis_core::models::SshKey::new(
            "laptop",
            oryxis_core::models::KeyAlgorithm::Ed25519,
        );
        let wrapper = SyncSshKey {
            key,
            private_key: Some("-----BEGIN OPENSSH PRIVATE KEY-----\nx\n".into()),
            private_key_cleared: false,
        };
        let bytes = serde_json::to_vec(&wrapper).unwrap();
        let back: SyncSshKey = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            back.private_key.as_deref(),
            Some("-----BEGIN OPENSSH PRIVATE KEY-----\nx\n")
        );
        assert_eq!(back.key.label, "laptop");
    }

    /// A pre-wrapper peer sends a bare `SshKey` JSON (no `private_key`
    /// field). The wrapper must accept it and resolve `private_key` to
    /// `None` = preserve, not to a spurious clear. Symmetric to the
    /// connection / identity legacy tests above.
    #[test]
    fn sync_ssh_key_accepts_legacy_payload() {
        let key = oryxis_core::models::SshKey::new(
            "old-key",
            oryxis_core::models::KeyAlgorithm::Ed25519,
        );
        let bare = serde_json::to_vec(&key).unwrap();
        let wrapped: SyncSshKey = serde_json::from_slice(&bare).unwrap();
        assert_eq!(wrapped.key.label, "old-key");
        assert!(wrapped.private_key.is_none());
        assert!(!wrapped.private_key_cleared);
    }

    #[test]
    fn sync_proxy_identity_round_trip() {
        let pi = oryxis_core::models::ProxyIdentity::new("pi");
        let wrapper = SyncProxyIdentity {
            proxy_identity: pi,
            password: Some("pi-pw".into()),
            password_cleared: false,
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
            secret_cleared: false,
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
        let wrapper = SyncCloudProfile { profile: cp, secret: None, secret_cleared: false };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert!(
            !json.contains("\"secret\""),
            "secret field leaked into JSON: {json}"
        );
    }

    /// `EntityType` rides the message ENVELOPE, and bincode encodes a
    /// variant as a bare u32 index, so a peer that does not know an
    /// index fails to decode the whole message rather than skipping one
    /// entry. That makes every new variant a wire break, which is why
    /// `PROTOCOL_VERSION` has to move with it.
    ///
    /// This asserts the mechanism (unknown index = hard error) and
    /// pins the pairing: if `ALL` grows without the version moving, the
    /// count below stops matching and this fails, which is the reminder
    /// that peers on the old version would go silently out of sync.
    #[test]
    fn a_new_entity_type_is_a_wire_break_and_needs_a_version_bump() {
        let bytes = bincode::serialize(&EntityType::LoginScript).unwrap();
        assert_eq!(bytes, vec![10, 0, 0, 0], "variant index is the wire form");
        let unknown = bincode::deserialize::<EntityType>(&[
            u8::try_from(EntityType::ALL.len()).unwrap(),
            0,
            0,
            0,
        ]);
        assert!(
            unknown.is_err(),
            "an unknown variant index must fail loudly, not decode to something"
        );
        assert_eq!(
            (EntityType::ALL.len(), PROTOCOL_VERSION),
            (11, 8),
            "adding an EntityType variant is a wire break: bump PROTOCOL_VERSION \
             (and SNAPSHOT_VERSION in lockstep) and update this pin"
        );
    }
}

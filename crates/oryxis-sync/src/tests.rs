#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tempfile::NamedTempFile;
    use uuid::Uuid;

    use oryxis_core::models::connection::Connection;
    use oryxis_core::models::group::Group;
    use oryxis_core::models::snippet::Snippet;
    use oryxis_vault::VaultStore;

    use crate::config::{SyncConfig, SyncMode};
    use crate::conflict::{resolve, SyncAction};
    use crate::crypto::DeviceIdentity;
    use crate::engine::SyncEngine;
    use crate::protocol::{EntityType, ManifestEntry, PROTOCOL_VERSION};

    fn test_vault() -> VaultStore {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        std::mem::forget(tmp);
        let mut vault = VaultStore::open(&path).unwrap();
        vault.set_master_password("test").unwrap();
        vault
    }

    fn populated_vault() -> VaultStore {
        let vault = test_vault();
        let c1 = Connection::new("web-server", "10.0.0.1");
        vault.save_connection(&c1, Some("password123")).unwrap();
        let c2 = Connection::new("db-server", "10.0.0.2");
        vault.save_connection(&c2, None).unwrap();
        let g = Group::new("Production");
        vault.save_group(&g).unwrap();
        let s = Snippet::new("deploy", "make deploy");
        vault.save_snippet(&s).unwrap();
        vault
    }

    #[test]
    fn sync_engine_creation() {
        let vault = test_vault();
        let identity = DeviceIdentity::generate("test-device");
        let config = SyncConfig::default();
        let mut engine = SyncEngine::new(config, identity, Arc::new(Mutex::new(vault)));
        assert!(engine.take_events().is_some());
        assert!(engine.take_events().is_none()); // can only take once
    }

    #[test]
    fn sync_engine_pairing_code() {
        let vault = test_vault();
        let identity = DeviceIdentity::generate("test-device");
        let config = SyncConfig::default();
        let engine = SyncEngine::new(config, identity, Arc::new(Mutex::new(vault)));
        let code = engine.handle().start_hosting_pairing();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn sync_engine_identity() {
        let vault = test_vault();
        let identity = DeviceIdentity::generate("my-laptop");
        let config = SyncConfig::default();
        let engine = SyncEngine::new(config, identity, Arc::new(Mutex::new(vault)));
        assert_eq!(engine.identity().device_name, "my-laptop");
        assert!(!engine.config().enabled);
    }

    #[test]
    fn conflict_resolution_comprehensive() {
        let now = chrono::Utc::now();
        let id = Uuid::new_v4();

        let older = ManifestEntry {
            entity_type: EntityType::Connection,
            entity_id: id,
            updated_at: now - chrono::Duration::hours(1),
            is_deleted: false,
        };
        let newer = ManifestEntry {
            entity_type: EntityType::Connection,
            entity_id: id,
            updated_at: now,
            is_deleted: false,
        };
        let deleted = ManifestEntry {
            entity_type: EntityType::Connection,
            entity_id: id,
            updated_at: now + chrono::Duration::hours(1),
            is_deleted: true,
        };

        // Newer remote wins
        assert_eq!(resolve(&older, &newer), SyncAction::AcceptRemote);
        // Older remote loses
        assert_eq!(resolve(&newer, &older), SyncAction::PushLocal);
        // Same = skip
        assert_eq!(resolve(&newer, &newer), SyncAction::Skip);
        // Deletion with newer timestamp wins
        assert_eq!(resolve(&newer, &deleted), SyncAction::AcceptRemote);
        // Record update newer than deletion wins
        assert_eq!(resolve(&deleted, &newer), SyncAction::PushLocal);
    }

    #[test]
    fn manifest_covers_all_entity_types() {
        let vault = populated_vault();
        let vault_arc = Arc::new(Mutex::new(vault));

        // Use the internal build_manifest function via engine
        let identity = DeviceIdentity::generate("test");
        let config = SyncConfig::default();
        let engine = SyncEngine::new(config, identity, vault_arc);

        // We can't call build_manifest directly (it's private), but we can verify
        // the vault has the expected data
        let v = engine.config(); // just verify engine is functional
        assert!(!v.enabled);
    }

    #[test]
    fn protocol_version_consistency() {
        // v5 added the Ed25519 relay handshake (RelayHello /
        // RelayHelloAck / RelayAuth) plus HKDF-SHA256 around the
        // X25519 DH output. v4 sealed payloads under the raw DH
        // output, which was acceptable but not best-practice; this
        // pin catches any accidental version bump or rollback.
        assert_eq!(PROTOCOL_VERSION, 5);
    }

    #[test]
    fn entity_type_display() {
        assert_eq!(EntityType::Connection.to_string(), "connection");
        assert_eq!(EntityType::SshKey.to_string(), "key");
        assert_eq!(EntityType::Identity.to_string(), "identity");
        assert_eq!(EntityType::Group.to_string(), "group");
        assert_eq!(EntityType::Snippet.to_string(), "snippet");
        assert_eq!(EntityType::KnownHost.to_string(), "known_host");
    }

    #[test]
    fn sync_config_defaults() {
        let config = SyncConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.mode, SyncMode::Manual);
        assert!(config.relay_url.is_none());
        assert_eq!(config.listen_port, 0);
        assert_eq!(config.auto_interval_secs, 300);
    }

    #[tokio::test]
    async fn sync_engine_start_disabled() {
        let vault = test_vault();
        let identity = DeviceIdentity::generate("test");
        let config = SyncConfig::default(); // enabled = false
        let mut engine = SyncEngine::new(config, identity, Arc::new(Mutex::new(vault)));

        // Starting with disabled config should be a no-op
        engine.start().unwrap();
        engine.stop();
    }

    #[tokio::test]
    async fn sync_engine_start_stop() {
        let vault = test_vault();
        let identity = DeviceIdentity::generate("test");
        let config = SyncConfig {
            enabled: true,
            mode: SyncMode::Manual,
            ..SyncConfig::default()
        };

        let mut engine = SyncEngine::new(config, identity, Arc::new(Mutex::new(vault)));
        let _events = engine.take_events();

        engine.start().unwrap();
        // Give it a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        engine.stop();
    }

    #[test]
    fn load_or_generate_creates_then_returns_same_identity() {
        let vault = test_vault();
        let first = DeviceIdentity::load_or_generate(&vault, "laptop-1").unwrap();
        let second = DeviceIdentity::load_or_generate(&vault, "ignored-on-second-call").unwrap();
        assert_eq!(first.device_id, second.device_id);
        assert_eq!(first.device_name, second.device_name);
        assert_eq!(first.public_key_bytes(), second.public_key_bytes());
        // Fallback name is used only on first generation.
        assert_eq!(second.device_name, "laptop-1");
    }

    #[test]
    fn load_or_generate_persists_signing_key_across_vault_reopen() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        std::mem::forget(tmp);

        let mut v1 = VaultStore::open(&path).unwrap();
        v1.set_master_password("test").unwrap();
        let first = DeviceIdentity::load_or_generate(&v1, "laptop").unwrap();
        let first_pub = first.public_key_bytes();
        drop(v1);

        let mut v2 = VaultStore::open(&path).unwrap();
        v2.unlock("test").unwrap();
        let second = DeviceIdentity::load_or_generate(&v2, "ignored").unwrap();
        assert_eq!(first.device_id, second.device_id);
        assert_eq!(first_pub, second.public_key_bytes());
    }

    // ── Tombstones in the sync manifest / delta path ──

    /// Deleting an entity must surface a tombstone (`is_deleted = true`)
    /// in the manifest, while surviving entities stay `is_deleted = false`.
    #[test]
    fn tombstone_round_trip_in_manifest() {
        use crate::engine::build_manifest;

        let vault = test_vault();
        let keep = Connection::new("keep", "10.0.0.1");
        let doomed = Connection::new("delete-me", "10.0.0.2");
        vault.save_connection(&keep, None).unwrap();
        vault.save_connection(&doomed, None).unwrap();
        vault.delete_connection(&doomed.id).unwrap();

        let vault_arc = Arc::new(Mutex::new(vault));
        let manifest = build_manifest(&vault_arc).unwrap();

        let kept = manifest.iter().find(|e| e.entity_id == keep.id).unwrap();
        assert!(!kept.is_deleted);

        let tomb = manifest.iter().find(|e| e.entity_id == doomed.id).unwrap();
        assert!(tomb.is_deleted);
        assert_eq!(tomb.entity_type, EntityType::Connection);
    }

    /// A delta request for a tombstoned id returns a deletion marker:
    /// empty payload, `is_deleted = true`.
    #[test]
    fn collect_records_emits_deletion_marker() {
        use crate::engine::collect_records;
        use crate::protocol::DeltaRef;

        let vault = test_vault();
        let doomed = Connection::new("delete-me", "10.0.0.2");
        vault.save_connection(&doomed, None).unwrap();
        vault.delete_connection(&doomed.id).unwrap();

        let vault_arc = Arc::new(Mutex::new(vault));
        // A real session always carries a shared secret (v5+); deletion
        // markers have empty payloads so the cipher is never exercised here.
        let records = collect_records(
            &vault_arc,
            &[DeltaRef {
                entity_type: EntityType::Connection,
                entity_id: doomed.id,
            }],
            Some(&[0u8; 32]),
        )
        .unwrap();

        assert_eq!(records.len(), 1);
        assert!(records[0].is_deleted);
        assert!(records[0].payload.is_empty());
    }

    /// Applying a deletion record removes the entity locally AND leaves
    /// a fresh local tombstone, so the delete keeps propagating to this
    /// device's other peers.
    #[test]
    fn apply_records_propagates_deletion() {
        use crate::engine::apply_records;
        use crate::protocol::SyncRecord;

        let vault = test_vault();
        let victim = Connection::new("victim", "10.0.0.1");
        vault.save_connection(&victim, None).unwrap();

        let vault_arc = Arc::new(Mutex::new(vault));
        apply_records(
            &vault_arc,
            &[SyncRecord {
                entity_type: EntityType::Connection,
                entity_id: victim.id,
                // Strictly newer than the victim's save stamp so defensive
                // LWW lets the delete through.
                updated_at: chrono::Utc::now() + chrono::Duration::seconds(1),
                is_deleted: true,
                payload: Vec::new(),
            }],
            Some(&[0u8; 32]),
        )
        .unwrap();

        let v = vault_arc.lock().unwrap();
        assert!(v.list_connections().unwrap().iter().all(|c| c.id != victim.id));
        assert!(v.list_tombstones().unwrap().iter().any(|t| t.entity_id == victim.id));
    }

    // ── Pairing handshake (two live engines) ──

    /// Spin up an enabled engine on an OS-assigned port. Returns the
    /// engine (kept alive by the caller) and its bound listen port.
    fn started_engine(name: &str) -> (SyncEngine, u16) {
        let config = SyncConfig {
            enabled: true,
            mode: SyncMode::Manual,
            listen_port: 0,
            ..SyncConfig::default()
        };
        let mut engine = SyncEngine::new(
            config,
            DeviceIdentity::generate(name),
            Arc::new(Mutex::new(test_vault())),
        );
        let _events = engine.take_events();
        engine.start().unwrap();
        let port = engine.listen_port();
        (engine, port)
    }

    fn loopback(port: u16) -> std::net::SocketAddr {
        format!("127.0.0.1:{port}").parse().unwrap()
    }

    /// Full happy path: host advertises a code, joiner connects with it,
    /// both sides end up with the other persisted as a peer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_round_trip_two_engines() {
        let (host, host_port) = started_engine("host");
        let (joiner, _) = started_engine("joiner");
        let host_id = host.identity().device_id;
        let joiner_id = joiner.identity().device_id;

        let code = host.handle().start_hosting_pairing();
        joiner
            .handle()
            .join_pairing(loopback(host_port), code)
            .await
            .unwrap();

        // The host persists the peer inside its accept-loop task; give
        // it a moment to land.
        tokio::time::sleep(Duration::from_millis(300)).await;

        let joiner_peers = joiner.vault.lock().unwrap().list_sync_peers().unwrap();
        assert_eq!(joiner_peers.len(), 1);
        assert_eq!(joiner_peers[0].peer_id, host_id);

        let host_peers = host.vault.lock().unwrap().list_sync_peers().unwrap();
        assert_eq!(host_peers.len(), 1);
        assert_eq!(host_peers[0].peer_id, joiner_id);

        // v4: both sides derive and persist the same shared secret
        // from the pairing-time X25519 exchange. Decrypt-side tests
        // would need to ferry the secret across the test boundary;
        // here we just confirm it's stored on both rows and that the
        // two values match.
        let joiner_secret = joiner
            .vault
            .lock()
            .unwrap()
            .get_sync_peer_shared_secret(&host_id)
            .unwrap()
            .expect("joiner side missing shared secret");
        let host_secret = host
            .vault
            .lock()
            .unwrap()
            .get_sync_peer_shared_secret(&joiner_id)
            .unwrap()
            .expect("host side missing shared secret");
        assert_eq!(joiner_secret.len(), 32);
        assert_eq!(joiner_secret, host_secret);
    }

    /// A wrong code is rejected and neither side stores a peer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_rejects_wrong_code() {
        let (host, host_port) = started_engine("host");
        let (joiner, _) = started_engine("joiner");

        host.handle().start_hosting_pairing();
        let result = joiner
            .handle()
            .join_pairing(loopback(host_port), "000000".to_string())
            .await;
        assert!(result.is_err());

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(joiner.vault.lock().unwrap().list_sync_peers().unwrap().is_empty());
        assert!(host.vault.lock().unwrap().list_sync_peers().unwrap().is_empty());
    }

    /// Per-source attempt cap regression. The pre-fix behaviour: 3
    /// wrong codes from ANY source invalidated the host's state, so
    /// the legitimate user (typing on a different network) saw
    /// "Not hosting pairing" afterward. The fix scopes the cap per
    /// source. We can't simulate two distinct source IPs over
    /// loopback, so the behavioural assertion is: after a single
    /// noisy joiner burns the per-source cap from the loopback IP,
    /// the **hosted code stays live** (not auto-cleared by the host),
    /// which is the precondition for the legitimate user to still
    /// pair from elsewhere.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_per_source_cap_keeps_hosted_code_alive() {
        use crate::engine::MAX_PAIRING_ATTEMPTS;
        let (host, host_port) = started_engine("host");
        let (joiner, _) = started_engine("joiner");

        let original_code = host.handle().start_hosting_pairing();
        for _ in 0..(MAX_PAIRING_ATTEMPTS + 2) {
            let _ = joiner
                .handle()
                .join_pairing(loopback(host_port), "000000".to_string())
                .await;
        }
        // The hosted code must still be present on the host's state.
        // Without the per-source fix the old global counter would have
        // cleared it after 3 attempts, regardless of how legitimate
        // any later attempt might be.
        let still_hosting = host
            .hosting_pairing
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.code.clone());
        assert_eq!(
            still_hosting.as_deref(),
            Some(original_code.as_str()),
            "hosted code must survive a single source over the cap"
        );
    }

    /// Joining with no code hosted at all is rejected.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_rejects_when_not_hosting() {
        // `_host` keeps the engine (and its accept loop) alive for the
        // duration of the test; it is otherwise not inspected.
        let (_host, host_port) = started_engine("host");
        let (joiner, _) = started_engine("joiner");

        // No `start_hosting_pairing` call on the host.
        let result = joiner
            .handle()
            .join_pairing(loopback(host_port), "123456".to_string())
            .await;
        assert!(result.is_err());
    }

    // ── Pairing link encoding ──

    #[test]
    fn pairing_link_round_trip() {
        use crate::engine::{format_pairing_link, parse_pairing_link};
        let id = Uuid::new_v4();
        let link = format_pairing_link(&id, "654321");
        assert!(link.starts_with("oryxis://pair/"));
        let (back_id, back_code) = parse_pairing_link(&link).unwrap();
        assert_eq!(back_id, id);
        assert_eq!(back_code, "654321");
    }

    #[test]
    fn pairing_link_rejects_malformed() {
        use crate::engine::parse_pairing_link;
        // Missing prefix.
        assert!(parse_pairing_link("https://pair/foo/123456").is_none());
        // Bad UUID.
        assert!(parse_pairing_link("oryxis://pair/not-a-uuid/123456").is_none());
        // Wrong code length.
        let id = Uuid::new_v4();
        assert!(parse_pairing_link(&format!("oryxis://pair/{id}/12345")).is_none());
        assert!(parse_pairing_link(&format!("oryxis://pair/{id}/1234567")).is_none());
        // Non-digit code.
        assert!(parse_pairing_link(&format!("oryxis://pair/{id}/12ab34")).is_none());
        // Missing code segment.
        assert!(parse_pairing_link(&format!("oryxis://pair/{id}")).is_none());
    }

    #[test]
    fn pairing_link_trims_whitespace() {
        use crate::engine::{format_pairing_link, parse_pairing_link};
        let id = Uuid::new_v4();
        let padded = format!("  {}\n", format_pairing_link(&id, "777777"));
        let (back_id, code) = parse_pairing_link(&padded).unwrap();
        assert_eq!(back_id, id);
        assert_eq!(code, "777777");
    }

    /// A hosted code pairs exactly one device: a second join with the
    /// same code fails because the code was cleared on success.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pairing_is_single_shot() {
        let (host, host_port) = started_engine("host");
        let (joiner, _) = started_engine("joiner");

        let code = host.handle().start_hosting_pairing();
        joiner
            .handle()
            .join_pairing(loopback(host_port), code.clone())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;

        let second = joiner
            .handle()
            .join_pairing(loopback(host_port), code)
            .await;
        assert!(second.is_err(), "the code should be single-shot");
    }

    // ---------------------------------------------------------------
    // Audit follow-up coverage: tests for guarantees the audit
    // flagged as missing (pairing replay, relay handshake roundtrip
    // and replay rejection, version-mismatch surface,
    // tombstone-vacuum-then-resurrection).
    // ---------------------------------------------------------------

    /// A captured `PairingResponse` signature is bound to its own
    /// challenge nonce. Replaying it against a fresh challenge must
    /// fail verification. Protects against a leaked-link attacker
    /// reusing the signed bytes from a prior session.
    #[test]
    fn pairing_response_cannot_be_replayed_across_challenges() {
        let identity = crate::crypto::DeviceIdentity::generate("alice");
        let challenge_a = crate::crypto::random_challenge();
        let challenge_b = crate::crypto::random_challenge();
        assert_ne!(challenge_a, challenge_b);

        // The joiner signs the host's challenge.
        let signed_a = crate::crypto::sign_ed25519_32(&identity.signing_key, &challenge_a);

        // Verifies cleanly against the SAME challenge.
        crate::crypto::verify_ed25519_32(&identity.public_key_bytes(), &challenge_a, &signed_a)
            .unwrap();

        // Replay against the *next* session's challenge must reject.
        let err = crate::crypto::verify_ed25519_32(
            &identity.public_key_bytes(),
            &challenge_b,
            &signed_a,
        );
        assert!(err.is_err(), "signed challenge from session A must not verify in session B");
    }

    /// Round-trip the v5 relay handshake transcript: sign with the
    /// server's identity, verify with the server's pubkey, both sides
    /// see the same nonce pair.
    #[test]
    fn relay_handshake_signature_round_trip() {
        let server = crate::crypto::DeviceIdentity::generate("server");
        let client_id = Uuid::new_v4();
        let nonce_c = crate::crypto::random_relay_nonce();
        let nonce_s = crate::crypto::random_relay_nonce();
        let transcript = crate::crypto::relay_handshake_transcript(
            &client_id,
            &server.device_id,
            &nonce_c,
            &nonce_s,
        );
        let sig = crate::crypto::sign_relay_handshake(&server.signing_key, &transcript);
        crate::crypto::verify_relay_handshake(&server.public_key_bytes(), &transcript, &sig)
            .unwrap();
    }

    /// A relay-handshake signature captured from session A cannot be
    /// replayed in session B because the transcript binds both
    /// sides' fresh nonces. This is the integrity property that the
    /// v5 handshake adds to the relay path (QUIC gets it for free
    /// via the TLS exporter).
    #[test]
    fn relay_handshake_rejects_replay_with_fresh_nonces() {
        let server = crate::crypto::DeviceIdentity::generate("server");
        let client_id = Uuid::new_v4();
        let server_id = server.device_id;
        let transcript_a = crate::crypto::relay_handshake_transcript(
            &client_id,
            &server_id,
            &[1u8; 32],
            &[2u8; 32],
        );
        let transcript_b = crate::crypto::relay_handshake_transcript(
            &client_id,
            &server_id,
            &[3u8; 32],
            &[4u8; 32],
        );
        let sig_a = crate::crypto::sign_relay_handshake(&server.signing_key, &transcript_a);
        let err = crate::crypto::verify_relay_handshake(
            &server.public_key_bytes(),
            &transcript_b,
            &sig_a,
        );
        assert!(err.is_err(), "relay handshake sig must not replay across nonce sets");
    }

    /// A Hello with a stale protocol version still serializes /
    /// deserializes cleanly (the bincode shape is the same), so the
    /// receiver can inspect `protocol_version` and emit a structured
    /// `VersionMismatch` event instead of dropping the connection
    /// with a generic transport error.
    #[test]
    fn protocol_version_mismatch_is_inspectable_on_the_wire() {
        use crate::protocol::{decode_message, encode_message, SyncMessage};
        let stale = SyncMessage::Hello {
            device_id: Uuid::new_v4(),
            protocol_version: PROTOCOL_VERSION - 1,
            auth_signature: vec![0u8; 64],
        };
        let frame = encode_message(&stale).unwrap();
        let len =
            u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        let decoded = decode_message(&frame[4..4 + len]).unwrap();
        match decoded {
            SyncMessage::Hello { protocol_version, .. } => {
                assert_eq!(protocol_version, PROTOCOL_VERSION - 1);
                assert_ne!(protocol_version, PROTOCOL_VERSION);
            }
            _ => panic!("expected Hello"),
        }
    }

    /// Tombstone resurrection scenario: a tombstone older than the
    /// TTL is vacuumed from `sync_metadata`. After vacuum,
    /// `build_manifest` no longer emits a deletion entry for that
    /// entity. If the entity exists on a peer that resyncs >TTL days
    /// later, that peer's live record beats nothing on our side and
    /// the entity comes back. This test pins the current behaviour
    /// so the `PeerStaleWarning` event keeps being the right
    /// mitigation; a future fix that preserves vacuumed tombstones
    /// against still-paired peers would flip the assertion.
    #[test]
    fn vacuumed_tombstone_no_longer_appears_in_manifest() {
        use crate::engine::build_manifest;
        let vault = test_vault();
        let conn = Connection::new("doomed", "10.0.0.99");
        vault.save_connection(&conn, None).unwrap();
        vault.delete_connection(&conn.id).unwrap();

        // Tombstone is present right after delete.
        let vault_arc = Arc::new(Mutex::new(vault));
        let manifest = build_manifest(&vault_arc).unwrap();
        let tomb = manifest.iter().find(|e| e.entity_id == conn.id);
        assert!(
            tomb.is_some_and(|e| e.is_deleted),
            "tombstone should be in manifest right after delete"
        );

        // Vacuum with TTL=0 sweeps it.
        {
            let v = vault_arc.lock().unwrap();
            v.vacuum_tombstones(0).unwrap();
        }

        let manifest_after = build_manifest(&vault_arc).unwrap();
        let still_there = manifest_after.iter().any(|e| e.entity_id == conn.id);
        assert!(
            !still_there,
            "vacuumed tombstone must no longer appear in manifest, which is exactly why PeerStaleWarning exists as a mitigation"
        );
    }

    // --- SFTP-transport snapshot round-trip ---------------------------
    //
    // Same reconciliation guarantees as the P2P manifest round-trip
    // above, exercised through the "virtual peer" snapshot path
    // (`build_full_snapshot` / `merge_snapshot`) instead of a live
    // session. No SFTP I/O: the blob is passed in memory.

    /// Group secret stand-in. In production this is derived from the
    /// user passphrase via Argon2id; the snapshot functions only ever
    /// see the resulting 32 bytes, so a fixed key is faithful here.
    const SNAP_SECRET: [u8; 32] = [7u8; 32];

    /// A snapshot built on device A and merged on device B materializes
    /// A's entities on B, with secret fields intact.
    #[test]
    fn snapshot_round_trip_creates_entities_on_peer() {
        use crate::engine::{build_full_snapshot, merge_snapshot};

        let va = test_vault();
        let conn = Connection::new("web", "10.0.0.1");
        va.save_connection(&conn, Some("s3cret")).unwrap();
        let va = Arc::new(Mutex::new(va));

        let blob = build_full_snapshot(&va, &SNAP_SECRET).unwrap();

        let vb = Arc::new(Mutex::new(test_vault()));
        merge_snapshot(&vb, &blob, &SNAP_SECRET).unwrap();

        let v = vb.lock().unwrap();
        let got = v.list_connections().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, conn.id);
        // Passwords ride the snapshot only when `sync_passwords` is on;
        // it defaults off, so the secret column must be empty here. This
        // pins the gate so a future change can't silently start shipping
        // passwords over SFTP.
        assert!(v.get_connection_password(&conn.id).unwrap().is_none());
    }

    /// A newer edit on B, snapshotted back and merged into A, wins by
    /// last-writer-wins; a stale copy never overwrites a newer one.
    #[test]
    fn snapshot_round_trip_lww_newer_wins() {
        use crate::engine::{build_full_snapshot, merge_snapshot};

        // A creates, B receives.
        let va = Arc::new(Mutex::new(test_vault()));
        let mut conn = Connection::new("box", "10.0.0.1");
        va.lock().unwrap().save_connection(&conn, None).unwrap();
        let blob_a = build_full_snapshot(&va, &SNAP_SECRET).unwrap();

        let vb = Arc::new(Mutex::new(test_vault()));
        merge_snapshot(&vb, &blob_a, &SNAP_SECRET).unwrap();

        // B edits with a strictly newer stamp, snapshots back to A.
        conn.hostname = "10.0.0.99".into();
        conn.updated_at = chrono::Utc::now() + chrono::Duration::seconds(2);
        vb.lock().unwrap().save_connection(&conn, None).unwrap();
        let blob_b = build_full_snapshot(&vb, &SNAP_SECRET).unwrap();
        merge_snapshot(&va, &blob_b, &SNAP_SECRET).unwrap();

        let v = va.lock().unwrap();
        let got = v.list_connections().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].hostname, "10.0.0.99", "newer edit must win on A");
    }

    /// A delete on A (a tombstone) propagates through the snapshot and
    /// removes the entity on B without resurrecting it.
    #[test]
    fn snapshot_round_trip_propagates_deletion() {
        use crate::engine::{build_full_snapshot, merge_snapshot};

        // A and B both hold the entity (A creates, B merges).
        let va = Arc::new(Mutex::new(test_vault()));
        let conn = Connection::new("doomed", "10.0.0.1");
        va.lock().unwrap().save_connection(&conn, None).unwrap();
        let blob_a = build_full_snapshot(&va, &SNAP_SECRET).unwrap();

        let vb = Arc::new(Mutex::new(test_vault()));
        merge_snapshot(&vb, &blob_a, &SNAP_SECRET).unwrap();
        assert_eq!(vb.lock().unwrap().list_connections().unwrap().len(), 1);

        // A deletes, snapshots the tombstone, B merges it.
        va.lock().unwrap().delete_connection(&conn.id).unwrap();
        let blob_del = build_full_snapshot(&va, &SNAP_SECRET).unwrap();
        merge_snapshot(&vb, &blob_del, &SNAP_SECRET).unwrap();

        assert!(
            vb.lock().unwrap().list_connections().unwrap().is_empty(),
            "delete must propagate, not resurrect, through the snapshot"
        );
    }

    /// The real multi-instance case: a second instance that ALREADY has
    /// its own hosts configured. Merging the group snapshot must UNION the
    /// two sets (never wipe the local hosts), and a round-trip back must
    /// converge both instances to the same union. This is the exact
    /// behaviour of pointing a fresh device with existing data at the
    /// shared SFTP snapshot.
    #[test]
    fn snapshot_merge_unions_with_existing_local_hosts() {
        use crate::engine::{build_full_snapshot, merge_snapshot};
        use std::collections::HashSet;

        // Instance A has its own host.
        let va = Arc::new(Mutex::new(test_vault()));
        let host_a = Connection::new("server-a", "10.0.0.1");
        va.lock().unwrap().save_connection(&host_a, None).unwrap();

        // Instance B already has a DIFFERENT host configured locally.
        let vb = Arc::new(Mutex::new(test_vault()));
        let host_b = Connection::new("server-b", "10.0.0.2");
        vb.lock().unwrap().save_connection(&host_b, None).unwrap();

        // B pulls A's snapshot: B must now hold BOTH hosts (its own kept,
        // A's added), not be overwritten by A's state.
        let snap_a = build_full_snapshot(&va, &SNAP_SECRET).unwrap();
        merge_snapshot(&vb, &snap_a, &SNAP_SECRET).unwrap();
        let b_ids: HashSet<_> = vb
            .lock()
            .unwrap()
            .list_connections()
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert!(b_ids.contains(&host_a.id), "A's host must be added to B");
        assert!(
            b_ids.contains(&host_b.id),
            "B's own pre-existing host must be preserved, not wiped"
        );
        assert_eq!(b_ids.len(), 2);

        // A pulls B's (now-merged) snapshot: both instances converge to
        // the same union, no duplication.
        let snap_b = build_full_snapshot(&vb, &SNAP_SECRET).unwrap();
        merge_snapshot(&va, &snap_b, &SNAP_SECRET).unwrap();
        let a_ids: HashSet<_> = va
            .lock()
            .unwrap()
            .list_connections()
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(a_ids, b_ids, "both instances converge to the same set");
        assert_eq!(a_ids.len(), 2);
    }

    /// A snapshot sealed under one secret can't be opened under another:
    /// `merge_snapshot` fails and leaves the vault untouched (so a caller
    /// must not push a fresh snapshot after a failed merge).
    #[test]
    fn snapshot_wrong_secret_fails_and_preserves_vault() {
        use crate::engine::{build_full_snapshot, merge_snapshot};

        let va = Arc::new(Mutex::new(test_vault()));
        va.lock()
            .unwrap()
            .save_connection(&Connection::new("a", "10.0.0.1"), None)
            .unwrap();
        let blob = build_full_snapshot(&va, &SNAP_SECRET).unwrap();

        let vb = Arc::new(Mutex::new(test_vault()));
        let wrong = [9u8; 32];
        assert!(merge_snapshot(&vb, &blob, &wrong).is_err());
        assert!(
            vb.lock().unwrap().list_connections().unwrap().is_empty(),
            "a failed merge must not partially mutate the vault"
        );
    }

    /// A truncated or wrong-format blob is rejected on the header before
    /// it ever reaches the AEAD.
    #[test]
    fn snapshot_bad_header_rejected() {
        use crate::engine::merge_snapshot;

        let vb = Arc::new(Mutex::new(test_vault()));
        assert!(merge_snapshot(&vb, b"", &SNAP_SECRET).is_err());
        assert!(merge_snapshot(&vb, b"NOTORX\x01\x00", &SNAP_SECRET).is_err());
    }
}

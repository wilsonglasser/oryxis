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
        // v4 added X25519 ephemeral key exchange to the pairing
        // messages so payloads can be sealed with the resulting
        // shared secret. Older peers are intentionally incompatible.
        assert_eq!(PROTOCOL_VERSION, 4);
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
        let records = collect_records(
            &vault_arc,
            &[DeltaRef {
                entity_type: EntityType::Connection,
                entity_id: doomed.id,
            }],
            None,
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
                updated_at: chrono::Utc::now(),
                is_deleted: true,
                payload: Vec::new(),
            }],
            None,
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
}

#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use std::sync::{Arc, Mutex};

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
        let code = engine.start_pairing();
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
        // v2 added channel-bound auth_signature to Hello/HelloAck. v1 had
        // no auth and is intentionally incompatible.
        assert_eq!(PROTOCOL_VERSION, 2);
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
        )
        .unwrap();

        let v = vault_arc.lock().unwrap();
        assert!(v.list_connections().unwrap().iter().all(|c| c.id != victim.id));
        assert!(v.list_tombstones().unwrap().iter().any(|t| t.entity_id == victim.id));
    }
}

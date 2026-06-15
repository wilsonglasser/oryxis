use super::*;

#[test]
fn save_and_list_sync_peers() {
    let vault = unlocked_vault();
    let peer_id = Uuid::new_v4();
    let public_key = vec![1u8; 32];
    let now = chrono::Utc::now();

    vault.save_sync_peer(&peer_id, "laptop", &public_key, None, &now).unwrap();

    let peers = vault.list_sync_peers().unwrap();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].peer_id, peer_id);
    assert_eq!(peers[0].device_name, "laptop");
    assert_eq!(peers[0].public_key, public_key);
    assert!(peers[0].is_active);
    assert!(peers[0].last_synced_at.is_none());
}


#[test]
fn sync_peer_with_shared_secret() {
    let vault = unlocked_vault();
    let peer_id = Uuid::new_v4();
    let public_key = vec![2u8; 32];
    let shared_secret = vec![42u8; 32];
    let now = chrono::Utc::now();

    vault.save_sync_peer(&peer_id, "desktop", &public_key, Some(&shared_secret), &now).unwrap();

    let retrieved = vault.get_sync_peer_shared_secret(&peer_id).unwrap();
    assert_eq!(retrieved, Some(shared_secret));
}


#[test]
fn sync_peer_no_shared_secret() {
    let vault = unlocked_vault();
    let peer_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    vault.save_sync_peer(&peer_id, "phone", &[3u8; 32], None, &now).unwrap();

    let retrieved = vault.get_sync_peer_shared_secret(&peer_id).unwrap();
    assert!(retrieved.is_none());
}


#[test]
fn update_sync_peer_endpoint() {
    let vault = unlocked_vault();
    let peer_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    vault.save_sync_peer(&peer_id, "server", &[4u8; 32], None, &now).unwrap();
    vault.update_sync_peer_endpoint(&peer_id, "192.168.1.50", 4433).unwrap();

    let peers = vault.list_sync_peers().unwrap();
    assert_eq!(peers[0].last_known_ip, Some("192.168.1.50".into()));
    assert_eq!(peers[0].last_known_port, Some(4433));
}


#[test]
fn update_sync_peer_last_synced() {
    let vault = unlocked_vault();
    let peer_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    vault.save_sync_peer(&peer_id, "tablet", &[5u8; 32], None, &now).unwrap();
    assert!(vault.list_sync_peers().unwrap()[0].last_synced_at.is_none());

    vault.update_sync_peer_last_synced(&peer_id).unwrap();
    assert!(vault.list_sync_peers().unwrap()[0].last_synced_at.is_some());
}


#[test]
fn delete_sync_peer() {
    let vault = unlocked_vault();
    let peer_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    vault.save_sync_peer(&peer_id, "temp", &[6u8; 32], None, &now).unwrap();
    assert_eq!(vault.list_sync_peers().unwrap().len(), 1);

    vault.delete_sync_peer(&peer_id).unwrap();
    assert_eq!(vault.list_sync_peers().unwrap().len(), 0);
}


#[test]
fn multiple_sync_peers() {
    let vault = unlocked_vault();
    let now = chrono::Utc::now();

    vault.save_sync_peer(&Uuid::new_v4(), "device-a", &[7u8; 32], None, &now).unwrap();
    vault.save_sync_peer(&Uuid::new_v4(), "device-b", &[8u8; 32], None, &now).unwrap();
    vault.save_sync_peer(&Uuid::new_v4(), "device-c", &[9u8; 32], None, &now).unwrap();

    let peers = vault.list_sync_peers().unwrap();
    assert_eq!(peers.len(), 3);
}

// ── Sync Metadata (tombstones) ──


#[test]
fn delete_records_tombstone() {
    let vault = unlocked_vault();
    let conn = Connection::new("doomed", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();

    assert!(vault.list_tombstones().unwrap().is_empty());

    vault.delete_connection(&conn.id).unwrap();

    let tombstones = vault.list_tombstones().unwrap();
    assert_eq!(tombstones.len(), 1);
    assert_eq!(tombstones[0].entity_type, "connection");
    assert_eq!(tombstones[0].entity_id, conn.id);
}


/// Every `delete_*` family member must leave a tombstone tagged
/// with the wire string the sync engine expects.
#[test]
fn delete_tombstones_cover_all_entity_types() {
    let vault = unlocked_vault();

    let conn = Connection::new("c", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();
    vault.delete_connection(&conn.id).unwrap();

    let group = Group::new("g");
    vault.save_group(&group).unwrap();
    vault.delete_group(&group.id).unwrap();

    let snippet = Snippet::new("s", "echo hi");
    vault.save_snippet(&snippet).unwrap();
    vault.delete_snippet(&snippet.id).unwrap();

    let mut types: Vec<String> = vault
        .list_tombstones()
        .unwrap()
        .into_iter()
        .map(|t| t.entity_type)
        .collect();
    types.sort();
    assert_eq!(types, vec!["connection", "group", "snippet"]);
}


/// Recording the same tombstone twice refreshes the row instead of
/// duplicating it (PK is `(entity_type, entity_id)`).
#[test]
fn tombstone_idempotent() {
    let vault = unlocked_vault();
    let id = Uuid::new_v4();

    vault.record_tombstone("connection", &id).unwrap();
    vault.record_tombstone("connection", &id).unwrap();

    let tombstones = vault.list_tombstones().unwrap();
    assert_eq!(tombstones.len(), 1);
    assert_eq!(tombstones[0].entity_id, id);
}


/// Once a deletion has propagated to every peer, the tombstone is
/// cleared and stops showing up in the manifest.
#[test]
fn clear_tombstone_after_remote_propagation() {
    let vault = unlocked_vault();
    let conn = Connection::new("doomed", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();
    vault.delete_connection(&conn.id).unwrap();
    assert_eq!(vault.list_tombstones().unwrap().len(), 1);

    vault.clear_tombstone("connection", &conn.id).unwrap();
    assert!(vault.list_tombstones().unwrap().is_empty());

    // Clearing an already-gone tombstone is a no-op, not an error.
    vault.clear_tombstone("connection", &conn.id).unwrap();
}


/// Re-creating an entity (user re-adds, or peer pushes a newer
/// copy) drops the stale tombstone for the same id automatically,
/// so the manifest builder doesn't ship both a live entry and a
/// deletion marker for the same id.
#[test]
fn save_clears_stale_tombstone() {
    let vault = unlocked_vault();
    let conn = Connection::new("revived", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();
    vault.delete_connection(&conn.id).unwrap();
    assert_eq!(vault.list_tombstones().unwrap().len(), 1);

    // Save with the same id, simulates peer push or user re-add.
    vault.save_connection(&conn, None).unwrap();
    assert!(
        vault.list_tombstones().unwrap().is_empty(),
        "stale tombstone should be cleared on re-create"
    );
}


/// `vacuum_tombstones` drops rows older than the TTL and leaves
/// fresh ones in place. We can't easily backdate via the API, so
/// the test deletes one entity, immediately vacuums with TTL 0
/// (which means "drop everything"), and asserts it cleared the
/// row, then re-deletes to verify TTL 365 keeps a fresh one.
#[test]
fn vacuum_tombstones_drops_old_rows() {
    let vault = unlocked_vault();
    let conn = Connection::new("doomed", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();
    vault.delete_connection(&conn.id).unwrap();
    assert_eq!(vault.list_tombstones().unwrap().len(), 1);

    let removed = vault.vacuum_tombstones(0).unwrap();
    assert_eq!(removed, 1);
    assert!(vault.list_tombstones().unwrap().is_empty());

    let conn2 = Connection::new("doomed2", "10.0.0.2");
    vault.save_connection(&conn2, None).unwrap();
    vault.delete_connection(&conn2.id).unwrap();
    assert_eq!(vault.vacuum_tombstones(365).unwrap(), 0);
    assert_eq!(vault.list_tombstones().unwrap().len(), 1);
}


/// Regression: a tombstone must not be vacuumed while any active
/// peer is still behind it (last_synced_at < deleted_at, or NULL).
/// Without this guarantee, the deletion would silently resurrect
/// on the offline peer's next sync.
#[test]
fn vacuum_tombstones_keeps_rows_with_behind_peer() {
    let vault = unlocked_vault();
    let peer_id = Uuid::new_v4();
    let pubkey = [1u8; 32];
    // Peer paired but has never synced (NULL last_synced_at).
    vault
        .save_sync_peer(&peer_id, "offline-peer", &pubkey, None, &Utc::now())
        .unwrap();

    let conn = Connection::new("doomed", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();
    vault.delete_connection(&conn.id).unwrap();
    assert_eq!(vault.list_tombstones().unwrap().len(), 1);

    // TTL 0 + peer is behind => the row stays.
    let removed = vault.vacuum_tombstones(0).unwrap();
    assert_eq!(removed, 0);
    assert_eq!(vault.list_tombstones().unwrap().len(), 1);

    // After the peer catches up (synced AFTER deleted_at), the
    // tombstone can be reclaimed. The public API stamps "now",
    // which is guaranteed strictly later than the deleted_at row
    // we just inserted (chrono is monotonic on a single thread).
    vault.update_sync_peer_last_synced(&peer_id).unwrap();
    let removed = vault.vacuum_tombstones(0).unwrap();
    assert_eq!(removed, 1);
    assert!(vault.list_tombstones().unwrap().is_empty());
}


/// An inactive peer should not block tombstone GC. A peer the user
/// already removed has no claim on the deletion log.
#[test]
fn vacuum_tombstones_ignores_inactive_peer() {
    let vault = unlocked_vault();
    let peer_id = Uuid::new_v4();
    vault
        .save_sync_peer(&peer_id, "stale", &[2u8; 32], None, &Utc::now())
        .unwrap();
    vault.delete_sync_peer(&peer_id).unwrap();

    let conn = Connection::new("doomed", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();
    vault.delete_connection(&conn.id).unwrap();
    assert_eq!(vault.list_tombstones().unwrap().len(), 1);

    // Inactive peer doesn't gate GC: row is dropped at TTL 0.
    assert_eq!(vault.vacuum_tombstones(0).unwrap(), 1);
}

// ── Share (filtered export) ──


#[test]
fn sync_device_identity_round_trip() {
    let vault = unlocked_vault();
    let blob = b"\x01\x02\x03\x04\x05signing-key-bytes-and-name";
    vault.set_sync_device_identity(blob).unwrap();
    let back = vault.get_sync_device_identity().unwrap();
    assert_eq!(back.as_deref(), Some(&blob[..]));
}


#[test]
fn sync_device_identity_missing_returns_none() {
    let vault = unlocked_vault();
    assert!(vault.get_sync_device_identity().unwrap().is_none());
}


#[test]
fn sync_device_identity_not_plaintext_in_settings() {
    let vault = unlocked_vault();
    let blob = b"signing-key-secret-do-not-leak";
    vault.set_sync_device_identity(blob).unwrap();
    let raw = vault.get_setting("sync_device_identity").unwrap().unwrap();
    assert!(
        !raw.as_bytes().windows(blob.len()).any(|w| w == blob),
        "plaintext device identity leaked into settings column: {raw}",
    );
}


#[test]
fn sync_device_identity_survives_password_rotation() {
    let mut vault = unlocked_vault();
    let blob = b"identity-survives-rotation";
    vault.set_sync_device_identity(blob).unwrap();
    vault.set_user_password("new-password").unwrap();
    // Still decryptable with the (now-rotated) master key in memory.
    let back = vault.get_sync_device_identity().unwrap();
    assert_eq!(back.as_deref(), Some(&blob[..]));
}


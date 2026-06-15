use super::*;

#[test]
fn session_log_roundtrips_appended_chunks() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"first chunk\n").unwrap();
    vault.append_session_data(&log_id, b"second chunk\n").unwrap();
    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, b"first chunk\nsecond chunk\n");
}


#[test]
fn session_log_chunks_are_not_stored_in_the_clear() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    let marker = b"TOP-SECRET-OUTPUT-MARKER";
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, marker).unwrap();
    // Structural check straight against the column: the stored blob
    // must not contain the recorded bytes.
    let raw: Vec<u8> = vault
        .db
        .query_row(
            "SELECT data FROM session_log_chunks WHERE log_id = ?1",
            params![log_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !raw.windows(marker.len()).any(|w| w == marker),
        "recorded output stored in the clear"
    );
    // And it still reads back through the API.
    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, marker);
}


#[test]
fn session_log_chunks_survive_master_password_change() {
    let mut vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"before change\n").unwrap();
    vault.set_user_password("brand-new-password").unwrap();
    // Drop the cached content key to force a re-unwrap with the new
    // master key, as a fresh process would.
    *vault.session_log_key.lock().unwrap() = None;
    vault.append_session_data(&log_id, b"after change\n").unwrap();
    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, b"before change\nafter change\n");
}


#[test]
fn session_log_chunks_concatenate_in_append_order() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault
        .create_session_log(&log_id, &conn_id, "web-01")
        .unwrap();

    // Append in three writes; the recorded stream must read back as
    // the exact byte-for-byte concatenation, in order.
    vault.append_session_data(&log_id, b"$ apt update\n").unwrap();
    vault.append_session_data(&log_id, b"Hit:1 http://deb\n").unwrap();
    vault.append_session_data(&log_id, b"Reading package lists\n").unwrap();
    // Empty appends are no-ops, never a stray zero-length chunk.
    vault.append_session_data(&log_id, b"").unwrap();

    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(
        data,
        b"$ apt update\nHit:1 http://deb\nReading package lists\n"
    );

    // Metadata size reflects the stored chunk bytes; each sealed
    // chunk carries a nonce + AEAD tag on top of the recording.
    let entry = vault
        .list_session_logs()
        .unwrap()
        .into_iter()
        .find(|e| e.id == log_id)
        .expect("log listed");
    assert_eq!(entry.data_size, data.len() + 3 * (NONCE_LEN + 16));
}


#[test]
fn session_log_reads_legacy_inline_blob_then_chunks() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault
        .create_session_log(&log_id, &conn_id, "legacy")
        .unwrap();
    // Simulate a row recorded before the chunk migration: bytes live
    // in the inline `data` column. New appends go to chunks; the read
    // path must stitch legacy-prefix + chunks.
    vault
        .db
        .execute(
            "UPDATE session_logs SET data = ?1 WHERE id = ?2",
            params![b"OLD".to_vec(), log_id.to_string()],
        )
        .unwrap();
    vault.append_session_data(&log_id, b"NEW").unwrap();

    let data = vault.get_session_data(&log_id).unwrap().unwrap();
    assert_eq!(data, b"OLDNEW");

    let entry = vault
        .list_session_logs()
        .unwrap()
        .into_iter()
        .find(|e| e.id == log_id)
        .unwrap();
    // Inline legacy bytes are raw; the appended chunk is sealed.
    assert_eq!(entry.data_size, 3 + 3 + NONCE_LEN + 16);
}


#[test]
fn deleting_session_log_drops_its_chunks() {
    let vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    vault
        .create_session_log(&log_id, &Uuid::new_v4(), "doomed")
        .unwrap();
    vault.append_session_data(&log_id, b"transient").unwrap();

    vault.delete_session_log(&log_id).unwrap();

    // No orphan chunks left behind.
    let orphans: i64 = vault
        .db
        .query_row(
            "SELECT COUNT(*) FROM session_log_chunks WHERE log_id = ?1",
            params![log_id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(orphans, 0);
    assert!(vault.get_session_data(&log_id).is_err());
}


#[test]
fn add_and_list_logs() {
    let vault = unlocked_vault();
    let entry = LogEntry::new("prod-web", "192.168.1.10", LogEvent::Connected, "OK");
    vault.add_log(&entry).unwrap();

    let logs = vault.list_logs(10).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].connection_label, "prod-web");
}


#[test]
fn logs_ordered_by_timestamp_desc() {
    let vault = unlocked_vault();
    vault.add_log(&LogEntry::new("first", "h1", LogEvent::Connected, "")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));
    vault.add_log(&LogEntry::new("second", "h2", LogEvent::Disconnected, "")).unwrap();

    let logs = vault.list_logs(10).unwrap();
    assert_eq!(logs[0].connection_label, "second"); // most recent first
}


#[test]
fn clear_logs() {
    let vault = unlocked_vault();
    vault.add_log(&LogEntry::new("x", "y", LogEvent::Error, "fail")).unwrap();
    vault.add_log(&LogEntry::new("a", "b", LogEvent::Connected, "ok")).unwrap();
    vault.clear_logs().unwrap();
    assert_eq!(vault.list_logs(100).unwrap().len(), 0);
}


#[test]
fn logs_limit_works() {
    let vault = unlocked_vault();
    for i in 0..20 {
        vault.add_log(&LogEntry::new(&format!("conn-{}", i), "h", LogEvent::Connected, "")).unwrap();
    }
    let logs = vault.list_logs(5).unwrap();
    assert_eq!(logs.len(), 5);
}

// ── MCP enabled field ──


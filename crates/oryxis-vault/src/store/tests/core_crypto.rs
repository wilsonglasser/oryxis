use super::*;

#[test]
fn legacy_vault_migrates_to_derived_key_on_unlock() {
    let mut vault = temp_vault();
    let pw = "legacy-pass";

    // Hand-craft a vault in the legacy format: per-field-KDF
    // password check plus a legacy-encrypted connection password
    // and AI API key, exactly what a pre-update vault holds.
    let check = encrypt(b"oryxis_vault_ok", pw.as_bytes()).unwrap();
    vault
        .db
        .execute(
            "INSERT INTO vault_meta (key, value) VALUES ('password_check', ?1)",
            params![check],
        )
        .unwrap();
    let conn = Connection::new("h", "example.com");
    vault.save_connection(&conn, None).unwrap();
    let legacy_pw_blob = encrypt(b"old-secret", pw.as_bytes()).unwrap();
    vault
        .db
        .execute(
            "UPDATE connections SET password = ?1 WHERE id = ?2",
            params![legacy_pw_blob, conn.id.to_string()],
        )
        .unwrap();
    let legacy_api_key = BASE64.encode(encrypt(b"sk-legacy", pw.as_bytes()).unwrap());
    vault.set_setting("ai_api_key", &legacy_api_key).unwrap();

    // Wrong password must still fail before any migration runs.
    assert!(vault.unlock("not-it").is_err());

    vault.unlock(pw).unwrap();

    // Secrets read back, and the stored blobs are in the new format.
    assert_eq!(
        vault.get_connection_password(&conn.id).unwrap().as_deref(),
        Some("old-secret")
    );
    assert_eq!(vault.get_ai_api_key().unwrap().as_deref(), Some("sk-legacy"));
    let migrated: Vec<u8> = vault
        .db
        .query_row(
            "SELECT password FROM connections WHERE id = ?1",
            params![conn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(migrated[0], FIELD_FORMAT_V2);

    // A second unlock takes the fast path and still works.
    vault.lock();
    vault.unlock(pw).unwrap();
    assert_eq!(
        vault.get_connection_password(&conn.id).unwrap().as_deref(),
        Some("old-secret")
    );
}


#[test]
fn every_encrypted_field_survives_master_password_change() {
    let mut vault = unlocked_vault();

    // One row in every table with an encrypted column.
    let conn = Connection::new("host-a", "h.example");
    vault.save_connection(&conn, Some("conn-pw")).unwrap();
    vault.set_proxy_password(&conn.id, Some("proxy-pw")).unwrap();

    let key = SshKey::new("k", KeyAlgorithm::Ed25519);
    vault.save_key(&key, Some("PRIVATE-PEM")).unwrap();

    let ident = Identity::new("i");
    vault.save_identity(&ident, Some("ident-pw")).unwrap();

    let proxy_ident = ProxyIdentity::new("p");
    vault.save_proxy_identity(&proxy_ident, Some("proxy-ident-pw")).unwrap();

    let profile = CloudProfile::new("aws-prod", "aws");
    vault.save_cloud_profile(&profile, Some("cloud-secret")).unwrap();

    let peer_id = Uuid::new_v4();
    vault
        .save_sync_peer(&peer_id, "laptop", b"pubkey", Some(b"shared-secret"), &Utc::now())
        .unwrap();

    vault.set_user_password("the-new-master-password").unwrap();

    // Every secret must decrypt under the new master key.
    assert_eq!(
        vault.get_connection_password(&conn.id).unwrap().as_deref(),
        Some("conn-pw")
    );
    assert_eq!(vault.get_proxy_password(&conn.id).unwrap().as_deref(), Some("proxy-pw"));
    assert_eq!(vault.get_key_private(&key.id).unwrap().as_deref(), Some("PRIVATE-PEM"));
    assert_eq!(vault.get_identity_password(&ident.id).unwrap().as_deref(), Some("ident-pw"));
    assert_eq!(
        vault.get_proxy_identity_password(&proxy_ident.id).unwrap().as_deref(),
        Some("proxy-ident-pw")
    );
    assert_eq!(
        vault.get_cloud_profile_secret(&profile.id).unwrap().as_deref(),
        Some("cloud-secret")
    );
    assert_eq!(
        vault.get_sync_peer_shared_secret(&peer_id).unwrap().as_deref(),
        Some(b"shared-secret".as_slice())
    );
}

// ── Session logs ──


#[test]
fn destroy_and_recreate_drops_every_table() {
    let mut vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"sensitive recording").unwrap();
    vault.destroy_and_recreate().unwrap();
    // No table created by create_tables may carry surviving rows.
    let mut stmt = vault
        .db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .unwrap();
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for table in tables {
        let count: i64 = vault
            .db
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "table {table} survived destroy_and_recreate");
    }
}

// ── Session groups ──


#[test]
fn encrypt_decrypt_roundtrip() {
    let password = b"mysecretpassword";
    let plaintext = b"hello world, this is a secret";
    let encrypted = encrypt(plaintext, password).unwrap();
    assert_ne!(encrypted, plaintext);
    assert!(encrypted.len() > plaintext.len());
    let decrypted = decrypt(&encrypted, password).unwrap();
    assert_eq!(decrypted, plaintext);
}


#[test]
fn decrypt_wrong_password_fails() {
    let encrypted = encrypt(b"secret data", b"correct_password").unwrap();
    let result = decrypt(&encrypted, b"wrong_password");
    assert!(result.is_err());
}


#[test]
fn decrypt_truncated_data_fails() {
    let result = decrypt(&[0u8; 10], b"password");
    assert!(result.is_err());
}


#[test]
fn encrypt_produces_different_ciphertext_each_time() {
    let password = b"password";
    let plaintext = b"same data";
    let a = encrypt(plaintext, password).unwrap();
    let b = encrypt(plaintext, password).unwrap();
    assert_ne!(a, b); // random salt + nonce
}

// ── Vault lifecycle ──


#[test]
fn new_vault_has_no_master_password() {
    let vault = temp_vault();
    assert!(!vault.has_master_password().unwrap());
    assert!(vault.is_locked());
}


#[test]
fn set_master_password_unlocks() {
    let mut vault = temp_vault();
    vault.set_master_password("mypass").unwrap();
    assert!(!vault.is_locked());
}


#[test]
fn set_master_password_twice_fails() {
    let mut vault = temp_vault();
    vault.set_master_password("mypass").unwrap();
    let result = vault.set_master_password("another");
    assert!(result.is_err());
}


#[test]
fn lock_and_unlock() {
    let mut vault = temp_vault();
    vault.set_master_password("mypass").unwrap();
    vault.lock();
    assert!(vault.is_locked());
    vault.unlock("mypass").unwrap();
    assert!(!vault.is_locked());
}


#[test]
fn unlock_wrong_password_fails() {
    let mut vault = temp_vault();
    vault.set_master_password("correct").unwrap();
    vault.lock();
    let result = vault.unlock("wrong");
    assert!(result.is_err());
    assert!(vault.is_locked());
}

// ── Connections CRUD ──


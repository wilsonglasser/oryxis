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
    vault
        .set_connection_totp_secret(&conn.id, Some("JBSWY3DPEHPK3PXP"))
        .unwrap();
    vault
        .set_connection_target_password(&conn.id, Some("asset-pw"))
        .unwrap();

    let key = SshKey::new("k", KeyAlgorithm::Ed25519);
    vault.save_key(&key, Some("PRIVATE-PEM")).unwrap();

    let ident = Identity::new("i");
    vault.save_identity(&ident, Some("ident-pw")).unwrap();

    let proxy_ident = ProxyIdentity::new("p");
    vault.save_proxy_identity(&proxy_ident, Some("proxy-ident-pw")).unwrap();

    // Encrypted SETTINGS are a second class, converted alongside the
    // BLOB columns in `convert_all_fields`. They were the exact drift
    // that shipped once (a new column the rotation walk forgot), so
    // pin every one: adding an encrypted setting without a rotation arm
    // must fail here.
    vault.set_ai_api_key("sk-secret-key").unwrap();
    vault
        .set_files_recent_folders(r#"["/srv/logs","/home/me"]"#)
        .unwrap();

    vault.set_user_password("the-new-master-password").unwrap();

    // Every secret must decrypt under the new master key.
    assert_eq!(
        vault.get_connection_password(&conn.id).unwrap().as_deref(),
        Some("conn-pw")
    );
    assert_eq!(vault.get_proxy_password(&conn.id).unwrap().as_deref(), Some("proxy-pw"));
    assert_eq!(
        vault.get_connection_totp_secret(&conn.id).unwrap().as_deref(),
        Some("JBSWY3DPEHPK3PXP")
    );
    assert_eq!(
        vault.get_connection_target_password(&conn.id).unwrap().as_deref(),
        Some("asset-pw")
    );
    assert_eq!(vault.get_key_private(&key.id).unwrap().as_deref(), Some("PRIVATE-PEM"));
    assert_eq!(vault.get_identity_password(&ident.id).unwrap().as_deref(), Some("ident-pw"));
    assert_eq!(
        vault.get_proxy_identity_password(&proxy_ident.id).unwrap().as_deref(),
        Some("proxy-ident-pw")
    );
    // The encrypted settings decrypt under the new master key too.
    assert_eq!(vault.get_ai_api_key().unwrap().as_deref(), Some("sk-secret-key"));
    assert_eq!(
        vault.get_files_recent_folders().unwrap().as_deref(),
        Some(r#"["/srv/logs","/home/me"]"#)
    );
}

// ── Session logs ──


#[test]
fn destroy_and_recreate_drops_every_table() {
    let mut vault = unlocked_vault();
    let log_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();
    vault.create_session_log(&log_id, &conn_id, "host-a").unwrap();
    vault.append_session_data(&log_id, b"sensitive recording", None, false).unwrap();
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


#[test]
fn verify_password_matches_without_changing_state() {
    let mut vault = temp_vault();
    vault.set_master_password("correct").unwrap();
    // Right password -> true, wrong -> false, neither mutates the
    // unlocked state (the change-password form relies on this).
    assert!(vault.verify_password("correct").unwrap());
    assert!(!vault.verify_password("wrong").unwrap());
    assert!(!vault.is_locked());
}

// ── E1: Argon2id auto-tuning ──

#[test]
fn tuned_params_roundtrip() {
    use crate::store::KdfParams;
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    std::mem::forget(tmp);
    // Inject tuned params (skip the ~1s calibration wall-time in tests)
    // and stash a secret under the tuned key.
    let tuned = KdfParams { m_kib: 32768, t: 3, p: 1 };
    {
        let mut vault = VaultStore::open(&path).unwrap();
        vault.set_master_password_with_params("tuned-pass", tuned).unwrap();
        assert_eq!(vault.kdf_params(), Some(tuned));
        vault.set_setting("probe", "guarded").unwrap();
    }
    // Reopen from disk: the params travel inside the vault file, so the
    // same password unlocks and the secret reads back.
    {
        let mut vault = VaultStore::open(&path).unwrap();
        assert_eq!(vault.kdf_params(), Some(tuned), "params persisted on disk");
        vault.unlock("tuned-pass").unwrap();
        assert_eq!(vault.get_setting("probe").unwrap().as_deref(), Some("guarded"));
        assert!(vault.unlock("wrong").is_err());
    }
}

#[test]
fn legacy_vault_without_params_unlocks() {
    let mut vault = temp_vault();
    vault.set_master_password("legacy").unwrap();
    // A pre-E1 vault has no kdf_params row: delete it to simulate one.
    vault
        .db
        .execute("DELETE FROM vault_meta WHERE key = 'kdf_params'", [])
        .unwrap();
    assert_eq!(vault.kdf_params(), None, "no params row = untuned");
    vault.lock();
    // Unlock still works: derive_vault_key falls back to the defaults.
    vault.unlock("legacy").unwrap();
    assert!(!vault.is_locked());
}

#[test]
fn rotation_writes_params() {
    use crate::store::KdfParams;
    // A vault set without a password change (default params), then a
    // password change with tuned params, must persist the new params
    // and unlock under the new password.
    let mut vault = temp_vault();
    vault.set_master_password("first").unwrap();
    let tuned = KdfParams { m_kib: 65536, t: 4, p: 1 };
    vault.set_user_password_with_params("second", tuned).unwrap();
    assert_eq!(vault.kdf_params(), Some(tuned));
    vault.lock();
    vault.unlock("second").unwrap();
    assert!(!vault.is_locked());
    // The old password no longer works (salt + params rotated).
    vault.lock();
    assert!(vault.unlock("first").is_err());
}

#[test]
fn removing_password_resets_to_default_params() {
    use crate::store::KdfParams;
    let mut vault = temp_vault();
    vault
        .set_master_password_with_params("secret", KdfParams { m_kib: 65536, t: 4, p: 1 })
        .unwrap();
    vault.remove_user_password().unwrap();
    // A passwordless vault has no entropy to protect: params reset to
    // the default profile.
    assert_eq!(vault.kdf_params(), Some(KdfParams::DEFAULT));
}

#[test]
fn failed_rotation_leaves_salt_and_unlock_intact() {
    // A password change that fails mid-way (here: one secret blob is
    // corrupt, so re_encrypt_all errors) must roll back ATOMICALLY,
    // including the new kdf_salt + params. A salt committed outside the
    // transaction would brick the vault: secrets stay under the old key
    // while unlock derives over the new salt, so no password ever works
    // again.
    let mut vault = temp_vault();
    vault.set_master_password("original").unwrap();
    let conn = Connection::new("h", "example.com");
    vault.save_connection(&conn, Some("secret")).unwrap();
    // Corrupt the encrypted blob so the re-encryption pass fails.
    vault
        .db
        .execute(
            "UPDATE connections SET password = ?1 WHERE id = ?2",
            params![vec![0xFFu8; 7], conn.id.to_string()],
        )
        .unwrap();
    let salt_before: Vec<u8> = vault
        .db
        .query_row(
            "SELECT value FROM vault_meta WHERE key = 'kdf_salt'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(vault.set_user_password("next").is_err());

    let salt_after: Vec<u8> = vault
        .db
        .query_row(
            "SELECT value FROM vault_meta WHERE key = 'kdf_salt'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(salt_before, salt_after, "failed rotation must not touch the salt");
    // The vault still unlocks under the ORIGINAL password only.
    vault.lock();
    assert!(vault.unlock("next").is_err());
    vault.unlock("original").unwrap();
    assert!(!vault.is_locked());
}

#[test]
fn calibrate_respects_floors() {
    let p = crate::store::calibrate_kdf();
    // Never weaker than the crate default; parallelism pinned to 1.
    assert!(p.m_kib >= crate::store::KdfParams::DEFAULT.m_kib);
    assert!(p.t >= crate::store::KdfParams::DEFAULT.t);
    assert!(p.t <= 8);
    assert_eq!(p.p, 1);
}

// ── Connections CRUD ──


use super::*;

#[test]
fn save_and_list_keys() {
    let vault = unlocked_vault();
    let key = SshKey::new("my-key", KeyAlgorithm::Ed25519);
    vault.save_key(&key, Some("-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----")).unwrap();

    let keys = vault.list_keys().unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].label, "my-key");
}


#[test]
fn key_private_encrypted_and_retrievable() {
    let vault = unlocked_vault();
    let key = SshKey::new("test-key", KeyAlgorithm::Rsa4096);
    let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----";
    vault.save_key(&key, Some(pem)).unwrap();

    let retrieved = vault.get_key_private(&key.id).unwrap();
    assert_eq!(retrieved, Some(pem.to_string()));
}


#[test]
fn delete_key() {
    let vault = unlocked_vault();
    let key = SshKey::new("disposable", KeyAlgorithm::Ed25519);
    vault.save_key(&key, None).unwrap();
    vault.delete_key(&key.id).unwrap();
    assert_eq!(vault.list_keys().unwrap().len(), 0);
}

// ── Groups CRUD ──


#[test]
fn key_has_updated_at() {
    let vault = unlocked_vault();
    let key = SshKey::new("test-key", KeyAlgorithm::Ed25519);
    assert!(key.updated_at.timestamp() > 0);
    vault.save_key(&key, None).unwrap();

    let keys = vault.list_keys().unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].updated_at.timestamp() > 0);
}


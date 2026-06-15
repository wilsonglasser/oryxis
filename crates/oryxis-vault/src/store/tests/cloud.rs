use super::*;

#[test]
fn save_and_list_cloud_profiles() {
    let vault = unlocked_vault();
    let mut p = CloudProfile::new("prod-aws", "aws");
    p.auth_kind = "profile".into();
    p.config = r#"{"profile_name":"production","region":"us-east-1"}"#.into();
    vault.save_cloud_profile(&p, None).unwrap();

    let listed = vault.list_cloud_profiles().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].label, "prod-aws");
    assert_eq!(listed[0].provider, "aws");
    assert_eq!(listed[0].auth_kind, "profile");
    assert!(listed[0].config.contains("us-east-1"));
}


#[test]
fn cloud_profile_secret_round_trip() {
    let vault = unlocked_vault();
    let p = CloudProfile::new("deploy-key", "aws");
    vault.save_cloud_profile(&p, Some("AKIAIOSFODNN7EXAMPLE")).unwrap();

    let got = vault.get_cloud_profile_secret(&p.id).unwrap();
    assert_eq!(got.as_deref(), Some("AKIAIOSFODNN7EXAMPLE"));
}


#[test]
fn cloud_profile_secret_preserved_when_none_passed() {
    let vault = unlocked_vault();
    let mut p = CloudProfile::new("aws-prof", "aws");
    vault.save_cloud_profile(&p, Some("first-secret")).unwrap();

    // Re-save with None, secret must stay intact (tri-state).
    p.label = "renamed".into();
    vault.save_cloud_profile(&p, None).unwrap();

    let got = vault.get_cloud_profile_secret(&p.id).unwrap();
    assert_eq!(got.as_deref(), Some("first-secret"));
}


#[test]
fn cloud_profile_secret_cleared_with_empty_string() {
    let vault = unlocked_vault();
    let p = CloudProfile::new("aws-prof", "aws");
    vault.save_cloud_profile(&p, Some("temp-secret")).unwrap();

    // `Some("")` clears the column.
    vault.save_cloud_profile(&p, Some("")).unwrap();
    let got = vault.get_cloud_profile_secret(&p.id).unwrap();
    assert!(got.is_none());
}


#[test]
fn delete_cloud_profile_removes_row() {
    let vault = unlocked_vault();
    let p = CloudProfile::new("temp", "aws");
    vault.save_cloud_profile(&p, Some("s")).unwrap();
    assert_eq!(vault.list_cloud_profiles().unwrap().len(), 1);

    vault.delete_cloud_profile(&p.id).unwrap();
    assert!(vault.list_cloud_profiles().unwrap().is_empty());
}


/// Critical: the plaintext `config` JSON column must never carry the
/// secret. Confirms credentials live only in the encrypted `secret`
/// column. Mirror of `proxy_password_does_not_leak_into_proxy_column`.
#[test]
fn cloud_profile_secret_does_not_leak_into_config_column() {
    let vault = unlocked_vault();
    let mut p = CloudProfile::new("leaky", "aws");
    p.config = r#"{"region":"us-east-1"}"#.into();
    vault.save_cloud_profile(&p, Some("super-secret-key")).unwrap();

    let raw_config: String = vault
        .db
        .query_row(
            "SELECT config FROM cloud_profiles WHERE id = ?1",
            params![p.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !raw_config.contains("super-secret-key"),
        "secret leaked into plaintext config column: {raw_config}"
    );
}

// ── Connection.cloud_ref + initial_command ──


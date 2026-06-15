use super::*;

#[test]
fn connection_session_logging_override_round_trips() {
    let vault = unlocked_vault();
    // All three states survive a save/list cycle: None (inherit
    // global), Some(true) (force on), Some(false) (force off).
    for value in [None, Some(true), Some(false)] {
        let mut conn = Connection::new("h", "example.com");
        conn.session_logging = value;
        vault.save_connection(&conn, None).unwrap();
        let loaded = vault
            .list_connections()
            .unwrap()
            .into_iter()
            .find(|c| c.id == conn.id)
            .expect("connection listed");
        assert_eq!(loaded.session_logging, value);
    }
}

// ── Crypto ──


#[test]
fn save_and_list_connections() {
    let vault = unlocked_vault();
    let conn = Connection::new("prod-web", "192.168.1.10");
    vault.save_connection(&conn, Some("secret123")).unwrap();

    let conns = vault.list_connections().unwrap();
    assert_eq!(conns.len(), 1);
    assert_eq!(conns[0].label, "prod-web");
    assert_eq!(conns[0].hostname, "192.168.1.10");
}


#[test]
fn connection_password_encrypted_and_retrievable() {
    let vault = unlocked_vault();
    let conn = Connection::new("test", "host.example.com");
    vault.save_connection(&conn, Some("supersecret")).unwrap();

    let pw = vault.get_connection_password(&conn.id).unwrap();
    assert_eq!(pw, Some("supersecret".to_string()));
}


#[test]
fn connection_password_not_readable_when_locked() {
    let mut vault = unlocked_vault();
    let conn = Connection::new("test", "host");
    vault.save_connection(&conn, Some("pw")).unwrap();
    vault.lock();

    let result = vault.get_connection_password(&conn.id);
    assert!(result.is_err());
}


#[test]
fn delete_connection() {
    let vault = unlocked_vault();
    let conn = Connection::new("temp", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();
    assert_eq!(vault.list_connections().unwrap().len(), 1);

    vault.delete_connection(&conn.id).unwrap();
    assert_eq!(vault.list_connections().unwrap().len(), 0);
}


#[test]
fn update_connection_preserves_password() {
    let vault = unlocked_vault();
    let mut conn = Connection::new("server", "1.2.3.4");
    vault.save_connection(&conn, Some("original_pw")).unwrap();

    conn.label = "server-renamed".into();
    vault.save_connection(&conn, None).unwrap(); // no password = keep existing

    let pw = vault.get_connection_password(&conn.id).unwrap();
    assert_eq!(pw, Some("original_pw".to_string()));

    let conns = vault.list_connections().unwrap();
    assert_eq!(conns[0].label, "server-renamed");
}


#[test]
fn terminal_theme_round_trip() {
    // Per-host terminal_theme survives the INSERT/SELECT cycle
    // and `None` is preserved (not coerced to "").
    let vault = unlocked_vault();
    let mut with_theme = Connection::new("themed", "host.example.com");
    with_theme.terminal_theme = Some("Dracula".to_string());
    vault.save_connection(&with_theme, None).unwrap();

    let without_theme = Connection::new("plain", "other.example.com");
    vault.save_connection(&without_theme, None).unwrap();

    let conns = vault.list_connections().unwrap();
    let themed = conns.iter().find(|c| c.label == "themed").unwrap();
    assert_eq!(themed.terminal_theme.as_deref(), Some("Dracula"));
    let plain = conns.iter().find(|c| c.label == "plain").unwrap();
    assert!(plain.terminal_theme.is_none());
}


#[test]
fn keepalive_interval_round_trip() {
    // The three meaningful states (None / Some(n) / Some(0)) must
    // each round-trip through the SQLite save+load pipeline. Some(0)
    // is distinct from None: the former means "explicitly disabled
    // on this host", the latter means "inherit the global setting".
    let vault = unlocked_vault();

    let inherits = Connection::new("inherits", "a.example.com");
    vault.save_connection(&inherits, None).unwrap();

    let mut overrides = Connection::new("overrides", "b.example.com");
    overrides.keepalive_interval = Some(60);
    vault.save_connection(&overrides, None).unwrap();

    let mut disabled = Connection::new("disabled", "c.example.com");
    disabled.keepalive_interval = Some(0);
    vault.save_connection(&disabled, None).unwrap();

    let conns = vault.list_connections().unwrap();
    let i = conns.iter().find(|c| c.label == "inherits").unwrap();
    let o = conns.iter().find(|c| c.label == "overrides").unwrap();
    let d = conns.iter().find(|c| c.label == "disabled").unwrap();
    assert_eq!(i.keepalive_interval, None);
    assert_eq!(o.keepalive_interval, Some(60));
    assert_eq!(d.keepalive_interval, Some(0));
}


#[test]
fn proxy_password_encrypted_round_trip() {
    let vault = unlocked_vault();
    let conn = Connection::new("h", "host.example.com");
    vault.save_connection(&conn, None).unwrap();

    vault.set_proxy_password(&conn.id, Some("proxy-secret")).unwrap();
    let pw = vault.get_proxy_password(&conn.id).unwrap();
    assert_eq!(pw.as_deref(), Some("proxy-secret"));
}


#[test]
fn proxy_password_clears_on_none_or_empty() {
    let vault = unlocked_vault();
    let conn = Connection::new("h", "host");
    vault.save_connection(&conn, None).unwrap();
    vault.set_proxy_password(&conn.id, Some("first")).unwrap();

    // Empty string is treated the same as None, both clear.
    vault.set_proxy_password(&conn.id, Some("")).unwrap();
    assert_eq!(vault.get_proxy_password(&conn.id).unwrap(), None);

    vault.set_proxy_password(&conn.id, Some("again")).unwrap();
    vault.set_proxy_password(&conn.id, None).unwrap();
    assert_eq!(vault.get_proxy_password(&conn.id).unwrap(), None);
}


/// Critical: the plaintext `proxy` JSON column must never carry the
/// password. Confirms the credential lives only in the encrypted
/// `proxy_password` column.
#[test]
fn proxy_password_does_not_leak_into_proxy_column() {
    use oryxis_core::models::connection::{ProxyConfig, ProxyType};
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "host");
    conn.proxy = Some(ProxyConfig {
        proxy_type: ProxyType::Http,
        host: "proxy.example.com".into(),
        port: 8080,
        username: Some("alice".into()),
        password: Some("should-not-persist".into()),
    });
    vault.save_connection(&conn, None).unwrap();

    let raw_proxy: Option<String> = vault
        .db
        .query_row(
            "SELECT proxy FROM connections WHERE id = ?1",
            params![conn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let raw = raw_proxy.unwrap();
    assert!(
        !raw.contains("should-not-persist"),
        "password leaked into plaintext proxy column: {raw}"
    );

    // After reloading, the in-memory model has no password until the
    // caller hydrates it from the encrypted column.
    let conns = vault.list_connections().unwrap();
    let proxy = conns[0].proxy.as_ref().unwrap();
    assert!(proxy.password.is_none());
    assert_eq!(proxy.host, "proxy.example.com");
    assert_eq!(proxy.username.as_deref(), Some("alice"));
}

// ── Keys CRUD ──


#[test]
fn connection_mcp_enabled_default_true() {
    let vault = unlocked_vault();
    let conn = Connection::new("test", "10.0.0.1");
    assert!(conn.mcp_enabled);
    vault.save_connection(&conn, None).unwrap();

    let conns = vault.list_connections().unwrap();
    assert_eq!(conns.len(), 1);
    assert!(conns[0].mcp_enabled);
}


#[test]
fn connection_mcp_enabled_toggle() {
    let vault = unlocked_vault();
    let mut conn = Connection::new("test", "10.0.0.1");
    conn.mcp_enabled = false;
    vault.save_connection(&conn, None).unwrap();

    let conns = vault.list_connections().unwrap();
    assert!(!conns[0].mcp_enabled);

    let mcp_conns = vault.list_mcp_connections().unwrap();
    assert_eq!(mcp_conns.len(), 0);
}


#[test]
fn list_mcp_connections_filters() {
    let vault = unlocked_vault();

    let mut c1 = Connection::new("enabled", "10.0.0.1");
    c1.mcp_enabled = true;
    vault.save_connection(&c1, None).unwrap();

    let mut c2 = Connection::new("disabled", "10.0.0.2");
    c2.mcp_enabled = false;
    vault.save_connection(&c2, None).unwrap();

    let all = vault.list_connections().unwrap();
    assert_eq!(all.len(), 2);

    let mcp = vault.list_mcp_connections().unwrap();
    assert_eq!(mcp.len(), 1);
    assert_eq!(mcp[0].label, "enabled");
}

// ── Updated timestamps on models ──


#[test]
fn connection_cloud_ref_and_initial_command_round_trip() {
    use oryxis_core::models::cloud::{CloudRef, CloudResourceType, TransportKind};

    let vault = unlocked_vault();
    let profile_id = uuid::Uuid::new_v4();
    let mut conn = Connection::new("prod-web-1", "10.0.0.1");
    conn.cloud_ref = Some(CloudRef {
        profile_id,
        resource_type: CloudResourceType::Ec2,
        resource_id: "i-0abcdef".into(),
        region: Some("us-east-1".into()),
        transport_pref: TransportKind::InstanceConnect,
        auto_refresh_hostname: true,
        orphaned_at: None,
    });
    conn.initial_command = Some("exec bash".into());
    vault.save_connection(&conn, None).unwrap();

    let listed = vault.list_connections().unwrap();
    let back = listed.iter().find(|c| c.id == conn.id).unwrap();
    let cr = back.cloud_ref.as_ref().expect("cloud_ref preserved");
    assert_eq!(cr.profile_id, profile_id);
    assert_eq!(cr.resource_id, "i-0abcdef");
    assert_eq!(cr.transport_pref, TransportKind::InstanceConnect);
    assert!(cr.auto_refresh_hostname);
    assert_eq!(back.initial_command.as_deref(), Some("exec bash"));
}

// ── Group.cloud_query ──


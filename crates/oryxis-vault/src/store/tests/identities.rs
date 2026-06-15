use super::*;

#[test]
fn proxy_identity_round_trip() {
    let vault = unlocked_vault();
    let mut pi = ProxyIdentity::new("home-bastion");
    pi.proxy_type = ProxyType::Socks5;
    pi.host = "proxy.home.lan".into();
    pi.port = 1080;
    pi.username = Some("alice".into());

    vault.save_proxy_identity(&pi, Some("topsecret")).unwrap();
    let listed = vault.list_proxy_identities().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].label, "home-bastion");
    assert_eq!(listed[0].host, "proxy.home.lan");
    assert_eq!(listed[0].port, 1080);
    assert_eq!(listed[0].username.as_deref(), Some("alice"));
    assert_eq!(
        vault.get_proxy_identity_password(&pi.id).unwrap().as_deref(),
        Some("topsecret"),
    );
}


#[test]
fn proxy_identity_delete_cascades_to_connections() {
    let vault = unlocked_vault();
    let pi = ProxyIdentity::new("temp");
    vault.save_proxy_identity(&pi, None).unwrap();

    let mut conn = Connection::new("h", "host");
    conn.proxy_identity_id = Some(pi.id);
    vault.save_connection(&conn, None).unwrap();

    // Sanity, the connection lists the identity reference.
    let conns = vault.list_connections().unwrap();
    assert_eq!(conns[0].proxy_identity_id, Some(pi.id));

    vault.delete_proxy_identity(&pi.id).unwrap();
    let conns = vault.list_connections().unwrap();
    assert_eq!(
        conns[0].proxy_identity_id, None,
        "delete_proxy_identity should NULL out connection.proxy_identity_id",
    );
}


#[test]
fn resolve_proxy_prefers_identity_over_inline() {
    use oryxis_core::models::connection::ProxyConfig;
    let vault = unlocked_vault();
    let mut pi = ProxyIdentity::new("ident-proxy");
    pi.proxy_type = ProxyType::Http;
    pi.host = "ident.example".into();
    pi.port = 8080;
    pi.username = Some("ident-user".into());
    vault.save_proxy_identity(&pi, Some("ident-pw")).unwrap();

    let mut conn = Connection::new("h", "host");
    conn.proxy = Some(ProxyConfig {
        proxy_type: ProxyType::Socks5,
        host: "inline.example".into(),
        port: 1080,
        username: Some("inline-user".into()),
        password: None,
    });
    conn.proxy_identity_id = Some(pi.id);
    vault.save_connection(&conn, None).unwrap();
    // Inline password set too, should be ignored once identity wins.
    vault
        .set_proxy_password(&conn.id, Some("inline-pw"))
        .unwrap();

    let resolved = vault.resolve_proxy(&conn).unwrap().unwrap();
    assert_eq!(resolved.host, "ident.example", "identity should win over inline");
    assert_eq!(resolved.port, 8080);
    assert_eq!(resolved.username.as_deref(), Some("ident-user"));
    assert_eq!(resolved.password.as_deref(), Some("ident-pw"));
}


#[test]
fn resolve_proxy_dangling_identity_returns_none() {
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "host");
    // Reference an identity that doesn't exist, must not error.
    conn.proxy_identity_id = Some(uuid::Uuid::new_v4());
    vault.save_connection(&conn, None).unwrap();

    let resolved = vault.resolve_proxy(&conn).unwrap();
    assert!(resolved.is_none());
}


#[test]
fn resolve_proxy_falls_back_to_inline_when_no_identity() {
    use oryxis_core::models::connection::ProxyConfig;
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "host");
    conn.proxy = Some(ProxyConfig {
        proxy_type: ProxyType::Socks5,
        host: "inline.example".into(),
        port: 1080,
        username: None,
        password: None,
    });
    vault.save_connection(&conn, None).unwrap();
    vault.set_proxy_password(&conn.id, Some("inline-pw")).unwrap();

    let resolved = vault.resolve_proxy(&conn).unwrap().unwrap();
    assert_eq!(resolved.host, "inline.example");
    assert_eq!(resolved.password.as_deref(), Some("inline-pw"));
}


use super::*;

#[test]
fn export_import_roundtrip() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

    let vault = unlocked_vault();

    // Populate vault
    let conn = Connection::new("prod-web", "192.168.1.10");
    vault.save_connection(&conn, Some("secret123")).unwrap();

    let g = Group::new("Production");
    vault.save_group(&g).unwrap();

    let s = Snippet::new("deploy", "make deploy");
    vault.save_snippet(&s).unwrap();

    let kh = KnownHost::new("192.168.1.10", 22, "ed25519", "SHA256:abc");
    vault.save_known_host(&kh).unwrap();

    let mut pf = oryxis_core::models::port_forward_rule::PortForwardRule::new(
        "db tunnel",
        oryxis_core::models::port_forward_rule::ForwardKind::Local,
        conn.id,
    );
    pf.listen_port = 5432;
    pf.target_host = "10.0.0.5".into();
    pf.target_port = 5432;
    vault.save_port_forward_rule(&pf).unwrap();

    // Export
    let export_pw = "export-password";
    let data = export_vault(&vault, export_pw, ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();

    // Verify header
    assert_eq!(&data[..6], b"ORYXIS");

    // Import into fresh vault
    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, export_pw).unwrap();

    assert_eq!(result.connections_added, 1);
    assert_eq!(result.groups_added, 1);
    assert_eq!(result.snippets_added, 1);
    assert_eq!(result.known_hosts_added, 1);
    assert_eq!(result.port_forward_rules_added, 1);

    // Verify data
    let conns = vault2.list_connections().unwrap();
    assert_eq!(conns.len(), 1);
    assert_eq!(conns[0].label, "prod-web");

    let pw = vault2.get_connection_password(&conns[0].id).unwrap();
    assert_eq!(pw, Some("secret123".into()));

    assert_eq!(vault2.list_groups().unwrap().len(), 1);
    assert_eq!(vault2.list_snippets().unwrap().len(), 1);
    assert_eq!(vault2.list_known_hosts().unwrap().len(), 1);
    let rules = vault2.list_port_forward_rules().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].listen_port, 5432);
    assert_eq!(rules[0].target_host, "10.0.0.5");
}


/// Round-trip a connection that uses both an inline proxy (with
/// password in its own encrypted column) and a saved proxy
/// identity (with its own password). Both passwords + the
/// identity reference must survive export → import.
#[test]
fn export_import_proxy_round_trip() {
    use crate::portable::{export_vault, import_vault, ExportFilter, ExportOptions};
    use oryxis_core::models::connection::{ProxyConfig, ProxyType};

    let vault = unlocked_vault();

    // Saved proxy identity (with password)
    let mut pi = ProxyIdentity::new("corp-bastion");
    pi.proxy_type = ProxyType::Http;
    pi.host = "proxy.corp.local".into();
    pi.port = 8080;
    pi.username = Some("alice".into());
    vault.save_proxy_identity(&pi, Some("ident-pw")).unwrap();

    // Connection 1: links to the saved identity
    let mut conn_id = Connection::new("via-identity", "10.0.0.1");
    conn_id.proxy_identity_id = Some(pi.id);
    vault.save_connection(&conn_id, None).unwrap();

    // Connection 2: inline proxy with its own password
    let mut conn_inline = Connection::new("via-inline", "10.0.0.2");
    conn_inline.proxy = Some(ProxyConfig {
        proxy_type: ProxyType::Socks5,
        host: "inline.proxy".into(),
        port: 1080,
        username: Some("bob".into()),
        password: None,
    });
    vault.save_connection(&conn_inline, None).unwrap();
    vault
        .set_proxy_password(&conn_inline.id, Some("inline-pw"))
        .unwrap();

    // Export everything
    let data = export_vault(
        &vault,
        "export-pw",
        ExportOptions {
            include_private_keys: false,
            filter: ExportFilter::All,
        },
    )
    .unwrap();

    // Import into a fresh vault
    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "export-pw").unwrap();
    assert_eq!(result.connections_added, 2);
    assert_eq!(result.proxy_identities_added, 1);

    // Saved identity round-tripped, including its password.
    let pi_round = vault2.list_proxy_identities().unwrap();
    assert_eq!(pi_round.len(), 1);
    assert_eq!(pi_round[0].host, "proxy.corp.local");
    assert_eq!(
        vault2
            .get_proxy_identity_password(&pi_round[0].id)
            .unwrap()
            .as_deref(),
        Some("ident-pw"),
    );

    // The identity-linked connection still references the same id.
    let conns = vault2.list_connections().unwrap();
    let by_label = |l: &str| conns.iter().find(|c| c.label == l).unwrap();
    assert_eq!(by_label("via-identity").proxy_identity_id, Some(pi.id));

    // The inline proxy survived with its password in the encrypted
    // column (NOT inside the proxy JSON).
    let inline_conn = by_label("via-inline");
    assert_eq!(inline_conn.proxy_identity_id, None);
    assert_eq!(
        inline_conn.proxy.as_ref().map(|p| p.host.clone()),
        Some("inline.proxy".into()),
    );
    assert_eq!(
        vault2
            .get_proxy_password(&inline_conn.id)
            .unwrap()
            .as_deref(),
        Some("inline-pw"),
    );
}


#[test]
fn export_wrong_password_fails() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

    let vault = unlocked_vault();
    let conn = Connection::new("test", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();

    let data = export_vault(&vault, "correct", ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();
    let result = import_vault(&vault, &data, "wrong");
    assert!(result.is_err());
}


#[test]
fn export_invalid_file_rejected() {
    use crate::portable::{import_vault, is_valid_export};

    let vault = unlocked_vault();
    assert!(!is_valid_export(b"not an oryxis file"));
    assert!(import_vault(&vault, b"not an oryxis file", "pw").is_err());
}


#[test]
fn import_skip_existing() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

    let vault = unlocked_vault();
    let conn = Connection::new("server", "10.0.0.1");
    vault.save_connection(&conn, Some("pw1")).unwrap();

    let data = export_vault(&vault, "pass", ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();

    // Import again into same vault, should skip
    let result = import_vault(&vault, &data, "pass").unwrap();
    assert_eq!(result.connections_skipped, 1);
    assert_eq!(result.connections_added, 0);

    // Still only 1 connection
    assert_eq!(vault.list_connections().unwrap().len(), 1);
}


#[test]
fn import_updates_newer() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

    let vault1 = unlocked_vault();
    let mut conn = Connection::new("server", "10.0.0.1");
    conn.updated_at = chrono::Utc::now();
    vault1.save_connection(&conn, Some("old_pw")).unwrap();

    // Export
    let data = export_vault(&vault1, "pass", ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();

    // Create vault2 with same connection but older timestamp
    let vault2 = unlocked_vault();
    let mut old_conn = conn.clone();
    old_conn.updated_at = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z").unwrap().with_timezone(&chrono::Utc);
    vault2.save_connection(&old_conn, Some("old_pw")).unwrap();

    // Import, should update because export is newer
    let result = import_vault(&vault2, &data, "pass").unwrap();
    assert_eq!(result.connections_updated, 1);
    assert_eq!(result.connections_added, 0);
}


#[test]
fn export_with_keys() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter, export_includes_keys};

    let vault = unlocked_vault();

    // Generate a key
    let generated = crate::keygen::generate_ed25519("test-key").unwrap();
    vault.save_key(&generated.key, Some(&generated.private_pem)).unwrap();

    // Export WITH keys
    let data_with = export_vault(&vault, "pass", ExportOptions { include_private_keys: true, filter: ExportFilter::All }).unwrap();
    assert!(export_includes_keys(&data_with));

    // Export WITHOUT keys
    let data_without = export_vault(&vault, "pass", ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();
    assert!(!export_includes_keys(&data_without));

    // Import with keys into fresh vault
    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data_with, "pass").unwrap();
    assert_eq!(result.keys_added, 1);

    let pk = vault2.get_key_private(&generated.key.id).unwrap();
    assert!(pk.is_some());

    // Import without keys, key added but no private key
    let vault3 = unlocked_vault();
    let result = import_vault(&vault3, &data_without, "pass").unwrap();
    assert_eq!(result.keys_added, 1);

    let pk = vault3.get_key_private(&generated.key.id).unwrap();
    assert!(pk.is_none());
}

// ── Sync Peers CRUD ──


#[test]
fn share_single_host() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

    let vault = unlocked_vault();

    let c1 = Connection::new("shared-host", "10.0.0.1");
    vault.save_connection(&c1, Some("pw1")).unwrap();
    let c2 = Connection::new("private-host", "10.0.0.2");
    vault.save_connection(&c2, Some("pw2")).unwrap();

    // Share only c1
    let data = export_vault(&vault, "share-pass", ExportOptions {
        include_private_keys: false,
        filter: ExportFilter::Hosts(vec![c1.id]),
    }).unwrap();

    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "share-pass").unwrap();

    assert_eq!(result.connections_added, 1);
    let conns = vault2.list_connections().unwrap();
    assert_eq!(conns.len(), 1);
    assert_eq!(conns[0].label, "shared-host");
}


#[test]
fn share_group() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

    let vault = unlocked_vault();

    let g = Group::new("Team");
    vault.save_group(&g).unwrap();

    let mut c1 = Connection::new("web", "10.0.0.1");
    c1.group_id = Some(g.id);
    vault.save_connection(&c1, None).unwrap();

    let mut c2 = Connection::new("db", "10.0.0.2");
    c2.group_id = Some(g.id);
    vault.save_connection(&c2, None).unwrap();

    let c3 = Connection::new("personal", "10.0.0.3");
    vault.save_connection(&c3, None).unwrap();

    // Share group
    let data = export_vault(&vault, "pass", ExportOptions {
        include_private_keys: false,
        filter: ExportFilter::Group(g.id),
    }).unwrap();

    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "pass").unwrap();

    assert_eq!(result.connections_added, 2);
    assert_eq!(result.groups_added, 1);
    let conns = vault2.list_connections().unwrap();
    assert_eq!(conns.len(), 2);
}


#[test]
fn share_includes_dependencies() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};
    use oryxis_core::models::identity::Identity;

    let vault = unlocked_vault();

    // Create key
    let generated = crate::keygen::generate_ed25519("shared-key").unwrap();
    vault.save_key(&generated.key, Some(&generated.private_pem)).unwrap();

    // Create identity
    let mut ident = Identity::new("shared-ident");
    ident.key_id = Some(generated.key.id);
    vault.save_identity(&ident, Some("ident-pw")).unwrap();

    // Create group
    let g = Group::new("Shared");
    vault.save_group(&g).unwrap();

    // Create connection referencing all deps
    let mut conn = Connection::new("server", "10.0.0.1");
    conn.key_id = Some(generated.key.id);
    conn.identity_id = Some(ident.id);
    conn.group_id = Some(g.id);
    vault.save_connection(&conn, Some("conn-pw")).unwrap();

    // Create unrelated data
    let unrelated = Connection::new("other", "10.0.0.2");
    vault.save_connection(&unrelated, None).unwrap();
    let unrelated_key = SshKey::new("other-key", KeyAlgorithm::Ed25519);
    vault.save_key(&unrelated_key, None).unwrap();

    // Share only the one connection
    let data = export_vault(&vault, "pass", ExportOptions {
        include_private_keys: true,
        filter: ExportFilter::Hosts(vec![conn.id]),
    }).unwrap();

    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "pass").unwrap();

    // Should have 1 connection, 1 key, 1 identity, 1 group
    assert_eq!(result.connections_added, 1);
    assert_eq!(result.keys_added, 1);
    assert_eq!(result.identities_added, 1);
    assert_eq!(result.groups_added, 1);

    // Should NOT have unrelated data
    assert_eq!(vault2.list_connections().unwrap().len(), 1);
    assert_eq!(vault2.list_keys().unwrap().len(), 1);

    // Password should be preserved
    let pw = vault2.get_connection_password(&conn.id).unwrap();
    assert_eq!(pw, Some("conn-pw".into()));
}


#[test]
fn share_no_snippets_or_known_hosts() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

    let vault = unlocked_vault();

    let conn = Connection::new("host", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();

    let s = Snippet::new("test", "echo hi");
    vault.save_snippet(&s).unwrap();

    let kh = KnownHost::new("10.0.0.1", 22, "ed25519", "SHA256:abc");
    vault.save_known_host(&kh).unwrap();

    // Share only the connection
    let data = export_vault(&vault, "pass", ExportOptions {
        include_private_keys: false,
        filter: ExportFilter::Hosts(vec![conn.id]),
    }).unwrap();

    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "pass").unwrap();

    assert_eq!(result.connections_added, 1);
    assert_eq!(result.snippets_added, 0);
    assert_eq!(result.known_hosts_added, 0);
}

// ── Settings (key/value) ─────────────────────────────────────
// Backs the app's user-preference persistence. Worth pinning
// explicitly since the UI relies on round-trip identity for
// anything stored as a string (timeouts, keepalive, parallelism,
// booleans-as-strings, etc.).


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
    let data = export_vault(&vault, export_pw, ExportOptions { include_private_keys: false, filter: ExportFilter::All, selection: crate::portable::ExportSelection::all() }).unwrap();

    // Verify header
    assert_eq!(&data[..6], b"ORYXIS");

    // Import into fresh vault
    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, export_pw, &crate::portable::ExportSelection::all()).unwrap();

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
            selection: crate::portable::ExportSelection::all(),
        },
    )
    .unwrap();

    // Import into a fresh vault
    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "export-pw", &crate::portable::ExportSelection::all()).unwrap();
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

    let data = export_vault(&vault, "correct", ExportOptions { include_private_keys: false, filter: ExportFilter::All, selection: crate::portable::ExportSelection::all() }).unwrap();
    let result = import_vault(&vault, &data, "wrong", &crate::portable::ExportSelection::all());
    assert!(result.is_err());
}


#[test]
fn export_invalid_file_rejected() {
    use crate::portable::{import_vault, is_valid_export};

    let vault = unlocked_vault();
    assert!(!is_valid_export(b"not an oryxis file"));
    assert!(import_vault(&vault, b"not an oryxis file", "pw", &crate::portable::ExportSelection::all()).is_err());
}


#[test]
fn import_skip_existing() {
    use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

    let vault = unlocked_vault();
    let conn = Connection::new("server", "10.0.0.1");
    vault.save_connection(&conn, Some("pw1")).unwrap();

    let data = export_vault(&vault, "pass", ExportOptions { include_private_keys: false, filter: ExportFilter::All, selection: crate::portable::ExportSelection::all() }).unwrap();

    // Import again into same vault, should skip
    let result = import_vault(&vault, &data, "pass", &crate::portable::ExportSelection::all()).unwrap();
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
    let data = export_vault(&vault1, "pass", ExportOptions { include_private_keys: false, filter: ExportFilter::All, selection: crate::portable::ExportSelection::all() }).unwrap();

    // Create vault2 with same connection but older timestamp
    let vault2 = unlocked_vault();
    let mut old_conn = conn.clone();
    old_conn.updated_at = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z").unwrap().with_timezone(&chrono::Utc);
    vault2.save_connection(&old_conn, Some("old_pw")).unwrap();

    // Import, should update because export is newer
    let result = import_vault(&vault2, &data, "pass", &crate::portable::ExportSelection::all()).unwrap();
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
    let data_with = export_vault(&vault, "pass", ExportOptions { include_private_keys: true, filter: ExportFilter::All, selection: crate::portable::ExportSelection::all() }).unwrap();
    assert!(export_includes_keys(&data_with));

    // Export WITHOUT keys
    let data_without = export_vault(&vault, "pass", ExportOptions { include_private_keys: false, filter: ExportFilter::All, selection: crate::portable::ExportSelection::all() }).unwrap();
    assert!(!export_includes_keys(&data_without));

    // Import with keys into fresh vault
    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data_with, "pass", &crate::portable::ExportSelection::all()).unwrap();
    assert_eq!(result.keys_added, 1);

    let pk = vault2.get_key_private(&generated.key.id).unwrap();
    assert!(pk.is_some());

    // Import without keys, key added but no private key
    let vault3 = unlocked_vault();
    let result = import_vault(&vault3, &data_without, "pass", &crate::portable::ExportSelection::all()).unwrap();
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
        selection: crate::portable::ExportSelection::all(),
    }).unwrap();

    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "share-pass", &crate::portable::ExportSelection::all()).unwrap();

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
        selection: crate::portable::ExportSelection::all(),
    }).unwrap();

    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "pass", &crate::portable::ExportSelection::all()).unwrap();

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
        selection: crate::portable::ExportSelection::all(),
    }).unwrap();

    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "pass", &crate::portable::ExportSelection::all()).unwrap();

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
        selection: crate::portable::ExportSelection::all(),
    }).unwrap();

    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "pass", &crate::portable::ExportSelection::all()).unwrap();

    assert_eq!(result.connections_added, 1);
    assert_eq!(result.snippets_added, 0);
    assert_eq!(result.known_hosts_added, 0);
}

// ── Settings (key/value) ─────────────────────────────────────
// Backs the app's user-preference persistence. Worth pinning
// explicitly since the UI relies on round-trip identity for
// anything stored as a string (timeouts, keepalive, parallelism,
// booleans-as-strings, etc.).

/// The denylist is the only thing standing between a portable export
/// and leaking device identity / lock state / plaintext tokens. Pin it
/// so a future setting can't silently start riding along.
#[test]
fn settings_denylist_blocks_device_local_keys() {
    use crate::portable::is_portable_setting;

    // Device identity, per-vault lock flag, device name + port,
    // plaintext bearer tokens, service-activation toggles, per-install
    // state, one-time hints, one-shot migration / boot markers.
    for denied in [
        "sync_device_identity",
        "has_user_password",
        "sync_device_name",
        "sync_listen_port",
        "sync_signaling_token",
        "mcp_server_token",
        "sync_enabled",
        "sync_mode",
        "mcp_server_enabled",
        "skipped_update_version",
        "pinned_tabs",
        "hint_link_click_used",
        "port_forwards_migrated",
        "keepalive_default_v2_applied",
    ] {
        assert!(!is_portable_setting(denied), "{denied} must not be portable");
    }

    // Genuine preferences ride along.
    for ok in [
        "language",
        "app_theme",
        "terminal_font_size",
        "scrollback_rows",
        "ai_provider",
        "ai_model",
        "ai_api_key",
        "sync_passwords",
        "sync_signaling_url",
    ] {
        assert!(is_portable_setting(ok), "{ok} should be portable");
    }
}

/// Full export → import round-trips portable preferences (including the
/// AI key, re-encrypted under the target's master key) and drops every
/// device-local / security key, even when one is present on the source.
#[test]
fn settings_export_import_roundtrip() {
    use crate::portable::{export_vault, import_vault, ExportFilter, ExportOptions, ExportSelection};

    let vault = unlocked_vault();

    // Portable preferences.
    vault.set_setting("language", "pt-BR").unwrap();
    vault.set_setting("app_theme", "Nord").unwrap();
    vault.set_setting("terminal_font_size", "15").unwrap();
    // AI key, stored encrypted under the source master key.
    vault.set_ai_api_key("sk-secret-123").unwrap();
    // Device-local / security keys that must never leave.
    vault.set_setting("has_user_password", "true").unwrap();
    vault.set_setting("sync_device_name", "source-laptop").unwrap();
    vault.set_setting("mcp_server_token", "tok-abc").unwrap();
    vault.set_setting("hint_link_click_used", "true").unwrap();

    let data = export_vault(
        &vault,
        "pass",
        ExportOptions {
            include_private_keys: false,
            filter: ExportFilter::All,
            selection: ExportSelection::all(),
        },
    )
    .unwrap();

    let vault2 = unlocked_vault();
    // vault2's own lock flag, set by `set_master_password`. Importing
    // the source's `has_user_password` must not touch it.
    let lock_flag_before = vault2.get_setting("has_user_password").unwrap();
    let result = import_vault(&vault2, &data, "pass", &ExportSelection::all()).unwrap();

    // language, app_theme, terminal_font_size, ai_api_key = 4 portable
    // keys written; the 4 device-local ones were filtered on the way out.
    assert_eq!(result.settings_imported, 4);

    assert_eq!(vault2.get_setting("language").unwrap().as_deref(), Some("pt-BR"));
    assert_eq!(vault2.get_setting("app_theme").unwrap().as_deref(), Some("Nord"));
    assert_eq!(vault2.get_setting("terminal_font_size").unwrap().as_deref(), Some("15"));

    // AI key decrypts to the original plaintext under vault2's own key.
    assert_eq!(vault2.get_ai_api_key().unwrap().as_deref(), Some("sk-secret-123"));
    // ...and is NOT stored verbatim, the column holds base64 ciphertext.
    assert_ne!(vault2.get_setting("ai_api_key").unwrap().as_deref(), Some("sk-secret-123"));

    // Device-local / security keys never crossed. The lock flag keeps
    // vault2's own value (the import didn't clobber it with the source's).
    assert_eq!(vault2.get_setting("has_user_password").unwrap(), lock_flag_before);
    assert_eq!(vault2.get_setting("sync_device_name").unwrap(), None);
    assert_eq!(vault2.get_setting("mcp_server_token").unwrap(), None);
    assert_eq!(vault2.get_setting("hint_link_click_used").unwrap(), None);
}

/// Unchecking the Settings category leaves an export with no settings,
/// and inspection reports the per-category counts the import dialog
/// needs.
#[test]
fn settings_excluded_when_category_off_and_inspect_counts() {
    use crate::portable::{export_vault, inspect_export, ExportFilter, ExportOptions, ExportSelection};

    let vault = unlocked_vault();
    vault.save_connection(&Connection::new("h", "10.0.0.1"), None).unwrap();
    vault.set_setting("language", "fr").unwrap();

    // Settings unchecked.
    let mut sel = ExportSelection::all();
    sel.settings = false;
    let data = export_vault(
        &vault,
        "pass",
        ExportOptions { include_private_keys: false, filter: ExportFilter::All, selection: sel },
    )
    .unwrap();

    let summary = inspect_export(&data, "pass").unwrap();
    assert_eq!(summary.connections, 1);
    assert_eq!(summary.settings, 0);
    assert!(summary.present(crate::portable::ExportCategory::Connections));
    assert!(!summary.present(crate::portable::ExportCategory::Settings));
}

/// Importing connections while deselecting their dependency categories
/// must NULL the now-dangling references, the app's invariant (deleted
/// parents cascade-NULL) means a `Some(missing)` group/key/identity
/// would make the host vanish from the dashboard.
#[test]
fn partial_import_nulls_dangling_refs() {
    use crate::portable::{export_vault, import_vault, ExportFilter, ExportOptions, ExportSelection};
    use oryxis_core::models::identity::Identity;

    let vault = unlocked_vault();
    let key = crate::keygen::generate_ed25519("k").unwrap();
    vault.save_key(&key.key, Some(&key.private_pem)).unwrap();
    let mut ident = Identity::new("id");
    ident.key_id = Some(key.key.id);
    vault.save_identity(&ident, None).unwrap();
    let g = Group::new("G");
    vault.save_group(&g).unwrap();
    let mut conn = Connection::new("h", "10.0.0.1");
    conn.group_id = Some(g.id);
    conn.key_id = Some(key.key.id);
    conn.identity_id = Some(ident.id);
    vault.save_connection(&conn, None).unwrap();

    let data = export_vault(
        &vault,
        "pw",
        ExportOptions { include_private_keys: true, filter: ExportFilter::All, selection: ExportSelection::all() },
    )
    .unwrap();

    // Import connections only, into a vault with none of the deps.
    let mut sel = ExportSelection::none();
    sel.connections = true;
    let vault2 = unlocked_vault();
    let result = import_vault(&vault2, &data, "pw", &sel).unwrap();
    assert_eq!(result.connections_added, 1);
    assert_eq!(result.groups_added, 0);
    assert_eq!(result.keys_added, 0);
    assert_eq!(result.identities_added, 0);

    let conns = vault2.list_connections().unwrap();
    assert_eq!(conns.len(), 1);
    assert_eq!(conns[0].group_id, None);
    assert_eq!(conns[0].key_id, None);
    assert_eq!(conns[0].identity_id, None);
}

/// The mirror case: when the parent already lives in the target vault,
/// a connections-only import must KEEP the link, not blindly NULL it.
#[test]
fn partial_import_preserves_existing_parent_link() {
    use crate::portable::{export_vault, import_vault, ExportFilter, ExportOptions, ExportSelection};

    let vault = unlocked_vault();
    let g = Group::new("G");
    vault.save_group(&g).unwrap();
    let mut conn = Connection::new("h", "10.0.0.1");
    conn.group_id = Some(g.id);
    vault.save_connection(&conn, None).unwrap();

    let data = export_vault(
        &vault,
        "pw",
        ExportOptions { include_private_keys: false, filter: ExportFilter::All, selection: ExportSelection::all() },
    )
    .unwrap();

    // Target already has the group; import connections only.
    let vault2 = unlocked_vault();
    vault2.save_group(&g).unwrap();
    let mut sel = ExportSelection::none();
    sel.connections = true;
    import_vault(&vault2, &data, "pw", &sel).unwrap();

    let conns = vault2.list_connections().unwrap();
    assert_eq!(conns.len(), 1);
    assert_eq!(conns[0].group_id, Some(g.id));
}


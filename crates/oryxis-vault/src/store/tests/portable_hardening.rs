//! A4 hardening suite for the portable export/import (1.0 release
//! gate): the all-entities roundtrip across different master
//! passwords, the structural no-plaintext-secret scan of the raw
//! blob, header error paths, and the 0.8/0.9 fixture imports that
//! prove 1.0 keeps reading files written by older releases.

use super::*;
use crate::portable::{
    export_vault, import_vault, inspect_export, ExportFilter, ExportOptions, ExportSelection,
};
use oryxis_core::models::connection::{ProxyConfig, ProxyType};
use oryxis_core::models::identity::Identity;
use oryxis_core::models::session_group::{PaneLayout, PaneMember, PaneSource, SessionGroup};
use oryxis_core::models::CustomTerminalTheme;

fn unlocked_vault_with(password: &str) -> VaultStore {
    let mut vault = temp_vault();
    vault.set_master_password(password).unwrap();
    vault
}

fn all_options() -> ExportOptions {
    ExportOptions {
        include_private_keys: true,
        filter: ExportFilter::All,
        selection: ExportSelection::all(),
    }
}

/// One vault carrying every entity family (with every secret slot
/// populated), exported and imported into a vault with a DIFFERENT
/// master password. Every field and every secret must arrive, and the
/// secrets must be re-encrypted under the target's key (proven by a
/// lock + re-unlock of the target before the final read).
#[test]
fn full_roundtrip_every_entity_different_master_password() {
    let vault = unlocked_vault_with("alpha");

    // Group tree with a parent link.
    let prod = Group::new("Prod");
    vault.save_group(&prod).unwrap();
    let mut eu = Group::new("Prod/EU");
    eu.parent_id = Some(prod.id);
    vault.save_group(&eu).unwrap();

    // Key with (passphrase-protected shaped) private material and a
    // certificate (B2): the cert is public material and must ride the
    // portable export like any plain field.
    let mut generated = crate::keygen::generate_ed25519("deploy-key").unwrap();
    generated.key.certificate =
        Some("ssh-ed25519-cert-v01@openssh.com AAAAcert... deploy@ca".to_string());
    vault
        .save_key(&generated.key, Some(&generated.private_pem))
        .unwrap();

    // Identity with password, bound to the key.
    let mut ident = Identity::new("deploy-ident");
    ident.username = Some("deploy".into());
    ident.key_id = Some(generated.key.id);
    vault.save_identity(&ident, Some("ident-pw")).unwrap();

    // Proxy identity with password.
    let mut pi = ProxyIdentity::new("corp-bastion");
    pi.proxy_type = ProxyType::Socks5;
    pi.host = "bastion.corp".into();
    pi.port = 1080;
    pi.username = Some("alice".into());
    vault.save_proxy_identity(&pi, Some("proxy-ident-pw")).unwrap();

    // Connection 2 first (connection 1 jumps through it): key auth +
    // proxy identity reference.
    let mut jump = Connection::new("jump-host", "10.0.0.2");
    jump.group_id = Some(prod.id);
    jump.key_id = Some(generated.key.id);
    jump.proxy_identity_id = Some(pi.id);
    vault.save_connection(&jump, None).unwrap();

    // Connection 1: password + TOTP + inline proxy with password +
    // jump chain + nested group + custom theme reference by name.
    let mut web = Connection::new("prod-web", "10.0.0.1");
    web.group_id = Some(eu.id);
    web.username = Some("root".into());
    web.identity_id = Some(ident.id);
    web.jump_chain = vec![jump.id];
    web.terminal_theme = Some("Solar Flare".into());
    web.proxy = Some(ProxyConfig {
        proxy_type: ProxyType::Http,
        host: "inline.proxy".into(),
        port: 8080,
        username: Some("bob".into()),
        password: None,
    });
    vault.save_connection(&web, Some("conn-pw")).unwrap();
    vault.set_proxy_password(&web.id, Some("inline-proxy-pw")).unwrap();
    vault
        .set_connection_totp_secret(&web.id, Some("JBSWY3DPEHPK3PXP"))
        .unwrap();

    // Snippet with vars, known host, port forward, session group.
    let snip = Snippet::new("deploy", "make deploy {env:prod}");
    vault.save_snippet(&snip).unwrap();
    let kh = KnownHost::new("10.0.0.1", 22, "ed25519", "SHA256:abcdef");
    vault.save_known_host(&kh).unwrap();
    let mut pf = oryxis_core::models::port_forward_rule::PortForwardRule::new(
        "db",
        oryxis_core::models::port_forward_rule::ForwardKind::Local,
        web.id,
    );
    pf.listen_port = 5432;
    pf.target_host = "db.internal".into();
    pf.target_port = 5432;
    vault.save_port_forward_rule(&pf).unwrap();
    let sg = SessionGroup::new(
        "ops",
        PaneLayout::Leaf(PaneMember {
            source: PaneSource::Host(web.id),
            initial_script: Some("htop".into()),
        }),
    );
    vault.save_session_group(&sg).unwrap();

    // Settings: AI key (encrypted per-field), language, theme, and the
    // custom terminal theme the connection references by name.
    vault.set_ai_api_key("sk-ai-key-material").unwrap();
    vault.set_setting("language", "pt-BR").unwrap();
    vault.set_setting("app_theme", "dark").unwrap();
    let mut theme = CustomTerminalTheme::new_default("Solar Flare".into());
    theme.background = "#101418".into();
    vault.save_custom_terminal_theme(&theme).unwrap();

    // Export from "alpha", import into "bravo".
    let data = export_vault(&vault, "export-pw", all_options()).unwrap();
    let mut target = unlocked_vault_with("bravo");
    let result = import_vault(&target, &data, "export-pw", &ExportSelection::all()).unwrap();
    assert_eq!(result.connections_added, 2);
    assert_eq!(result.groups_added, 2);
    assert_eq!(result.keys_added, 1);
    assert_eq!(result.identities_added, 1);
    assert_eq!(result.proxy_identities_added, 1);
    assert_eq!(result.snippets_added, 1);
    assert_eq!(result.known_hosts_added, 1);
    assert_eq!(result.port_forward_rules_added, 1);
    assert_eq!(result.session_groups_added, 1);
    assert_eq!(result.custom_themes_added, 1);
    assert!(result.settings_imported >= 3, "language/theme/ai key");

    // Lock and re-unlock the TARGET with its own password before
    // reading secrets: proves the imported material was re-encrypted
    // under bravo's key, not carried on alpha's.
    target.lock();
    target.unlock("bravo").unwrap();

    // Structure.
    let groups = target.list_groups().unwrap();
    let eu2 = groups.iter().find(|g| g.label == "Prod/EU").unwrap();
    let prod2 = groups.iter().find(|g| g.label == "Prod").unwrap();
    assert_eq!(eu2.parent_id, Some(prod2.id));

    let conns = target.list_connections().unwrap();
    let web2 = conns.iter().find(|c| c.label == "prod-web").unwrap();
    let jump2 = conns.iter().find(|c| c.label == "jump-host").unwrap();
    assert_eq!(web2.group_id, Some(eu2.id));
    assert_eq!(web2.identity_id, Some(ident.id));
    assert_eq!(web2.jump_chain, vec![jump2.id]);
    assert_eq!(web2.terminal_theme.as_deref(), Some("Solar Flare"));
    let inline = web2.proxy.as_ref().expect("inline proxy");
    assert_eq!(inline.host, "inline.proxy");
    assert_eq!(inline.username.as_deref(), Some("bob"));
    assert_eq!(jump2.key_id, Some(generated.key.id));
    assert_eq!(jump2.proxy_identity_id, Some(pi.id));

    // Every secret slot.
    assert_eq!(
        target.get_connection_password(&web2.id).unwrap().as_deref(),
        Some("conn-pw")
    );
    assert_eq!(
        target.get_proxy_password(&web2.id).unwrap().as_deref(),
        Some("inline-proxy-pw")
    );
    assert_eq!(
        target
            .get_connection_totp_secret(&web2.id)
            .unwrap()
            .as_deref(),
        Some("JBSWY3DPEHPK3PXP")
    );
    assert_eq!(
        target.get_identity_password(&ident.id).unwrap().as_deref(),
        Some("ident-pw")
    );
    assert_eq!(
        target
            .get_proxy_identity_password(&pi.id)
            .unwrap()
            .as_deref(),
        Some("proxy-ident-pw")
    );
    assert_eq!(
        target.get_key_private(&generated.key.id).unwrap().as_deref(),
        Some(generated.private_pem.as_str())
    );
    // The certificate survived export + import as public material.
    assert_eq!(
        target
            .list_keys()
            .unwrap()
            .iter()
            .find(|k| k.id == generated.key.id)
            .and_then(|k| k.certificate.clone()),
        generated.key.certificate,
    );
    assert_eq!(
        target.get_ai_api_key().unwrap().as_deref(),
        Some("sk-ai-key-material")
    );

    // Settings + the custom theme the host references.
    assert_eq!(
        target.get_setting("language").unwrap().as_deref(),
        Some("pt-BR")
    );
    let themes = target.list_custom_terminal_themes().unwrap();
    assert_eq!(themes.len(), 1);
    assert_eq!(themes[0].name, "Solar Flare");
    assert_eq!(themes[0].background, "#101418");
    assert_eq!(themes[0].ansi, theme.ansi);
}

/// Every secret is a unique high-entropy marker; none may appear in
/// the raw export bytes (the payload is ciphertext). The negative
/// control decrypts the file and requires every marker present, so
/// the test cannot pass because a marker was silently dropped.
#[test]
fn export_blob_contains_no_plaintext_secrets() {
    const MARKERS: &[&str] = &[
        "LEAK-conn-pw-9f2b1c",
        "LEAK-proxy-pw-4e7a92",
        "LEAK-totp-8c1d33",
        "LEAK-ident-pw-b65f01",
        "LEAK-proxy-ident-pw-72e9c4",
        "LEAK-key-material-1a3c58",
        "LEAK-cloud-secret-e04b76",
        "LEAK-ai-key-53d2af",
    ];

    let vault = unlocked_vault_with("alpha");
    let mut pi = ProxyIdentity::new("pi");
    pi.host = "p.example".into();
    pi.port = 1080;
    vault.save_proxy_identity(&pi, Some(MARKERS[4])).unwrap();

    let mut key = oryxis_core::models::key::SshKey::new(
        "k",
        oryxis_core::models::key::KeyAlgorithm::Ed25519,
    );
    key.public_key = "ssh-ed25519 AAAA test".into();
    vault.save_key(&key, Some(MARKERS[5])).unwrap();

    let ident = Identity::new("i");
    vault.save_identity(&ident, Some(MARKERS[3])).unwrap();

    let mut conn = Connection::new("c", "10.0.0.1");
    conn.proxy = Some(ProxyConfig {
        proxy_type: ProxyType::Http,
        host: "inline".into(),
        port: 8080,
        username: None,
        password: None,
    });
    vault.save_connection(&conn, Some(MARKERS[0])).unwrap();
    vault.set_proxy_password(&conn.id, Some(MARKERS[1])).unwrap();
    vault
        .set_connection_totp_secret(&conn.id, Some(MARKERS[2]))
        .unwrap();

    vault.set_ai_api_key(MARKERS[7]).unwrap();

    let data = export_vault(&vault, "export-pw", all_options()).unwrap();

    // Header parses, and no marker appears anywhere in the raw bytes.
    assert_eq!(&data[..6], b"ORYXIS");
    assert!(crate::portable::is_valid_export(&data));
    let haystack = &data[..];
    for marker in MARKERS {
        assert!(
            !contains_bytes(haystack, marker.as_bytes()),
            "plaintext secret {marker} leaked into the export blob"
        );
    }

    // Negative control: the sealed payload DOES carry every marker.
    let target = unlocked_vault_with("bravo");
    import_vault(&target, &data, "export-pw", &ExportSelection::all()).unwrap();
    let conns = target.list_connections().unwrap();
    assert_eq!(
        target
            .get_connection_password(&conns[0].id)
            .unwrap()
            .as_deref(),
        Some(MARKERS[0])
    );
    assert_eq!(target.get_ai_api_key().unwrap().as_deref(), Some(MARKERS[7]));
    assert_eq!(
        target.get_key_private(&key.id).unwrap().as_deref(),
        Some(MARKERS[5])
    );
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A file claiming a FUTURE format version fails with a clean
/// VaultError (never a panic), telling the user to update.
#[test]
fn unsupported_format_version_is_clean_error() {
    let vault = unlocked_vault_with("alpha");
    let mut data = export_vault(&vault, "pw", all_options()).unwrap();
    // Bump the little-endian version field past FORMAT_VERSION.
    data[6] = 0xFF;
    data[7] = 0x00;
    let err = import_vault(&vault, &data, "pw", &ExportSelection::all());
    assert!(err.is_err());
    let msg = format!("{}", err.err().unwrap());
    assert!(msg.contains("Unsupported format version"), "{msg}");
    assert!(inspect_export(&data, "pw").is_err());
}

/// A header whose keys flag lies (claims private keys, payload has
/// none) must stay harmless: inspect reports the flag, import applies
/// what is actually there.
#[test]
fn keys_flag_mismatch_is_harmless() {
    let vault = unlocked_vault_with("alpha");
    let conn = Connection::new("c", "10.0.0.1");
    vault.save_connection(&conn, None).unwrap();
    let mut data = export_vault(
        &vault,
        "pw",
        ExportOptions {
            include_private_keys: false,
            filter: ExportFilter::All,
            selection: ExportSelection::all(),
        },
    )
    .unwrap();
    // Forge FLAG_INCLUDES_KEYS into the header flags (LE u32 at 8..12).
    data[8] |= 1;
    let summary = inspect_export(&data, "pw").unwrap();
    assert!(summary.includes_private_keys);
    assert_eq!(summary.keys, 0);
    let target = unlocked_vault_with("bravo");
    let result = import_vault(&target, &data, "pw", &ExportSelection::all()).unwrap();
    assert_eq!(result.connections_added, 1);
    assert_eq!(result.keys_added, 0);
}

/// Importing connections while the ProxyIdentities category is
/// excluded nulls the dangling `proxy_identity_id`, the same contract
/// the key/identity refs already have.
#[test]
fn partial_import_nulls_dangling_proxy_identity() {
    let vault = unlocked_vault_with("alpha");
    let mut pi = ProxyIdentity::new("pi");
    pi.host = "p.example".into();
    pi.port = 1080;
    vault.save_proxy_identity(&pi, Some("pw")).unwrap();
    let mut conn = Connection::new("c", "10.0.0.1");
    conn.proxy_identity_id = Some(pi.id);
    vault.save_connection(&conn, None).unwrap();

    let data = export_vault(&vault, "pw", all_options()).unwrap();

    let mut sel = ExportSelection::none();
    sel.connections = true;
    let target = unlocked_vault_with("bravo");
    let result = import_vault(&target, &data, "pw", &sel).unwrap();
    assert_eq!(result.connections_added, 1);
    assert_eq!(result.proxy_identities_added, 0);
    let conns = target.list_connections().unwrap();
    assert_eq!(conns[0].proxy_identity_id, None);
}

/// A name conflict with a LOCAL theme skips the imported one (theme
/// names key the per-host overrides and must stay unique); a re-import
/// of the same id is idempotent.
#[test]
fn custom_theme_import_conflicts() {
    let vault = unlocked_vault_with("alpha");
    let theme = CustomTerminalTheme::new_default("Night".into());
    vault.save_custom_terminal_theme(&theme).unwrap();
    let data = export_vault(&vault, "pw", all_options()).unwrap();

    // Same id already present: skipped, not duplicated.
    let result = import_vault(&vault, &data, "pw", &ExportSelection::all()).unwrap();
    assert_eq!(result.custom_themes_added, 0);
    assert_eq!(result.custom_themes_skipped, 1);
    assert_eq!(vault.list_custom_terminal_themes().unwrap().len(), 1);

    // Different id, same name on the target: local theme wins.
    let target = unlocked_vault_with("bravo");
    let local = CustomTerminalTheme::new_default("Night".into());
    target.save_custom_terminal_theme(&local).unwrap();
    let result = import_vault(&target, &data, "pw", &ExportSelection::all()).unwrap();
    assert_eq!(result.custom_themes_added, 0);
    assert_eq!(result.custom_themes_skipped, 1);
    let themes = target.list_custom_terminal_themes().unwrap();
    assert_eq!(themes.len(), 1);
    assert_eq!(themes[0].id, local.id);
}

// ---------------------------------------------------------------------------
// Forward-compat fixtures (files in the 0.8 / 0.9 payload shape).
// See crates/oryxis-vault/tests/fixtures/FIXTURES.md for provenance.
// ---------------------------------------------------------------------------

const FIXTURE_PASSWORD: &str = "fixture";

/// Builds the synthetic fixture vault, exports it with the CURRENT
/// code, then prunes the payload back to the target version's shape
/// (removing the JSON keys that did not exist yet) and re-seals it.
/// Shape-valid by construction: field names come from the real
/// serializers, only the version delta is hand-maintained. The deltas
/// (verified against `git show v0.8.3:crates/oryxis-vault/src/portable.rs`):
///
/// - 0.8.3 -> 0.9.0: connections gained `totp_secret`.
/// - 0.9.0 -> 1.0:   payload root gained `custom_terminal_themes`.
fn build_fixture_bytes(strip_totp: bool) -> Vec<u8> {
    let vault = unlocked_vault_with("fixture-source");
    let group = Group::new("Fixture Group");
    vault.save_group(&group).unwrap();
    let mut conn = Connection::new("fixture-host", "203.0.113.10");
    conn.username = Some("admin".into());
    conn.group_id = Some(group.id);
    conn.notes = Some("written by the fixture generator".into());
    vault.save_connection(&conn, Some("fixture-conn-pw")).unwrap();
    vault
        .set_connection_totp_secret(&conn.id, Some("JBSWY3DPEHPK3PXP"))
        .unwrap();
    let snip = Snippet::new("list", "ls -la");
    vault.save_snippet(&snip).unwrap();
    vault.set_setting("app_theme", "dark").unwrap();

    let data = export_vault(&vault, FIXTURE_PASSWORD, all_options()).unwrap();
    let json = crate::store::decrypt(&data[12..], FIXTURE_PASSWORD.as_bytes()).unwrap();
    let mut payload: serde_json::Value = serde_json::from_slice(&json).unwrap();

    // 1.0 root field: absent in both fixture shapes.
    payload.as_object_mut().unwrap().remove("custom_terminal_themes");
    if strip_totp {
        // 0.9 connection field: absent in the 0.8 shape.
        for c in payload["connections"].as_array_mut().unwrap() {
            c.as_object_mut().unwrap().remove("totp_secret");
        }
    }
    // Settings arrive with export-time noise (portable defaults);
    // keep only the deterministic app_theme entry.
    let settings = payload["settings"].as_array_mut().unwrap();
    settings.retain(|s| s["key"] == "app_theme");

    let sealed = crate::store::encrypt(
        &serde_json::to_vec(&payload).unwrap(),
        FIXTURE_PASSWORD.as_bytes(),
    )
    .unwrap();
    let mut out = Vec::with_capacity(12 + sealed.len());
    out.extend_from_slice(b"ORYXIS");
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&sealed);
    out
}

/// Regenerates the committed fixture files. Run manually after a
/// deliberate format change (see FIXTURES.md):
/// `cargo test -p oryxis-vault regenerate_fixtures -- --ignored`
#[test]
#[ignore]
fn regenerate_fixtures() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("export-0.8.oryxis"), build_fixture_bytes(true)).unwrap();
    std::fs::write(dir.join("export-0.9.oryxis"), build_fixture_bytes(false)).unwrap();
}

fn assert_fixture_imports(data: &[u8], expect_totp: Option<&str>) {
    let summary = inspect_export(data, FIXTURE_PASSWORD).unwrap();
    assert_eq!(summary.connections, 1);
    assert_eq!(summary.groups, 1);
    assert_eq!(summary.snippets, 1);
    assert!(!summary.includes_private_keys);

    let vault = unlocked_vault_with("target");
    let result =
        import_vault(&vault, data, FIXTURE_PASSWORD, &ExportSelection::all()).unwrap();
    assert_eq!(result.connections_added, 1);
    assert_eq!(result.groups_added, 1);
    assert_eq!(result.snippets_added, 1);
    assert_eq!(result.custom_themes_added, 0);

    let conns = vault.list_connections().unwrap();
    assert_eq!(conns.len(), 1);
    let c = &conns[0];
    assert_eq!(c.label, "fixture-host");
    assert_eq!(c.hostname, "203.0.113.10");
    assert_eq!(c.username.as_deref(), Some("admin"));
    let groups = vault.list_groups().unwrap();
    let g = groups.iter().find(|g| g.label == "Fixture Group").unwrap();
    assert_eq!(c.group_id, Some(g.id));
    // Fields newer than the file default cleanly.
    assert_eq!(c.proxy_identity_id, None);
    assert!(c.jump_chain.is_empty());
    assert_eq!(
        vault.get_connection_password(&c.id).unwrap().as_deref(),
        Some("fixture-conn-pw")
    );
    assert_eq!(
        vault.get_connection_totp_secret(&c.id).unwrap().as_deref(),
        expect_totp
    );
    assert_eq!(
        vault.get_setting("app_theme").unwrap().as_deref(),
        Some("dark")
    );
}

/// 1.0 must keep reading export files in the 0.8 payload shape.
#[test]
fn imports_fixture_export_v08() {
    let data = include_bytes!("../../../tests/fixtures/export-0.8.oryxis");
    assert_fixture_imports(data, None);
}

/// 1.0 must keep reading export files in the 0.9 payload shape.
#[test]
fn imports_fixture_export_v09() {
    let data = include_bytes!("../../../tests/fixtures/export-0.9.oryxis");
    assert_fixture_imports(data, Some("JBSWY3DPEHPK3PXP"));
}

/// The per-device approval of a command proxy is not vault DATA, and a
/// portable export must not carry it.
///
/// An export is a file that travels: onto another machine, into a
/// colleague's hands, through whatever moved it. Carrying the approval
/// would let the file answer "yes, run this" on a computer whose owner
/// never saw the line, which is the same hole in a different courier
/// (the removed sync engine was the other one). The proxy itself
/// still exports, exactly like any other connection field: what stays
/// behind is only the permission to run it.
#[test]
fn a_command_proxy_approval_never_leaves_the_device() {
    const CMD: &str = "aws ssm start-session --target i-0123456789";
    let vault = unlocked_vault_with("alpha");

    let mut conn = Connection::new("bastion", "10.0.0.9");
    conn.proxy = Some(ProxyConfig {
        proxy_type: ProxyType::Command(CMD.into()),
        host: String::new(),
        port: 0,
        username: None,
        password: None,
    });
    vault.save_connection(&conn, None).unwrap();
    vault.trust_proxy_command(CMD, "bastion:22").unwrap();
    assert!(vault.is_proxy_command_trusted(CMD));

    let blob = export_vault(&vault, "pack", all_options()).unwrap();
    let target = unlocked_vault_with("beta");
    import_vault(&target, &blob, "pack", &ExportSelection::all()).unwrap();

    // The route arrived.
    let imported = target
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("the connection should import");
    assert!(matches!(
        imported.proxy.as_ref().map(|p| &p.proxy_type),
        Some(ProxyType::Command(c)) if c == CMD
    ));
    // The permission to run it did not.
    assert!(
        !target.is_proxy_command_trusted(CMD),
        "an imported command proxy must arrive unapproved"
    );
    assert!(target.list_trusted_proxy_commands().unwrap().is_empty());
}

/// Same rule for the Telnet TLS escape: "accept a certificate the
/// trust store rejects" is a decision made about ONE appliance on ONE
/// machine, so a file that travels must not carry it. The TLS toggle
/// itself is host data (it describes the endpoint) and does export; the
/// escape is stripped on the way out AND on the way in, so a
/// hand-edited file cannot arm it either.
#[test]
fn a_telnet_certificate_escape_never_leaves_the_device() {
    use oryxis_core::models::connection::ConnectionProtocol;
    let vault = unlocked_vault_with("alpha");

    let mut conn = Connection::new("switch", "10.0.0.1");
    conn.protocol = ConnectionProtocol::Telnet;
    conn.port = 992;
    conn.telnet = Some(oryxis_core::models::telnet::TelnetOptions {
        tls: true,
        tls_insecure: true,
    });
    vault.save_connection(&conn, None).unwrap();

    let blob = export_vault(&vault, "pack", all_options()).unwrap();
    let target = unlocked_vault_with("beta");
    import_vault(&target, &blob, "pack", &ExportSelection::all()).unwrap();

    let imported = target
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("the connection should import");
    let opts = imported.telnet.expect("the TLS setting itself travels");
    assert!(opts.tls, "TLS describes the endpoint and must import");
    assert!(
        !opts.tls_insecure,
        "an imported host must verify certificates until its owner says otherwise"
    );
    // And the source keeps its own decision.
    let local = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("the source host is untouched");
    assert!(local.telnet.expect("still set").tls_insecure);
}

/// A pin is a trust decision made at a fingerprint prompt, so a file
/// must not be able to REPLACE one. Dedup by id alone would let it:
/// `save_known_host` keeps a single row per (hostname, port, key_type)
/// and deletes the others first, so a fresh id reads as "new" and the
/// local fingerprint is gone, silently, with the real row tombstoned on
/// the way out.
#[test]
fn an_imported_pin_never_replaces_one_this_vault_already_made() {
    use oryxis_core::models::known_host::KnownHost;

    let vault = unlocked_vault_with("alpha");
    // What the file carries: the same endpoint, a different key, and an
    // id this vault has never seen.
    let hostile = KnownHost::new("bastion.corp", 22, "ssh-ed25519", "SHA256:ATTACKER");
    vault.save_known_host(&hostile).unwrap();
    let blob = export_vault(&vault, "pack", all_options()).unwrap();

    // What the target already trusts for that endpoint.
    let target = unlocked_vault_with("beta");
    let mine = KnownHost::new("bastion.corp", 22, "ssh-ed25519", "SHA256:REAL");
    target.save_known_host(&mine).unwrap();

    let result = import_vault(&target, &blob, "pack", &ExportSelection::all()).unwrap();

    let pins = target.list_known_hosts().unwrap();
    let pin = pins
        .iter()
        .find(|k| k.hostname == "bastion.corp" && k.port == 22)
        .expect("the local pin must survive the import");
    assert_eq!(
        pin.fingerprint, "SHA256:REAL",
        "an imported pin must not overwrite a fingerprint this vault accepted"
    );
    assert_eq!(pin.id, mine.id, "the local row itself must survive");
    assert_eq!(pins.len(), 1, "the file's row must not land beside the local one");
    assert_eq!(result.known_hosts_skipped, 1);
    assert_eq!(result.known_hosts_added, 0);
}

/// An endpoint this vault has NOT pinned is the migration case the
/// category exists for, so it still imports.
#[test]
fn an_imported_pin_for_an_unpinned_endpoint_still_lands() {
    use oryxis_core::models::known_host::KnownHost;

    let vault = unlocked_vault_with("alpha");
    let pin = KnownHost::new("fresh.corp", 2222, "ssh-ed25519", "SHA256:FRESH");
    vault.save_known_host(&pin).unwrap();
    let blob = export_vault(&vault, "pack", all_options()).unwrap();

    let target = unlocked_vault_with("beta");
    let result = import_vault(&target, &blob, "pack", &ExportSelection::all()).unwrap();

    assert_eq!(result.known_hosts_added, 1);
    let pins = target.list_known_hosts().unwrap();
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].fingerprint, "SHA256:FRESH");
}

/// `auto_start` turns a stored rule into a DIAL at the next launch with
/// nobody present, and that dial resolves the rule's host (proxy
/// included). A file the user merely picked must not schedule that, so
/// the rule imports disarmed and the count says so.
#[test]
fn an_imported_forward_never_arms_itself() {
    use oryxis_core::models::port_forward_rule::{ForwardKind, PortForwardRule};

    let vault = unlocked_vault_with("alpha");
    let conn = Connection::new("bastion", "10.0.0.9");
    vault.save_connection(&conn, None).unwrap();
    let mut rule = PortForwardRule::new("tunnel", ForwardKind::Local, conn.id);
    rule.listen_port = 8080;
    rule.target_host = "10.0.0.9".into();
    rule.target_port = 80;
    rule.auto_start = true;
    vault.save_port_forward_rule(&rule).unwrap();

    let blob = export_vault(&vault, "pack", all_options()).unwrap();
    let target = unlocked_vault_with("beta");
    let result = import_vault(&target, &blob, "pack", &ExportSelection::all()).unwrap();

    let imported = target
        .list_port_forward_rules()
        .unwrap()
        .into_iter()
        .find(|r| r.id == rule.id)
        .expect("the rule itself still imports");
    assert!(
        !imported.auto_start,
        "an imported forward must not dial on its own at the next launch"
    );
    assert_eq!(imported.listen_port, 8080, "the rest of the rule is untouched");
    assert_eq!(result.port_forward_rules_added, 1);
    assert_eq!(
        result.port_forward_rules_disarmed, 1,
        "the count is what lets the UI explain why it will not come up"
    );
}

/// Settings whose VALUE is a trust decision stay behind: the import
/// dialog shows a category count, never a key list, so nothing on that
/// path would let the user see one of these change.
#[test]
fn security_settings_never_ride_a_portable_file() {
    let vault = unlocked_vault_with("alpha");
    // Each one re-points something the local user decided: where
    // GitHub-bound requests go, whether a server may READ the clipboard,
    // whether the signing service runs and whether it confirms, when the
    // vault locks itself, which release strand this install follows,
    // where AI requests (bearing the user's key) go, and when a stored
    // password gets typed into a session.
    let planted = [
        ("download_mirror", "https://attacker.example"),
        ("terminal_clipboard_access", "readwrite"),
        ("agent_server_enabled", "true"),
        ("agent_server_confirm", "false"),
        ("agent_server_allow_add", "true"),
        ("agent_server_openssh_pipe", "true"),
        ("auto_lock_minutes", "0"),
        ("update_channel", "nightly"),
        ("ai_api_url", "https://attacker.example/v1"),
        ("terminal_password_autofill", "true"),
    ];
    for (k, v) in planted {
        vault.set_setting(k, v).unwrap();
    }
    // A portable preference, to prove the category still works at all.
    vault.set_setting("terminal_theme", "dracula").unwrap();

    let blob = export_vault(&vault, "pack", all_options()).unwrap();
    let target = unlocked_vault_with("beta");
    import_vault(&target, &blob, "pack", &ExportSelection::all()).unwrap();

    for (k, _) in planted {
        assert_eq!(
            target.get_setting(k).unwrap(),
            None,
            "{k} must not cross a portable file"
        );
    }
    assert_eq!(
        target.get_setting("terminal_theme").unwrap().as_deref(),
        Some("dracula"),
        "ordinary preferences must still travel"
    );
}

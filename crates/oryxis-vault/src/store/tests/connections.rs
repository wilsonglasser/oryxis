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

#[test]
fn connection_disk_key_fields_round_trip() {
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "example.com");
    conn.use_disk_key = true;
    conn.identity_file = Some("/home/u/.ssh/work_ed25519".into());
    vault.save_connection(&conn, None).unwrap();
    let loaded = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("connection listed");
    // Guards the positional column indices in `store/connections.rs`:
    // both fields were appended to the INSERT and both SELECT lists, and
    // an off-by-one there would silently reload every host with the
    // disk source off (the exact failure mode the feature exists to
    // remove).
    assert!(loaded.use_disk_key);
    assert_eq!(
        loaded.identity_file.as_deref(),
        Some("/home/u/.ssh/work_ed25519")
    );
}

#[test]
fn a_blank_identity_file_reads_back_as_no_path() {
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "example.com");
    conn.use_disk_key = true;
    // An emptied editor field must mean "scan the default names", which
    // is `None`, not a path that is the empty string.
    conn.identity_file = Some("   ".into());
    vault.save_connection(&conn, None).unwrap();
    let loaded = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("connection listed");
    assert!(loaded.use_disk_key);
    assert!(loaded.identity_file.is_none());
}

#[test]
fn a_legacy_row_reads_back_with_the_disk_source_off() {
    let vault = unlocked_vault();
    let conn = Connection::new("h", "example.com");
    vault.save_connection(&conn, None).unwrap();
    // Every host that existed before this column did must keep
    // authenticating exactly as it did: nothing from disk is offered
    // until someone opts in.
    let loaded = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("connection listed");
    assert!(!loaded.use_disk_key);
    assert!(loaded.identity_file.is_none());
}

#[test]
fn connection_address_family_round_trips() {
    use oryxis_core::models::connection::AddressFamily;
    let vault = unlocked_vault();
    // Every variant survives the save/list cycle; this guards the
    // string mapping ('auto' | 'v4' | 'v6') and the positional column
    // index in `store/connections.rs` (a typo there would silently
    // reload everything as Auto via the fallthrough).
    for family in [AddressFamily::Auto, AddressFamily::V4, AddressFamily::V6] {
        let mut conn = Connection::new("h", "example.com");
        conn.address_family = family;
        vault.save_connection(&conn, None).unwrap();
        let loaded = vault
            .list_connections()
            .unwrap()
            .into_iter()
            .find(|c| c.id == conn.id)
            .expect("connection listed");
        assert_eq!(loaded.address_family, family);
    }
}

#[test]
fn connection_ambiguous_width_round_trips() {
    use oryxis_core::models::connection::AmbiguousWidth;
    let vault = unlocked_vault();
    // Same guard as the address-family test, and the same failure mode:
    // the column was appended LAST, so a wrong positional index reloads
    // someone else's value and the `_ =>` fallthrough turns it into a
    // silent Auto, which looks exactly like the default nobody set.
    for width in [AmbiguousWidth::Auto, AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
        let mut conn = Connection::new("h", "example.com");
        conn.ambiguous_width = width;
        // A CJK encoding alongside it, so a `Wide` that silently reloaded
        // as `Auto` could not pass by resolving wide anyway.
        conn.encoding = Some("Big5".to_string());
        vault.save_connection(&conn, None).unwrap();
        let loaded = vault
            .list_connections()
            .unwrap()
            .into_iter()
            .find(|c| c.id == conn.id)
            .expect("connection listed");
        assert_eq!(loaded.ambiguous_width, width);
        assert_eq!(loaded.encoding.as_deref(), Some("Big5"));
    }
}

#[test]
fn monitor_disks_round_trip_keeps_all_three_states() {
    // Auto / Custom / Custom-with-nothing are three different answers
    // (issue #135) and the column has to tell them apart: NULL is Auto,
    // `[]` is "report no disks on this host". Also guards the positional
    // index 56 in `store/connections.rs`, where a wrong index reloads
    // someone else's column as a silent Auto.
    let vault = unlocked_vault();
    let mut auto = Connection::new("auto", "example.com");
    auto.monitor_disks = None;
    vault.save_connection(&auto, None).unwrap();

    let mut custom = Connection::new("custom", "example.com");
    custom.monitor_disks = Some(vec!["/".into(), "/mnt/*".into()]);
    vault.save_connection(&custom, None).unwrap();

    let mut silent = Connection::new("silent", "example.com");
    silent.monitor_disks = Some(Vec::new());
    vault.save_connection(&silent, None).unwrap();

    let list = vault.list_connections().unwrap();
    let by_id = |id| list.iter().find(|c: &&Connection| c.id == id).unwrap();
    assert_eq!(by_id(auto.id).monitor_disks, None);
    assert_eq!(
        by_id(custom.id).monitor_disks.as_deref(),
        Some(["/".to_string(), "/mnt/*".to_string()].as_slice())
    );
    assert_eq!(by_id(silent.id).monitor_disks, Some(Vec::new()));
}

#[test]
fn connection_quirks_and_rekey_round_trip() {
    use oryxis_core::models::terminal_quirks::{
        BackspaceMode, FunctionKeyMode, HomeEndMode, OptionAsMeta, Osc52Override, TerminalQuirks,
    };
    let vault = unlocked_vault();
    // None quirks reload as None (all-xterm default), and a fully
    // populated quirks + rekey limit survive the save/list cycle. Guards
    // the JSON column and the positional indices 46/47 in
    // `store/connections.rs` (a wrong index silently reloads defaults).
    let mut plain = Connection::new("plain", "example.com");
    plain.quirks = None;
    plain.rekey_limit_mb = None;
    vault.save_connection(&plain, None).unwrap();

    let mut fancy = Connection::new("fancy", "example.com");
    fancy.quirks = Some(TerminalQuirks {
        backspace: BackspaceMode::CtrlH,
        home_end: HomeEndMode::Rxvt,
        function_keys: FunctionKeyMode::LinuxConsole,
        disable_mouse_reporting: true,
        disable_title_change: true,
        osc52: Some(Osc52Override::On),
        option_as_meta: OptionAsMeta::Both,
    });
    fancy.rekey_limit_mb = Some(512);
    vault.save_connection(&fancy, None).unwrap();

    let list = vault.list_connections().unwrap();
    let loaded_plain = list.iter().find(|c| c.id == plain.id).unwrap();
    assert_eq!(loaded_plain.quirks, None);
    assert_eq!(loaded_plain.rekey_limit_mb, None);
    let loaded_fancy = list.iter().find(|c| c.id == fancy.id).unwrap();
    assert_eq!(loaded_fancy.quirks, fancy.quirks);
    assert_eq!(loaded_fancy.rekey_limit_mb, Some(512));
}

#[test]
fn connection_protocol_round_trips() {
    use oryxis_core::models::connection::ConnectionProtocol;
    let vault = unlocked_vault();
    // Every variant survives the save/list cycle; this guards the
    // string mapping in `store/connections.rs` (a missed variant
    // silently reloads as Ssh via the fallthrough).
    for protocol in [
        ConnectionProtocol::Ssh,
        ConnectionProtocol::Telnet,
        ConnectionProtocol::Serial,
    ] {
        let mut conn = Connection::new("h", "example.com");
        conn.protocol = protocol;
        vault.save_connection(&conn, None).unwrap();
        let loaded = vault
            .list_connections()
            .unwrap()
            .into_iter()
            .find(|c| c.id == conn.id)
            .expect("connection listed");
        assert_eq!(loaded.protocol, protocol);
    }
}

#[test]
fn mcp_list_excludes_non_ssh_hosts() {
    use oryxis_core::models::connection::ConnectionProtocol;
    let vault = unlocked_vault();
    // An SSH host with MCP on is listed.
    let mut ssh = Connection::new("box", "example.com");
    ssh.mcp_enabled = true;
    vault.save_connection(&ssh, None).unwrap();
    // A Telnet host that (e.g. via sync from an old peer) still carries
    // mcp_enabled = true must NOT be advertised: the MCP handler resolves
    // through the SSH engine and would dial it as SSH.
    let mut telnet = Connection::new("router", "192.168.0.1");
    telnet.protocol = ConnectionProtocol::Telnet;
    telnet.mcp_enabled = true;
    vault.save_connection(&telnet, None).unwrap();
    let mut serial = Connection::new("uart", "/dev/ttyUSB0");
    serial.protocol = ConnectionProtocol::Serial;
    serial.mcp_enabled = true;
    vault.save_connection(&serial, None).unwrap();

    let mcp = vault.list_mcp_connections().unwrap();
    assert!(mcp.iter().any(|c| c.id == ssh.id), "ssh host missing");
    assert!(
        !mcp.iter().any(|c| c.id == telnet.id),
        "telnet host must not be MCP-advertised"
    );
    assert!(
        !mcp.iter().any(|c| c.id == serial.id),
        "serial host must not be MCP-advertised"
    );
}

#[test]
fn connection_remote_desktop_round_trip() {
    use oryxis_core::models::connection::ConnectionProtocol;
    use oryxis_core::models::remote_desktop::RemoteDesktopKind;
    let vault = unlocked_vault();
    let gateway_id = uuid::Uuid::new_v4();
    let mut conn = Connection::new("desktop", "10.0.0.9");
    conn.protocol = ConnectionProtocol::RemoteDesktop;
    conn.port = 5901;
    conn.rd_kind = RemoteDesktopKind::Vnc;
    conn.rd_gateway_id = Some(gateway_id);
    vault.save_connection(&conn, None).unwrap();
    let loaded = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("connection listed");
    assert_eq!(loaded.protocol, ConnectionProtocol::RemoteDesktop);
    assert_eq!(loaded.rd_kind, RemoteDesktopKind::Vnc);
    assert_eq!(loaded.rd_gateway_id, Some(gateway_id));

    // A plain SSH host defaults: RDP kind, no gateway.
    let plain = Connection::new("plain", "x");
    vault.save_connection(&plain, None).unwrap();
    let loaded = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == plain.id)
        .unwrap();
    assert_eq!(loaded.protocol, ConnectionProtocol::Ssh);
    assert_eq!(loaded.rd_kind, RemoteDesktopKind::Rdp);
    assert_eq!(loaded.rd_gateway_id, None);
}

#[test]
fn connection_serial_params_round_trip() {
    use oryxis_core::models::connection::ConnectionProtocol;
    use oryxis_core::models::serial::{
        SerialFlowControl, SerialLineEnding, SerialParams, SerialParity, SerialStopBits,
    };
    let vault = unlocked_vault();
    // Non-default params must survive the JSON column round trip.
    let params = SerialParams {
        baud: 250000,
        data_bits: 7,
        parity: SerialParity::Even,
        stop_bits: SerialStopBits::Two,
        flow_control: SerialFlowControl::Hardware,
        local_echo: true,
        line_ending: SerialLineEnding::CrLf,
    };
    let mut conn = Connection::new("uart", "/dev/ttyUSB0");
    conn.protocol = ConnectionProtocol::Serial;
    conn.serial = Some(params);
    vault.save_connection(&conn, None).unwrap();
    let loaded = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("connection listed");
    assert_eq!(loaded.serial, Some(params));

    // A non-serial host stores no params (NULL column -> None).
    let ssh = Connection::new("box", "example.com");
    vault.save_connection(&ssh, None).unwrap();
    let loaded_ssh = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == ssh.id)
        .unwrap();
    assert_eq!(loaded_ssh.serial, None);
}

#[test]
fn connection_auth_method_round_trips() {
    use oryxis_core::models::connection::AuthMethod;
    let vault = unlocked_vault();
    // Every variant must survive the save/list cycle. This guards the
    // string mapping in `store/connections.rs` (serialize + the
    // `_ => Auto` deserialize fallthrough), which the compiler can't
    // check: a missed variant silently reloads as Auto.
    for method in [
        AuthMethod::Auto,
        AuthMethod::Password,
        AuthMethod::Key,
        AuthMethod::Agent,
        AuthMethod::Interactive,
        AuthMethod::PasswordPrompt,
    ] {
        let mut conn = Connection::new("h", "example.com");
        conn.auth_method = method.clone();
        vault.save_connection(&conn, None).unwrap();
        let loaded = vault
            .list_connections()
            .unwrap()
            .into_iter()
            .find(|c| c.id == conn.id)
            .expect("connection listed");
        assert_eq!(loaded.auth_method, method);
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
fn connection_password_clears_with_empty_string() {
    let vault = unlocked_vault();
    let conn = Connection::new("h", "host");
    vault.save_connection(&conn, Some("first")).unwrap();
    assert_eq!(
        vault.get_connection_password(&conn.id).unwrap(),
        Some("first".to_string())
    );

    // `Some("")` clears the column (NULL), not an encrypted empty blob.
    vault.save_connection(&conn, Some("")).unwrap();
    assert_eq!(vault.get_connection_password(&conn.id).unwrap(), None);
    let raw: Option<Vec<u8>> = vault
        .db
        .query_row(
            "SELECT password FROM connections WHERE id = ?1",
            params![conn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(raw.is_none(), "cleared password left a non-NULL column");
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


/// Re-saving a connection (any edit that doesn't touch the proxy
/// password) must not wipe the stored proxy password. `INSERT OR
/// REPLACE` resets columns missing from its list to NULL, so
/// `save_connection` has to carry the encrypted column and
/// SELECT-preserve it exactly like the main password.
#[test]
fn proxy_password_survives_unrelated_resave() {
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "host");
    vault.save_connection(&conn, None).unwrap();
    vault.set_proxy_password(&conn.id, Some("keepme")).unwrap();

    conn.label = "renamed".into();
    vault.save_connection(&conn, None).unwrap();

    assert_eq!(
        vault.get_proxy_password(&conn.id).unwrap().as_deref(),
        Some("keepme"),
        "editing an unrelated field wiped the proxy password"
    );
}


/// Telnet options and local-shell settings live in their own JSON
/// columns, written by hand-maintained positional parameter lists and
/// read back by hand-maintained column indices. Nothing about that is
/// checked by the model's serde tests: a wrong index reads `None`
/// SILENTLY, and the host quietly loses its TLS or its terminal on the
/// next listing. This is the test that catches that.
#[test]
fn telnet_and_local_columns_round_trip() {
    use oryxis_core::models::connection::ConnectionProtocol;
    let vault = unlocked_vault();

    let mut telnet = Connection::new("switch", "10.0.0.1");
    telnet.protocol = ConnectionProtocol::Telnet;
    telnet.port = 992;
    telnet.telnet = Some(oryxis_core::models::telnet::TelnetOptions {
        tls: true,
        tls_insecure: true,
    });
    vault.save_connection(&telnet, None).unwrap();

    let terminal_id = uuid::Uuid::new_v4();
    let mut local = Connection::new("Claude", "");
    local.protocol = ConnectionProtocol::Local;
    local.initial_command = Some("claude".into());
    local.local = Some(oryxis_core::models::local::LocalConfig {
        terminal_id: Some(terminal_id),
        terminal_label: Some("PowerShell".into()),
        cwd: Some("~/work".into()),
    });
    vault.save_connection(&local, None).unwrap();

    let listed = vault.list_connections().unwrap();
    let stored_telnet = listed.iter().find(|c| c.id == telnet.id).expect("telnet host");
    assert_eq!(stored_telnet.protocol, ConnectionProtocol::Telnet);
    let opts = stored_telnet.telnet.expect("telnet options survive the column");
    assert!(opts.tls);
    assert!(opts.tls_insecure);

    let stored_local = listed.iter().find(|c| c.id == local.id).expect("local host");
    assert_eq!(stored_local.protocol, ConnectionProtocol::Local);
    let cfg = stored_local.local.as_ref().expect("local config survives the column");
    assert_eq!(cfg.terminal_id, Some(terminal_id));
    assert_eq!(cfg.terminal_label.as_deref(), Some("PowerShell"));
    assert_eq!(cfg.effective_cwd(), Some("~/work"));
    assert_eq!(stored_local.initial_command.as_deref(), Some("claude"));
}

/// The same columns must survive a save that has nothing to do with
/// them: `INSERT OR REPLACE` nulls every column missing from its
/// parameter list, which is how a field silently disappears on the next
/// unrelated edit (regression shape of
/// `proxy_password_survives_unrelated_resave`).
#[test]
fn telnet_and_local_columns_survive_an_unrelated_resave() {
    use oryxis_core::models::connection::ConnectionProtocol;
    let vault = unlocked_vault();
    let mut conn = Connection::new("switch", "10.0.0.1");
    conn.protocol = ConnectionProtocol::Telnet;
    conn.telnet = Some(oryxis_core::models::telnet::TelnetOptions {
        tls: true,
        tls_insecure: false,
    });
    vault.save_connection(&conn, None).unwrap();

    conn.label = "renamed".into();
    vault.save_connection(&conn, None).unwrap();

    let stored = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("host still listed");
    assert!(
        stored.telnet.is_some_and(|t| t.tls),
        "an unrelated rename dropped the Telnet TLS option"
    );
}

/// An all-default options blob is stored as NULL, so a host whose TLS
/// was turned on and back off is indistinguishable from one that never
/// had it. Otherwise the column would record a visit rather than a
/// setting, and every such host would differ on the sync wire.
#[test]
fn default_telnet_options_are_stored_as_none() {
    use oryxis_core::models::connection::ConnectionProtocol;
    let vault = unlocked_vault();
    let mut conn = Connection::new("switch", "10.0.0.1");
    conn.protocol = ConnectionProtocol::Telnet;
    conn.telnet = Some(oryxis_core::models::telnet::TelnetOptions::default());
    vault.save_connection(&conn, None).unwrap();

    let stored = vault
        .list_connections()
        .unwrap()
        .into_iter()
        .find(|c| c.id == conn.id)
        .expect("host still listed");
    assert_eq!(stored.telnet, None);
}

/// TOTP secret round-trip: set / get / clear via empty and None, and
/// the encrypted column must never hold the plaintext bytes.
#[test]
fn totp_secret_roundtrip_and_never_plaintext() {
    let vault = unlocked_vault();
    let conn = Connection::new("h", "host");
    vault.save_connection(&conn, None).unwrap();

    let secret = "JBSWY3DPEHPK3PXP";
    vault.set_connection_totp_secret(&conn.id, Some(secret)).unwrap();
    assert_eq!(
        vault.get_connection_totp_secret(&conn.id).unwrap().as_deref(),
        Some(secret)
    );

    // The stored blob is ciphertext, not the raw secret.
    let raw: Option<Vec<u8>> = vault
        .db
        .query_row(
            "SELECT totp_secret FROM connections WHERE id = ?1",
            params![conn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    let raw = raw.expect("column must be populated");
    assert!(
        !raw.windows(secret.len()).any(|w| w == secret.as_bytes()),
        "TOTP secret stored in plaintext"
    );

    vault.set_connection_totp_secret(&conn.id, Some("")).unwrap();
    assert_eq!(vault.get_connection_totp_secret(&conn.id).unwrap(), None);
    vault.set_connection_totp_secret(&conn.id, Some(secret)).unwrap();
    vault.set_connection_totp_secret(&conn.id, None).unwrap();
    assert_eq!(vault.get_connection_totp_secret(&conn.id).unwrap(), None);
}


/// Like the proxy password, the TOTP secret must survive a re-save of
/// the connection that doesn't touch it.
#[test]
fn totp_secret_survives_unrelated_resave() {
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "host");
    vault.save_connection(&conn, None).unwrap();
    vault
        .set_connection_totp_secret(&conn.id, Some("JBSWY3DPEHPK3PXP"))
        .unwrap();

    conn.label = "renamed".into();
    vault.save_connection(&conn, None).unwrap();

    assert_eq!(
        vault.get_connection_totp_secret(&conn.id).unwrap().as_deref(),
        Some("JBSWY3DPEHPK3PXP"),
        "editing an unrelated field wiped the TOTP secret"
    );
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

// ── Connection.sftp_initial_path ──

#[test]
fn sftp_initial_path_roundtrips_and_blank_reads_as_none() {
    // The per-host SFTP landing folder is a plain additive column, but a
    // blank value must come back as `None` ("the login directory"), never
    // as an empty path the mount would try to canonicalize.
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "host");
    conn.sftp_initial_path = Some("/srv/www".into());
    vault.save_connection(&conn, None).unwrap();
    let stored = vault.list_connections().unwrap();
    assert_eq!(
        stored[0].sftp_initial_path.as_deref(),
        Some("/srv/www"),
        "landing folder must survive a save/list round trip"
    );

    conn.sftp_initial_path = Some("   ".into());
    vault.save_connection(&conn, None).unwrap();
    assert_eq!(vault.list_connections().unwrap()[0].sftp_initial_path, None);

    conn.sftp_initial_path = None;
    vault.save_connection(&conn, None).unwrap();
    assert_eq!(vault.list_connections().unwrap()[0].sftp_initial_path, None);
}

// ── Connection.zmodem_drops ──

#[test]
fn zmodem_drops_roundtrips_and_defaults_to_off() {
    // The per-host drop-transport flag is a plain additive column; an
    // untouched host must read back as off (the standard drop routing).
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "host");
    vault.save_connection(&conn, None).unwrap();
    assert!(
        !vault.list_connections().unwrap()[0].zmodem_drops,
        "a host that never set the flag reads as off"
    );

    conn.zmodem_drops = true;
    vault.save_connection(&conn, None).unwrap();
    assert!(
        vault.list_connections().unwrap()[0].zmodem_drops,
        "the flag must survive a save/list round trip"
    );

    conn.zmodem_drops = false;
    vault.save_connection(&conn, None).unwrap();
    assert!(!vault.list_connections().unwrap()[0].zmodem_drops);
}

// ── Group.cloud_query ──

// ── Login scripts ──

/// The script reference + its variables round-trip through the plain
/// `login_script` column, and both halves survive independently.
#[test]
fn login_script_reference_round_trips() {
    use oryxis_core::models::connection::ScriptVar;
    let vault = unlocked_vault();
    let script = LoginScript::new("jumpserver");
    vault.save_login_script(&script).unwrap();

    let mut conn = Connection::new("h", "bastion.example");
    conn.login_script_id = Some(script.id);
    conn.login_script_vars = vec![ScriptVar {
        name: "asset".into(),
        value: "web-01".into(),
    }];
    vault.save_connection(&conn, None).unwrap();

    let stored = &vault.list_connections().unwrap()[0];
    assert_eq!(stored.login_script_id, Some(script.id));
    assert_eq!(stored.login_script_vars.len(), 1);
    assert_eq!(stored.login_script_vars[0].name, "asset");
    assert_eq!(stored.login_script_vars[0].value, "web-01");

    // Detaching clears both halves.
    conn.login_script_id = None;
    conn.login_script_vars.clear();
    vault.save_connection(&conn, None).unwrap();
    let stored = &vault.list_connections().unwrap()[0];
    assert_eq!(stored.login_script_id, None);
    assert!(stored.login_script_vars.is_empty());
}

/// Deleting a script detaches every host that referenced it, so the
/// editor can't show a picker entry pointing at nothing.
#[test]
fn deleting_a_login_script_detaches_its_hosts() {
    let vault = unlocked_vault();
    let script = LoginScript::new("koko");
    vault.save_login_script(&script).unwrap();

    let mut conn = Connection::new("h", "bastion.example");
    conn.login_script_id = Some(script.id);
    vault.save_connection(&conn, None).unwrap();
    assert_eq!(
        vault.login_script_usage().unwrap().get(&script.id),
        Some(&1usize)
    );

    vault.delete_login_script(&script.id).unwrap();
    assert!(vault.list_login_scripts().unwrap().is_empty());
    assert_eq!(vault.list_connections().unwrap()[0].login_script_id, None);
}

/// Same tri-state contract as every other credential: None preserves,
/// Some("") clears, Some(v) stores.
#[test]
fn target_password_tri_state() {
    let vault = unlocked_vault();
    let conn = Connection::new("h", "host");
    vault.save_connection(&conn, None).unwrap();

    vault
        .set_connection_target_password(&conn.id, Some("asset-pw"))
        .unwrap();
    assert_eq!(
        vault.get_connection_target_password(&conn.id).unwrap().as_deref(),
        Some("asset-pw")
    );
    vault.set_connection_target_password(&conn.id, Some("")).unwrap();
    assert_eq!(vault.get_connection_target_password(&conn.id).unwrap(), None);
}

/// Like the proxy password and the TOTP secret, the target password
/// must survive a re-save of the connection that doesn't touch it.
#[test]
fn target_password_survives_unrelated_resave() {
    let vault = unlocked_vault();
    let mut conn = Connection::new("h", "host");
    vault.save_connection(&conn, None).unwrap();
    vault
        .set_connection_target_password(&conn.id, Some("asset-pw"))
        .unwrap();

    conn.label = "renamed".into();
    vault.save_connection(&conn, None).unwrap();

    assert_eq!(
        vault.get_connection_target_password(&conn.id).unwrap().as_deref(),
        Some("asset-pw"),
        "editing an unrelated field wiped the target password"
    );
}

/// Structural, in the spirit of `proxy_password_does_not_leak_into_proxy_column`:
/// neither the script's own `steps` column nor the host's `login_script`
/// column may ever contain credential material. The types already make
/// it impossible (a step carries a `SecretRef` discriminant, and a
/// variable is a plain name/value pair), so this is the guard that keeps
/// it that way.
#[test]
fn login_script_columns_never_carry_a_secret() {
    use oryxis_core::login_script::{ExpectPattern, LoginStep, SecretRef, SendPayload};
    use oryxis_core::models::connection::ScriptVar;
    let vault = unlocked_vault();

    let mut script = LoginScript::new("koko");
    script.steps = vec![
        LoginStep {
            expect: Some(ExpectPattern::Suffix("opt>".into())),
            send: SendPayload::Text("{asset}".into()),
            timeout_ms: 0,
            optional: false,
        },
        LoginStep {
            expect: Some(ExpectPattern::Suffix("password:".into())),
            send: SendPayload::Secret(SecretRef::TargetPassword),
            timeout_ms: 0,
            optional: false,
        },
    ];
    vault.save_login_script(&script).unwrap();

    let mut conn = Connection::new("h", "bastion.example");
    conn.login_script_id = Some(script.id);
    conn.login_script_vars = vec![ScriptVar {
        name: "asset".into(),
        value: "web-01".into(),
    }];
    vault.save_connection(&conn, None).unwrap();
    vault
        .set_connection_target_password(&conn.id, Some("should-not-persist"))
        .unwrap();

    let raw_steps: String = vault
        .db
        .query_row(
            "SELECT steps FROM login_scripts WHERE id = ?1",
            params![script.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !raw_steps.contains("should-not-persist"),
        "target password leaked into the plaintext steps column: {raw_steps}"
    );

    let raw_ref: String = vault
        .db
        .query_row(
            "SELECT login_script FROM connections WHERE id = ?1",
            params![conn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !raw_ref.contains("should-not-persist"),
        "target password leaked into the plaintext login_script column: {raw_ref}"
    );
}

#[test]
fn per_host_highlight_rules_round_trip() {
    use oryxis_core::models::{HighlightRule, HostHighlightRules, TriggerAction};
    let vault = unlocked_vault();
    // Guards the JSON column and its positional index in
    // `store/connections.rs`: a wrong index reloads silently as None,
    // which reads as "this host follows the global rules" and would be
    // indistinguishable from working.
    let mut plain = Connection::new("plain", "example.com");
    plain.highlight_rules = None;
    vault.save_connection(&plain, None).unwrap();

    let mut fancy = Connection::new("fancy", "example.com");
    fancy.highlight_rules = Some(HostHighlightRules {
        rules: vec![HighlightRule {
            id: "r1".into(),
            name: "Disk full".into(),
            pattern: "No space left".into(),
            is_regex: false,
            case_sensitive: true,
            color: "#ff0000".into(),
            enabled: true,
            action: TriggerAction::Beep,
        }],
        replace: true,
    });
    vault.save_connection(&fancy, None).unwrap();

    let list = vault.list_connections().unwrap();
    assert_eq!(
        list.iter().find(|c| c.id == plain.id).unwrap().highlight_rules,
        None
    );
    assert_eq!(
        list.iter().find(|c| c.id == fancy.id).unwrap().highlight_rules,
        fancy.highlight_rules
    );
}

/// mosh options survive the column, and a host nobody configured keeps
/// a NULL rather than growing a blob that only records a visit.
#[test]
fn mosh_options_round_trip_and_a_plain_host_stores_nothing() {
    let vault = unlocked_vault();

    let plain = Connection::new("plain", "10.0.0.1");
    vault.save_connection(&plain, None).unwrap();

    let mut carried = Connection::new("carried", "10.0.0.2");
    carried.mosh = Some(oryxis_core::models::mosh::MoshOptions {
        enabled: true,
        server_path: "/opt/mosh/bin/mosh-server".into(),
        port_range: "60000:60010".into(),
        command: "tmux new -A -s main".into(),
    });
    vault.save_connection(&carried, None).unwrap();

    // An all-default value is a host nobody configured, so it is stored
    // as nothing at all rather than as an object full of defaults.
    let mut visited = Connection::new("visited", "10.0.0.3");
    visited.mosh = Some(oryxis_core::models::mosh::MoshOptions::default());
    vault.save_connection(&visited, None).unwrap();

    let list = vault.list_connections().unwrap();
    let of = |id| list.iter().find(|c| c.id == id).unwrap().mosh.clone();
    assert_eq!(of(plain.id), None, "an untouched host is not a mosh host");
    assert_eq!(of(visited.id), None, "and neither is one merely opened");
    let back = of(carried.id).expect("the options come back");
    assert!(back.enabled);
    assert_eq!(back.server_path, "/opt/mosh/bin/mosh-server");
    assert_eq!(back.port_range, "60000:60010");
    assert_eq!(back.command, "tmux new -A -s main");
}

use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Connections CRUD
    // -----------------------------------------------------------------------

    /// Save a connection. If `password` is provided, it's encrypted.
    pub fn save_connection(
        &self,
        conn: &Connection,
        password: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted_pw = match password {
            // Tri-state: empty string clears the password (NULL column),
            // never an encrypted empty blob (mirrors `save_cloud_profile`).
            Some("") => None,
            Some(pw) => Some(self.encrypt_field(pw)?),
            None => {
                // Keep existing password if not provided
                let existing: Option<Vec<u8>> = self
                    .db
                    .query_row(
                        "SELECT password FROM connections WHERE id = ?1",
                        params![conn.id.to_string()],
                        |row| row.get(0),
                    )
                    .ok();
                existing
            }
        };

        let auth_str = match conn.auth_method {
            AuthMethod::Auto => "auto",
            AuthMethod::Password => "password",
            AuthMethod::Key => "key",
            AuthMethod::Agent => "agent",
            AuthMethod::Interactive => "interactive",
            AuthMethod::PasswordPrompt => "password_prompt",
            AuthMethod::Certificate => "certificate",
        };

        let protocol_str = match conn.protocol {
            ConnectionProtocol::Ssh => "ssh",
            ConnectionProtocol::Telnet => "telnet",
            ConnectionProtocol::Raw => "raw",
            ConnectionProtocol::Serial => "serial",
            ConnectionProtocol::Local => "local",
            ConnectionProtocol::RemoteDesktop => "remote_desktop",
        };
        // Serial line parameters as JSON (NULL on non-serial hosts).
        let serial_json = conn
            .serial
            .as_ref()
            .map(|s| serde_json::to_string(s).unwrap_or_default());
        // Telnet options as JSON. An all-default value is written as
        // NULL: a host whose TLS was turned on and back off must be
        // indistinguishable from one that never had it, so the column
        // never grows a blob that only records a visit.
        let telnet_json = conn
            .telnet
            .filter(|t| !t.is_default())
            .map(|t| serde_json::to_string(&t).unwrap_or_default());
        // mosh options as JSON, on the same terms: an all-default value
        // is a host nobody configured, and NULL says so.
        let mosh_json = conn
            .mosh
            .as_ref()
            .filter(|m| !m.is_default())
            .map(|m| serde_json::to_string(m).unwrap_or_default());
        // Local-shell settings as JSON (NULL on non-local hosts, and on
        // a local host that just takes the default shell).
        let local_json = conn
            .local
            .as_ref()
            .filter(|l| !l.is_default())
            .map(|l| serde_json::to_string(l).unwrap_or_default());
        // Remote-desktop fields: kind + optional SSH gateway. Written on
        // every host (cheap scalars); only meaningful for RemoteDesktop.
        let rd_kind_str = match conn.rd_kind {
            RemoteDesktopKind::Rdp => "rdp",
            RemoteDesktopKind::Vnc => "vnc",
        };
        let rd_gateway_str = conn.rd_gateway_id.map(|u| u.to_string());
        // Address-family preference ('auto' | 'v4' | 'v6'); NULL and
        // 'auto' both read back as Auto.
        let family_str = match conn.address_family {
            AddressFamily::Auto => "auto",
            AddressFamily::V4 => "v4",
            AddressFamily::V6 => "v6",
        };
        // Ambiguous width ('auto' | 'narrow' | 'wide'); NULL and 'auto'
        // both read back as Auto, so every host that predates the column
        // keeps the narrow measurement it has always had.
        let width_str = match conn.ambiguous_width {
            AmbiguousWidth::Auto => "auto",
            AmbiguousWidth::Narrow => "narrow",
            AmbiguousWidth::Wide => "wide",
        };

        // The proxy password, TOTP secret and target password live in
        // their own encrypted columns, written only by their dedicated
        // setters. INSERT OR REPLACE resets every column missing from
        // its list to NULL, so all three must be carried through each
        // save (regression: `proxy_password_survives_unrelated_resave`).
        type CarriedSecrets = (Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>);
        let (existing_proxy_pw, existing_totp, existing_target_pw): CarriedSecrets = self
            .db
            .query_row(
                "SELECT proxy_password, totp_secret, target_password FROM connections WHERE id = ?1",
                params![conn.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap_or((None, None, None));

        // `{ id, vars }` in one plaintext column: the id is a reference
        // and the vars are placeholder values, never a credential.
        let login_script_json = conn.login_script_id.map(|id| {
            serde_json::json!({
                "id": id.to_string(),
                "vars": conn.login_script_vars,
            })
            .to_string()
        });

        self.db.execute(
            "INSERT OR REPLACE INTO connections
             (id, label, hostname, port, username, auth_method, key_id, group_id,
              jump_chain, proxy, tags, notes, color, password, last_used, created_at, updated_at, identity_id, mcp_enabled, port_forwards,
              detected_os, custom_icon, custom_color, agent_forwarding, proxy_identity_id, terminal_theme, cloud_ref, initial_command, keepalive_interval, icon_style, customized_fields, env_vars, encoding, session_logging, startup_snippet_id, auto_title, terminal_type, ciphers, kex, macs, host_key_algorithms, privacy_mode, proxy_password, totp_secret, protocol, serial_config, rd_kind, rd_gateway_id, address_family, quirks, rekey_limit_mb, monitor_enabled, sidebar_auto_open, x11_forwarding, sftp_initial_path, mac_address, login_script, target_password, terminal_appearance, highlight_rules, monitor_disks, use_disk_key, identity_file, telnet_config, local_config, mosh_config, zmodem_drops, ambiguous_width)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37,?38,?39,?40,?41,?42,?43,?44,?45,?46,?47,?48,?49,?50,?51,?52,?53,?54,?55,?56,?57,?58,?59,?60,?61,?62,?63,?64,?65,?66,?67,?68)",
            params![
                conn.id.to_string(),
                conn.label,
                conn.hostname,
                conn.port,
                conn.username,
                auth_str,
                conn.key_id.map(|u| u.to_string()),
                conn.group_id.map(|u| u.to_string()),
                serde_json::to_string(&conn.jump_chain).unwrap_or_default(),
                conn.proxy.as_ref().map(|p| serde_json::to_string(p).unwrap_or_default()),
                serde_json::to_string(&conn.tags).unwrap_or_default(),
                conn.notes,
                conn.color,
                encrypted_pw,
                conn.last_used.map(|d| d.to_rfc3339()),
                conn.created_at.to_rfc3339(),
                conn.updated_at.to_rfc3339(),
                conn.identity_id.map(|u| u.to_string()),
                conn.mcp_enabled as i32,
                if conn.port_forwards.is_empty() { None } else { Some(serde_json::to_string(&conn.port_forwards).unwrap_or_default()) },
                // OS detection + custom icon overrides, saved on every
                // write so they survive edits. Previously these were left
                // out and got wiped to NULL on each save.
                conn.detected_os,
                conn.custom_icon,
                conn.custom_color,
                conn.agent_forwarding as i32,
                conn.proxy_identity_id.map(|u| u.to_string()),
                conn.terminal_theme,
                conn.cloud_ref.as_ref().map(|r| serde_json::to_string(r).unwrap_or_default()),
                conn.initial_command,
                conn.keepalive_interval,
                conn.icon_style,
                if conn.customized_fields.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&conn.customized_fields).unwrap_or_default())
                },
                if conn.env_vars.is_empty() { None } else { Some(serde_json::to_string(&conn.env_vars).unwrap_or_default()) },
                conn.encoding,
                conn.session_logging.map(|b| b as i32),
                conn.startup_snippet_id.map(|u| u.to_string()),
                conn.auto_title.map(|b| b as i32),
                conn.terminal_type,
                conn.ciphers.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                conn.kex.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                conn.macs.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                conn.host_key_algorithms.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
                conn.privacy_mode.map(|b| b as i32),
                existing_proxy_pw,
                existing_totp,
                protocol_str,
                serial_json,
                rd_kind_str,
                rd_gateway_str,
                family_str,
                conn.quirks.as_ref().map(|q| serde_json::to_string(q).unwrap_or_default()),
                conn.rekey_limit_mb,
                conn.monitor_enabled as i32,
                conn.sidebar_auto_open.map(|b| b as i32),
                conn.x11_forwarding as i32,
                conn.sftp_initial_path,
                conn.mac_address,
                login_script_json,
                existing_target_pw,
                conn.terminal_appearance
                    .as_ref()
                    .map(|a| serde_json::to_string(a).unwrap_or_default()),
                conn.highlight_rules
                    .as_ref()
                    .map(|r| serde_json::to_string(r).unwrap_or_default()),
                // Auto is NULL; Custom is the JSON list, empty list
                // included (that one means "no disks on this host").
                conn.monitor_disks
                    .as_ref()
                    .map(|d| serde_json::to_string(d).unwrap_or_default()),
                conn.use_disk_key as i32,
                // Blank reads as "no explicit path" (scan the defaults),
                // so an emptied editor field means the same thing as one
                // that was never filled.
                conn.identity_file
                    .as_ref()
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty()),
                telnet_json,
                local_json,
                mosh_json,
                conn.zmodem_drops as i32,
                width_str,
            ],
        )?;
        // Re-creation clears any stale tombstone for this id (the
        // entity came back from a peer after a local delete, or the
        // user re-added a host they'd just deleted).
        self.clear_tombstone("connection", &conn.id)?;
        Ok(())
    }

    pub fn list_connections(&self) -> Result<Vec<Connection>, VaultError> {
        self.list_connections_filtered(None)
    }

    /// List only MCP-enabled connections.
    pub fn list_mcp_connections(&self) -> Result<Vec<Connection>, VaultError> {
        self.list_connections_filtered(Some(true))
    }

    fn list_connections_filtered(&self, mcp_filter: Option<bool>) -> Result<Vec<Connection>, VaultError> {
        let query = match mcp_filter {
            Some(true) => {
                "SELECT id, label, hostname, port, username, auth_method, key_id, group_id,
                        jump_chain, proxy, tags, notes, color, last_used, created_at, updated_at, identity_id, mcp_enabled, port_forwards, detected_os, custom_icon, custom_color, agent_forwarding, proxy_identity_id, terminal_theme, cloud_ref, initial_command, keepalive_interval, icon_style, customized_fields, env_vars, encoding, session_logging, startup_snippet_id, auto_title, terminal_type, ciphers, kex, macs, host_key_algorithms, privacy_mode, protocol, serial_config, rd_kind, rd_gateway_id, address_family, quirks, rekey_limit_mb, monitor_enabled, sidebar_auto_open, x11_forwarding, sftp_initial_path, mac_address, login_script, terminal_appearance, highlight_rules, monitor_disks, use_disk_key, identity_file, telnet_config, local_config, mosh_config, zmodem_drops, ambiguous_width
                 FROM connections WHERE mcp_enabled = 1 AND (protocol IS NULL OR protocol = 'ssh') ORDER BY label"
            }
            _ => {
                "SELECT id, label, hostname, port, username, auth_method, key_id, group_id,
                        jump_chain, proxy, tags, notes, color, last_used, created_at, updated_at, identity_id, mcp_enabled, port_forwards, detected_os, custom_icon, custom_color, agent_forwarding, proxy_identity_id, terminal_theme, cloud_ref, initial_command, keepalive_interval, icon_style, customized_fields, env_vars, encoding, session_logging, startup_snippet_id, auto_title, terminal_type, ciphers, kex, macs, host_key_algorithms, privacy_mode, protocol, serial_config, rd_kind, rd_gateway_id, address_family, quirks, rekey_limit_mb, monitor_enabled, sidebar_auto_open, x11_forwarding, sftp_initial_path, mac_address, login_script, terminal_appearance, highlight_rules, monitor_disks, use_disk_key, identity_file, telnet_config, local_config, mosh_config, zmodem_drops, ambiguous_width
                 FROM connections ORDER BY label"
            }
        };
        let mut stmt = self.db.prepare(query)?;
        let conns = stmt
            .query_map([], |row| {
                let auth_str: String = row.get(5)?;
                let auth_method = match auth_str.as_str() {
                    "auto" => AuthMethod::Auto,
                    "password" => AuthMethod::Password,
                    "key" => AuthMethod::Key,
                    "agent" => AuthMethod::Agent,
                    "interactive" => AuthMethod::Interactive,
                    "password_prompt" => AuthMethod::PasswordPrompt,
                    "certificate" => AuthMethod::Certificate,
                    _ => AuthMethod::Auto,
                };

                // NULL (pre-Telnet rows) and any unknown value read as
                // SSH, the only protocol those rows could have meant.
                let protocol = match row.get::<_, Option<String>>(41).ok().flatten().as_deref() {
                    Some("telnet") => ConnectionProtocol::Telnet,
                    Some("raw") => ConnectionProtocol::Raw,
                    Some("serial") => ConnectionProtocol::Serial,
                    Some("local") => ConnectionProtocol::Local,
                    Some("remote_desktop") => ConnectionProtocol::RemoteDesktop,
                    _ => ConnectionProtocol::Ssh,
                };
                // Serial params JSON (NULL / malformed -> None; the
                // connect path falls back to SerialParams::default()).
                let serial = row
                    .get::<_, Option<String>>(42)
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str(&s).ok());
                // Telnet options JSON. Malformed reads as None, i.e.
                // plain Telnet with verification on: the failure mode of
                // an unreadable column must never be a session that
                // skips certificate checks.
                let telnet = row
                    .get::<_, Option<String>>(59)
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str(&s).ok());
                // mosh options JSON (NULL / malformed -> None, which is
                // an ordinary SSH shell). An unreadable column must
                // never decode into a session carried somewhere the
                // user did not ask for.
                let mosh = row
                    .get::<_, Option<String>>(61)
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str(&s).ok());
                // Local-shell settings JSON (NULL / malformed -> None,
                // which spawns the user's default shell).
                let local = row
                    .get::<_, Option<String>>(60)
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str(&s).ok());
                // Remote-desktop kind (NULL / unknown -> RDP) + optional
                // SSH gateway id.
                let rd_kind = match row.get::<_, Option<String>>(43).ok().flatten().as_deref() {
                    Some("vnc") => RemoteDesktopKind::Vnc,
                    _ => RemoteDesktopKind::Rdp,
                };
                let rd_gateway_id = row
                    .get::<_, Option<String>>(44)
                    .ok()
                    .flatten()
                    .and_then(|s| Uuid::parse_str(&s).ok());
                // Address-family preference (NULL / unknown -> Auto).
                let address_family =
                    match row.get::<_, Option<String>>(45).ok().flatten().as_deref() {
                        Some("v4") => AddressFamily::V4,
                        Some("v6") => AddressFamily::V6,
                        _ => AddressFamily::Auto,
                    };
                // Ambiguous width (NULL / unknown -> Auto).
                let ambiguous_width =
                    match row.get::<_, Option<String>>(63).ok().flatten().as_deref() {
                        Some("narrow") => AmbiguousWidth::Narrow,
                        Some("wide") => AmbiguousWidth::Wide,
                        _ => AmbiguousWidth::Auto,
                    };
                // Login automation reference `{ id, vars }`. Malformed
                // JSON reads as no automation rather than failing the
                // whole row: a host that cannot be listed is worse than
                // one whose bastion script needs re-picking.
                let login_script: Option<serde_json::Value> = row
                    .get::<_, Option<String>>(53)
                    .ok()
                    .flatten()
                    .and_then(|s| serde_json::from_str(&s).ok());

                Ok(Connection {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    hostname: row.get(2)?,
                    port: row.get(3)?,
                    protocol,
                    serial,
                    telnet,
                    mosh,
                    local,
                    rd_kind,
                    rd_gateway_id,
                    address_family,
                    ambiguous_width,
                    username: row.get(4)?,
                    auth_method,
                    key_id: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    identity_id: row
                        .get::<_, Option<String>>(16)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    group_id: row
                        .get::<_, Option<String>>(7)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    jump_chain: row
                        .get::<_, Option<String>>(8)?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    proxy: row
                        .get::<_, Option<String>>(9)?
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    tags: row
                        .get::<_, Option<String>>(10)?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    notes: row.get(11)?,
                    color: row.get(12)?,
                    port_forwards: row
                        .get::<_, Option<String>>(18)?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    env_vars: row
                        .get::<_, Option<String>>(30)?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    encoding: row.get::<_, Option<String>>(31)?,
                    mcp_enabled: row.get::<_, Option<i32>>(17)?.unwrap_or(1) != 0,
                    last_used: row
                        .get::<_, Option<String>>(13)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc)),
                    created_at: row
                        .get::<_, String>(14)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, String>(15)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    detected_os: row.get::<_, Option<String>>(19).unwrap_or(None),
                    custom_icon: row.get::<_, Option<String>>(20).unwrap_or(None),
                    custom_color: row.get::<_, Option<String>>(21).unwrap_or(None),
                    agent_forwarding: row
                        .get::<_, Option<i32>>(22)
                        .unwrap_or(None)
                        .unwrap_or(0)
                        != 0,
                    proxy_identity_id: row
                        .get::<_, Option<String>>(23)
                        .ok()
                        .flatten()
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    terminal_theme: row
                        .get::<_, Option<String>>(24)
                        .ok()
                        .flatten(),
                    cloud_ref: row
                        .get::<_, Option<String>>(25)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str::<CloudRef>(&s).ok()),
                    initial_command: row
                        .get::<_, Option<String>>(26)
                        .ok()
                        .flatten(),
                    keepalive_interval: row
                        .get::<_, Option<i64>>(27)
                        .ok()
                        .flatten()
                        .and_then(|v| u32::try_from(v).ok()),
                    icon_style: row.get::<_, Option<String>>(28).ok().flatten(),
                    customized_fields: row
                        .get::<_, Option<String>>(29)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    session_logging: row
                        .get::<_, Option<i64>>(32)
                        .ok()
                        .flatten()
                        .map(|n| n != 0),
                    startup_snippet_id: row
                        .get::<_, Option<String>>(33)
                        .ok()
                        .flatten()
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    auto_title: row
                        .get::<_, Option<i64>>(34)
                        .ok()
                        .flatten()
                        .map(|n| n != 0),
                    terminal_type: row.get::<_, Option<String>>(35).ok().flatten(),
                    ciphers: row
                        .get::<_, Option<String>>(36)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    kex: row
                        .get::<_, Option<String>>(37)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    macs: row
                        .get::<_, Option<String>>(38)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    host_key_algorithms: row
                        .get::<_, Option<String>>(39)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    privacy_mode: row
                        .get::<_, Option<i64>>(40)
                        .ok()
                        .flatten()
                        .map(|n| n != 0),
                    quirks: row
                        .get::<_, Option<String>>(46)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    rekey_limit_mb: row
                        .get::<_, Option<i64>>(47)
                        .ok()
                        .flatten()
                        .and_then(|v| u32::try_from(v).ok()),
                    monitor_enabled: row
                        .get::<_, Option<i32>>(48)
                        .unwrap_or(None)
                        .unwrap_or(0)
                        != 0,
                    sidebar_auto_open: row
                        .get::<_, Option<i64>>(49)
                        .ok()
                        .flatten()
                        .map(|n| n != 0),
                    x11_forwarding: row
                        .get::<_, Option<i32>>(50)
                        .unwrap_or(None)
                        .unwrap_or(0)
                        != 0,
                    // Blank reads as "no override": an empty text field in
                    // the editor must mean the login directory, not a mount
                    // attempt on "".
                    sftp_initial_path: row
                        .get::<_, Option<String>>(51)
                        .ok()
                        .flatten()
                        .filter(|s| !s.trim().is_empty()),
                    // Blank reads as "no MAC" for the same reason: an
                    // emptied editor field must hide the card action.
                    mac_address: row
                        .get::<_, Option<String>>(52)
                        .ok()
                        .flatten()
                        .filter(|s| !s.trim().is_empty()),
                    terminal_appearance: row
                        .get::<_, Option<String>>(54)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    highlight_rules: row
                        .get::<_, Option<String>>(55)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    // NULL is Auto. A stored `[]` is Custom-with-nothing
                    // and must survive as `Some(vec![])`, so the parse
                    // failure fallback is the only path back to Auto.
                    monitor_disks: row
                        .get::<_, Option<String>>(56)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    use_disk_key: row
                        .get::<_, Option<i32>>(57)
                        .unwrap_or(None)
                        .unwrap_or(0)
                        != 0,
                    // Blank reads as "no explicit path", so the scan of
                    // the default names is what an emptied field means.
                    identity_file: row
                        .get::<_, Option<String>>(58)
                        .ok()
                        .flatten()
                        .filter(|s| !s.trim().is_empty()),
                    // NULL (a row from before the migration) reads as
                    // off, the standard drop routing.
                    zmodem_drops: row
                        .get::<_, Option<i32>>(62)
                        .unwrap_or(None)
                        .unwrap_or(0)
                        != 0,
                    login_script_id: login_script
                        .as_ref()
                        .and_then(|s| s.get("id"))
                        .and_then(|v| v.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok()),
                    login_script_vars: login_script
                        .as_ref()
                        .and_then(|s| s.get("vars"))
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(conns)
    }

    /// Update just the detected OS for a connection, used by the background
    /// OS-detection task so we don't overwrite other columns (e.g. last_used).
    pub fn set_detected_os(&self, id: &Uuid, os: Option<&str>) -> Result<(), VaultError> {
        self.db.execute(
            "UPDATE connections SET detected_os = ?1 WHERE id = ?2",
            params![os, id.to_string()],
        )?;
        Ok(())
    }

    /// IDs of connections whose `password` column is non-NULL. Mirrors
    /// [`Self::list_identity_ids_with_password`]: an existence check, so
    /// no decrypt and no `require_unlocked()`. The password-autofill
    /// popup (issue #117) uses it to decide which hosts have anything to
    /// offer without paying a decrypt per candidate.
    pub fn list_connection_ids_with_password(
        &self,
    ) -> Result<std::collections::HashSet<Uuid>, VaultError> {
        let mut stmt = self
            .db
            .prepare("SELECT id FROM connections WHERE password IS NOT NULL")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok().and_then(|s| Uuid::parse_str(&s).ok()))
            .collect();
        Ok(ids)
    }

    /// Whether a connection has a stored password, WITHOUT decrypting
    /// it. Presence feeds the group-inheritance resolver: credentials
    /// are one parameter family, so a host that stores its own password
    /// has answered it and a group identity default must not eclipse
    /// it. Presence-only on purpose (no unlock, no plaintext).
    pub fn connection_has_password(&self, id: &Uuid) -> Result<bool, VaultError> {
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT password FROM connections WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Connection {}", id)))?;
        Ok(data.is_some_and(|d| !d.is_empty()))
    }

    /// Get the decrypted password for a connection.
    pub fn get_connection_password(&self, id: &Uuid) -> Result<Option<String>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT password FROM connections WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Connection {}", id)))?;

        match data {
            Some(encrypted) => Ok(Some(self.decrypt_field(&encrypted)?)),
            None => Ok(None),
        }
    }

    /// Set the proxy password for a connection. `None` or an empty string
    /// clears it; otherwise the value is encrypted with the vault key.
    /// Stored in its own column so the plaintext `proxy` JSON column
    /// never carries credentials. Vault must be unlocked when setting a
    /// non-empty value (encryption needs the key); clearing works while
    /// locked.
    pub fn set_proxy_password(
        &self,
        id: &Uuid,
        password: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted: Option<Vec<u8>> = match password {
            Some(pw) if !pw.is_empty() => Some(self.encrypt_field(pw)?),
            _ => None,
        };
        self.db.execute(
            "UPDATE connections SET proxy_password = ?1 WHERE id = ?2",
            params![encrypted, id.to_string()],
        )?;
        Ok(())
    }

    /// Set the TOTP secret for a connection (the user's raw input, a bare
    /// Base32 secret or a full otpauth:// URI). `None` or an empty string
    /// clears it; otherwise the value is encrypted with the vault key.
    /// Stored in its own BLOB column, mirroring `set_proxy_password`, so
    /// no plaintext column ever carries it.
    pub fn set_connection_totp_secret(
        &self,
        id: &Uuid,
        secret: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted: Option<Vec<u8>> = match secret {
            Some(s) if !s.is_empty() => Some(self.encrypt_field(s)?),
            _ => None,
        };
        self.db.execute(
            "UPDATE connections SET totp_secret = ?1 WHERE id = ?2",
            params![encrypted, id.to_string()],
        )?;
        Ok(())
    }

    /// Get the decrypted TOTP secret for a connection.
    pub fn get_connection_totp_secret(
        &self,
        id: &Uuid,
    ) -> Result<Option<String>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT totp_secret FROM connections WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Connection {}", id)))?;

        match data {
            Some(encrypted) => Ok(Some(self.decrypt_field(&encrypted)?)),
            None => Ok(None),
        }
    }

    /// Set the target password for a connection: the credential a login
    /// script types at the ASSET's prompt, after the connection's own
    /// `password` has already been spent on the bastion login. `None`
    /// or an empty string clears it. Same encrypted-column scheme as
    /// `set_connection_totp_secret`, for the same reason.
    pub fn set_connection_target_password(
        &self,
        id: &Uuid,
        password: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted: Option<Vec<u8>> = match password {
            Some(pw) if !pw.is_empty() => Some(self.encrypt_field(pw)?),
            _ => None,
        };
        self.db.execute(
            "UPDATE connections SET target_password = ?1 WHERE id = ?2",
            params![encrypted, id.to_string()],
        )?;
        Ok(())
    }

    /// Get the decrypted target password for a connection.
    pub fn get_connection_target_password(
        &self,
        id: &Uuid,
    ) -> Result<Option<String>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT target_password FROM connections WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Connection {}", id)))?;

        match data {
            Some(encrypted) => Ok(Some(self.decrypt_field(&encrypted)?)),
            None => Ok(None),
        }
    }

    /// Get the decrypted proxy password for a connection.
    pub fn get_proxy_password(&self, id: &Uuid) -> Result<Option<String>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT proxy_password FROM connections WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Connection {}", id)))?;

        match data {
            Some(encrypted) => Ok(Some(self.decrypt_field(&encrypted)?)),
            None => Ok(None),
        }
    }

    pub fn delete_connection(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM connections WHERE id = ?1",
            params![id.to_string()],
        )?;
        self.record_tombstone("connection", id)?;
        // Cascade to port-forward rules: `host_id` is NOT NULL, so a rule is
        // useless once its host is gone and would otherwise linger as an
        // orphan that still enumerates into sync and portable export. Drop
        // each referencing rule with its own tombstone so the delete
        // propagates to peers. Session groups are intentionally left intact:
        // a group can reference several hosts and prunes dead panes at open
        // time, so deleting the whole group on one host's removal is wrong.
        let orphan_rules: Vec<Uuid> = {
            let mut stmt = self
                .db
                .prepare("SELECT id FROM port_forward_rules WHERE host_id = ?1")?;
            stmt.query_map(params![id.to_string()], |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .filter_map(|s| Uuid::parse_str(&s).ok())
                .collect()
        };
        for rid in orphan_rules {
            self.delete_port_forward_rule(&rid)?;
        }
        // Command history is local-only and meaningless without its host;
        // dropping it here is also the privacy-correct behavior (deleting a
        // host must not leave its command trail behind). No tombstone: the
        // table never syncs.
        self.clear_command_history(id)?;
        // Install-run rows (issue #147) are the same class of local
        // bookkeeping: statements about this host, meaningless (and
        // wrong, should the id ever be reused) once it is gone.
        self.db.execute(
            "DELETE FROM install_runs WHERE host_id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    /// Resolve the effective proxy for a connection, hydrating the
    /// password from the appropriate encrypted column. Order:
    ///
    /// 1. `proxy_identity_id` set → load proxy identity + its password.
    /// 2. Inline `proxy` set → clone + hydrate from `proxy_password`.
    /// 3. Otherwise `None`.
    ///
    /// A dangling identity reference (id no longer exists) is treated
    /// as no proxy, better than failing the whole connect.
    pub fn resolve_proxy(
        &self,
        conn: &Connection,
    ) -> Result<Option<oryxis_core::models::connection::ProxyConfig>, VaultError> {
        use oryxis_core::models::connection::ProxyConfig;

        if let Some(pid) = conn.proxy_identity_id {
            // Look up the identity. If it's gone, fall through to None
            //, the user removed the identity but the connection still
            // points at it. Surfacing this as an error would block
            // connecting to every host that referenced it.
            let Some(ident) = self.get_proxy_identity(&pid)? else {
                tracing::warn!(
                    "proxy_identity_id {} not found for connection {}, falling back to no proxy",
                    pid,
                    conn.id
                );
                return Ok(None);
            };
            let password = self.get_proxy_identity_password(&pid).ok().flatten();
            return Ok(Some(ProxyConfig {
                proxy_type: ident.proxy_type,
                host: ident.host,
                port: ident.port,
                username: ident.username,
                password,
            }));
        }

        if let Some(inline) = conn.proxy.as_ref() {
            let password = self.get_proxy_password(&conn.id).ok().flatten();
            return Ok(Some(ProxyConfig {
                proxy_type: inline.proxy_type.clone(),
                host: inline.host.clone(),
                port: inline.port,
                username: inline.username.clone(),
                password,
            }));
        }

        Ok(None)
    }
}

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
        };

        self.db.execute(
            "INSERT OR REPLACE INTO connections
             (id, label, hostname, port, username, auth_method, key_id, group_id,
              jump_chain, proxy, tags, notes, color, password, last_used, created_at, updated_at, identity_id, mcp_enabled, port_forwards,
              detected_os, custom_icon, custom_color, agent_forwarding, proxy_identity_id, terminal_theme, cloud_ref, initial_command, keepalive_interval, icon_style, customized_fields, env_vars, encoding, session_logging)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34)",
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
                        jump_chain, proxy, tags, notes, color, last_used, created_at, updated_at, identity_id, mcp_enabled, port_forwards, detected_os, custom_icon, custom_color, agent_forwarding, proxy_identity_id, terminal_theme, cloud_ref, initial_command, keepalive_interval, icon_style, customized_fields, env_vars, encoding, session_logging
                 FROM connections WHERE mcp_enabled = 1 ORDER BY label"
            }
            _ => {
                "SELECT id, label, hostname, port, username, auth_method, key_id, group_id,
                        jump_chain, proxy, tags, notes, color, last_used, created_at, updated_at, identity_id, mcp_enabled, port_forwards, detected_os, custom_icon, custom_color, agent_forwarding, proxy_identity_id, terminal_theme, cloud_ref, initial_command, keepalive_interval, icon_style, customized_fields, env_vars, encoding, session_logging
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
                    _ => AuthMethod::Auto,
                };

                Ok(Connection {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    hostname: row.get(2)?,
                    port: row.get(3)?,
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
        Ok(())
    }

}

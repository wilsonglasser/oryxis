use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Cloud Profiles CRUD
    // -----------------------------------------------------------------------

    /// Save a cloud profile. `secret` follows the tri-state convention:
    /// `None` preserves the existing column, `Some("")` clears it,
    /// `Some(value)` encrypts and stores. Mirrors `save_identity`.
    pub fn save_cloud_profile(
        &self,
        profile: &CloudProfile,
        secret: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted_secret: Option<Vec<u8>> = match secret {
            Some("") => None,
            Some(s) => Some(self.encrypt_field(s)?),
            None => self
                .db
                .query_row(
                    "SELECT secret FROM cloud_profiles WHERE id = ?1",
                    params![profile.id.to_string()],
                    |row| row.get::<_, Option<Vec<u8>>>(0),
                )
                .ok()
                .flatten(),
        };

        self.db.execute(
            "INSERT OR REPLACE INTO cloud_profiles
             (id, label, provider, auth_kind, config, secret, last_discovered, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                profile.id.to_string(),
                profile.label,
                profile.provider,
                profile.auth_kind,
                profile.config,
                encrypted_secret,
                profile.last_discovered.map(|d| d.to_rfc3339()),
                profile.created_at.to_rfc3339(),
                profile.updated_at.to_rfc3339(),
            ],
        )?;
        self.clear_tombstone("cloud_profile", &profile.id)?;
        Ok(())
    }

    /// List cloud profiles. The transient `secret` field on each row
    /// is left empty, callers that need the secret hydrate it via
    /// `get_cloud_profile_secret` right before a cloud API call so
    /// secrets don't sit in memory unnecessarily.
    pub fn list_cloud_profiles(&self) -> Result<Vec<CloudProfile>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, provider, auth_kind, config, last_discovered, created_at, updated_at
             FROM cloud_profiles ORDER BY label",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(CloudProfile {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    provider: row.get(2)?,
                    auth_kind: row.get(3)?,
                    config: row.get(4)?,
                    last_discovered: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc)),
                    created_at: row
                        .get::<_, String>(6)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, String>(7)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    secret: None,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Decrypt and return the profile's secret blob (access key secret,
    /// inline kubeconfig, SSO refresh token, …). Provider crates own
    /// the schema of this string.
    pub fn get_cloud_profile_secret(
        &self,
        id: &Uuid,
    ) -> Result<Option<String>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT secret FROM cloud_profiles WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Cloud profile {}", id)))?;

        match data {
            Some(encrypted) => Ok(Some(self.decrypt_field(&encrypted)?)),
            None => Ok(None),
        }
    }

    pub fn delete_cloud_profile(&self, id: &Uuid) -> Result<(), VaultError> {
        // No FK cascade on connections.cloud_ref / groups.cloud_query
        // (they're JSON blobs); call sites that care about dangling
        // references handle them at resolve time, same approach as
        // the proxy-identity dangling check.
        self.db.execute(
            "DELETE FROM cloud_profiles WHERE id = ?1",
            params![id.to_string()],
        )?;
        self.record_tombstone("cloud_profile", id)?;
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

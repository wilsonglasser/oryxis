use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Identities CRUD
    // -----------------------------------------------------------------------

    /// Save an identity. If `password` is provided, it's encrypted.
    pub fn save_identity(
        &self,
        identity: &Identity,
        password: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted_pw = match password {
            // Tri-state: empty string clears the password (NULL column),
            // never an encrypted empty blob (mirrors `save_cloud_profile`).
            Some("") => None,
            Some(pw) => Some(self.encrypt_field(pw)?),
            None => {
                // Keep existing password if not provided
                self.db
                    .query_row(
                        "SELECT password FROM identities WHERE id = ?1",
                        params![identity.id.to_string()],
                        |row| row.get::<_, Option<Vec<u8>>>(0),
                    )
                    .ok()
                    .flatten()
            }
        };

        self.db.execute(
            "INSERT OR REPLACE INTO identities
             (id, label, username, password, key_id, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                identity.id.to_string(),
                identity.label,
                identity.username,
                encrypted_pw,
                identity.key_id.map(|u| u.to_string()),
                identity.created_at.to_rfc3339(),
                identity.updated_at.to_rfc3339(),
            ],
        )?;
        self.clear_tombstone("identity", &identity.id)?;
        Ok(())
    }

    pub fn list_identities(&self) -> Result<Vec<Identity>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, username, key_id, created_at, updated_at
             FROM identities ORDER BY label",
        )?;
        let identities = stmt
            .query_map([], |row| {
                Ok(Identity {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    username: row.get(2)?,
                    key_id: row
                        .get::<_, Option<String>>(3)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    created_at: row
                        .get::<_, String>(4)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, String>(5)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(identities)
    }

    /// IDs of identities whose `password` column is non-NULL. Cheap
    /// existence check, no decrypt and no `require_unlocked()` since the
    /// caller only learns *that* a password exists, not its value.
    /// Used by the keychain view to render the masked-bullets badge
    /// without paying a per-frame decrypt on every identity card.
    pub fn list_identity_ids_with_password(
        &self,
    ) -> Result<std::collections::HashSet<Uuid>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id FROM identities WHERE password IS NOT NULL",
        )?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok().and_then(|s| Uuid::parse_str(&s).ok()))
            .collect();
        Ok(ids)
    }

    /// Get the decrypted password for an identity.
    pub fn get_identity_password(&self, id: &Uuid) -> Result<Option<String>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT password FROM identities WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Identity {}", id)))?;

        match data {
            Some(encrypted) => Ok(Some(self.decrypt_field(&encrypted)?)),
            None => Ok(None),
        }
    }

    pub fn delete_identity(&self, id: &Uuid) -> Result<(), VaultError> {
        // NULL out identity_id on connections referencing this identity
        self.db.execute(
            "UPDATE connections SET identity_id = NULL WHERE identity_id = ?1",
            params![id.to_string()],
        )?;
        self.db.execute(
            "DELETE FROM identities WHERE id = ?1",
            params![id.to_string()],
        )?;
        self.record_tombstone("identity", id)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Proxy Identities CRUD
    // -----------------------------------------------------------------------

    /// Save a proxy identity. If `password` is provided, it's encrypted
    /// in the dedicated column; otherwise the existing one is preserved
    /// (mirrors `save_identity`).
    pub fn save_proxy_identity(
        &self,
        identity: &ProxyIdentity,
        password: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted_pw = match password {
            // Tri-state: empty string clears the password (NULL column),
            // never an encrypted empty blob (mirrors `save_cloud_profile`).
            Some("") => None,
            Some(pw) => Some(self.encrypt_field(pw)?),
            None => self
                .db
                .query_row(
                    "SELECT password FROM proxy_identities WHERE id = ?1",
                    params![identity.id.to_string()],
                    |row| row.get::<_, Option<Vec<u8>>>(0),
                )
                .ok()
                .flatten(),
        };

        let proxy_type_str = serde_json::to_string(&identity.proxy_type).unwrap_or_default();

        self.db.execute(
            "INSERT OR REPLACE INTO proxy_identities
             (id, label, proxy_type, host, port, username, password, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                identity.id.to_string(),
                identity.label,
                proxy_type_str,
                identity.host,
                identity.port,
                identity.username,
                encrypted_pw,
                identity.created_at.to_rfc3339(),
                identity.updated_at.to_rfc3339(),
            ],
        )?;
        self.clear_tombstone("proxy_identity", &identity.id)?;
        Ok(())
    }

    pub fn list_proxy_identities(&self) -> Result<Vec<ProxyIdentity>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, proxy_type, host, port, username, created_at, updated_at
             FROM proxy_identities ORDER BY label",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let proxy_type_str: String = row.get(2)?;
                let proxy_type: ProxyType = serde_json::from_str(&proxy_type_str)
                    .unwrap_or(ProxyType::Socks5);
                Ok(ProxyIdentity {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    proxy_type,
                    host: row.get(3)?,
                    port: row.get(4)?,
                    username: row.get(5)?,
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Fetch a single proxy identity by id. `None` when the row is gone
    /// (used by `resolve_proxy`, where a dangling reference must
    /// degrade gracefully rather than error).
    pub fn get_proxy_identity(&self, id: &Uuid) -> Result<Option<ProxyIdentity>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, proxy_type, host, port, username, created_at, updated_at
             FROM proxy_identities WHERE id = ?1",
        )?;
        let mut rows = stmt
            .query_map(params![id.to_string()], |row| {
                let proxy_type_str: String = row.get(2)?;
                let proxy_type: ProxyType = serde_json::from_str(&proxy_type_str)
                    .unwrap_or(ProxyType::Socks5);
                Ok(ProxyIdentity {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    proxy_type,
                    host: row.get(3)?,
                    port: row.get(4)?,
                    username: row.get(5)?,
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.pop())
    }

    /// Get the decrypted password for a proxy identity.
    pub fn get_proxy_identity_password(
        &self,
        id: &Uuid,
    ) -> Result<Option<String>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT password FROM proxy_identities WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Proxy identity {}", id)))?;

        match data {
            Some(encrypted) => Ok(Some(self.decrypt_field(&encrypted)?)),
            None => Ok(None),
        }
    }

    pub fn delete_proxy_identity(&self, id: &Uuid) -> Result<(), VaultError> {
        // NULL out proxy_identity_id on connections referencing this
        // identity BEFORE deleting the row, so we never have a window
        // with a dangling reference.
        self.db.execute(
            "UPDATE connections SET proxy_identity_id = NULL WHERE proxy_identity_id = ?1",
            params![id.to_string()],
        )?;
        self.db.execute(
            "DELETE FROM proxy_identities WHERE id = ?1",
            params![id.to_string()],
        )?;
        self.record_tombstone("proxy_identity", id)?;
        Ok(())
    }

}

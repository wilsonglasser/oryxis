use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Settings (key/value in settings table)
    // -----------------------------------------------------------------------

    /// Get a plain-text setting from the settings table.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, VaultError> {
        let val: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .ok();
        Ok(val)
    }

    /// Set a plain-text setting in the settings table.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Delete every setting whose key starts with `prefix`. Backs the
    /// "Reset hints" action, which clears all `hint_*` one-time flags
    /// in one sweep so future hints don't each need their own reset.
    pub fn delete_settings_with_prefix(&self, prefix: &str) -> Result<(), VaultError> {
        // ESCAPE so a literal `_`/`%` in the prefix can't wildcard-match.
        let pattern = format!(
            "{}%",
            prefix.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
        );
        self.db.execute(
            "DELETE FROM settings WHERE key LIKE ?1 ESCAPE '\\'",
            params![pattern],
        )?;
        Ok(())
    }

    /// Store an AI API key encrypted in the settings table (base64-encoded).
    pub fn set_ai_api_key(&self, api_key: &str) -> Result<(), VaultError> {
        let encrypted = self.encrypt_field(api_key)?;
        let encoded = BASE64.encode(&encrypted);
        self.set_setting("ai_api_key", &encoded)
    }

    /// Retrieve and decrypt the AI API key from the settings table.
    pub fn get_ai_api_key(&self) -> Result<Option<String>, VaultError> {
        match self.get_setting("ai_api_key")? {
            Some(encoded) => {
                let encrypted = BASE64.decode(encoded.as_bytes())
                    .map_err(|e| VaultError::Crypto(format!("Base64 decode: {}", e)))?;
                Ok(Some(self.decrypt_field(&encrypted)?))
            }
            None => Ok(None),
        }
    }

    /// Persist the sync `DeviceIdentity` blob encrypted at rest. The
    /// caller (oryxis-sync) is responsible for the byte layout (see
    /// `crypto::DeviceIdentity::to_bytes`). We treat the bytes as
    /// opaque secret material and encrypt with the master key, then
    /// base64-encode for storage in the `settings` text column.
    pub fn set_sync_device_identity(&self, bytes: &[u8]) -> Result<(), VaultError> {
        let key = self.require_unlocked()?;
        let encrypted = encrypt_with_key(bytes, key)?;
        let encoded = BASE64.encode(&encrypted);
        self.set_setting("sync_device_identity", &encoded)
    }

    /// Retrieve and decrypt the sync `DeviceIdentity` blob, or `None`
    /// if no identity has been persisted yet. Returns an error if the
    /// stored value is corrupt or the master key has rotated without
    /// re-encryption (see `convert_all_fields`).
    pub fn get_sync_device_identity(&self) -> Result<Option<Vec<u8>>, VaultError> {
        let key = self.require_unlocked()?;
        match self.get_setting("sync_device_identity")? {
            Some(encoded) => {
                let encrypted = BASE64
                    .decode(encoded.as_bytes())
                    .map_err(|e| VaultError::Crypto(format!("Base64 decode: {}", e)))?;
                Ok(Some(decrypt_with_key(&encrypted, key)?))
            }
            None => Ok(None),
        }
    }

}

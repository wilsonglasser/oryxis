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

    /// Return every `(key, value)` pair in the settings table, sorted by
    /// key for a stable export order. Values are returned verbatim, so
    /// encrypted settings (`ai_api_key`, `sync_device_identity`) come
    /// back as their base64-encoded ciphertext, the caller is
    /// responsible for substituting decrypted material when needed.
    pub fn list_settings(&self) -> Result<Vec<(String, String)>, VaultError> {
        let mut stmt = self
            .db
            .prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
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

    /// Store the Files-sidebar folder history encrypted in the settings
    /// table (base64-encoded JSON). Same field-encryption path as the AI
    /// API key, so it rides key rotation via `convert_settings_b64`.
    ///
    /// Not a credential, but not plaintext material either: the settings
    /// table is read WITHOUT unlocking (that is what hydrates theme and
    /// language on the lock screen), so a plain row would hand the
    /// directory layout of every host to anyone holding the file. The
    /// rest of the user's browsing trail (command history, session logs,
    /// chat) is encrypted, and this belongs with it.
    pub fn set_files_recent_folders(&self, json: &str) -> Result<(), VaultError> {
        let encrypted = self.encrypt_field(json)?;
        let encoded = BASE64.encode(&encrypted);
        self.set_setting("files_recent_folders", &encoded)
    }

    /// Retrieve and decrypt the Files-sidebar folder history.
    ///
    /// Anything that fails to decode is DELETED and reported as absent:
    /// the only way to get a non-ciphertext row here is a pre-encryption
    /// build (the plain JSON this replaces), and leaving that row in
    /// place would keep the very plaintext this method exists to remove.
    /// The list is a convenience, so dropping it costs nothing.
    pub fn get_files_recent_folders(&self) -> Result<Option<String>, VaultError> {
        let Some(encoded) = self.get_setting("files_recent_folders")? else {
            return Ok(None);
        };
        let decoded = BASE64
            .decode(encoded.as_bytes())
            .ok()
            .and_then(|bytes| self.decrypt_field(&bytes).ok());
        match decoded {
            Some(json) => Ok(Some(json)),
            None => {
                self.db.execute(
                    "DELETE FROM settings WHERE key = ?1",
                    params!["files_recent_folders"],
                )?;
                Ok(None)
            }
        }
    }







}

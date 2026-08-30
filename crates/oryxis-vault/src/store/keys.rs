use super::*;

impl VaultStore {
    // -----------------------------------------------------------------------
    // Keys CRUD
    // -----------------------------------------------------------------------

    /// Save an SSH key. Private key is encrypted.
    pub fn save_key(
        &self,
        key: &SshKey,
        private_key_pem: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted_pk = match private_key_pem {
            // Tri-state: empty string clears the private key (NULL column),
            // never an encrypted empty blob (mirrors `save_cloud_profile`).
            Some("") => None,
            Some(pem) => Some(self.encrypt_field(pem)?),
            None => {
                // Keep existing
                self.db
                    .query_row(
                        "SELECT private_key FROM keys WHERE id = ?1",
                        params![key.id.to_string()],
                        |row| row.get::<_, Option<Vec<u8>>>(0),
                    )
                    .ok()
                    .flatten()
            }
        };

        let algo_str = match key.algorithm {
            KeyAlgorithm::Ed25519 => "ed25519",
            KeyAlgorithm::Rsa2048 => "rsa2048",
            KeyAlgorithm::Rsa3072 => "rsa3072",
            KeyAlgorithm::Rsa4096 => "rsa4096",
            KeyAlgorithm::EcdsaP256 => "ecdsa-p256",
            KeyAlgorithm::EcdsaP384 => "ecdsa-p384",
            KeyAlgorithm::EcdsaP521 => "ecdsa-p521",
            KeyAlgorithm::SkEd25519 => "sk-ed25519",
            KeyAlgorithm::SkEcdsaP256 => "sk-ecdsa-p256",
        };

        self.db.execute(
            "INSERT OR REPLACE INTO keys
             (id, label, fingerprint, algorithm, public_key, private_key, has_passphrase, expose_via_agent, certificate, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                key.id.to_string(),
                key.label,
                key.fingerprint,
                algo_str,
                key.public_key,
                encrypted_pk,
                key.has_passphrase as i32,
                key.expose_via_agent as i32,
                key.certificate,
                key.created_at.to_rfc3339(),
                key.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_keys(&self) -> Result<Vec<SshKey>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, fingerprint, algorithm, public_key, has_passphrase, expose_via_agent, created_at, updated_at, certificate, (private_key IS NOT NULL)
             FROM keys ORDER BY label",
        )?;
        let keys = stmt
            .query_map([], |row| {
                let algo_str: String = row.get(3)?;
                let algorithm = match algo_str.as_str() {
                    "rsa2048" => KeyAlgorithm::Rsa2048,
                    "rsa3072" => KeyAlgorithm::Rsa3072,
                    "rsa4096" => KeyAlgorithm::Rsa4096,
                    "ecdsa-p256" => KeyAlgorithm::EcdsaP256,
                    "ecdsa-p384" => KeyAlgorithm::EcdsaP384,
                    "ecdsa-p521" => KeyAlgorithm::EcdsaP521,
                    "sk-ed25519" => KeyAlgorithm::SkEd25519,
                    "sk-ecdsa-p256" => KeyAlgorithm::SkEcdsaP256,
                    _ => KeyAlgorithm::Ed25519,
                };

                Ok(SshKey {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    fingerprint: row.get(2)?,
                    algorithm,
                    public_key: row.get(4)?,
                    file_ref: String::new(),
                    has_passphrase: row.get::<_, i32>(5)? != 0,
                    expose_via_agent: row.get::<_, i32>(6).unwrap_or(1) != 0,
                    created_at: row
                        .get::<_, String>(7)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, Option<String>>(8)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    // Column added by ALTER TABLE; older rows have no value.
                    certificate: row.get::<_, Option<String>>(9).unwrap_or(None),
                    // Computed: whether the private_key column is non-NULL
                    // (B3 security-key / public-only rows are NULL here).
                    has_private: row.get::<_, i64>(10).unwrap_or(1) != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(keys)
    }

    /// Get the decrypted private key PEM.
    pub fn get_key_private(&self, id: &Uuid) -> Result<Option<String>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT private_key FROM keys WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Key {}", id)))?;

        match data {
            Some(encrypted) => Ok(Some(self.decrypt_field(&encrypted)?)),
            None => Ok(None),
        }
    }

    pub fn delete_key(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db
            .execute("DELETE FROM keys WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

}

use std::path::{Path, PathBuf};

use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use rusqlite::{params, Connection as SqliteConn};
use uuid::Uuid;

use oryxis_core::models::connection::{AuthMethod, Connection};
use oryxis_core::models::group::Group;
use oryxis_core::models::key::{KeyAlgorithm, SshKey};
use oryxis_core::models::snippet::Snippet;

use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Vault is locked")]
    Locked,

    #[error("Invalid master password")]
    InvalidPassword,

    #[error("Database error: {0}")]
    Database(String),

    #[error("Crypto error: {0}")]
    Crypto(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Not found: {0}")]
    NotFound(String),
}

impl From<rusqlite::Error> for VaultError {
    fn from(e: rusqlite::Error) -> Self {
        VaultError::Database(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Crypto helpers
// ---------------------------------------------------------------------------

const SALT_LEN: usize = 32;
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Derive a 256-bit key from a password using Argon2id.
fn derive_key(password: &[u8], salt: &[u8]) -> Result<[u8; KEY_LEN], VaultError> {
    let mut key = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| VaultError::Crypto(format!("Argon2 error: {}", e)))?;
    Ok(key)
}

/// Encrypt data with ChaCha20Poly1305. Returns: salt(32) + nonce(12) + ciphertext.
fn encrypt(plaintext: &[u8], password: &[u8]) -> Result<Vec<u8>, VaultError> {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let key = derive_key(password, &salt)?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| VaultError::Crypto(format!("Cipher init: {}", e)))?;
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| VaultError::Crypto(format!("Encrypt: {}", e)))?;

    let mut result = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt data encrypted by `encrypt`. Input: salt(32) + nonce(12) + ciphertext.
fn decrypt(data: &[u8], password: &[u8]) -> Result<Vec<u8>, VaultError> {
    if data.len() < SALT_LEN + NONCE_LEN + 16 {
        return Err(VaultError::Crypto("Data too short".into()));
    }
    let salt = &data[..SALT_LEN];
    let nonce_bytes = &data[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ciphertext = &data[SALT_LEN + NONCE_LEN..];

    let key = derive_key(password, salt)?;
    let nonce = Nonce::from_slice(nonce_bytes);

    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|e| VaultError::Crypto(format!("Cipher init: {}", e)))?;
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| VaultError::InvalidPassword)
}

// ---------------------------------------------------------------------------
// VaultStore
// ---------------------------------------------------------------------------

/// Vault store — manages SQLite database with encrypted secrets.
pub struct VaultStore {
    db: SqliteConn,
    /// Derived key material (password bytes kept for field-level encryption).
    master_key: Option<Vec<u8>>,
    _db_path: PathBuf,
}

impl VaultStore {
    /// Open or create the vault database at the default location (~/.oryxis/vault.db).
    pub fn open_default() -> Result<Self, VaultError> {
        let dir = dirs::home_dir()
            .ok_or_else(|| VaultError::Io(std::io::Error::other("No home directory")))?
            .join(".oryxis");
        std::fs::create_dir_all(&dir)?;
        Self::open(dir.join("vault.db"))
    }

    /// Open or create the vault database at a specific path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, VaultError> {
        let path = path.as_ref().to_path_buf();
        let db = SqliteConn::open(&path)?;
        db.execute_batch("PRAGMA journal_mode=WAL;")?;

        let mut store = Self {
            db,
            master_key: None,
            _db_path: path,
        };
        store.create_tables()?;
        Ok(store)
    }

    fn create_tables(&mut self) -> Result<(), VaultError> {
        self.db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS vault_meta (
                key   TEXT PRIMARY KEY,
                value BLOB NOT NULL
            );

            CREATE TABLE IF NOT EXISTS groups (
                id         TEXT PRIMARY KEY,
                label      TEXT NOT NULL,
                parent_id  TEXT,
                color      TEXT,
                icon       TEXT,
                sort_order INTEGER DEFAULT 0,
                is_shared  INTEGER DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS connections (
                id          TEXT PRIMARY KEY,
                label       TEXT NOT NULL,
                hostname    TEXT NOT NULL,
                port        INTEGER DEFAULT 22,
                username    TEXT,
                auth_method TEXT NOT NULL DEFAULT 'password',
                key_id      TEXT,
                group_id    TEXT REFERENCES groups(id),
                jump_chain  TEXT,
                proxy       TEXT,
                tags        TEXT,
                notes       TEXT,
                color       TEXT,
                password    BLOB,
                last_used   TEXT,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS keys (
                id            TEXT PRIMARY KEY,
                label         TEXT NOT NULL,
                fingerprint   TEXT,
                algorithm     TEXT NOT NULL,
                public_key    TEXT,
                private_key   BLOB,
                has_passphrase INTEGER DEFAULT 0,
                created_at    TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS snippets (
                id          TEXT PRIMARY KEY,
                label       TEXT NOT NULL,
                command     TEXT NOT NULL,
                description TEXT,
                tags        TEXT,
                created_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS known_hosts (
                id          TEXT PRIMARY KEY,
                hostname    TEXT NOT NULL,
                port        INTEGER DEFAULT 22,
                key_type    TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                first_seen  TEXT NOT NULL,
                last_seen   TEXT NOT NULL,
                UNIQUE(hostname, port)
            );

            CREATE TABLE IF NOT EXISTS logs (
                id               TEXT PRIMARY KEY,
                connection_label TEXT NOT NULL,
                hostname         TEXT NOT NULL,
                event            TEXT NOT NULL,
                message          TEXT NOT NULL,
                timestamp        TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Vault state
    // -----------------------------------------------------------------------

    pub fn is_locked(&self) -> bool {
        self.master_key.is_none()
    }

    /// Check if a master password has been set (vault_meta has "password_check").
    pub fn has_master_password(&self) -> Result<bool, VaultError> {
        let exists: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM vault_meta WHERE key = 'password_check')",
            [],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    /// Set the master password for the first time.
    pub fn set_master_password(&mut self, password: &str) -> Result<(), VaultError> {
        if self.has_master_password()? {
            return Err(VaultError::Crypto(
                "Master password already set. Use unlock() instead.".into(),
            ));
        }
        let pw_bytes = password.as_bytes();
        // Store an encrypted known value so we can verify the password on unlock.
        let check = encrypt(b"oryxis_vault_ok", pw_bytes)?;
        self.db.execute(
            "INSERT INTO vault_meta (key, value) VALUES ('password_check', ?1)",
            params![check],
        )?;
        self.master_key = Some(pw_bytes.to_vec());
        tracing::info!("Vault master password set");
        Ok(())
    }

    /// Unlock the vault by verifying the master password.
    pub fn unlock(&mut self, password: &str) -> Result<(), VaultError> {
        let check: Vec<u8> = self
            .db
            .query_row(
                "SELECT value FROM vault_meta WHERE key = 'password_check'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::Locked)?;

        let pw_bytes = password.as_bytes();
        let plain = decrypt(&check, pw_bytes)?;
        if plain != b"oryxis_vault_ok" {
            return Err(VaultError::InvalidPassword);
        }

        self.master_key = Some(pw_bytes.to_vec());
        tracing::info!("Vault unlocked");
        Ok(())
    }

    pub fn lock(&mut self) {
        self.master_key = None;
        tracing::info!("Vault locked");
    }

    fn require_unlocked(&self) -> Result<&[u8], VaultError> {
        self.master_key.as_deref().ok_or(VaultError::Locked)
    }

    // -----------------------------------------------------------------------
    // Encrypt / decrypt field helpers
    // -----------------------------------------------------------------------

    fn encrypt_field(&self, plaintext: &str) -> Result<Vec<u8>, VaultError> {
        let key = self.require_unlocked()?;
        encrypt(plaintext.as_bytes(), key)
    }

    fn decrypt_field(&self, data: &[u8]) -> Result<String, VaultError> {
        let key = self.require_unlocked()?;
        let plain = decrypt(data, key)?;
        String::from_utf8(plain).map_err(|e| VaultError::Crypto(e.to_string()))
    }

    // -----------------------------------------------------------------------
    // Groups CRUD
    // -----------------------------------------------------------------------

    pub fn save_group(&self, group: &Group) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO groups (id, label, parent_id, color, icon, sort_order, is_shared)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                group.id.to_string(),
                group.label,
                group.parent_id.map(|u| u.to_string()),
                group.color,
                group.icon,
                group.sort_order,
                group.is_shared as i32,
            ],
        )?;
        Ok(())
    }

    pub fn list_groups(&self) -> Result<Vec<Group>, VaultError> {
        let mut stmt = self
            .db
            .prepare("SELECT id, label, parent_id, color, icon, sort_order, is_shared FROM groups ORDER BY sort_order")?;
        let groups = stmt
            .query_map([], |row| {
                Ok(Group {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    parent_id: row
                        .get::<_, Option<String>>(2)?
                        .and_then(|s| Uuid::parse_str(&s).ok()),
                    color: row.get(3)?,
                    icon: row.get(4)?,
                    sort_order: row.get(5)?,
                    is_shared: row.get::<_, i32>(6)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(groups)
    }

    pub fn delete_group(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db
            .execute("DELETE FROM groups WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

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
            AuthMethod::Password => "password",
            AuthMethod::Key => "key",
            AuthMethod::Agent => "agent",
            AuthMethod::Interactive => "interactive",
        };

        self.db.execute(
            "INSERT OR REPLACE INTO connections
             (id, label, hostname, port, username, auth_method, key_id, group_id,
              jump_chain, proxy, tags, notes, color, password, last_used, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
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
            ],
        )?;
        Ok(())
    }

    pub fn list_connections(&self) -> Result<Vec<Connection>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, hostname, port, username, auth_method, key_id, group_id,
                    jump_chain, proxy, tags, notes, color, last_used, created_at, updated_at
             FROM connections ORDER BY label",
        )?;
        let conns = stmt
            .query_map([], |row| {
                let auth_str: String = row.get(5)?;
                let auth_method = match auth_str.as_str() {
                    "key" => AuthMethod::Key,
                    "agent" => AuthMethod::Agent,
                    "interactive" => AuthMethod::Interactive,
                    _ => AuthMethod::Password,
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(conns)
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

    pub fn delete_connection(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM connections WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

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
            KeyAlgorithm::Rsa4096 => "rsa4096",
            KeyAlgorithm::EcdsaP256 => "ecdsa-p256",
            KeyAlgorithm::EcdsaP384 => "ecdsa-p384",
        };

        self.db.execute(
            "INSERT OR REPLACE INTO keys
             (id, label, fingerprint, algorithm, public_key, private_key, has_passphrase, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                key.id.to_string(),
                key.label,
                key.fingerprint,
                algo_str,
                key.public_key,
                encrypted_pk,
                key.has_passphrase as i32,
                key.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_keys(&self) -> Result<Vec<SshKey>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, fingerprint, algorithm, public_key, has_passphrase, created_at
             FROM keys ORDER BY label",
        )?;
        let keys = stmt
            .query_map([], |row| {
                let algo_str: String = row.get(3)?;
                let algorithm = match algo_str.as_str() {
                    "rsa4096" => KeyAlgorithm::Rsa4096,
                    "ecdsa-p256" => KeyAlgorithm::EcdsaP256,
                    "ecdsa-p384" => KeyAlgorithm::EcdsaP384,
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
                    created_at: row
                        .get::<_, String>(6)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
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

    // -----------------------------------------------------------------------
    // Snippets CRUD
    // -----------------------------------------------------------------------

    pub fn save_snippet(&self, snippet: &Snippet) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO snippets (id, label, command, description, tags, created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                snippet.id.to_string(),
                snippet.label,
                snippet.command,
                snippet.description,
                serde_json::to_string(&snippet.tags).unwrap_or_default(),
                snippet.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_snippets(&self) -> Result<Vec<Snippet>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, command, description, tags, created_at FROM snippets ORDER BY label",
        )?;
        let snippets = stmt
            .query_map([], |row| {
                Ok(Snippet {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    label: row.get(1)?,
                    command: row.get(2)?,
                    description: row.get(3)?,
                    tags: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or_default(),
                    created_at: row
                        .get::<_, String>(5)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(snippets)
    }

    pub fn delete_snippet(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM snippets WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Known Hosts CRUD
    // -----------------------------------------------------------------------

    pub fn save_known_host(&self, kh: &oryxis_core::models::known_host::KnownHost) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO known_hosts (id, hostname, port, key_type, fingerprint, first_seen, last_seen)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                kh.id.to_string(), kh.hostname, kh.port, kh.key_type,
                kh.fingerprint, kh.first_seen.to_rfc3339(), kh.last_seen.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_known_hosts(&self) -> Result<Vec<oryxis_core::models::known_host::KnownHost>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, hostname, port, key_type, fingerprint, first_seen, last_seen
             FROM known_hosts ORDER BY hostname",
        )?;
        let hosts = stmt.query_map([], |row| {
            Ok(oryxis_core::models::known_host::KnownHost {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                hostname: row.get(1)?,
                port: row.get(2)?,
                key_type: row.get(3)?,
                fingerprint: row.get(4)?,
                first_seen: row.get::<_, String>(5).ok()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now),
                last_seen: row.get::<_, String>(6).ok()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now),
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(hosts)
    }

    pub fn delete_known_host(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute("DELETE FROM known_hosts WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Logs CRUD
    // -----------------------------------------------------------------------

    pub fn add_log(&self, entry: &oryxis_core::models::log_entry::LogEntry) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT INTO logs (id, connection_label, hostname, event, message, timestamp)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                entry.id.to_string(), entry.connection_label, entry.hostname,
                entry.event.to_string(), entry.message, entry.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_logs(&self, limit: usize) -> Result<Vec<oryxis_core::models::log_entry::LogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_label, hostname, event, message, timestamp
             FROM logs ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let logs = stmt.query_map(params![limit as i64], |row| {
            let event_str: String = row.get(3)?;
            let event = match event_str.as_str() {
                "Connected" => oryxis_core::models::log_entry::LogEvent::Connected,
                "Disconnected" => oryxis_core::models::log_entry::LogEvent::Disconnected,
                "Auth Failed" => oryxis_core::models::log_entry::LogEvent::AuthFailed,
                _ => oryxis_core::models::log_entry::LogEvent::Error,
            };
            Ok(oryxis_core::models::log_entry::LogEntry {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                connection_label: row.get(1)?,
                hostname: row.get(2)?,
                event,
                message: row.get(4)?,
                timestamp: row.get::<_, String>(5).ok()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now),
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    pub fn clear_logs(&self) -> Result<(), VaultError> {
        self.db.execute("DELETE FROM logs", [])?;
        Ok(())
    }
}

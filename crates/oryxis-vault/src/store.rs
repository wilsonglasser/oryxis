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
use oryxis_core::models::identity::Identity;
use oryxis_core::models::key::{KeyAlgorithm, SshKey};
use oryxis_core::models::snippet::Snippet;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{DateTime, Utc};
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
// Session log entry (for terminal recording)
// ---------------------------------------------------------------------------

/// Metadata for a recorded terminal session.
#[derive(Debug, Clone)]
pub struct SessionLogEntry {
    pub id: Uuid,
    pub connection_id: Uuid,
    pub label: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub data_size: usize,
}

/// Row from the sync_peers table.
#[derive(Debug, Clone)]
pub struct SyncPeerRow {
    pub peer_id: Uuid,
    pub device_name: String,
    pub public_key: Vec<u8>,
    pub last_known_ip: Option<String>,
    pub last_known_port: Option<u16>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub paired_at: DateTime<Utc>,
    pub is_active: bool,
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
pub(crate) fn encrypt(plaintext: &[u8], password: &[u8]) -> Result<Vec<u8>, VaultError> {
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
pub(crate) fn decrypt(data: &[u8], password: &[u8]) -> Result<Vec<u8>, VaultError> {
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

            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
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

            CREATE TABLE IF NOT EXISTS identities (
                id         TEXT PRIMARY KEY,
                label      TEXT NOT NULL,
                username   TEXT,
                password   BLOB,
                key_id     TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
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

            CREATE TABLE IF NOT EXISTS session_logs (
                id            TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                label         TEXT NOT NULL,
                started_at    TEXT NOT NULL,
                ended_at      TEXT,
                data          BLOB
            );
            ",
        )?;

        // Migrations: add columns to existing tables (ignore errors if already present)
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN identity_id TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN mcp_enabled INTEGER DEFAULT 1;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN port_forwards TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN detected_os TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN custom_icon TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE connections ADD COLUMN custom_color TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE keys ADD COLUMN updated_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE groups ADD COLUMN created_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE groups ADD COLUMN updated_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE snippets ADD COLUMN updated_at TEXT;");
        let _ = self.db.execute_batch("ALTER TABLE known_hosts ADD COLUMN updated_at TEXT;");

        // Populate new timestamp columns with sensible defaults
        let _ = self.db.execute_batch("UPDATE keys SET updated_at = created_at WHERE updated_at IS NULL;");
        let _ = self.db.execute_batch("UPDATE groups SET created_at = datetime('now'), updated_at = datetime('now') WHERE created_at IS NULL;");
        let _ = self.db.execute_batch("UPDATE snippets SET updated_at = created_at WHERE updated_at IS NULL;");
        let _ = self.db.execute_batch("UPDATE known_hosts SET updated_at = last_seen WHERE updated_at IS NULL;");

        // Sync tables
        let _ = self.db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sync_peers (
                peer_id         TEXT PRIMARY KEY,
                device_name     TEXT NOT NULL,
                public_key      BLOB NOT NULL,
                shared_secret   BLOB,
                last_known_ip   TEXT,
                last_known_port INTEGER,
                last_synced_at  TEXT,
                paired_at       TEXT NOT NULL,
                is_active       INTEGER DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS sync_metadata (
                entity_type TEXT NOT NULL,
                entity_id   TEXT NOT NULL,
                updated_at  TEXT NOT NULL,
                is_deleted  INTEGER DEFAULT 0,
                deleted_at  TEXT,
                PRIMARY KEY (entity_type, entity_id)
            );
            ",
        );

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
            "INSERT OR REPLACE INTO groups (id, label, parent_id, color, icon, sort_order, is_shared, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                group.id.to_string(),
                group.label,
                group.parent_id.map(|u| u.to_string()),
                group.color,
                group.icon,
                group.sort_order,
                group.is_shared as i32,
                group.created_at.to_rfc3339(),
                group.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_groups(&self) -> Result<Vec<Group>, VaultError> {
        let mut stmt = self
            .db
            .prepare("SELECT id, label, parent_id, color, icon, sort_order, is_shared, created_at, updated_at FROM groups ORDER BY sort_order")?;
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
                    created_at: row
                        .get::<_, Option<String>>(7)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    updated_at: row
                        .get::<_, Option<String>>(8)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
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
              detected_os, custom_icon, custom_color)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
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
                // OS detection + custom icon overrides — saved on every
                // write so they survive edits. Previously these were left
                // out and got wiped to NULL on each save.
                conn.detected_os,
                conn.custom_icon,
                conn.custom_color,
            ],
        )?;
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
                        jump_chain, proxy, tags, notes, color, last_used, created_at, updated_at, identity_id, mcp_enabled, port_forwards, detected_os, custom_icon, custom_color
                 FROM connections WHERE mcp_enabled = 1 ORDER BY label"
            }
            _ => {
                "SELECT id, label, hostname, port, username, auth_method, key_id, group_id,
                        jump_chain, proxy, tags, notes, color, last_used, created_at, updated_at, identity_id, mcp_enabled, port_forwards, detected_os, custom_icon, custom_color
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
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(conns)
    }

    /// Update just the detected OS for a connection — used by the background
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
             (id, label, fingerprint, algorithm, public_key, private_key, has_passphrase, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                key.id.to_string(),
                key.label,
                key.fingerprint,
                algo_str,
                key.public_key,
                encrypted_pk,
                key.has_passphrase as i32,
                key.created_at.to_rfc3339(),
                key.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_keys(&self) -> Result<Vec<SshKey>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, fingerprint, algorithm, public_key, has_passphrase, created_at, updated_at
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
                    updated_at: row
                        .get::<_, Option<String>>(7)?
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
    // Identities CRUD
    // -----------------------------------------------------------------------

    /// Save an identity. If `password` is provided, it's encrypted.
    pub fn save_identity(
        &self,
        identity: &Identity,
        password: Option<&str>,
    ) -> Result<(), VaultError> {
        let encrypted_pw = match password {
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
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Snippets CRUD
    // -----------------------------------------------------------------------

    pub fn save_snippet(&self, snippet: &Snippet) -> Result<(), VaultError> {
        self.db.execute(
            "INSERT OR REPLACE INTO snippets (id, label, command, description, tags, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                snippet.id.to_string(),
                snippet.label,
                snippet.command,
                snippet.description,
                serde_json::to_string(&snippet.tags).unwrap_or_default(),
                snippet.created_at.to_rfc3339(),
                snippet.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_snippets(&self) -> Result<Vec<Snippet>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, label, command, description, tags, created_at, updated_at FROM snippets ORDER BY label",
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
                    updated_at: row
                        .get::<_, Option<String>>(6)?
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
            "INSERT OR REPLACE INTO known_hosts (id, hostname, port, key_type, fingerprint, first_seen, last_seen, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                kh.id.to_string(), kh.hostname, kh.port, kh.key_type,
                kh.fingerprint, kh.first_seen.to_rfc3339(), kh.last_seen.to_rfc3339(),
                kh.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_known_hosts(&self) -> Result<Vec<oryxis_core::models::known_host::KnownHost>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, hostname, port, key_type, fingerprint, first_seen, last_seen, updated_at
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
                updated_at: row.get::<_, Option<String>>(7).ok().flatten()
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
        self.list_logs_page(0, limit)
    }

    /// Paginated variant: skip `offset` rows and return up to `limit` rows
    /// (still ordered by timestamp desc).
    pub fn list_logs_page(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<oryxis_core::models::log_entry::LogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_label, hostname, event, message, timestamp
             FROM logs ORDER BY timestamp DESC LIMIT ?1 OFFSET ?2",
        )?;
        let logs = stmt.query_map(params![limit as i64, offset as i64], |row| {
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

    /// Total number of log rows — used to drive pagination controls.
    pub fn count_logs(&self) -> Result<usize, VaultError> {
        let n: i64 = self
            .db
            .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    // -----------------------------------------------------------------------
    // Session Logs CRUD (terminal recording)
    // -----------------------------------------------------------------------

    /// Create a new session log entry with started_at = now.
    pub fn create_session_log(
        &self,
        id: &Uuid,
        connection_id: &Uuid,
        label: &str,
    ) -> Result<(), VaultError> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "INSERT INTO session_logs (id, connection_id, label, started_at, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id.to_string(),
                connection_id.to_string(),
                label,
                now,
                Vec::<u8>::new(),
            ],
        )?;
        Ok(())
    }

    /// Append bytes to an existing session log's data BLOB.
    pub fn append_session_data(&self, id: &Uuid, data: &[u8]) -> Result<(), VaultError> {
        let id_str = id.to_string();
        let existing: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT data FROM session_logs WHERE id = ?1",
                params![id_str],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        let mut buf = existing.unwrap_or_default();
        buf.extend_from_slice(data);
        self.db.execute(
            "UPDATE session_logs SET data = ?1 WHERE id = ?2",
            params![buf, id_str],
        )?;
        Ok(())
    }

    /// Set ended_at = now on a session log.
    pub fn end_session_log(&self, id: &Uuid) -> Result<(), VaultError> {
        let now = Utc::now().to_rfc3339();
        self.db.execute(
            "UPDATE session_logs SET ended_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }

    /// List all session logs (metadata only, no data blob).
    pub fn list_session_logs(&self) -> Result<Vec<SessionLogEntry>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT id, connection_id, label, started_at, ended_at, LENGTH(COALESCE(data, X''))
             FROM session_logs ORDER BY started_at DESC",
        )?;
        let logs = stmt
            .query_map([], |row| {
                Ok(SessionLogEntry {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    connection_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap_or_default(),
                    label: row.get(2)?,
                    started_at: row
                        .get::<_, String>(3)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now),
                    ended_at: row
                        .get::<_, Option<String>>(4)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&Utc)),
                    data_size: row.get::<_, i64>(5).unwrap_or(0) as usize,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(logs)
    }

    /// Get the raw data bytes for a session log.
    pub fn get_session_data(&self, id: &Uuid) -> Result<Option<Vec<u8>>, VaultError> {
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT data FROM session_logs WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .map_err(|_| VaultError::NotFound(format!("Session log {}", id)))?;
        Ok(data)
    }

    /// Delete a session log.
    pub fn delete_session_log(&self, id: &Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM session_logs WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Password-less vault support
    // -----------------------------------------------------------------------

    /// Check if the vault has a user-set master password (vs the default empty password).
    pub fn has_user_password(&self) -> Result<bool, VaultError> {
        let val: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key = 'has_user_password'",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(val.as_deref() == Some("1"))
    }

    /// Open the vault without a user password (uses empty string as key).
    /// If the vault has never been set up, sets the master password to "".
    /// If the vault already has a password_check with "", unlocks it.
    pub fn open_without_password(&mut self) -> Result<(), VaultError> {
        if self.has_master_password()? {
            // Vault already set up — try unlocking with empty password
            self.unlock("")
        } else {
            // First time: set up with empty password
            self.set_master_password("")?;
            Ok(())
        }
    }

    /// Check if the vault has ANY master password set (regardless of user flag).
    pub fn is_initialized(&self) -> bool {
        self.has_master_password().unwrap_or(false)
    }

    /// Destroy the vault database and recreate it fresh.
    pub fn destroy_and_recreate(&mut self) -> Result<(), VaultError> {
        // Drop all tables and recreate
        self.db.execute_batch(
            "DROP TABLE IF EXISTS vault_meta;
             DROP TABLE IF EXISTS connections;
             DROP TABLE IF EXISTS keys;
             DROP TABLE IF EXISTS groups;
             DROP TABLE IF EXISTS known_hosts;
             DROP TABLE IF EXISTS logs;
             DROP TABLE IF EXISTS snippets;
             DROP TABLE IF EXISTS session_logs;
             DROP TABLE IF EXISTS identities;
             DROP TABLE IF EXISTS settings;"
        )?;
        self.master_key = None;
        self.create_tables()?;
        tracing::info!("Vault destroyed and recreated");
        Ok(())
    }

    /// Set a user password on the vault. Re-encrypts all encrypted fields
    /// from the current master key to the new password.
    pub fn set_user_password(&mut self, new_password: &str) -> Result<(), VaultError> {
        let old_key = self.require_unlocked()?.to_vec();
        let new_key = new_password.as_bytes().to_vec();

        // Re-encrypt all connection passwords
        self.re_encrypt_connections(&old_key, &new_key)?;
        // Re-encrypt all key private keys
        self.re_encrypt_keys(&old_key, &new_key)?;
        // Re-encrypt all identity passwords
        self.re_encrypt_identities(&old_key, &new_key)?;
        // Re-encrypt AI API key if present
        self.re_encrypt_ai_api_key(&old_key, &new_key)?;

        // Update the password_check with the new password
        let check = encrypt(b"oryxis_vault_ok", &new_key)?;
        self.db.execute(
            "INSERT OR REPLACE INTO vault_meta (key, value) VALUES ('password_check', ?1)",
            params![check],
        )?;

        // Mark that user has set a password
        self.db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('has_user_password', '1')",
            [],
        )?;

        self.master_key = Some(new_key);
        tracing::info!("Vault user password set");
        Ok(())
    }

    /// Remove the user password, reverting to the default empty password.
    /// Re-encrypts all encrypted fields from the current key to empty string.
    pub fn remove_user_password(&mut self) -> Result<(), VaultError> {
        let old_key = self.require_unlocked()?.to_vec();
        let new_key = b"".to_vec();

        // Re-encrypt all encrypted fields
        self.re_encrypt_connections(&old_key, &new_key)?;
        self.re_encrypt_keys(&old_key, &new_key)?;
        self.re_encrypt_identities(&old_key, &new_key)?;
        self.re_encrypt_ai_api_key(&old_key, &new_key)?;

        // Update the password_check with empty password
        let check = encrypt(b"oryxis_vault_ok", &new_key)?;
        self.db.execute(
            "INSERT OR REPLACE INTO vault_meta (key, value) VALUES ('password_check', ?1)",
            params![check],
        )?;

        // Mark no user password
        self.db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('has_user_password', '0')",
            [],
        )?;

        self.master_key = Some(new_key);
        tracing::info!("Vault user password removed");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Sync Peers CRUD
    // -----------------------------------------------------------------------

    pub fn save_sync_peer(
        &self,
        peer_id: &uuid::Uuid,
        device_name: &str,
        public_key: &[u8],
        shared_secret: Option<&[u8]>,
        paired_at: &chrono::DateTime<chrono::Utc>,
    ) -> Result<(), VaultError> {
        let encrypted_secret = match shared_secret {
            Some(s) => Some(self.encrypt_field(&base64::engine::general_purpose::STANDARD.encode(s))?),
            None => None,
        };
        self.db.execute(
            "INSERT OR REPLACE INTO sync_peers
             (peer_id, device_name, public_key, shared_secret, paired_at, is_active)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)",
            params![
                peer_id.to_string(),
                device_name,
                public_key,
                encrypted_secret,
                paired_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn update_sync_peer_endpoint(
        &self,
        peer_id: &uuid::Uuid,
        ip: &str,
        port: u16,
    ) -> Result<(), VaultError> {
        self.db.execute(
            "UPDATE sync_peers SET last_known_ip = ?1, last_known_port = ?2 WHERE peer_id = ?3",
            params![ip, port as i32, peer_id.to_string()],
        )?;
        Ok(())
    }

    pub fn update_sync_peer_last_synced(
        &self,
        peer_id: &uuid::Uuid,
    ) -> Result<(), VaultError> {
        self.db.execute(
            "UPDATE sync_peers SET last_synced_at = ?1 WHERE peer_id = ?2",
            params![chrono::Utc::now().to_rfc3339(), peer_id.to_string()],
        )?;
        Ok(())
    }

    pub fn list_sync_peers(&self) -> Result<Vec<SyncPeerRow>, VaultError> {
        let mut stmt = self.db.prepare(
            "SELECT peer_id, device_name, public_key, last_known_ip, last_known_port,
                    last_synced_at, paired_at, is_active
             FROM sync_peers ORDER BY paired_at",
        )?;
        let peers = stmt
            .query_map([], |row| {
                Ok(SyncPeerRow {
                    peer_id: uuid::Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    device_name: row.get(1)?,
                    public_key: row.get(2)?,
                    last_known_ip: row.get(3)?,
                    last_known_port: row.get::<_, Option<i32>>(4)?.map(|p| p as u16),
                    last_synced_at: row
                        .get::<_, Option<String>>(5)?
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc)),
                    paired_at: row
                        .get::<_, String>(6)
                        .ok()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(chrono::Utc::now),
                    is_active: row.get::<_, i32>(7)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(peers)
    }

    pub fn get_sync_peer_shared_secret(
        &self,
        peer_id: &uuid::Uuid,
    ) -> Result<Option<Vec<u8>>, VaultError> {
        self.require_unlocked()?;
        let data: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT shared_secret FROM sync_peers WHERE peer_id = ?1",
                params![peer_id.to_string()],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        match data {
            Some(encrypted) => {
                let b64 = self.decrypt_field(&encrypted)?;
                let bytes = base64::engine::general_purpose::STANDARD.decode(&b64)
                    .map_err(|e| VaultError::Crypto(format!("Base64 decode: {}", e)))?;
                Ok(Some(bytes))
            }
            None => Ok(None),
        }
    }

    pub fn delete_sync_peer(&self, peer_id: &uuid::Uuid) -> Result<(), VaultError> {
        self.db.execute(
            "DELETE FROM sync_peers WHERE peer_id = ?1",
            params![peer_id.to_string()],
        )?;
        Ok(())
    }

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

    // -----------------------------------------------------------------------
    // Re-encryption helpers
    // -----------------------------------------------------------------------

    fn re_encrypt_connections(&self, old_key: &[u8], new_key: &[u8]) -> Result<(), VaultError> {
        let mut stmt = self.db.prepare("SELECT id, password FROM connections WHERE password IS NOT NULL")?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, encrypted_pw) in rows {
            let plain = decrypt(&encrypted_pw, old_key)?;
            let re_encrypted = encrypt(&plain, new_key)?;
            self.db.execute(
                "UPDATE connections SET password = ?1 WHERE id = ?2",
                params![re_encrypted, id],
            )?;
        }
        Ok(())
    }

    fn re_encrypt_keys(&self, old_key: &[u8], new_key: &[u8]) -> Result<(), VaultError> {
        let mut stmt = self.db.prepare("SELECT id, private_key FROM keys WHERE private_key IS NOT NULL")?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, encrypted_key) in rows {
            let plain = decrypt(&encrypted_key, old_key)?;
            let re_encrypted = encrypt(&plain, new_key)?;
            self.db.execute(
                "UPDATE keys SET private_key = ?1 WHERE id = ?2",
                params![re_encrypted, id],
            )?;
        }
        Ok(())
    }

    fn re_encrypt_ai_api_key(&self, old_key: &[u8], new_key: &[u8]) -> Result<(), VaultError> {
        let encoded: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key = 'ai_api_key'",
                [],
                |row| row.get(0),
            )
            .ok();
        if let Some(encoded) = encoded
            && let Ok(encrypted) = BASE64.decode(encoded.as_bytes()) {
                let plain = decrypt(&encrypted, old_key)?;
                let re_encrypted = encrypt(&plain, new_key)?;
                let re_encoded = BASE64.encode(&re_encrypted);
                self.db.execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES ('ai_api_key', ?1)",
                    params![re_encoded],
                )?;
        }
        Ok(())
    }

    fn re_encrypt_identities(&self, old_key: &[u8], new_key: &[u8]) -> Result<(), VaultError> {
        let mut stmt = self.db.prepare("SELECT id, password FROM identities WHERE password IS NOT NULL")?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, encrypted_pw) in rows {
            let plain = decrypt(&encrypted_pw, old_key)?;
            let re_encrypted = encrypt(&plain, new_key)?;
            self.db.execute(
                "UPDATE identities SET password = ?1 WHERE id = ?2",
                params![re_encrypted, id],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oryxis_core::models::connection::{AuthMethod, Connection};
    use oryxis_core::models::group::Group;
    use oryxis_core::models::key::{KeyAlgorithm, SshKey};
    use oryxis_core::models::known_host::KnownHost;
    use oryxis_core::models::log_entry::{LogEntry, LogEvent};
    use oryxis_core::models::snippet::Snippet;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn temp_vault() -> VaultStore {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        // Keep the file alive by leaking it (tests are short-lived)
        std::mem::forget(tmp);
        VaultStore::open(&path).unwrap()
    }

    fn unlocked_vault() -> VaultStore {
        let mut vault = temp_vault();
        vault.set_master_password("testpass123").unwrap();
        vault
    }

    // ── Crypto ──

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let password = b"mysecretpassword";
        let plaintext = b"hello world, this is a secret";
        let encrypted = encrypt(plaintext, password).unwrap();
        assert_ne!(encrypted, plaintext);
        assert!(encrypted.len() > plaintext.len());
        let decrypted = decrypt(&encrypted, password).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_wrong_password_fails() {
        let encrypted = encrypt(b"secret data", b"correct_password").unwrap();
        let result = decrypt(&encrypted, b"wrong_password");
        assert!(result.is_err());
    }

    #[test]
    fn decrypt_truncated_data_fails() {
        let result = decrypt(&[0u8; 10], b"password");
        assert!(result.is_err());
    }

    #[test]
    fn encrypt_produces_different_ciphertext_each_time() {
        let password = b"password";
        let plaintext = b"same data";
        let a = encrypt(plaintext, password).unwrap();
        let b = encrypt(plaintext, password).unwrap();
        assert_ne!(a, b); // random salt + nonce
    }

    // ── Vault lifecycle ──

    #[test]
    fn new_vault_has_no_master_password() {
        let vault = temp_vault();
        assert!(!vault.has_master_password().unwrap());
        assert!(vault.is_locked());
    }

    #[test]
    fn set_master_password_unlocks() {
        let mut vault = temp_vault();
        vault.set_master_password("mypass").unwrap();
        assert!(!vault.is_locked());
    }

    #[test]
    fn set_master_password_twice_fails() {
        let mut vault = temp_vault();
        vault.set_master_password("mypass").unwrap();
        let result = vault.set_master_password("another");
        assert!(result.is_err());
    }

    #[test]
    fn lock_and_unlock() {
        let mut vault = temp_vault();
        vault.set_master_password("mypass").unwrap();
        vault.lock();
        assert!(vault.is_locked());
        vault.unlock("mypass").unwrap();
        assert!(!vault.is_locked());
    }

    #[test]
    fn unlock_wrong_password_fails() {
        let mut vault = temp_vault();
        vault.set_master_password("correct").unwrap();
        vault.lock();
        let result = vault.unlock("wrong");
        assert!(result.is_err());
        assert!(vault.is_locked());
    }

    // ── Connections CRUD ──

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

    // ── Keys CRUD ──

    #[test]
    fn save_and_list_keys() {
        let vault = unlocked_vault();
        let key = SshKey::new("my-key", KeyAlgorithm::Ed25519);
        vault.save_key(&key, Some("-----BEGIN PRIVATE KEY-----\nfake\n-----END PRIVATE KEY-----")).unwrap();

        let keys = vault.list_keys().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].label, "my-key");
    }

    #[test]
    fn key_private_encrypted_and_retrievable() {
        let vault = unlocked_vault();
        let key = SshKey::new("test-key", KeyAlgorithm::Rsa4096);
        let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIE...\n-----END RSA PRIVATE KEY-----";
        vault.save_key(&key, Some(pem)).unwrap();

        let retrieved = vault.get_key_private(&key.id).unwrap();
        assert_eq!(retrieved, Some(pem.to_string()));
    }

    #[test]
    fn delete_key() {
        let vault = unlocked_vault();
        let key = SshKey::new("disposable", KeyAlgorithm::Ed25519);
        vault.save_key(&key, None).unwrap();
        vault.delete_key(&key.id).unwrap();
        assert_eq!(vault.list_keys().unwrap().len(), 0);
    }

    // ── Groups CRUD ──

    #[test]
    fn save_and_list_groups() {
        let vault = unlocked_vault();
        let g = Group::new("Production");
        vault.save_group(&g).unwrap();

        let groups = vault.list_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "Production");
    }

    #[test]
    fn delete_group() {
        let vault = unlocked_vault();
        let g = Group::new("Temp");
        vault.save_group(&g).unwrap();
        vault.delete_group(&g.id).unwrap();
        assert_eq!(vault.list_groups().unwrap().len(), 0);
    }

    // ── Snippets CRUD ──

    #[test]
    fn save_and_list_snippets() {
        let vault = unlocked_vault();
        let s = Snippet::new("restart-nginx", "sudo systemctl restart nginx");
        vault.save_snippet(&s).unwrap();

        let snippets = vault.list_snippets().unwrap();
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].command, "sudo systemctl restart nginx");
    }

    #[test]
    fn delete_snippet() {
        let vault = unlocked_vault();
        let s = Snippet::new("temp", "echo hi");
        vault.save_snippet(&s).unwrap();
        vault.delete_snippet(&s.id).unwrap();
        assert_eq!(vault.list_snippets().unwrap().len(), 0);
    }

    // ── Known Hosts ──

    #[test]
    fn save_and_list_known_hosts() {
        let vault = unlocked_vault();
        let kh = KnownHost::new("example.com", 22, "ssh-ed25519", "SHA256:abc123");
        vault.save_known_host(&kh).unwrap();

        let hosts = vault.list_known_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "example.com");
        assert_eq!(hosts[0].fingerprint, "SHA256:abc123");
    }

    #[test]
    fn known_host_unique_per_host_port() {
        let vault = unlocked_vault();
        let kh1 = KnownHost::new("server.com", 22, "ssh-ed25519", "SHA256:first");
        vault.save_known_host(&kh1).unwrap();

        let kh2 = KnownHost::new("server.com", 22, "ssh-rsa", "SHA256:second");
        vault.save_known_host(&kh2).unwrap();

        let hosts = vault.list_known_hosts().unwrap();
        assert_eq!(hosts.len(), 1); // UNIQUE constraint
    }

    #[test]
    fn delete_known_host() {
        let vault = unlocked_vault();
        let kh = KnownHost::new("host.test", 22, "ed25519", "SHA256:xyz");
        vault.save_known_host(&kh).unwrap();
        vault.delete_known_host(&kh.id).unwrap();
        assert_eq!(vault.list_known_hosts().unwrap().len(), 0);
    }

    // ── Logs ──

    #[test]
    fn add_and_list_logs() {
        let vault = unlocked_vault();
        let entry = LogEntry::new("prod-web", "192.168.1.10", LogEvent::Connected, "OK");
        vault.add_log(&entry).unwrap();

        let logs = vault.list_logs(10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].connection_label, "prod-web");
    }

    #[test]
    fn logs_ordered_by_timestamp_desc() {
        let vault = unlocked_vault();
        vault.add_log(&LogEntry::new("first", "h1", LogEvent::Connected, "")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        vault.add_log(&LogEntry::new("second", "h2", LogEvent::Disconnected, "")).unwrap();

        let logs = vault.list_logs(10).unwrap();
        assert_eq!(logs[0].connection_label, "second"); // most recent first
    }

    #[test]
    fn clear_logs() {
        let vault = unlocked_vault();
        vault.add_log(&LogEntry::new("x", "y", LogEvent::Error, "fail")).unwrap();
        vault.add_log(&LogEntry::new("a", "b", LogEvent::Connected, "ok")).unwrap();
        vault.clear_logs().unwrap();
        assert_eq!(vault.list_logs(100).unwrap().len(), 0);
    }

    #[test]
    fn logs_limit_works() {
        let vault = unlocked_vault();
        for i in 0..20 {
            vault.add_log(&LogEntry::new(&format!("conn-{}", i), "h", LogEvent::Connected, "")).unwrap();
        }
        let logs = vault.list_logs(5).unwrap();
        assert_eq!(logs.len(), 5);
    }

    // ── MCP enabled field ──

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
    fn group_has_timestamps() {
        let vault = unlocked_vault();
        let g = Group::new("test-group");
        assert!(g.created_at <= g.updated_at);
        vault.save_group(&g).unwrap();

        let groups = vault.list_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert!(groups[0].created_at.timestamp() > 0);
        assert!(groups[0].updated_at.timestamp() > 0);
    }

    #[test]
    fn key_has_updated_at() {
        let vault = unlocked_vault();
        let key = SshKey::new("test-key", KeyAlgorithm::Ed25519);
        assert!(key.updated_at.timestamp() > 0);
        vault.save_key(&key, None).unwrap();

        let keys = vault.list_keys().unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].updated_at.timestamp() > 0);
    }

    #[test]
    fn snippet_has_updated_at() {
        let vault = unlocked_vault();
        let s = Snippet::new("test", "echo hi");
        assert!(s.updated_at.timestamp() > 0);
        vault.save_snippet(&s).unwrap();

        let snippets = vault.list_snippets().unwrap();
        assert_eq!(snippets.len(), 1);
        assert!(snippets[0].updated_at.timestamp() > 0);
    }

    #[test]
    fn known_host_has_updated_at() {
        let vault = unlocked_vault();
        let kh = KnownHost::new("host.test", 22, "ed25519", "SHA256:xyz");
        assert!(kh.updated_at.timestamp() > 0);
        vault.save_known_host(&kh).unwrap();

        let hosts = vault.list_known_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert!(hosts[0].updated_at.timestamp() > 0);
    }

    // ── Export / Import ──

    #[test]
    fn export_import_roundtrip() {
        use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

        let vault = unlocked_vault();

        // Populate vault
        let conn = Connection::new("prod-web", "192.168.1.10");
        vault.save_connection(&conn, Some("secret123")).unwrap();

        let g = Group::new("Production");
        vault.save_group(&g).unwrap();

        let s = Snippet::new("deploy", "make deploy");
        vault.save_snippet(&s).unwrap();

        let kh = KnownHost::new("192.168.1.10", 22, "ed25519", "SHA256:abc");
        vault.save_known_host(&kh).unwrap();

        // Export
        let export_pw = "export-password";
        let data = export_vault(&vault, export_pw, ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();

        // Verify header
        assert_eq!(&data[..6], b"ORYXIS");

        // Import into fresh vault
        let vault2 = unlocked_vault();
        let result = import_vault(&vault2, &data, export_pw).unwrap();

        assert_eq!(result.connections_added, 1);
        assert_eq!(result.groups_added, 1);
        assert_eq!(result.snippets_added, 1);
        assert_eq!(result.known_hosts_added, 1);

        // Verify data
        let conns = vault2.list_connections().unwrap();
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].label, "prod-web");

        let pw = vault2.get_connection_password(&conns[0].id).unwrap();
        assert_eq!(pw, Some("secret123".into()));

        assert_eq!(vault2.list_groups().unwrap().len(), 1);
        assert_eq!(vault2.list_snippets().unwrap().len(), 1);
        assert_eq!(vault2.list_known_hosts().unwrap().len(), 1);
    }

    #[test]
    fn export_wrong_password_fails() {
        use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

        let vault = unlocked_vault();
        let conn = Connection::new("test", "10.0.0.1");
        vault.save_connection(&conn, None).unwrap();

        let data = export_vault(&vault, "correct", ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();
        let result = import_vault(&vault, &data, "wrong");
        assert!(result.is_err());
    }

    #[test]
    fn export_invalid_file_rejected() {
        use crate::portable::{import_vault, is_valid_export};

        let vault = unlocked_vault();
        assert!(!is_valid_export(b"not an oryxis file"));
        assert!(import_vault(&vault, b"not an oryxis file", "pw").is_err());
    }

    #[test]
    fn import_skip_existing() {
        use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

        let vault = unlocked_vault();
        let conn = Connection::new("server", "10.0.0.1");
        vault.save_connection(&conn, Some("pw1")).unwrap();

        let data = export_vault(&vault, "pass", ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();

        // Import again into same vault — should skip
        let result = import_vault(&vault, &data, "pass").unwrap();
        assert_eq!(result.connections_skipped, 1);
        assert_eq!(result.connections_added, 0);

        // Still only 1 connection
        assert_eq!(vault.list_connections().unwrap().len(), 1);
    }

    #[test]
    fn import_updates_newer() {
        use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

        let vault1 = unlocked_vault();
        let mut conn = Connection::new("server", "10.0.0.1");
        conn.updated_at = chrono::Utc::now();
        vault1.save_connection(&conn, Some("old_pw")).unwrap();

        // Export
        let data = export_vault(&vault1, "pass", ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();

        // Create vault2 with same connection but older timestamp
        let vault2 = unlocked_vault();
        let mut old_conn = conn.clone();
        old_conn.updated_at = chrono::DateTime::parse_from_rfc3339("2020-01-01T00:00:00Z").unwrap().with_timezone(&chrono::Utc);
        vault2.save_connection(&old_conn, Some("old_pw")).unwrap();

        // Import — should update because export is newer
        let result = import_vault(&vault2, &data, "pass").unwrap();
        assert_eq!(result.connections_updated, 1);
        assert_eq!(result.connections_added, 0);
    }

    #[test]
    fn export_with_keys() {
        use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter, export_includes_keys};

        let vault = unlocked_vault();

        // Generate a key
        let generated = crate::keygen::generate_ed25519("test-key").unwrap();
        vault.save_key(&generated.key, Some(&generated.private_pem)).unwrap();

        // Export WITH keys
        let data_with = export_vault(&vault, "pass", ExportOptions { include_private_keys: true, filter: ExportFilter::All }).unwrap();
        assert!(export_includes_keys(&data_with));

        // Export WITHOUT keys
        let data_without = export_vault(&vault, "pass", ExportOptions { include_private_keys: false, filter: ExportFilter::All }).unwrap();
        assert!(!export_includes_keys(&data_without));

        // Import with keys into fresh vault
        let vault2 = unlocked_vault();
        let result = import_vault(&vault2, &data_with, "pass").unwrap();
        assert_eq!(result.keys_added, 1);

        let pk = vault2.get_key_private(&generated.key.id).unwrap();
        assert!(pk.is_some());

        // Import without keys — key added but no private key
        let vault3 = unlocked_vault();
        let result = import_vault(&vault3, &data_without, "pass").unwrap();
        assert_eq!(result.keys_added, 1);

        let pk = vault3.get_key_private(&generated.key.id).unwrap();
        assert!(pk.is_none());
    }

    // ── Sync Peers CRUD ──

    #[test]
    fn save_and_list_sync_peers() {
        let vault = unlocked_vault();
        let peer_id = Uuid::new_v4();
        let public_key = vec![1u8; 32];
        let now = chrono::Utc::now();

        vault.save_sync_peer(&peer_id, "laptop", &public_key, None, &now).unwrap();

        let peers = vault.list_sync_peers().unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].peer_id, peer_id);
        assert_eq!(peers[0].device_name, "laptop");
        assert_eq!(peers[0].public_key, public_key);
        assert!(peers[0].is_active);
        assert!(peers[0].last_synced_at.is_none());
    }

    #[test]
    fn sync_peer_with_shared_secret() {
        let vault = unlocked_vault();
        let peer_id = Uuid::new_v4();
        let public_key = vec![2u8; 32];
        let shared_secret = vec![42u8; 32];
        let now = chrono::Utc::now();

        vault.save_sync_peer(&peer_id, "desktop", &public_key, Some(&shared_secret), &now).unwrap();

        let retrieved = vault.get_sync_peer_shared_secret(&peer_id).unwrap();
        assert_eq!(retrieved, Some(shared_secret));
    }

    #[test]
    fn sync_peer_no_shared_secret() {
        let vault = unlocked_vault();
        let peer_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        vault.save_sync_peer(&peer_id, "phone", &[3u8; 32], None, &now).unwrap();

        let retrieved = vault.get_sync_peer_shared_secret(&peer_id).unwrap();
        assert!(retrieved.is_none());
    }

    #[test]
    fn update_sync_peer_endpoint() {
        let vault = unlocked_vault();
        let peer_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        vault.save_sync_peer(&peer_id, "server", &[4u8; 32], None, &now).unwrap();
        vault.update_sync_peer_endpoint(&peer_id, "192.168.1.50", 4433).unwrap();

        let peers = vault.list_sync_peers().unwrap();
        assert_eq!(peers[0].last_known_ip, Some("192.168.1.50".into()));
        assert_eq!(peers[0].last_known_port, Some(4433));
    }

    #[test]
    fn update_sync_peer_last_synced() {
        let vault = unlocked_vault();
        let peer_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        vault.save_sync_peer(&peer_id, "tablet", &[5u8; 32], None, &now).unwrap();
        assert!(vault.list_sync_peers().unwrap()[0].last_synced_at.is_none());

        vault.update_sync_peer_last_synced(&peer_id).unwrap();
        assert!(vault.list_sync_peers().unwrap()[0].last_synced_at.is_some());
    }

    #[test]
    fn delete_sync_peer() {
        let vault = unlocked_vault();
        let peer_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        vault.save_sync_peer(&peer_id, "temp", &[6u8; 32], None, &now).unwrap();
        assert_eq!(vault.list_sync_peers().unwrap().len(), 1);

        vault.delete_sync_peer(&peer_id).unwrap();
        assert_eq!(vault.list_sync_peers().unwrap().len(), 0);
    }

    #[test]
    fn multiple_sync_peers() {
        let vault = unlocked_vault();
        let now = chrono::Utc::now();

        vault.save_sync_peer(&Uuid::new_v4(), "device-a", &[7u8; 32], None, &now).unwrap();
        vault.save_sync_peer(&Uuid::new_v4(), "device-b", &[8u8; 32], None, &now).unwrap();
        vault.save_sync_peer(&Uuid::new_v4(), "device-c", &[9u8; 32], None, &now).unwrap();

        let peers = vault.list_sync_peers().unwrap();
        assert_eq!(peers.len(), 3);
    }

    // ── Share (filtered export) ──

    #[test]
    fn share_single_host() {
        use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

        let vault = unlocked_vault();

        let c1 = Connection::new("shared-host", "10.0.0.1");
        vault.save_connection(&c1, Some("pw1")).unwrap();
        let c2 = Connection::new("private-host", "10.0.0.2");
        vault.save_connection(&c2, Some("pw2")).unwrap();

        // Share only c1
        let data = export_vault(&vault, "share-pass", ExportOptions {
            include_private_keys: false,
            filter: ExportFilter::Hosts(vec![c1.id]),
        }).unwrap();

        let vault2 = unlocked_vault();
        let result = import_vault(&vault2, &data, "share-pass").unwrap();

        assert_eq!(result.connections_added, 1);
        let conns = vault2.list_connections().unwrap();
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].label, "shared-host");
    }

    #[test]
    fn share_group() {
        use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

        let vault = unlocked_vault();

        let g = Group::new("Team");
        vault.save_group(&g).unwrap();

        let mut c1 = Connection::new("web", "10.0.0.1");
        c1.group_id = Some(g.id);
        vault.save_connection(&c1, None).unwrap();

        let mut c2 = Connection::new("db", "10.0.0.2");
        c2.group_id = Some(g.id);
        vault.save_connection(&c2, None).unwrap();

        let c3 = Connection::new("personal", "10.0.0.3");
        vault.save_connection(&c3, None).unwrap();

        // Share group
        let data = export_vault(&vault, "pass", ExportOptions {
            include_private_keys: false,
            filter: ExportFilter::Group(g.id),
        }).unwrap();

        let vault2 = unlocked_vault();
        let result = import_vault(&vault2, &data, "pass").unwrap();

        assert_eq!(result.connections_added, 2);
        assert_eq!(result.groups_added, 1);
        let conns = vault2.list_connections().unwrap();
        assert_eq!(conns.len(), 2);
    }

    #[test]
    fn share_includes_dependencies() {
        use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};
        use oryxis_core::models::identity::Identity;

        let vault = unlocked_vault();

        // Create key
        let generated = crate::keygen::generate_ed25519("shared-key").unwrap();
        vault.save_key(&generated.key, Some(&generated.private_pem)).unwrap();

        // Create identity
        let mut ident = Identity::new("shared-ident");
        ident.key_id = Some(generated.key.id);
        vault.save_identity(&ident, Some("ident-pw")).unwrap();

        // Create group
        let g = Group::new("Shared");
        vault.save_group(&g).unwrap();

        // Create connection referencing all deps
        let mut conn = Connection::new("server", "10.0.0.1");
        conn.key_id = Some(generated.key.id);
        conn.identity_id = Some(ident.id);
        conn.group_id = Some(g.id);
        vault.save_connection(&conn, Some("conn-pw")).unwrap();

        // Create unrelated data
        let unrelated = Connection::new("other", "10.0.0.2");
        vault.save_connection(&unrelated, None).unwrap();
        let unrelated_key = SshKey::new("other-key", KeyAlgorithm::Ed25519);
        vault.save_key(&unrelated_key, None).unwrap();

        // Share only the one connection
        let data = export_vault(&vault, "pass", ExportOptions {
            include_private_keys: true,
            filter: ExportFilter::Hosts(vec![conn.id]),
        }).unwrap();

        let vault2 = unlocked_vault();
        let result = import_vault(&vault2, &data, "pass").unwrap();

        // Should have 1 connection, 1 key, 1 identity, 1 group
        assert_eq!(result.connections_added, 1);
        assert_eq!(result.keys_added, 1);
        assert_eq!(result.identities_added, 1);
        assert_eq!(result.groups_added, 1);

        // Should NOT have unrelated data
        assert_eq!(vault2.list_connections().unwrap().len(), 1);
        assert_eq!(vault2.list_keys().unwrap().len(), 1);

        // Password should be preserved
        let pw = vault2.get_connection_password(&conn.id).unwrap();
        assert_eq!(pw, Some("conn-pw".into()));
    }

    #[test]
    fn share_no_snippets_or_known_hosts() {
        use crate::portable::{export_vault, import_vault, ExportOptions, ExportFilter};

        let vault = unlocked_vault();

        let conn = Connection::new("host", "10.0.0.1");
        vault.save_connection(&conn, None).unwrap();

        let s = Snippet::new("test", "echo hi");
        vault.save_snippet(&s).unwrap();

        let kh = KnownHost::new("10.0.0.1", 22, "ed25519", "SHA256:abc");
        vault.save_known_host(&kh).unwrap();

        // Share only the connection
        let data = export_vault(&vault, "pass", ExportOptions {
            include_private_keys: false,
            filter: ExportFilter::Hosts(vec![conn.id]),
        }).unwrap();

        let vault2 = unlocked_vault();
        let result = import_vault(&vault2, &data, "pass").unwrap();

        assert_eq!(result.connections_added, 1);
        assert_eq!(result.snippets_added, 0);
        assert_eq!(result.known_hosts_added, 0);
    }
}

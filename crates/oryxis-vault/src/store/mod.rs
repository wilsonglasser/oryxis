use std::path::{Path, PathBuf};

use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use rusqlite::{params, Connection as SqliteConn};
use zeroize::Zeroizing;
use uuid::Uuid;

pub(crate) use oryxis_core::models::cloud::{CloudQuery, CloudRef};
pub(crate) use oryxis_core::models::cloud_profile::CloudProfile;
pub(crate) use oryxis_core::models::custom_terminal_theme::CustomTerminalTheme;
pub(crate) use oryxis_core::models::custom_ui_theme::CustomUiTheme;
pub(crate) use oryxis_core::models::connection::{AuthMethod, Connection, ProxyType};
pub(crate) use oryxis_core::models::group::Group;
pub(crate) use oryxis_core::models::identity::Identity;
pub(crate) use oryxis_core::models::proxy_identity::ProxyIdentity;
pub(crate) use oryxis_core::models::session_group::SessionGroup;
pub(crate) use oryxis_core::models::key::{KeyAlgorithm, SshKey};
pub(crate) use oryxis_core::models::snippet::Snippet;
pub(crate) use oryxis_core::models::port_forward_rule::{ForwardKind, PortForwardRule};

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

    #[error("Key is encrypted; passphrase required")]
    KeyNeedsPassphrase,

    #[error("Wrong key passphrase")]
    WrongKeyPassphrase,

    /// Legacy OpenSSL-encrypted PEM (`Proc-Type: 4,ENCRYPTED` +
    /// `DEK-Info:`). We don't ship a PBKDF1-MD5 + DES-EDE3
    /// implementation for this corner case. The caller surfaces an
    /// i18n'd message with conversion hints.
    #[error("Legacy OpenSSL-encrypted key (Proc-Type:4,ENCRYPTED) not supported")]
    EncryptedLegacyPem,

    /// PPK file references an algorithm or KDF we don't implement
    /// (e.g. DSA, or an unknown PPK version). Caller surfaces an i18n
    /// message with the verbatim spec name in `0` so the user can act
    /// on it. Separate from `Crypto(_)` because we want a structural
    /// match path in the UI.
    #[error("Unsupported key kind: {0}")]
    UnsupportedKeyKind(String),
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

/// A deletion record from the `sync_metadata` table. Every `delete_*`
/// records one so the sync engine can propagate the removal: without
/// it, a peer that still holds the entity would push its stale copy
/// back on the next sync and the delete would silently undo itself.
///
/// `entity_type` is the wire string from `oryxis_sync::protocol::
/// EntityType`'s `Display` impl (`"connection"`, `"key"`, …). The vault
/// stays string-typed here to avoid a dependency cycle on `oryxis-sync`.
#[derive(Debug, Clone)]
pub struct Tombstone {
    pub entity_type: String,
    pub entity_id: Uuid,
    pub deleted_at: DateTime<Utc>,
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

/// Byte-to-byte field codec used by `convert_all_fields` (decrypt with
/// one key/format, re-encrypt with another).
type FieldCodec<'a> = &'a dyn Fn(&[u8]) -> Result<Vec<u8>, VaultError>;

/// Format tag for fields encrypted directly with the derived vault key.
/// Legacy blobs (per-field Argon2id, `encrypt` above) start with a
/// random salt byte and are at least 60 bytes; the tag plus the AEAD
/// makes the two formats unambiguous in practice, and the eager
/// migration on unlock removes legacy blobs from the vault anyway.
const FIELD_FORMAT_V2: u8 = 2;

/// Encrypt with an already-derived 256-bit key. Returns:
/// tag(1) + nonce(12) + ciphertext. No KDF, so per-field operations
/// are microseconds instead of an Argon2id pass each.
pub(crate) fn encrypt_with_key(plaintext: &[u8], key: &[u8]) -> Result<Vec<u8>, VaultError> {
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| VaultError::Crypto(format!("Cipher init: {}", e)))?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|e| VaultError::Crypto(format!("Encrypt: {}", e)))?;
    let mut result = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    result.push(FIELD_FORMAT_V2);
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt data produced by `encrypt_with_key`.
pub(crate) fn decrypt_with_key(data: &[u8], key: &[u8]) -> Result<Vec<u8>, VaultError> {
    if data.len() < 1 + NONCE_LEN + 16 || data[0] != FIELD_FORMAT_V2 {
        return Err(VaultError::Crypto("not a derived-key field".into()));
    }
    let nonce_bytes = &data[1..1 + NONCE_LEN];
    let ciphertext = &data[1 + NONCE_LEN..];
    let cipher = ChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| VaultError::Crypto(format!("Cipher init: {}", e)))?;
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|_| VaultError::InvalidPassword)
}

// ---------------------------------------------------------------------------
// VaultStore
// ---------------------------------------------------------------------------

/// Vault store, manages SQLite database with encrypted secrets.
pub struct VaultStore {
    db: SqliteConn,
    /// The vault key derived once at unlock (Argon2id over the master
    /// password and the vault-level `kdf_salt`). Field operations use
    /// it directly, so they don't pay a KDF each. `Zeroizing` wipes
    /// the buffer on lock/replace/drop.
    master_key: Option<Zeroizing<Vec<u8>>>,
    /// Unwrapped session-recording content key, cached after first use so
    /// chunk writes don't pay the master-key KDF. Interior mutability
    /// because append/read paths only hold `&self`. Cleared on `lock()`.
    session_log_key: std::sync::Mutex<Option<[u8; KEY_LEN]>>,
    db_path: PathBuf,
}


mod cloud;
mod connections;
mod forwarding;
mod groups;
mod identities;
mod keys;
mod logs;
mod schema;
mod settings;
mod snippets;
mod sync;

#[cfg(test)]
mod tests;

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
        // WAL lets readers and a writer coexist; `busy_timeout` covers
        // the rare two-writer overlap (e.g. the sync engine opens its
        // own handle on the same file, see `oryxis-app::sync_runtime`)
        // so a contended write waits briefly instead of failing with
        // SQLITE_BUSY.
        //
        // `synchronous=NORMAL` is the SQLite-recommended pairing with WAL:
        // the writer no longer fsyncs on every commit (only at checkpoint),
        // which is what made high-frequency writers (session-log appends)
        // hammer the disk. The durability trade-off is bounded: a power
        // loss can drop the last committed transaction, never corrupt the
        // file. Acceptable here, the vault is re-derivable user state.
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA busy_timeout=5000;",
        )?;

        // Tighten file permissions to 0600 on Unix, the vault holds
        // encrypted credentials but if another local user can read the
        // ciphertext they can attempt offline brute force at leisure.
        // Best-effort: a missing chmod (e.g., readonly fs, exotic mount)
        // shouldn't break vault open, just log and move on.
        Self::tighten_perms(&path);

        let mut store = Self {
            db,
            master_key: None,
            session_log_key: std::sync::Mutex::new(None),
            db_path: path,
        };
        store.create_tables()?;
        Ok(store)
    }

    /// Filesystem path of the SQLite database backing this vault. The
    /// sync engine opens its own handle on this same path (see
    /// `oryxis-app::sync_runtime`).
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    #[cfg(unix)]
    fn tighten_perms(path: &Path) {
        use std::os::unix::fs::PermissionsExt as _;
        for ext in ["", "-wal", "-shm"] {
            let mut p = path.to_path_buf();
            if !ext.is_empty()
                && let Some(name) = p.file_name().and_then(|n| n.to_str())
            {
                p.set_file_name(format!("{}{}", name, ext));
            }
            if !p.exists() {
                continue;
            }
            if let Err(e) = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)) {
                tracing::warn!("vault chmod 0600 on {}: {e}", p.display());
            }
        }
    }

    #[cfg(not(unix))]
    fn tighten_perms(_path: &Path) {
        // Windows ACL hardening is a separate effort, the default
        // user-profile ACL already keeps other local users out of
        // %APPDATA%, so we no-op here for now.
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
        let key = self.derive_vault_key(pw_bytes)?;
        // Store an encrypted known value so we can verify the password on unlock.
        let check = encrypt_with_key(b"oryxis_vault_ok", &key)?;
        self.db.execute(
            "INSERT INTO vault_meta (key, value) VALUES ('password_check', ?1)",
            params![check],
        )?;
        // Mirror the flag that `set_user_password` / `remove_user_password`
        // maintain. Lets `Oryxis::boot` skip the wake-up Argon2id KDF on
        // brand-new vaults without ever running an empty-password unlock
        // attempt to discover the state.
        let flag = if pw_bytes.is_empty() { "0" } else { "1" };
        self.db.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('has_user_password', ?1)",
            params![flag],
        )?;
        self.master_key = Some(Zeroizing::new(key.to_vec()));
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
        if check.first() == Some(&FIELD_FORMAT_V2) {
            // Current format: one KDF over the vault salt, then the
            // check value verifies password + key in one shot.
            let key = self.derive_vault_key(pw_bytes)?;
            let plain = decrypt_with_key(&check, &key)?;
            if plain != b"oryxis_vault_ok" {
                return Err(VaultError::InvalidPassword);
            }
            self.master_key = Some(Zeroizing::new(key.to_vec()));
        } else {
            // Vault written by an older version: verify through the
            // legacy per-field-KDF path, then migrate every encrypted
            // blob to the derived-key format. One-time cost (an Argon2
            // pass per stored secret), after which every field
            // operation is microseconds.
            let plain = decrypt(&check, pw_bytes)?;
            if plain != b"oryxis_vault_ok" {
                return Err(VaultError::InvalidPassword);
            }
            let key = self.derive_vault_key(pw_bytes)?;
            self.migrate_fields_to_derived_key(pw_bytes, &key)?;
            self.master_key = Some(Zeroizing::new(key.to_vec()));
        }
        tracing::info!("Vault unlocked");
        Ok(())
    }

    /// Argon2id over the vault-level salt stored in `vault_meta`
    /// (`kdf_salt`), created on first use.
    fn derive_vault_key(&self, password: &[u8]) -> Result<[u8; KEY_LEN], VaultError> {
        let salt: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT value FROM vault_meta WHERE key = 'kdf_salt'",
                [],
                |row| row.get(0),
            )
            .ok();
        let salt = match salt {
            Some(s) if s.len() == SALT_LEN => s,
            _ => {
                let mut s = [0u8; SALT_LEN];
                OsRng.fill_bytes(&mut s);
                self.db.execute(
                    "INSERT OR REPLACE INTO vault_meta (key, value) VALUES ('kdf_salt', ?1)",
                    params![s.to_vec()],
                )?;
                s.to_vec()
            }
        };
        derive_key(password, &salt)
    }

    /// One-time migration from the legacy per-field-KDF format to the
    /// derived-key format, run inside a transaction during the first
    /// unlock after the update. Lenient per row: a blob that fails to
    /// decrypt was already unreadable, so it's left in place rather
    /// than blocking the unlock.
    fn migrate_fields_to_derived_key(
        &mut self,
        password: &[u8],
        key: &[u8; KEY_LEN],
    ) -> Result<(), VaultError> {
        let dec = |b: &[u8]| decrypt(b, password);
        let enc = |p: &[u8]| encrypt_with_key(p, key);
        self.db.execute_batch("BEGIN")?;
        let result = (|| -> Result<(), VaultError> {
            self.convert_all_fields(&dec, &enc, true)?;
            let check = encrypt_with_key(b"oryxis_vault_ok", key)?;
            self.db.execute(
                "INSERT OR REPLACE INTO vault_meta (key, value) VALUES ('password_check', ?1)",
                params![check],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.db.execute_batch("COMMIT")?;
                tracing::info!("Vault fields migrated to derived-key format");
                Ok(())
            }
            Err(e) => {
                let _ = self.db.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    pub fn lock(&mut self) {
        self.master_key = None;
        *self.session_log_key.lock().unwrap() = None;
        tracing::info!("Vault locked");
    }

    fn require_unlocked(&self) -> Result<&[u8], VaultError> {
        self.master_key
            .as_ref()
            .map(|k| k.as_slice())
            .ok_or(VaultError::Locked)
    }

    // -----------------------------------------------------------------------
    // Encrypt / decrypt field helpers
    // -----------------------------------------------------------------------

    fn encrypt_field(&self, plaintext: &str) -> Result<Vec<u8>, VaultError> {
        let key = self.require_unlocked()?;
        encrypt_with_key(plaintext.as_bytes(), key)
    }

    fn decrypt_field(&self, data: &[u8]) -> Result<String, VaultError> {
        let key = self.require_unlocked()?;
        let plain = decrypt_with_key(data, key)?;
        String::from_utf8(plain).map_err(|e| VaultError::Crypto(e.to_string()))
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
            // Vault already set up, try unlocking with empty password
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

    /// Destroy the vault database and recreate it fresh. Every table is
    /// dropped (the list must cover everything `create_tables` creates,
    /// since that uses IF NOT EXISTS and would silently keep surviving
    /// rows), then VACUUM releases the freed pages so the destroyed
    /// data doesn't linger in the file.
    pub fn destroy_and_recreate(&mut self) -> Result<(), VaultError> {
        self.db.execute_batch(
            "DROP TABLE IF EXISTS vault_meta;
             DROP TABLE IF EXISTS settings;
             DROP TABLE IF EXISTS groups;
             DROP TABLE IF EXISTS session_groups;
             DROP TABLE IF EXISTS connections;
             DROP TABLE IF EXISTS keys;
             DROP TABLE IF EXISTS snippets;
             DROP TABLE IF EXISTS custom_terminal_themes;
             DROP TABLE IF EXISTS custom_ui_themes;
             DROP TABLE IF EXISTS port_forward_rules;
             DROP TABLE IF EXISTS identities;
             DROP TABLE IF EXISTS proxy_identities;
             DROP TABLE IF EXISTS known_hosts;
             DROP TABLE IF EXISTS logs;
             DROP TABLE IF EXISTS session_logs;
             DROP TABLE IF EXISTS session_log_chunks;
             DROP TABLE IF EXISTS cloud_profiles;
             DROP TABLE IF EXISTS sync_peers;
             DROP TABLE IF EXISTS sync_metadata;
             VACUUM;"
        )?;
        self.master_key = None;
        *self.session_log_key.lock().unwrap() = None;
        self.create_tables()?;
        tracing::info!("Vault destroyed and recreated");
        Ok(())
    }

    /// Set a user password on the vault. Re-encrypts all encrypted fields
    /// from the current master key to the new password.
    pub fn set_user_password(&mut self, new_password: &str) -> Result<(), VaultError> {
        let old_key = self.require_unlocked()?.to_vec();
        let new_key = self.rotate_vault_key(new_password)?;

        self.change_master_key(&old_key, &new_key, "1")?;
        tracing::info!("Vault user password set");
        Ok(())
    }

    /// Generate a fresh `kdf_salt` and derive the vault key for
    /// `password` over it. The salt write participates in the caller's
    /// transaction when one is open.
    fn rotate_vault_key(&self, password: &str) -> Result<Vec<u8>, VaultError> {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        self.db.execute(
            "INSERT OR REPLACE INTO vault_meta (key, value) VALUES ('kdf_salt', ?1)",
            params![salt.to_vec()],
        )?;
        Ok(derive_key(password.as_bytes(), &salt)?.to_vec())
    }

    /// Shared tail of a master password change: re-encrypt every
    /// field, rewrite the password check and the `has_user_password`
    /// flag, all inside one transaction so a crash mid-change can't
    /// leave the vault half re-encrypted.
    fn change_master_key(
        &mut self,
        old_key: &[u8],
        new_key: &[u8],
        user_flag: &str,
    ) -> Result<(), VaultError> {
        self.db.execute_batch("BEGIN")?;
        let result = (|| -> Result<(), VaultError> {
            self.re_encrypt_all(old_key, new_key)?;
            let check = encrypt_with_key(b"oryxis_vault_ok", new_key)?;
            self.db.execute(
                "INSERT OR REPLACE INTO vault_meta (key, value) VALUES ('password_check', ?1)",
                params![check],
            )?;
            self.db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('has_user_password', ?1)",
                params![user_flag],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.db.execute_batch("COMMIT")?;
                self.master_key = Some(Zeroizing::new(new_key.to_vec()));
                Ok(())
            }
            Err(e) => {
                let _ = self.db.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Remove the user password, reverting to the default empty password.
    /// Re-encrypts all encrypted fields from the current key to empty string.
    pub fn remove_user_password(&mut self) -> Result<(), VaultError> {
        let old_key = self.require_unlocked()?.to_vec();
        let new_key = self.rotate_vault_key("")?;

        self.change_master_key(&old_key, &new_key, "0")?;
        tracing::info!("Vault user password removed");
        Ok(())
    }
    // -----------------------------------------------------------------------
    // Re-encryption helpers
    // -----------------------------------------------------------------------

    /// Re-encrypt every encrypted field from `old_key` to `new_key`
    /// (both derived vault keys). This list must cover everything
    /// `encrypt_field` ever writes; a column missed here becomes
    /// undecryptable after a master password change.
    fn re_encrypt_all(&self, old_key: &[u8], new_key: &[u8]) -> Result<(), VaultError> {
        let dec = |b: &[u8]| decrypt_with_key(b, old_key);
        let enc = |p: &[u8]| encrypt_with_key(p, new_key);
        self.convert_all_fields(&dec, &enc, false)
    }

    /// Walk every encrypted field, decoding with `dec` and re-encoding
    /// with `enc`. `lenient` skips rows whose blob fails to decode
    /// (used by the format migration, where an unreadable blob was
    /// already unreadable); the strict mode aborts, used by password
    /// changes where every secret is expected to convert.
    fn convert_all_fields(
        &self,
        dec: FieldCodec,
        enc: FieldCodec,
        lenient: bool,
    ) -> Result<(), VaultError> {
        for (table, id_col, col) in [
            ("connections", "id", "password"),
            ("connections", "id", "proxy_password"),
            ("keys", "id", "private_key"),
            ("identities", "id", "password"),
            ("proxy_identities", "id", "password"),
            ("cloud_profiles", "id", "secret"),
            ("sync_peers", "peer_id", "shared_secret"),
        ] {
            self.convert_blob_column(table, id_col, col, dec, enc, lenient)?;
        }
        self.convert_settings_b64("ai_api_key", dec, enc, lenient)?;
        self.convert_settings_b64("sync_device_identity", dec, enc, lenient)?;
        // The session-recording content key only needs its wrapper
        // converted; the chunks themselves are sealed with the
        // (unchanged) content key.
        self.convert_meta_blob("session_log_key", dec, enc, lenient)?;
        Ok(())
    }

    /// Convert one BLOB column of one table. The identifiers are
    /// compile-time constants from `convert_all_fields`, never user
    /// input, so the `format!`-built SQL stays injection-free.
    fn convert_blob_column(
        &self,
        table: &str,
        id_col: &str,
        col: &str,
        dec: FieldCodec,
        enc: FieldCodec,
        lenient: bool,
    ) -> Result<(), VaultError> {
        let mut stmt = self.db.prepare(&format!(
            "SELECT {id_col}, {col} FROM {table} WHERE {col} IS NOT NULL"
        ))?;
        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (id, blob) in rows {
            let plain = match dec(&blob) {
                Ok(p) => p,
                Err(e) if lenient => {
                    tracing::warn!("skipping unreadable {table}.{col} for {id}: {e}");
                    continue;
                }
                Err(e) => return Err(e),
            };
            let re_encrypted = enc(&plain)?;
            self.db.execute(
                &format!("UPDATE {table} SET {col} = ?1 WHERE {id_col} = ?2"),
                params![re_encrypted, id],
            )?;
        }
        Ok(())
    }

    /// Convert a base64-encoded encrypted value in the `settings` table.
    fn convert_settings_b64(
        &self,
        setting: &str,
        dec: FieldCodec,
        enc: FieldCodec,
        lenient: bool,
    ) -> Result<(), VaultError> {
        let encoded: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![setting],
                |row| row.get(0),
            )
            .ok();
        if let Some(encoded) = encoded
            && let Ok(encrypted) = BASE64.decode(encoded.as_bytes())
        {
            let plain = match dec(&encrypted) {
                Ok(p) => p,
                Err(e) if lenient => {
                    tracing::warn!("skipping unreadable setting {setting}: {e}");
                    return Ok(());
                }
                Err(e) => return Err(e),
            };
            let re_encoded = BASE64.encode(&enc(&plain)?);
            self.db.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                params![setting, re_encoded],
            )?;
        }
        Ok(())
    }

    /// Convert a raw encrypted BLOB in `vault_meta`.
    fn convert_meta_blob(
        &self,
        key_name: &str,
        dec: FieldCodec,
        enc: FieldCodec,
        lenient: bool,
    ) -> Result<(), VaultError> {
        let wrapped: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT value FROM vault_meta WHERE key = ?1",
                params![key_name],
                |row| row.get(0),
            )
            .ok();
        if let Some(w) = wrapped {
            let plain = match dec(&w) {
                Ok(p) => p,
                Err(e) if lenient => {
                    tracing::warn!("skipping unreadable vault_meta {key_name}: {e}");
                    return Ok(());
                }
                Err(e) => return Err(e),
            };
            self.db.execute(
                "INSERT OR REPLACE INTO vault_meta (key, value) VALUES (?1, ?2)",
                params![key_name, enc(&plain)?],
            )?;
        }
        Ok(())
    }



}

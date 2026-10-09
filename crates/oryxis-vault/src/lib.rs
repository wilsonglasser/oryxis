pub mod change_notice;
pub mod ephemeral;
pub mod keygen;
pub mod portable;
pub mod store;

pub use ephemeral::EphemeralKey;

pub use keygen::{
    encrypt_private_pem, generate_ed25519, generate_key, import_key, import_public_key,
    is_key_encrypted, resolve_disk_key, DiskKey, DiskKeyStatus, DiskKeyWanted, EcdsaCurveChoice,
    GenerateSpec, GeneratedKey, RsaBits,
};
pub use portable::{export_vault, import_vault, inspect_export, is_valid_export, export_includes_keys, ExportCategory, ExportFilter, ExportOptions, ExportSelection, ExportSummary, ImportResult};
pub use store::{
    calibrate_kdf, derive_sync_secret, ChatConversationEntry, ChatMessageEntry,
    CommandHistoryEntry, KdfParams, SealedSessionOutput,
    SessionLogEntry, SessionLogEvent, SyncPeerRow,
    Tombstone,
    VaultError, VaultStore,
};

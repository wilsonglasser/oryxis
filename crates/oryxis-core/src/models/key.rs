use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKey {
    pub id: Uuid,
    pub label: String,
    pub fingerprint: String,
    pub algorithm: KeyAlgorithm,
    pub public_key: String,
    pub file_ref: String,
    pub has_passphrase: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl SshKey {
    pub fn new(label: impl Into<String>, algorithm: KeyAlgorithm) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            fingerprint: String::new(),
            algorithm,
            public_key: String::new(),
            file_ref: String::new(),
            has_passphrase: false,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyAlgorithm {
    Ed25519,
    Rsa4096,
    EcdsaP256,
    EcdsaP384,
}

impl std::fmt::Display for KeyAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ed25519 => write!(f, "Ed25519"),
            Self::Rsa4096 => write!(f, "RSA 4096"),
            Self::EcdsaP256 => write!(f, "ECDSA P-256"),
            Self::EcdsaP384 => write!(f, "ECDSA P-384"),
        }
    }
}

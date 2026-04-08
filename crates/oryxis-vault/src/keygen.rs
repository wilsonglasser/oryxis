use ssh_key::{Algorithm, HashAlg, PrivateKey};

use oryxis_core::models::key::{KeyAlgorithm, SshKey};

use crate::store::VaultError;

/// Generated key pair — private PEM + SshKey model.
pub struct GeneratedKey {
    pub key: SshKey,
    pub private_pem: String,
}

/// Generate an Ed25519 SSH key pair.
pub fn generate_ed25519(label: &str) -> Result<GeneratedKey, VaultError> {
    let mut rng = rand::thread_rng();
    let private_key = PrivateKey::random(&mut rng, Algorithm::Ed25519)
        .map_err(|e| VaultError::Crypto(format!("Key generation failed: {}", e)))?;

    let public_key = private_key.public_key();
    let fingerprint = public_key
        .fingerprint(HashAlg::Sha256)
        .to_string();
    let public_key_str = public_key.to_openssh()
        .map_err(|e| VaultError::Crypto(format!("Public key encoding failed: {}", e)))?;
    let private_pem = private_key
        .to_openssh(ssh_key::LineEnding::LF)
        .map_err(|e| VaultError::Crypto(format!("Private key encoding failed: {}", e)))?
        .to_string();

    let mut key = SshKey::new(label, KeyAlgorithm::Ed25519);
    key.fingerprint = fingerprint;
    key.public_key = public_key_str;

    Ok(GeneratedKey { key, private_pem })
}

/// Import an SSH key from PEM/OpenSSH format.
pub fn import_key(label: &str, private_pem: &str) -> Result<GeneratedKey, VaultError> {
    // Normalize line endings (CRLF → LF) to avoid Base64 parse errors
    let normalized = private_pem.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();
    let private_key = PrivateKey::from_openssh(trimmed)
        .map_err(|e| VaultError::Crypto(format!("Failed to parse key: {}", e)))?;

    let public_key = private_key.public_key();
    let fingerprint = public_key.fingerprint(HashAlg::Sha256).to_string();
    let public_key_str = public_key.to_openssh()
        .map_err(|e| VaultError::Crypto(format!("Public key encoding failed: {}", e)))?;

    let algorithm = match private_key.algorithm() {
        Algorithm::Ed25519 => KeyAlgorithm::Ed25519,
        Algorithm::Rsa { .. } => KeyAlgorithm::Rsa4096,
        Algorithm::Ecdsa { curve } => match curve {
            ssh_key::EcdsaCurve::NistP256 => KeyAlgorithm::EcdsaP256,
            ssh_key::EcdsaCurve::NistP384 => KeyAlgorithm::EcdsaP384,
            _ => KeyAlgorithm::EcdsaP256,
        },
        _ => KeyAlgorithm::Ed25519,
    };

    let mut key = SshKey::new(label, algorithm);
    key.fingerprint = fingerprint;
    key.public_key = public_key_str;

    Ok(GeneratedKey {
        key,
        private_pem: trimmed.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_ed25519_produces_valid_key() {
        let result = generate_ed25519("test-key").unwrap();
        assert_eq!(result.key.label, "test-key");
        assert_eq!(result.key.algorithm, KeyAlgorithm::Ed25519);
        assert!(!result.key.fingerprint.is_empty());
        assert!(result.key.public_key.starts_with("ssh-ed25519 "));
        assert!(result.private_pem.contains("BEGIN OPENSSH PRIVATE KEY"));
    }

    #[test]
    fn generate_ed25519_unique_keys() {
        let a = generate_ed25519("key-a").unwrap();
        let b = generate_ed25519("key-b").unwrap();
        assert_ne!(a.key.fingerprint, b.key.fingerprint);
        assert_ne!(a.private_pem, b.private_pem);
    }

    #[test]
    fn import_roundtrip() {
        // Generate then import
        let generated = generate_ed25519("original").unwrap();
        let imported = import_key("imported", &generated.private_pem).unwrap();
        assert_eq!(imported.key.fingerprint, generated.key.fingerprint);
        assert_eq!(imported.key.algorithm, KeyAlgorithm::Ed25519);
        assert_eq!(imported.key.public_key, generated.key.public_key);
    }

    #[test]
    fn import_invalid_pem_fails() {
        let result = import_key("bad", "this is not a key");
        assert!(result.is_err());
    }

    #[test]
    fn import_with_whitespace() {
        let generated = generate_ed25519("ws-test").unwrap();
        let padded = format!("\n  {}  \n", generated.private_pem);
        let imported = import_key("trimmed", &padded).unwrap();
        assert_eq!(imported.key.fingerprint, generated.key.fingerprint);
    }
}

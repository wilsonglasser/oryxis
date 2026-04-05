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
    let private_key = PrivateKey::from_openssh(private_pem)
        .or_else(|_| {
            // Try decoding as russh format too
            PrivateKey::from_openssh(private_pem.trim())
        })
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
        private_pem: private_pem.to_string(),
    })
}

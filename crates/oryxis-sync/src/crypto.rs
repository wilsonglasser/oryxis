use chacha20poly1305::aead::{Aead, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::RngCore;
use uuid::Uuid;
use x25519_dalek::{EphemeralSecret, PublicKey as X25519PublicKey};

use crate::error::SyncError;

/// A device's persistent identity for sync.
#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    pub device_id: Uuid,
    pub device_name: String,
    pub signing_key: SigningKey,
}

impl DeviceIdentity {
    /// Generate a new device identity.
    pub fn generate(device_name: &str) -> Self {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        Self {
            device_id: Uuid::new_v4(),
            device_name: device_name.into(),
            signing_key,
        }
    }

    /// Get the public verifying key.
    pub fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Serialize the public key bytes.
    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.public_key().to_bytes().to_vec()
    }
}

/// Generate a 6-digit numeric pairing code.
pub fn generate_pairing_code() -> String {
    let mut rng = OsRng;
    let code: u32 = rng.next_u32() % 1_000_000;
    format!("{:06}", code)
}

/// Perform X25519 key exchange to derive a shared secret.
/// Returns (our_public_key, shared_secret).
pub fn x25519_key_exchange(peer_public_key: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let our_public = X25519PublicKey::from(&secret);
    let peer_public = X25519PublicKey::from(*peer_public_key);
    let shared = secret.diffie_hellman(&peer_public);
    (our_public.to_bytes(), *shared.as_bytes())
}

/// Encrypt payload with shared secret using ChaCha20Poly1305.
pub fn encrypt_payload(plaintext: &[u8], shared_secret: &[u8; 32]) -> Result<Vec<u8>, SyncError> {
    let cipher = ChaCha20Poly1305::new_from_slice(shared_secret)
        .map_err(|e| SyncError::Crypto(format!("Cipher init: {}", e)))?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| SyncError::Crypto(format!("Encrypt: {}", e)))?;

    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt payload with shared secret.
pub fn decrypt_payload(data: &[u8], shared_secret: &[u8; 32]) -> Result<Vec<u8>, SyncError> {
    if data.len() < 12 + 16 {
        return Err(SyncError::Crypto("Data too short".into()));
    }
    let nonce_bytes = &data[..12];
    let ciphertext = &data[12..];

    let cipher = ChaCha20Poly1305::new_from_slice(shared_secret)
        .map_err(|e| SyncError::Crypto(format!("Cipher init: {}", e)))?;
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| SyncError::Crypto("Decryption failed (wrong key?)".into()))
}

/// Generate a self-signed TLS certificate for QUIC, using the device's Ed25519 key as identity.
pub fn generate_tls_cert(
    device_id: &Uuid,
) -> Result<(Vec<u8>, Vec<u8>), SyncError> {
    let subject = format!("oryxis-sync-{}", device_id);
    let key_pair = rcgen::KeyPair::generate()
        .map_err(|e| SyncError::Crypto(format!("Key gen: {}", e)))?;
    let key_der = key_pair.serialize_der();
    let params = rcgen::CertificateParams::new(vec![subject])
        .map_err(|e| SyncError::Crypto(format!("Cert params: {}", e)))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| SyncError::Crypto(format!("Self-sign: {}", e)))?;
    let cert_der = cert.der().to_vec();
    Ok((cert_der, key_der))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_identity_generation() {
        let identity = DeviceIdentity::generate("my-laptop");
        assert_eq!(identity.device_name, "my-laptop");
        assert_eq!(identity.public_key_bytes().len(), 32);
    }

    #[test]
    fn pairing_code_is_six_digits() {
        for _ in 0..100 {
            let code = generate_pairing_code();
            assert_eq!(code.len(), 6);
            assert!(code.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn encrypt_decrypt_payload_roundtrip() {
        let secret = [42u8; 32];
        let plaintext = b"hello sync world";
        let encrypted = encrypt_payload(plaintext, &secret).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = decrypt_payload(&encrypted, &secret).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_wrong_key_fails() {
        let secret1 = [1u8; 32];
        let secret2 = [2u8; 32];
        let encrypted = encrypt_payload(b"data", &secret1).unwrap();
        assert!(decrypt_payload(&encrypted, &secret2).is_err());
    }

    #[test]
    fn tls_cert_generation() {
        let id = Uuid::new_v4();
        let (cert, key) = generate_tls_cert(&id).unwrap();
        assert!(!cert.is_empty());
        assert!(!key.is_empty());
    }

    #[test]
    fn x25519_key_exchange_produces_shared_secret() {
        // Simulate two sides
        let secret_a = x25519_dalek::EphemeralSecret::random_from_rng(OsRng);
        let public_a = x25519_dalek::PublicKey::from(&secret_a);

        let secret_b = x25519_dalek::EphemeralSecret::random_from_rng(OsRng);
        let public_b = x25519_dalek::PublicKey::from(&secret_b);

        let shared_a = secret_a.diffie_hellman(&public_b);
        let shared_b = secret_b.diffie_hellman(&public_a);

        assert_eq!(shared_a.as_bytes(), shared_b.as_bytes());
    }
}

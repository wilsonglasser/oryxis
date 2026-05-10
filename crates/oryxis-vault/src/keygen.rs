use ssh_key::{Algorithm, HashAlg, PrivateKey};

use oryxis_core::models::key::{KeyAlgorithm, SshKey};

use crate::store::VaultError;

/// Generated key pair — private PEM + SshKey model.
#[derive(Debug)]
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

/// Try to parse a traditional PEM key (PKCS#1, PKCS#8, SEC1) and convert to ssh_key::PrivateKey.
fn parse_ec_p256(sk: p256::SecretKey) -> PrivateKey {
    let public = sk.public_key().into();
    let private = ssh_key::private::EcdsaPrivateKey::<32>::from(sk);
    PrivateKey::from(ssh_key::private::EcdsaKeypair::NistP256 { public, private })
}

fn parse_ec_p384(sk: p384::SecretKey) -> PrivateKey {
    let public = sk.public_key().into();
    let private = ssh_key::private::EcdsaPrivateKey::<48>::from(sk);
    PrivateKey::from(ssh_key::private::EcdsaKeypair::NistP384 { public, private })
}

/// Try to parse a traditional PEM key (PKCS#1, PKCS#8, SEC1) and convert to ssh_key::PrivateKey.
fn parse_traditional_pem(pem: &str) -> Result<PrivateKey, VaultError> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;

    // PKCS#1 RSA: "BEGIN RSA PRIVATE KEY"
    if pem.contains("BEGIN RSA PRIVATE KEY") {
        let rsa_key = rsa::RsaPrivateKey::from_pkcs1_pem(pem)
            .map_err(|e| VaultError::Crypto(format!("PKCS#1 parse error: {}", e)))?;
        let keypair = ssh_key::private::RsaKeypair::try_from(rsa_key)
            .map_err(|e| VaultError::Crypto(format!("RSA key conversion error: {}", e)))?;
        return Ok(PrivateKey::from(keypair));
    }

    // SEC1 EC: "BEGIN EC PRIVATE KEY"
    if pem.contains("BEGIN EC PRIVATE KEY") {
        if let Ok(sk) = p256::SecretKey::from_sec1_pem(pem) {
            return Ok(parse_ec_p256(sk));
        }
        if let Ok(sk) = p384::SecretKey::from_sec1_pem(pem) {
            return Ok(parse_ec_p384(sk));
        }
        return Err(VaultError::Crypto("Unsupported EC curve (only P-256 and P-384 are supported)".into()));
    }

    // PKCS#8: "BEGIN PRIVATE KEY" (can contain RSA, EC, or Ed25519)
    if pem.contains("BEGIN PRIVATE KEY") {
        if let Ok(rsa_key) = rsa::RsaPrivateKey::from_pkcs8_pem(pem) {
            let keypair = ssh_key::private::RsaKeypair::try_from(rsa_key)
                .map_err(|e| VaultError::Crypto(format!("RSA key conversion error: {}", e)))?;
            return Ok(PrivateKey::from(keypair));
        }
        if let Ok(sk) = p256::SecretKey::from_pkcs8_pem(pem) {
            return Ok(parse_ec_p256(sk));
        }
        if let Ok(sk) = p384::SecretKey::from_pkcs8_pem(pem) {
            return Ok(parse_ec_p384(sk));
        }
        return Err(VaultError::Crypto("Unsupported PKCS#8 key type (supported: RSA, ECDSA P-256/P-384)".into()));
    }

    Err(VaultError::Crypto("Unrecognized PEM format".into()))
}

/// Cheap structural check: returns `true` if the PEM looks like an
/// encrypted key. Used by the UI to surface the passphrase field as
/// soon as the user picks the file, without waiting for a Save click.
/// Conservative — false negatives are fine (Save will still surface
/// `KeyNeedsPassphrase`); false positives would prompt unnecessarily.
pub fn is_key_encrypted(private_pem: &str) -> bool {
    let stripped = private_pem.strip_prefix('\u{FEFF}').unwrap_or(private_pem);
    let normalized = stripped.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();

    // PKCS#8 / OpenSSL traditional PEM with explicit ENCRYPTED header.
    if trimmed.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        return true;
    }
    if trimmed.contains("ENCRYPTED")
        && (trimmed.contains("BEGIN RSA PRIVATE KEY")
            || trimmed.contains("BEGIN EC PRIVATE KEY"))
    {
        return true;
    }
    // OpenSSH format — parse cheaply just to read the cipher field.
    if trimmed.contains("BEGIN OPENSSH PRIVATE KEY")
        && let Ok(parsed) = ssh_key::PrivateKey::from_openssh(trimmed) {
            return parsed.is_encrypted();
        }
    false
}

/// Import an SSH key from any supported format:
/// - OpenSSH (`BEGIN OPENSSH PRIVATE KEY`) — supports passphrase-encrypted keys
/// - PKCS#1 RSA (`BEGIN RSA PRIVATE KEY`)
/// - PKCS#8 (`BEGIN PRIVATE KEY`) — RSA, ECDSA P-256/P-384, Ed25519
/// - SEC1 EC (`BEGIN EC PRIVATE KEY`) — P-256, P-384
///
/// `passphrase` is consulted only when the key is detected as encrypted.
/// Returns `KeyNeedsPassphrase` if the key is encrypted and `passphrase` is
/// `None`/empty, or `WrongKeyPassphrase` if decryption fails. The decrypted
/// key is stored unencrypted (the vault's master key already protects it).
pub fn import_key(
    label: &str,
    private_pem: &str,
    passphrase: Option<&str>,
) -> Result<GeneratedKey, VaultError> {
    // Strip a UTF-8 BOM if present — Windows editors (Notepad, some
    // PowerShell redirects) write keys with a BOM and PEM parsers see
    // the leading bytes as junk before `-----BEGIN`. Then normalize
    // line endings (CRLF → LF) so Base64 decoding doesn't trip on \r.
    let stripped = private_pem.strip_prefix('\u{FEFF}').unwrap_or(private_pem);
    let normalized = stripped.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();

    // Traditional PEM encrypted with PKCS#5/PKCS#8 carry an explicit
    // header. We don't support that path yet; surface a clear error so
    // the user knows to convert the key.
    let traditional_encrypted = trimmed.contains("BEGIN ENCRYPTED PRIVATE KEY")
        || (trimmed.contains("ENCRYPTED")
            && (trimmed.contains("BEGIN RSA PRIVATE KEY")
                || trimmed.contains("BEGIN EC PRIVATE KEY")));
    if traditional_encrypted && !trimmed.contains("BEGIN OPENSSH PRIVATE KEY") {
        return Err(VaultError::Crypto(
            "Encrypted PKCS#1/PKCS#8 keys are not supported. \
             Remove the passphrase first (e.g. ssh-keygen -p -f <file> -N '')."
                .into(),
        ));
    }

    let private_key = if trimmed.contains("BEGIN OPENSSH PRIVATE KEY") {
        let parsed = PrivateKey::from_openssh(trimmed)
            .map_err(|e| VaultError::Crypto(format!("Failed to parse OpenSSH key: {}", e)))?;
        if parsed.is_encrypted() {
            let pass = passphrase.unwrap_or("");
            if pass.is_empty() {
                return Err(VaultError::KeyNeedsPassphrase);
            }
            parsed
                .decrypt(pass.as_bytes())
                .map_err(|_| VaultError::WrongKeyPassphrase)?
        } else {
            parsed
        }
    } else {
        parse_traditional_pem(trimmed)?
    };

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

    // Re-encode as OpenSSH format for consistent storage
    let stored_pem = private_key
        .to_openssh(ssh_key::LineEnding::LF)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| trimmed.to_string());

    let mut key = SshKey::new(label, algorithm);
    key.fingerprint = fingerprint;
    key.public_key = public_key_str;

    Ok(GeneratedKey {
        key,
        private_pem: stored_pem,
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
        let imported = import_key("imported", &generated.private_pem, None).unwrap();
        assert_eq!(imported.key.fingerprint, generated.key.fingerprint);
        assert_eq!(imported.key.algorithm, KeyAlgorithm::Ed25519);
        assert_eq!(imported.key.public_key, generated.key.public_key);
    }

    #[test]
    fn import_invalid_pem_fails() {
        let result = import_key("bad", "this is not a key", None);
        assert!(result.is_err());
    }

    #[test]
    fn import_strips_utf8_bom() {
        let generated = generate_ed25519("bom-test").unwrap();
        let with_bom = format!("\u{FEFF}{}", generated.private_pem);
        let imported = import_key("bom", &with_bom, None).unwrap();
        assert_eq!(imported.key.fingerprint, generated.key.fingerprint);
    }

    #[test]
    fn import_handles_crlf() {
        let generated = generate_ed25519("crlf-test").unwrap();
        let crlf = generated.private_pem.replace('\n', "\r\n");
        let imported = import_key("crlf", &crlf, None).unwrap();
        assert_eq!(imported.key.fingerprint, generated.key.fingerprint);
    }

    #[test]
    fn import_with_whitespace() {
        let generated = generate_ed25519("ws-test").unwrap();
        let padded = format!("\n  {}  \n", generated.private_pem);
        let imported = import_key("trimmed", &padded, None).unwrap();
        assert_eq!(imported.key.fingerprint, generated.key.fingerprint);
    }

    #[test]
    fn import_encrypted_openssh_requires_passphrase() {
        // Build an encrypted OpenSSH key via the ssh-key crate directly.
        use ssh_key::{Algorithm, PrivateKey};
        let mut rng = rand::thread_rng();
        let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
        let encrypted = key.encrypt(&mut rng, b"hunter2").unwrap();
        let pem = encrypted.to_openssh(ssh_key::LineEnding::LF).unwrap().to_string();

        // No passphrase → KeyNeedsPassphrase.
        let err = import_key("enc", &pem, None).unwrap_err();
        assert!(matches!(err, VaultError::KeyNeedsPassphrase));

        // Empty passphrase counted as missing.
        let err = import_key("enc", &pem, Some("")).unwrap_err();
        assert!(matches!(err, VaultError::KeyNeedsPassphrase));

        // Wrong passphrase → WrongKeyPassphrase.
        let err = import_key("enc", &pem, Some("nope")).unwrap_err();
        assert!(matches!(err, VaultError::WrongKeyPassphrase));

        // Correct passphrase succeeds and the stored PEM is unencrypted.
        let imported = import_key("enc", &pem, Some("hunter2")).unwrap();
        assert!(imported.private_pem.contains("BEGIN OPENSSH PRIVATE KEY"));
        let reparsed = PrivateKey::from_openssh(&imported.private_pem).unwrap();
        assert!(!reparsed.is_encrypted());
    }
}

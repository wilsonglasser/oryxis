use ssh_key::{Algorithm, HashAlg, PrivateKey};

use oryxis_core::models::key::{KeyAlgorithm, SshKey};

use crate::store::VaultError;

mod pem;
mod ppk;

pub use pem::is_traditional_encrypted;

/// Generated key pair, private PEM + SshKey model.
#[derive(Debug)]
pub struct GeneratedKey {
    pub key: SshKey,
    pub private_pem: String,
}

/// Generate an Ed25519 SSH key pair.
pub fn generate_ed25519(label: &str) -> Result<GeneratedKey, VaultError> {
    let mut rng = rand::rng();
    let private_key = PrivateKey::random(&mut rng, Algorithm::Ed25519)
        .map_err(|e| VaultError::Crypto(format!("Key generation failed: {}", e)))?;

    finalize(label, private_key)
}

/// Cheap structural check: returns `true` if the key file looks
/// encrypted. Used by the UI to surface the passphrase field as soon
/// as the user picks the file, without waiting for a Save click.
/// Conservative, false negatives are fine (Save will still surface
/// `KeyNeedsPassphrase`); false positives would prompt unnecessarily.
pub fn is_key_encrypted(private_pem: &str) -> bool {
    let stripped = private_pem.strip_prefix('\u{FEFF}').unwrap_or(private_pem);
    let normalized = stripped.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();

    if ppk::is_ppk(trimmed) {
        return ppk::is_encrypted(trimmed);
    }

    if is_traditional_encrypted(trimmed) {
        return true;
    }

    // OpenSSH format, parse cheaply just to read the cipher field.
    if trimmed.contains("BEGIN OPENSSH PRIVATE KEY")
        && let Ok(parsed) = ssh_key::PrivateKey::from_openssh(trimmed) {
            return parsed.is_encrypted();
        }
    false
}

/// Import an SSH key from any supported format:
/// - OpenSSH (`BEGIN OPENSSH PRIVATE KEY`), supports passphrase-encrypted keys
/// - PuTTY PPK v2 / v3 (`PuTTY-User-Key-File-2/3:`), supports passphrase-encrypted keys
/// - PKCS#1 RSA (`BEGIN RSA PRIVATE KEY`)
/// - PKCS#8 (`BEGIN PRIVATE KEY`), RSA, ECDSA P-256/P-384/P-521, Ed25519
/// - Encrypted PKCS#8 (`BEGIN ENCRYPTED PRIVATE KEY`), RSA, ECDSA P-256/P-384/P-521
/// - SEC1 EC (`BEGIN EC PRIVATE KEY`), P-256, P-384, P-521
/// - OpenSSL-legacy traditional PEM (`Proc-Type: 4,ENCRYPTED` + `DEK-Info`),
///   PKCS#1 RSA and SEC1 EC, decrypted with EVP_BytesToKey + AES/3DES-CBC
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
    // Strip a UTF-8 BOM if present, Windows editors (Notepad, some
    // PowerShell redirects) write keys with a BOM and PEM parsers see
    // the leading bytes as junk before `-----BEGIN`. Then normalize
    // line endings (CRLF → LF) so Base64 decoding doesn't trip on \r.
    let stripped = private_pem.strip_prefix('\u{FEFF}').unwrap_or(private_pem);
    let normalized = stripped.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();

    let private_key = if ppk::is_ppk(trimmed) {
        ppk::parse(trimmed, passphrase)?
    } else if trimmed.contains("BEGIN OPENSSH PRIVATE KEY") {
        // Try the input as-is first (preserves OpenSSH's native 70-char
        // wrapping). If that fails with a Base64 error, retry after
        // rewrapping at 64 chars: PuTTYgen's "Export OpenSSH key (force
        // new file format)" emits a 76-char body that `ssh-key`'s PEM
        // decoder rejects with a misleading "invalid Base64 encoding".
        let parsed = match PrivateKey::from_openssh(trimmed) {
            Ok(k) => k,
            Err(first_err) => {
                let rewrapped = pem::rewrap_pem_body_at(trimmed, 70);
                PrivateKey::from_openssh(&rewrapped).map_err(|_| {
                    VaultError::Crypto(format!("Failed to parse OpenSSH key: {}", first_err))
                })?
            }
        };
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
        pem::parse(trimmed, passphrase)?
    };

    finalize(label, private_key)
}

/// Map an `ssh_key::PrivateKey` to the `KeyAlgorithm` enum and an
/// OpenSSH-encoded PEM, then build the resulting `GeneratedKey`.
/// Returns an error for algorithms we don't claim to support, rather
/// than silently mislabeling them.
fn finalize(label: &str, private_key: PrivateKey) -> Result<GeneratedKey, VaultError> {
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
            ssh_key::EcdsaCurve::NistP521 => KeyAlgorithm::EcdsaP521,
        },
        other => {
            return Err(VaultError::UnsupportedKeyKind(other.as_str().to_string()));
        }
    };

    let private_pem = private_key
        .to_openssh(ssh_key::LineEnding::LF)
        .map_err(|e| VaultError::Crypto(format!("Private key encoding failed: {}", e)))?
        .to_string();

    let mut key = SshKey::new(label, algorithm);
    key.fingerprint = fingerprint;
    key.public_key = public_key_str;

    Ok(GeneratedKey { key, private_pem })
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
    fn import_encrypted_openssh_round_trips_with_passphrase() {
        // Build an encrypted OpenSSH key in-process (no embedded secret, no
        // external `ssh-keygen`): generate one, then encrypt it. Guards the
        // passphrase path the host-key import UI depends on.
        let src = generate_ed25519("enc-src").unwrap();
        let key = PrivateKey::from_openssh(&src.private_pem).unwrap();
        let mut rng = rand::rng();
        let enc_pem = key
            .encrypt(&mut rng, "pw123")
            .unwrap()
            .to_openssh(ssh_key::LineEnding::LF)
            .unwrap()
            .to_string();

        // No passphrase: the UI is told to prompt for one.
        assert!(matches!(
            import_key("k", &enc_pem, None),
            Err(VaultError::KeyNeedsPassphrase)
        ));
        // Wrong passphrase: a distinct, surfaceable error.
        assert!(matches!(
            import_key("k", &enc_pem, Some("nope")),
            Err(VaultError::WrongKeyPassphrase)
        ));
        // Correct passphrase: imported, same key.
        let imported = import_key("k", &enc_pem, Some("pw123")).unwrap();
        assert_eq!(imported.key.fingerprint, src.key.fingerprint);
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
    fn import_openssh_with_76_char_lines() {
        // Mimic PuTTYgen's "Export OpenSSH key (force new file format)"
        // output: same base64 body, but wrapped at 76 chars instead of
        // RFC 7468's 64. Used to fail with "invalid Base64 encoding".
        let generated = generate_ed25519("force-new-format").unwrap();
        let begin = generated.private_pem.find('\n').unwrap() + 1;
        let end_tag = "-----END";
        let end = generated.private_pem.find(end_tag).unwrap();
        let body: String = generated.private_pem[begin..end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let mut rewrapped = String::new();
        rewrapped.push_str(&generated.private_pem[..begin]);
        for chunk in body.as_bytes().chunks(76) {
            rewrapped.push_str(std::str::from_utf8(chunk).unwrap());
            rewrapped.push('\n');
        }
        rewrapped.push_str(&generated.private_pem[end..]);

        let imported = import_key("force-new-format", &rewrapped, None).unwrap();
        assert_eq!(imported.key.fingerprint, generated.key.fingerprint);
    }

    #[test]
    fn import_encrypted_openssh_requires_passphrase() {
        use ssh_key::{Algorithm, PrivateKey};
        let mut rng = rand::rng();
        let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
        let encrypted = key.encrypt(&mut rng, b"hunter2").unwrap();
        let pem = encrypted.to_openssh(ssh_key::LineEnding::LF).unwrap().to_string();

        let err = import_key("enc", &pem, None).unwrap_err();
        assert!(matches!(err, VaultError::KeyNeedsPassphrase));

        let err = import_key("enc", &pem, Some("")).unwrap_err();
        assert!(matches!(err, VaultError::KeyNeedsPassphrase));

        let err = import_key("enc", &pem, Some("nope")).unwrap_err();
        assert!(matches!(err, VaultError::WrongKeyPassphrase));

        let imported = import_key("enc", &pem, Some("hunter2")).unwrap();
        assert!(imported.private_pem.contains("BEGIN OPENSSH PRIVATE KEY"));
        let reparsed = PrivateKey::from_openssh(&imported.private_pem).unwrap();
        assert!(!reparsed.is_encrypted());
    }
}

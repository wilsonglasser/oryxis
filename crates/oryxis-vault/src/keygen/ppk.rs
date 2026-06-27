//! PuTTY `.ppk` private key parser. Supports v2 and v3 envelopes.
//!
//! Format reference: https://the.earth.li/~sgtatham/putty/0.81/htmldoc/AppendixC.html
//!
//! Layout of a v2 / v3 file (one header per line, LF or CRLF terminated):
//!
//! ```text
//! PuTTY-User-Key-File-<2|3>: <key-type>
//! Encryption: <none|aes256-cbc>
//! Comment: <user comment>
//! Public-Lines: <N>
//! <base64 public blob, 64 chars per line>
//! # v3 only, if Encryption != none:
//! Key-Derivation: <Argon2id|Argon2i|Argon2d>
//! Argon2-Memory: <kibibytes>
//! Argon2-Passes: <iterations>
//! Argon2-Parallelism: <lanes>
//! Argon2-Salt: <hex>
//! Private-Lines: <M>
//! <base64 private blob, 64 chars per line>
//! Private-MAC: <hex>
//! ```
//!
//! Inner public/private blob shape is SSH wire format (4-byte big-endian
//! lengths, two's complement mpints) for the chosen key type:
//!
//! - `ssh-rsa`
//!   public:  string "ssh-rsa", mpint e, mpint n
//!   private: mpint d, mpint p, mpint q, mpint iqmp
//! - `ssh-ed25519`
//!   public:  string "ssh-ed25519", string pubkey(32)
//!   private: string seed(32)
//! - `ecdsa-sha2-nistp256/384/521`
//!   public:  string "ecdsa-sha2-nistpXXX", string curve, string Q
//!   private: mpint d

use base64::Engine;
use ssh_key::{Mpint, PrivateKey};
use ssh_key::private::{Ed25519Keypair, RsaKeypair, RsaPrivateKey};
use ssh_key::public::RsaPublicKey;
use subtle::ConstantTimeEq;

use crate::store::VaultError;

const HEADER_V2: &str = "PuTTY-User-Key-File-2:";
const HEADER_V3: &str = "PuTTY-User-Key-File-3:";

/// Does the input look like a PuTTY `.ppk` file?
pub fn is_ppk(input: &str) -> bool {
    input.starts_with(HEADER_V2) || input.starts_with(HEADER_V3)
}

/// Cheap structural check used by the UI's passphrase prompt.
pub fn is_encrypted(input: &str) -> bool {
    if !is_ppk(input) {
        return false;
    }
    for line in input.lines() {
        if let Some(value) = line.strip_prefix("Encryption:") {
            return value.trim() != "none";
        }
    }
    false
}

#[derive(Debug)]
struct Headers<'a> {
    version: u8,
    key_type: &'a str,
    encryption: &'a str,
    comment: &'a str,
    public_blob: Vec<u8>,
    private_blob: Vec<u8>,
    mac: Vec<u8>,
    // v3 only.
    kdf: Option<&'a str>,
    argon_memory: Option<u32>,
    argon_passes: Option<u32>,
    argon_parallelism: Option<u32>,
    argon_salt: Option<Vec<u8>>,
}

/// Parse a `.ppk` blob into an `ssh_key::PrivateKey`, decrypting with
/// `passphrase` if `Encryption` is not `none`.
pub fn parse(input: &str, passphrase: Option<&str>) -> Result<PrivateKey, VaultError> {
    let headers = parse_headers(input)?;

    let encrypted = headers.encryption != "none";
    let pass = passphrase.unwrap_or("");
    if encrypted && pass.is_empty() {
        return Err(VaultError::KeyNeedsPassphrase);
    }
    if !encrypted && headers.encryption != "none" {
        return Err(VaultError::Crypto(format!(
            "Unsupported PPK encryption: {}",
            headers.encryption
        )));
    }
    if encrypted && headers.encryption != "aes256-cbc" {
        return Err(VaultError::Crypto(format!(
            "Unsupported PPK encryption: {} (only aes256-cbc is supported)",
            headers.encryption
        )));
    }

    // Reject unsupported key types before doing any KDF / MAC work, so
    // the user sees the actionable "DSA is deprecated" message instead
    // of a generic integrity failure.
    match headers.key_type {
        "ssh-rsa" | "ssh-ed25519"
        | "ecdsa-sha2-nistp256" | "ecdsa-sha2-nistp384" | "ecdsa-sha2-nistp521" => {}
        "ssh-dss" => {
            return Err(VaultError::UnsupportedKeyKind("ssh-dss".into()));
        }
        other => {
            return Err(VaultError::UnsupportedKeyKind(format!("ppk:{}", other)));
        }
    }

    let (cipher_key, iv, mac_key) = derive_keys(&headers, pass)?;

    let mut private_plain = headers.private_blob.clone();
    if encrypted {
        if private_plain.len() % 16 != 0 {
            return Err(VaultError::Crypto(
                "PPK private body length is not a multiple of the AES block size".into(),
            ));
        }
        aes256_cbc_decrypt(&cipher_key, &iv, &mut private_plain)?;
    }

    verify_mac(&headers, &mac_key, &private_plain, pass)?;

    let private_key = match headers.key_type {
        "ssh-rsa" => build_rsa(&headers.public_blob, &private_plain)?,
        "ssh-ed25519" => build_ed25519(&headers.public_blob, &private_plain)?,
        "ecdsa-sha2-nistp256" | "ecdsa-sha2-nistp384" | "ecdsa-sha2-nistp521" => {
            build_ecdsa(headers.key_type, &headers.public_blob, &private_plain)?
        }
        _ => unreachable!(),
    };

    Ok(private_key)
}

fn parse_headers(input: &str) -> Result<Headers<'_>, VaultError> {
    let mut lines = input.lines().peekable();

    let first = lines.next().ok_or_else(|| crypto("empty PPK file"))?;
    let (version, key_type) = if let Some(rest) = first.strip_prefix(HEADER_V2) {
        (2u8, rest.trim())
    } else if let Some(rest) = first.strip_prefix(HEADER_V3) {
        (3u8, rest.trim())
    } else {
        return Err(crypto("missing PPK header"));
    };

    let mut encryption: &str = "";
    let mut comment: &str = "";
    let mut public_blob_b64 = String::new();
    let mut private_blob_b64 = String::new();
    let mut mac: Vec<u8> = Vec::new();
    let mut kdf: Option<&str> = None;
    let mut argon_memory: Option<u32> = None;
    let mut argon_passes: Option<u32> = None;
    let mut argon_parallelism: Option<u32> = None;
    let mut argon_salt: Option<Vec<u8>> = None;

    while let Some(line) = lines.next() {
        let (key, value) = match line.split_once(':') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => continue,
        };
        match key {
            "Encryption" => encryption = value,
            "Comment" => comment = value,
            "Public-Lines" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| crypto("invalid Public-Lines"))?;
                for _ in 0..n {
                    let l = lines.next().ok_or_else(|| crypto("truncated public block"))?;
                    public_blob_b64.push_str(l.trim());
                }
            }
            "Private-Lines" => {
                let n: usize = value
                    .parse()
                    .map_err(|_| crypto("invalid Private-Lines"))?;
                for _ in 0..n {
                    let l = lines.next().ok_or_else(|| crypto("truncated private block"))?;
                    private_blob_b64.push_str(l.trim());
                }
            }
            "Private-MAC" => {
                mac = hex_decode(value)?;
            }
            "Private-Hash" => {
                // Older draft of the v3 spec used `Private-Hash`; the
                // shipped format settled on `Private-MAC` for everything.
                // Accept either marker name; verify_mac dispatches based
                // on version + encryption.
                mac = hex_decode(value)?;
            }
            "Key-Derivation" => kdf = Some(value),
            "Argon2-Memory" => {
                argon_memory =
                    Some(value.parse().map_err(|_| crypto("invalid Argon2-Memory"))?);
            }
            "Argon2-Passes" => {
                argon_passes =
                    Some(value.parse().map_err(|_| crypto("invalid Argon2-Passes"))?);
            }
            "Argon2-Parallelism" => {
                argon_parallelism = Some(
                    value
                        .parse()
                        .map_err(|_| crypto("invalid Argon2-Parallelism"))?,
                );
            }
            "Argon2-Salt" => argon_salt = Some(hex_decode(value)?),
            _ => {}
        }
    }

    let public_blob = base64::engine::general_purpose::STANDARD
        .decode(public_blob_b64)
        .map_err(|e| crypto(format!("public blob base64: {}", e)))?;
    let private_blob = base64::engine::general_purpose::STANDARD
        .decode(private_blob_b64)
        .map_err(|e| crypto(format!("private blob base64: {}", e)))?;

    Ok(Headers {
        version,
        key_type,
        encryption,
        comment,
        public_blob,
        private_blob,
        mac,
        kdf,
        argon_memory,
        argon_passes,
        argon_parallelism,
        argon_salt,
    })
}

/// Derive (cipher_key, iv, mac_key) per the PPK spec. For v2 this is
/// SHA-1-based; for v3 it's Argon2 with the parameters baked into the
/// file. For unencrypted v2 the cipher_key/iv are unused but we return
/// zero buffers to keep the signature uniform.
type DerivedKeys = (Vec<u8>, [u8; 16], Vec<u8>);

fn derive_keys(headers: &Headers<'_>, pass: &str) -> Result<DerivedKeys, VaultError> {
    let encrypted = headers.encryption != "none";
    match headers.version {
        2 => {
            // v2 cipher key: SHA-1("\0\0\0\0" || pass) || SHA-1("\0\0\0\1" || pass), take 32 bytes.
            // v2 IV: 16 zero bytes.
            // v2 MAC key: SHA-1("putty-private-key-file-mac-key" || pass).
            let cipher_key = if encrypted {
                use sha1::{Digest, Sha1};
                let mut k = Vec::with_capacity(32);
                for counter in 0u32..2 {
                    let mut h = Sha1::new();
                    h.update(counter.to_be_bytes());
                    h.update(pass.as_bytes());
                    let digest = h.finalize();
                    k.extend_from_slice(&digest);
                }
                k.truncate(32);
                k
            } else {
                vec![0u8; 32]
            };
            let iv = [0u8; 16];

            let mac_key = {
                use sha1::{Digest, Sha1};
                let mut h = Sha1::new();
                h.update(b"putty-private-key-file-mac-key");
                if encrypted {
                    h.update(pass.as_bytes());
                }
                h.finalize().to_vec()
            };

            Ok((cipher_key, iv, mac_key))
        }
        3 => {
            // v3 unencrypted: PuTTY's spec says HMAC-SHA-256 with an
            // empty key. HMAC pads sub-blocksize keys to the block size
            // with zeros, so empty / `\0` / all-zeros all produce the
            // same MAC. Use empty to mirror PuTTY's canonical form.
            if !encrypted {
                return Ok((vec![0u8; 32], [0u8; 16], Vec::new()));
            }

            let kdf = headers.kdf.ok_or_else(|| crypto("missing Key-Derivation"))?;
            let memory = headers
                .argon_memory
                .ok_or_else(|| crypto("missing Argon2-Memory"))?;
            let passes = headers
                .argon_passes
                .ok_or_else(|| crypto("missing Argon2-Passes"))?;
            let parallelism = headers
                .argon_parallelism
                .ok_or_else(|| crypto("missing Argon2-Parallelism"))?;
            let salt = headers
                .argon_salt
                .as_ref()
                .ok_or_else(|| crypto("missing Argon2-Salt"))?;

            let algorithm = match kdf {
                "Argon2id" => argon2::Algorithm::Argon2id,
                "Argon2i" => argon2::Algorithm::Argon2i,
                "Argon2d" => argon2::Algorithm::Argon2d,
                other => {
                    return Err(crypto(format!("unsupported Key-Derivation: {}", other)));
                }
            };
            let params = argon2::Params::new(memory, passes, parallelism, Some(80))
                .map_err(|e| crypto(format!("invalid Argon2 params: {}", e)))?;
            let argon = argon2::Argon2::new(algorithm, argon2::Version::V0x13, params);
            let mut out = vec![0u8; 80];
            argon
                .hash_password_into(pass.as_bytes(), salt, &mut out)
                .map_err(|e| crypto(format!("Argon2 derivation failed: {}", e)))?;
            let cipher_key = out[..32].to_vec();
            let mut iv = [0u8; 16];
            iv.copy_from_slice(&out[32..48]);
            let mac_key = out[48..80].to_vec();
            Ok((cipher_key, iv, mac_key))
        }
        _ => Err(crypto("unknown PPK version")),
    }
}

fn verify_mac(
    headers: &Headers<'_>,
    mac_key: &[u8],
    private_plain: &[u8],
    pass: &str,
) -> Result<(), VaultError> {
    // MAC content: ssh-string(key-type) || ssh-string(encryption) ||
    //              ssh-string(comment)  || ssh-string(public)      ||
    //              ssh-string(private-plaintext)
    let mut content: Vec<u8> = Vec::new();
    append_string(&mut content, headers.key_type.as_bytes());
    append_string(&mut content, headers.encryption.as_bytes());
    append_string(&mut content, headers.comment.as_bytes());
    append_string(&mut content, &headers.public_blob);
    append_string(&mut content, private_plain);

    let computed: Vec<u8> = match headers.version {
        2 => {
            use hmac::{Hmac, Mac};
            type HmacSha1 = Hmac<sha1::Sha1>;
            let mut mac = HmacSha1::new_from_slice(mac_key)
                .map_err(|e| crypto(format!("HMAC init failed: {}", e)))?;
            mac.update(&content);
            mac.finalize().into_bytes().to_vec()
        }
        3 => {
            // v3 always HMAC-SHA-256. For unencrypted files the key is
            // an empty string (per PuTTY spec); HMAC's K-padding then
            // makes it deterministic without revealing anything secret.
            use hmac::{Hmac, Mac};
            type HmacSha256 = Hmac<sha2::Sha256>;
            let mut mac = HmacSha256::new_from_slice(mac_key)
                .map_err(|e| crypto(format!("HMAC init failed: {}", e)))?;
            mac.update(&content);
            mac.finalize().into_bytes().to_vec()
        }
        _ => return Err(crypto("unknown PPK version")),
    };

    if computed.ct_eq(&headers.mac).into() {
        Ok(())
    } else if headers.encryption == "none" {
        Err(crypto("PPK integrity check failed (tampered or corrupt file)"))
    } else {
        // For encrypted files a MAC mismatch is overwhelmingly "wrong
        // passphrase". Surface that distinct variant so the UI can keep
        // the passphrase field visible.
        let _ = pass;
        Err(VaultError::WrongKeyPassphrase)
    }
}

fn aes256_cbc_decrypt(key: &[u8], iv: &[u8; 16], buf: &mut [u8]) -> Result<(), VaultError> {
    use aes::cipher::{
        block_padding::NoPadding, BlockDecryptMut, KeyIvInit,
    };
    type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
    let dec = Aes256CbcDec::new_from_slices(key, iv)
        .map_err(|e| crypto(format!("AES-CBC init failed: {}", e)))?;
    // PPK does NOT use PKCS#7 padding (length is already a multiple of
    // the block size). `decrypt_padded_mut::<NoPadding>` walks the
    // whole buffer in order so CBC chaining is preserved.
    dec.decrypt_padded_mut::<NoPadding>(buf)
        .map_err(|e| crypto(format!("AES-CBC decrypt failed: {}", e)))?;
    Ok(())
}

fn append_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn read_string<'a>(buf: &'a [u8], cur: &mut usize) -> Result<&'a [u8], VaultError> {
    if buf.len() < *cur + 4 {
        return Err(crypto("truncated PPK string"));
    }
    let len = u32::from_be_bytes(buf[*cur..*cur + 4].try_into().unwrap()) as usize;
    *cur += 4;
    if buf.len() < *cur + len {
        return Err(crypto("truncated PPK string body"));
    }
    let out = &buf[*cur..*cur + len];
    *cur += len;
    Ok(out)
}

fn read_mpint(buf: &[u8], cur: &mut usize) -> Result<Mpint, VaultError> {
    let bytes = read_string(buf, cur)?;
    Mpint::from_bytes(bytes).map_err(|e| crypto(format!("invalid mpint: {}", e)))
}

fn build_rsa(public_blob: &[u8], private_plain: &[u8]) -> Result<PrivateKey, VaultError> {
    let mut cur = 0usize;
    let label = read_string(public_blob, &mut cur)?;
    if label != b"ssh-rsa" {
        return Err(crypto("PPK public label mismatch"));
    }
    let e = read_mpint(public_blob, &mut cur)?;
    let n = read_mpint(public_blob, &mut cur)?;

    cur = 0;
    let d = read_mpint(private_plain, &mut cur)?;
    let p = read_mpint(private_plain, &mut cur)?;
    let q = read_mpint(private_plain, &mut cur)?;
    let iqmp = read_mpint(private_plain, &mut cur)?;

    let public = RsaPublicKey::new(e, n).map_err(|e| crypto(format!("invalid RSA public: {}", e)))?;
    let private =
        RsaPrivateKey::new(d, iqmp, p, q).map_err(|e| crypto(format!("invalid RSA private: {}", e)))?;
    let keypair =
        RsaKeypair::new(public, private).map_err(|e| crypto(format!("invalid RSA keypair: {}", e)))?;
    Ok(PrivateKey::from(keypair))
}

fn build_ed25519(public_blob: &[u8], private_plain: &[u8]) -> Result<PrivateKey, VaultError> {
    let mut cur = 0usize;
    let label = read_string(public_blob, &mut cur)?;
    if label != b"ssh-ed25519" {
        return Err(crypto("PPK public label mismatch"));
    }
    let _pub_bytes = read_string(public_blob, &mut cur)?;

    cur = 0;
    let seed_bytes = read_string(private_plain, &mut cur)?;
    if seed_bytes.len() != 32 {
        return Err(crypto("Ed25519 seed length invalid"));
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(seed_bytes);
    Ok(PrivateKey::from(Ed25519Keypair::from_seed(&seed)))
}

fn build_ecdsa(
    key_type: &str,
    public_blob: &[u8],
    private_plain: &[u8],
) -> Result<PrivateKey, VaultError> {
    let mut cur = 0usize;
    let label = read_string(public_blob, &mut cur)?;
    if label != key_type.as_bytes() {
        return Err(crypto("PPK public label mismatch"));
    }
    // We discard the public components from the PPK file; `p256` /
    // `p384` `SecretKey::public_key()` regenerates the matching point
    // from the private scalar, so the file can't lie about it.
    let _curve_name = read_string(public_blob, &mut cur)?;
    let _q = read_string(public_blob, &mut cur)?;

    cur = 0;
    let d_mpint = read_mpint(private_plain, &mut cur)?;
    let d_bytes = d_mpint
        .as_positive_bytes()
        .ok_or_else(|| crypto("ECDSA private scalar is negative"))?;

    match key_type {
        "ecdsa-sha2-nistp256" => {
            let padded = pad_to::<32>(d_bytes)?;
            let sk = p256::SecretKey::from_slice(&padded)
                .map_err(|e| crypto(format!("ECDSA P-256 scalar invalid: {}", e)))?;
            let public = sk.public_key().into();
            let private = ssh_key::private::EcdsaPrivateKey::<32>::from(sk);
            Ok(PrivateKey::from(ssh_key::private::EcdsaKeypair::NistP256 {
                public,
                private,
            }))
        }
        "ecdsa-sha2-nistp384" => {
            let padded = pad_to::<48>(d_bytes)?;
            let sk = p384::SecretKey::from_slice(&padded)
                .map_err(|e| crypto(format!("ECDSA P-384 scalar invalid: {}", e)))?;
            let public = sk.public_key().into();
            let private = ssh_key::private::EcdsaPrivateKey::<48>::from(sk);
            Ok(PrivateKey::from(ssh_key::private::EcdsaKeypair::NistP384 {
                public,
                private,
            }))
        }
        "ecdsa-sha2-nistp521" => Err(VaultError::UnsupportedKeyKind("ecdsa-sha2-nistp521".into())),
        _ => unreachable!(),
    }
}

fn pad_to<const N: usize>(bytes: &[u8]) -> Result<[u8; N], VaultError> {
    if bytes.len() > N {
        return Err(crypto("ECDSA scalar exceeds curve size"));
    }
    let mut out = [0u8; N];
    out[N - bytes.len()..].copy_from_slice(bytes);
    Ok(out)
}

fn hex_decode(s: &str) -> Result<Vec<u8>, VaultError> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return Err(crypto("hex value has odd length"));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_nibble(bytes[i])?;
        let lo = hex_nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, VaultError> {
    Ok(match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => return Err(crypto("invalid hex digit")),
    })
}

fn crypto<S: Into<String>>(s: S) -> VaultError {
    VaultError::Crypto(s.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh_key::{Algorithm, HashAlg, PrivateKey};

    /// Build a synthetic PPK file from an existing OpenSSH private key.
    /// Used to round-trip-verify the parser. NOTE: this synthesizer
    /// shares spec-interpretation code with the parser, so a passing
    /// round-trip proves consistency, not correctness against real
    /// PuTTY output. Replace with PuTTY-emitted fixtures when possible.
    fn synthesize_ppk(
        key: &PrivateKey,
        passphrase: Option<&str>,
        version: u8,
        comment: &str,
    ) -> String {
        let key_type = match key.algorithm() {
            Algorithm::Ed25519 => "ssh-ed25519",
            Algorithm::Rsa { .. } => "ssh-rsa",
            Algorithm::Ecdsa { curve } => match curve {
                ssh_key::EcdsaCurve::NistP256 => "ecdsa-sha2-nistp256",
                ssh_key::EcdsaCurve::NistP384 => "ecdsa-sha2-nistp384",
                ssh_key::EcdsaCurve::NistP521 => "ecdsa-sha2-nistp521",
            },
            _ => panic!("unsupported algo for synthesizer"),
        };

        let public_blob = match key.key_data() {
            ssh_key::private::KeypairData::Ed25519(kp) => {
                let mut out = Vec::new();
                append_string(&mut out, b"ssh-ed25519");
                append_string(&mut out, kp.public.as_ref());
                out
            }
            ssh_key::private::KeypairData::Rsa(kp) => {
                let mut out = Vec::new();
                append_string(&mut out, b"ssh-rsa");
                append_string(&mut out, kp.public().e().as_bytes());
                append_string(&mut out, kp.public().n().as_bytes());
                out
            }
            _ => panic!("synthesizer only handles RSA / Ed25519 today"),
        };

        let private_plaintext = match key.key_data() {
            ssh_key::private::KeypairData::Ed25519(kp) => {
                let mut out = Vec::new();
                append_string(&mut out, kp.private.as_ref());
                out
            }
            ssh_key::private::KeypairData::Rsa(kp) => {
                let mut out = Vec::new();
                append_string(&mut out, kp.private().d().as_bytes());
                append_string(&mut out, kp.private().p().as_bytes());
                append_string(&mut out, kp.private().q().as_bytes());
                append_string(&mut out, kp.private().iqmp().as_bytes());
                out
            }
            _ => panic!("synthesizer only handles RSA / Ed25519 today"),
        };

        let encryption = if passphrase.is_some() { "aes256-cbc" } else { "none" };
        // Pad private plaintext to 16-byte boundary with random bytes
        // (PuTTY uses SHA-1 of the body; we use zeros for determinism in
        // tests). The parser doesn't care about the pad contents.
        let mut padded = private_plaintext.clone();
        let pad_len = (16 - (padded.len() % 16)) % 16;
        padded.extend(std::iter::repeat_n(0u8, pad_len));

        // Headers struct just enough to call derive_keys / verify_mac.
        let argon_salt = if version == 3 && passphrase.is_some() {
            Some(vec![0x42u8; 16])
        } else {
            None
        };
        let headers = Headers {
            version,
            key_type,
            encryption,
            comment,
            public_blob: public_blob.clone(),
            private_blob: Vec::new(),
            mac: Vec::new(),
            kdf: if version == 3 && passphrase.is_some() { Some("Argon2id") } else { None },
            argon_memory: if version == 3 && passphrase.is_some() { Some(8) } else { None },
            argon_passes: if version == 3 && passphrase.is_some() { Some(1) } else { None },
            argon_parallelism: if version == 3 && passphrase.is_some() { Some(1) } else { None },
            argon_salt,
        };
        let pass = passphrase.unwrap_or("");
        let (cipher_key, iv, mac_key) = derive_keys(&headers, pass).unwrap();

        let private_ciphertext = if passphrase.is_some() {
            let mut buf = padded.clone();
            aes256_cbc_encrypt(&cipher_key, &iv, &mut buf);
            buf
        } else {
            padded.clone()
        };

        // Compute the MAC (or hash) over the canonical content.
        let mut content: Vec<u8> = Vec::new();
        append_string(&mut content, key_type.as_bytes());
        append_string(&mut content, encryption.as_bytes());
        append_string(&mut content, comment.as_bytes());
        append_string(&mut content, &public_blob);
        append_string(&mut content, &padded);

        let mac_bytes: Vec<u8> = match version {
            2 => {
                use hmac::{Hmac, Mac};
                type HmacSha1 = Hmac<sha1::Sha1>;
                let mut m = HmacSha1::new_from_slice(&mac_key).unwrap();
                m.update(&content);
                m.finalize().into_bytes().to_vec()
            }
            3 => {
                use hmac::{Hmac, Mac};
                type HmacSha256 = Hmac<sha2::Sha256>;
                let mut m = HmacSha256::new_from_slice(&mac_key).unwrap();
                m.update(&content);
                m.finalize().into_bytes().to_vec()
            }
            _ => unreachable!(),
        };

        let mac_hex: String = mac_bytes.iter().map(|b| format!("{:02x}", b)).collect();
        let pub_b64 = base64::engine::general_purpose::STANDARD.encode(&public_blob);
        let priv_b64 = base64::engine::general_purpose::STANDARD.encode(&private_ciphertext);

        let mut out = String::new();
        let header = if version == 2 { "PuTTY-User-Key-File-2" } else { "PuTTY-User-Key-File-3" };
        out.push_str(&format!("{}: {}\n", header, key_type));
        out.push_str(&format!("Encryption: {}\n", encryption));
        out.push_str(&format!("Comment: {}\n", comment));

        let pub_lines: Vec<&str> = pub_b64.as_bytes().chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        out.push_str(&format!("Public-Lines: {}\n", pub_lines.len()));
        for l in &pub_lines { out.push_str(l); out.push('\n'); }

        if version == 3 && passphrase.is_some() {
            out.push_str("Key-Derivation: Argon2id\n");
            out.push_str("Argon2-Memory: 8\n");
            out.push_str("Argon2-Passes: 1\n");
            out.push_str("Argon2-Parallelism: 1\n");
            let salt_hex: String = headers.argon_salt.unwrap().iter()
                .map(|b| format!("{:02x}", b)).collect();
            out.push_str(&format!("Argon2-Salt: {}\n", salt_hex));
        }

        let priv_lines: Vec<&str> = priv_b64.as_bytes().chunks(64)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        out.push_str(&format!("Private-Lines: {}\n", priv_lines.len()));
        for l in &priv_lines { out.push_str(l); out.push('\n'); }

        out.push_str(&format!("Private-MAC: {}\n", mac_hex));
        out
    }

    fn aes256_cbc_encrypt(key: &[u8], iv: &[u8; 16], buf: &mut [u8]) {
        use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
        type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
        let enc = Aes256CbcEnc::new_from_slices(key, iv).unwrap();
        let buf_len = buf.len();
        enc.encrypt_padded_mut::<NoPadding>(buf, buf_len).unwrap();
    }

    fn fingerprint(key: &PrivateKey) -> String {
        key.public_key().fingerprint(HashAlg::Sha256).to_string()
    }

    #[test]
    fn ppk_v2_ed25519_unencrypted_roundtrip() {
        let mut rng = rand::rng();
        let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
        let ppk = synthesize_ppk(&key, None, 2, "rt-v2");
        let parsed = parse(&ppk, None).unwrap();
        assert_eq!(fingerprint(&parsed), fingerprint(&key));
    }

    #[test]
    fn ppk_v2_ed25519_encrypted_roundtrip() {
        let mut rng = rand::rng();
        let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
        let ppk = synthesize_ppk(&key, Some("hunter2"), 2, "rt-v2-enc");
        let parsed = parse(&ppk, Some("hunter2")).unwrap();
        assert_eq!(fingerprint(&parsed), fingerprint(&key));
    }

    #[test]
    fn ppk_v2_encrypted_wrong_passphrase() {
        let mut rng = rand::rng();
        let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
        let ppk = synthesize_ppk(&key, Some("hunter2"), 2, "rt-v2-wrong");
        let err = parse(&ppk, Some("nope")).unwrap_err();
        assert!(matches!(err, VaultError::WrongKeyPassphrase));
    }

    #[test]
    fn ppk_v2_encrypted_missing_passphrase() {
        let mut rng = rand::rng();
        let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
        let ppk = synthesize_ppk(&key, Some("hunter2"), 2, "rt-v2-needs");
        let err = parse(&ppk, None).unwrap_err();
        assert!(matches!(err, VaultError::KeyNeedsPassphrase));
    }

    #[test]
    fn ppk_v3_ed25519_unencrypted_roundtrip() {
        let mut rng = rand::rng();
        let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
        let ppk = synthesize_ppk(&key, None, 3, "rt-v3");
        let parsed = parse(&ppk, None).unwrap();
        assert_eq!(fingerprint(&parsed), fingerprint(&key));
    }

    #[test]
    fn ppk_v3_ed25519_encrypted_roundtrip() {
        let mut rng = rand::rng();
        let key = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
        let ppk = synthesize_ppk(&key, Some("s3cret"), 3, "rt-v3-enc");
        let parsed = parse(&ppk, Some("s3cret")).unwrap();
        assert_eq!(fingerprint(&parsed), fingerprint(&key));
    }

    #[test]
    fn ppk_detects_envelope() {
        assert!(is_ppk("PuTTY-User-Key-File-2: ssh-rsa\n"));
        assert!(is_ppk("PuTTY-User-Key-File-3: ssh-ed25519\n"));
        assert!(!is_ppk("-----BEGIN OPENSSH PRIVATE KEY-----\n"));
        assert!(!is_ppk(""));
    }

    #[test]
    fn ppk_is_encrypted_detects_aes() {
        let header = "PuTTY-User-Key-File-2: ssh-rsa\nEncryption: aes256-cbc\nComment: x\n";
        assert!(is_encrypted(header));
        let header_none = "PuTTY-User-Key-File-2: ssh-rsa\nEncryption: none\n";
        assert!(!is_encrypted(header_none));
    }

    #[test]
    fn ppk_dsa_rejected() {
        // A header that says ssh-dss should be rejected with a clear
        // error, not mislabeled. Build the minimal envelope by hand.
        let ppk = "PuTTY-User-Key-File-2: ssh-dss\n\
                   Encryption: none\n\
                   Comment: dsa\n\
                   Public-Lines: 1\n\
                   AAAAB3NzaC1kc3M=\n\
                   Private-Lines: 1\n\
                   AAAAAQA=\n\
                   Private-MAC: 0000000000000000000000000000000000000000\n";
        let err = parse(ppk, None).unwrap_err();
        match err {
            VaultError::UnsupportedKeyKind(s) => assert_eq!(s, "ssh-dss"),
            other => panic!("unexpected error: {:?}", other),
        }
    }
}

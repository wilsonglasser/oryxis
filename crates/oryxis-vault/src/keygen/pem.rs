use ssh_key::PrivateKey;

use crate::store::VaultError;

/// Re-wrap the base64 body of a traditional PEM block to exactly 64 chars
/// per line. The pem-rfc7468 parser used by `rsa` / `pkcs8` is strict about
/// the 64-char convention (RFC 7468 §3) and rejects OpenSSL's legacy
/// 76-char wrapping with a misleading "invalid Base64 encoding" error.
/// Returns the input unchanged if no BEGIN/END envelope is found.
pub(super) fn rewrap_pem_body(pem: &str) -> String {
    rewrap_pem_body_at(pem, 64)
}

/// Same as [`rewrap_pem_body`] but with a configurable line width.
/// Useful for OpenSSH keys, where `ssh-encoding`'s PEM decoder
/// requires exactly 70-char lines (not the RFC 7468 default of 64).
pub(super) fn rewrap_pem_body_at(pem: &str, width: usize) -> String {
    let begin = match pem.find("-----BEGIN ") {
        Some(i) => i,
        None => return pem.to_string(),
    };
    let begin_line_end = match pem[begin..].find('\n') {
        Some(off) => begin + off,
        None => return pem.to_string(),
    };
    let end_marker = match pem[begin_line_end..].find("-----END ") {
        Some(off) => begin_line_end + off,
        None => return pem.to_string(),
    };
    let body: String = pem[begin_line_end..end_marker]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut wrapped = String::with_capacity(body.len() + body.len() / width + 8);
    for chunk in body.as_bytes().chunks(width) {
        wrapped.push('\n');
        wrapped.push_str(std::str::from_utf8(chunk).unwrap_or(""));
    }
    wrapped.push('\n');
    let mut out = String::with_capacity(pem.len() + 16);
    out.push_str(&pem[..begin_line_end]);
    out.push_str(&wrapped);
    out.push_str(&pem[end_marker..]);
    out
}

/// Returns true if the PEM carries an OpenSSL-legacy DEK-Info header
/// (`Proc-Type: 4,ENCRYPTED`) inside a PKCS#1 or SEC1 envelope, or is a
/// PKCS#8 `ENCRYPTED PRIVATE KEY`. The UI uses this to reveal the
/// passphrase field early; [`parse`] decrypts both on Save.
pub fn is_traditional_encrypted(pem: &str) -> bool {
    if pem.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        return true;
    }
    pem.contains("ENCRYPTED")
        && (pem.contains("BEGIN RSA PRIVATE KEY") || pem.contains("BEGIN EC PRIVATE KEY"))
        && pem.contains("DEK-Info:")
}

/// Decrypt a PKCS#8 `ENCRYPTED PRIVATE KEY` PEM with `passphrase` and
/// hand the inner plaintext PKCS#8 PEM back to the regular dispatcher,
/// so RSA, ECDSA, and Ed25519 all benefit from the same probe order.
fn decrypt_pkcs8(pem: &str, passphrase: &[u8]) -> Result<PrivateKey, VaultError> {
    use base64::Engine;

    let begin_tag = "-----BEGIN ENCRYPTED PRIVATE KEY-----";
    let end_tag = "-----END ENCRYPTED PRIVATE KEY-----";
    let begin = pem
        .find(begin_tag)
        .ok_or_else(|| VaultError::Crypto("missing PEM begin".into()))?
        + begin_tag.len();
    let end = pem[begin..]
        .find(end_tag)
        .ok_or_else(|| VaultError::Crypto("missing PEM end".into()))?
        + begin;
    let b64: String = pem[begin..end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| VaultError::Crypto(format!("PEM base64: {}", e)))?;

    let info = pkcs8::EncryptedPrivateKeyInfoRef::try_from(der.as_slice())
        .map_err(|e| VaultError::Crypto(format!("encrypted PKCS#8 parse: {}", e)))?;
    let plaintext = info
        .decrypt(passphrase)
        .map_err(|_| VaultError::WrongKeyPassphrase)?;

    // Re-wrap the decrypted DER as a plain "BEGIN PRIVATE KEY" PEM and
    // dispatch through the plaintext path. This keeps all algorithm
    // probes (RSA, ECDSA, Ed25519) in one place.
    let plain_pem = der_to_pkcs8_pem(plaintext.as_bytes());
    parse(&plain_pem, None)
}

fn der_to_pkcs8_pem(der: &[u8]) -> String {
    use base64::Engine;
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = String::with_capacity(body.len() + 64);
    out.push_str("-----BEGIN PRIVATE KEY-----\n");
    for chunk in body.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        out.push('\n');
    }
    out.push_str("-----END PRIVATE KEY-----\n");
    out
}

/// OpenSSL's `EVP_BytesToKey` with MD5 and a single iteration, the KDF
/// used by traditional (DEK-Info) PEM encryption. Concatenates
/// `MD5(prev || password || salt)` digests until `key_len` bytes are
/// available. `salt` is the first 8 bytes of the DEK-Info IV.
fn evp_bytes_to_key(password: &[u8], salt: &[u8], key_len: usize) -> Vec<u8> {
    use md5::{Digest, Md5};
    let mut out = Vec::with_capacity(key_len + 16);
    let mut prev: Vec<u8> = Vec::new();
    while out.len() < key_len {
        let mut h = Md5::new();
        h.update(&prev);
        h.update(password);
        h.update(salt);
        prev = h.finalize().to_vec();
        out.extend_from_slice(&prev);
    }
    out.truncate(key_len);
    out
}

/// Decode an even-length ASCII hex string to bytes. Returns `None` on
/// odd length or any non-hex digit.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Wrap raw DER under a traditional PEM label at the 64-char convention.
fn der_to_labeled_pem(label: &str, der: &[u8]) -> String {
    use base64::Engine;
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = String::with_capacity(body.len() + 64);
    out.push_str("-----BEGIN ");
    out.push_str(label);
    out.push_str("-----\n");
    for chunk in body.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        out.push('\n');
    }
    out.push_str("-----END ");
    out.push_str(label);
    out.push_str("-----\n");
    out
}

/// Decrypt an OpenSSL-legacy traditional PEM carrying a `Proc-Type:
/// 4,ENCRYPTED` plus `DEK-Info: <cipher>,<hex-iv>` header pair. Derives
/// the key via `EVP_BytesToKey`/MD5, decrypts the named CBC cipher, and
/// re-dispatches the recovered PKCS#1 (RSA) or SEC1 (EC) plaintext
/// through [`parse`]. A wrong passphrase surfaces as a PKCS#7 unpad
/// failure mapped to [`VaultError::WrongKeyPassphrase`].
fn decrypt_legacy_pem(pem: &str, passphrase: &[u8]) -> Result<PrivateKey, VaultError> {
    use base64::Engine;
    use cbc::cipher::block_padding::Pkcs7;
    use cbc::cipher::{BlockDecryptMut, KeyIvInit};

    let label = if pem.contains("BEGIN RSA PRIVATE KEY") {
        "RSA PRIVATE KEY"
    } else {
        "EC PRIVATE KEY"
    };

    let dek_line = pem
        .lines()
        .find(|l| l.trim_start().starts_with("DEK-Info:"))
        .ok_or_else(|| VaultError::Crypto("missing DEK-Info header".into()))?;
    let dek = dek_line.trim_start().trim_start_matches("DEK-Info:").trim();
    let (cipher_name, iv_hex) = dek
        .split_once(',')
        .ok_or_else(|| VaultError::Crypto("malformed DEK-Info header".into()))?;
    let cipher_name = cipher_name.trim();
    let iv = hex_decode(iv_hex).ok_or_else(|| VaultError::Crypto("invalid DEK-Info IV".into()))?;

    let (key_len, iv_len) = match cipher_name {
        "AES-128-CBC" => (16usize, 16usize),
        "AES-192-CBC" => (24, 16),
        "AES-256-CBC" => (32, 16),
        "DES-EDE3-CBC" => (24, 8),
        "DES-CBC" => (8, 8),
        other => {
            return Err(VaultError::Crypto(format!(
                "unsupported legacy PEM cipher: {}",
                other
            )));
        }
    };
    if iv.len() != iv_len {
        return Err(VaultError::Crypto("DEK-Info IV length mismatch".into()));
    }

    // Body: the base64 between the blank line after the headers and END.
    let mut body = String::new();
    let mut in_body = false;
    for line in pem.lines() {
        let t = line.trim();
        if t.starts_with("-----END") {
            break;
        }
        if in_body {
            body.push_str(t);
        } else if t.is_empty() {
            in_body = true;
        }
    }
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|e| VaultError::Crypto(format!("legacy PEM base64: {}", e)))?;

    let salt = &iv[..8.min(iv.len())];
    let key = evp_bytes_to_key(passphrase, salt, key_len);

    let mut buf = ciphertext;
    macro_rules! decrypt_with {
        ($cipher:ty) => {{
            let pt = cbc::Decryptor::<$cipher>::new_from_slices(&key, &iv)
                .map_err(|_| VaultError::Crypto("legacy PEM key/iv size".into()))?
                .decrypt_padded_mut::<Pkcs7>(&mut buf)
                .map_err(|_| VaultError::WrongKeyPassphrase)?;
            pt.to_vec()
        }};
    }
    let plaintext_der = match cipher_name {
        "AES-128-CBC" => decrypt_with!(aes::Aes128),
        "AES-192-CBC" => decrypt_with!(aes::Aes192),
        "AES-256-CBC" => decrypt_with!(aes::Aes256),
        "DES-EDE3-CBC" => decrypt_with!(des::TdesEde3),
        "DES-CBC" => decrypt_with!(des::Des),
        _ => unreachable!("cipher_name already validated above"),
    };

    let plain_pem = der_to_labeled_pem(label, &plaintext_der);
    parse(&plain_pem, None)
}

// ssh-key 0.7's `EcdsaKeypair` carries the public half as a raw
// `sec1::EncodedPoint`, so hand it the uncompressed SEC1 point derived
// from the secret key's public component.
fn parse_ec_p256(sk: p256::SecretKey) -> PrivateKey {
    use p256::elliptic_curve::sec1::ToSec1Point;
    let public = sk.public_key().to_sec1_point(false);
    let private = ssh_key::private::EcdsaPrivateKey::<32>::from(sk);
    PrivateKey::from(ssh_key::private::EcdsaKeypair::NistP256 { public, private })
}

fn parse_ec_p384(sk: p384::SecretKey) -> PrivateKey {
    use p384::elliptic_curve::sec1::ToSec1Point;
    let public = sk.public_key().to_sec1_point(false);
    let private = ssh_key::private::EcdsaPrivateKey::<48>::from(sk);
    PrivateKey::from(ssh_key::private::EcdsaKeypair::NistP384 { public, private })
}

fn parse_ec_p521(sk: p521::SecretKey) -> PrivateKey {
    use p521::elliptic_curve::sec1::ToSec1Point;
    let public = sk.public_key().to_sec1_point(false);
    let private = ssh_key::private::EcdsaPrivateKey::<66>::from(sk);
    PrivateKey::from(ssh_key::private::EcdsaKeypair::NistP521 { public, private })
}

/// Parse a PKCS#8 OneAsymmetricKey (RFC 5958) DER body and return the
/// inner Ed25519 32-byte seed, if the algorithm OID matches
/// `1.3.101.112` (RFC 8410). The structure we expect is fixed:
///
/// ```text
/// SEQUENCE {
///   INTEGER 0,                       -- version
///   SEQUENCE { OID 1.3.101.112 },    -- algorithm
///   OCTET STRING { OCTET STRING { 32 bytes seed } },
///   [0] OPTIONAL public key          -- ignored
/// }
/// ```
fn try_extract_ed25519_seed(pem: &str) -> Option<[u8; 32]> {
    use base64::Engine;
    let begin_tag = "-----BEGIN PRIVATE KEY-----";
    let end_tag = "-----END PRIVATE KEY-----";
    let begin = pem.find(begin_tag)? + begin_tag.len();
    let end = pem[begin..].find(end_tag)? + begin;
    let b64: String = pem[begin..end].chars().filter(|c| !c.is_whitespace()).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;

    // Outer SEQUENCE.
    let body = read_tlv(&der, 0x30)?;
    let mut cur = body;

    // version INTEGER (0)
    let (_, rest) = take_tlv(cur, 0x02)?;
    cur = rest;

    // algorithm SEQUENCE { OID }
    let (algo_body, rest) = take_tlv(cur, 0x30)?;
    cur = rest;
    let (oid_bytes, _) = take_tlv(algo_body, 0x06)?;
    // OID 1.3.101.112 encodes to bytes: 2b 65 70
    if oid_bytes != [0x2b, 0x65, 0x70] {
        return None;
    }

    // privateKey OCTET STRING { OCTET STRING { 32 bytes } }
    let (outer_octets, _) = take_tlv(cur, 0x04)?;
    let (inner_octets, _) = take_tlv(outer_octets, 0x04)?;
    if inner_octets.len() != 32 {
        return None;
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(inner_octets);
    Some(seed)
}

/// Minimal DER reader: read a single TLV at the start of `buf` whose tag
/// matches `expected_tag`, returning the value bytes. Lengths follow
/// the short/long form rules from X.690. Returns None on any mismatch.
fn read_tlv(buf: &[u8], expected_tag: u8) -> Option<&[u8]> {
    take_tlv(buf, expected_tag).map(|(v, _)| v)
}

fn take_tlv(buf: &[u8], expected_tag: u8) -> Option<(&[u8], &[u8])> {
    if buf.first()? != &expected_tag {
        return None;
    }
    let len_byte = *buf.get(1)?;
    let (len, header_len) = if len_byte & 0x80 == 0 {
        (len_byte as usize, 2)
    } else {
        let n = (len_byte & 0x7f) as usize;
        if n == 0 || n > 4 {
            return None;
        }
        let mut len: usize = 0;
        for i in 0..n {
            len = (len << 8) | (*buf.get(2 + i)? as usize);
        }
        (len, 2 + n)
    };
    let end = header_len.checked_add(len)?;
    if end > buf.len() {
        return None;
    }
    Some((&buf[header_len..end], &buf[end..]))
}

/// Parse a traditional PEM key (PKCS#1, PKCS#8, SEC1) and convert to
/// `ssh_key::PrivateKey`. Encrypted PKCS#8 (`BEGIN ENCRYPTED PRIVATE
/// KEY`) and OpenSSL-legacy traditional PEM (`DEK-Info`) are both
/// decrypted using `passphrase`; `KeyNeedsPassphrase` is returned when
/// an encrypted key arrives without one.
pub fn parse(pem: &str, passphrase: Option<&str>) -> Result<PrivateKey, VaultError> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::DecodePrivateKey;

    // OpenSSL-legacy traditional PEM (`Proc-Type: 4,ENCRYPTED` +
    // `DEK-Info`). Decrypt with EVP_BytesToKey(MD5) + the named CBC
    // cipher, then re-dispatch the recovered PKCS#1 / SEC1 plaintext
    // through the unencrypted path below.
    if pem.contains("DEK-Info:")
        && (pem.contains("BEGIN RSA PRIVATE KEY") || pem.contains("BEGIN EC PRIVATE KEY"))
    {
        let pass = passphrase.unwrap_or("");
        if pass.is_empty() {
            return Err(VaultError::KeyNeedsPassphrase);
        }
        return decrypt_legacy_pem(pem, pass.as_bytes());
    }

    let normalized = rewrap_pem_body(pem);
    let pem = normalized.as_str();

    // Encrypted PKCS#8: "BEGIN ENCRYPTED PRIVATE KEY". Decrypt once
    // here, then re-dispatch to the plain PKCS#8 algorithm probes below.
    if pem.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        let pass = passphrase.unwrap_or("");
        if pass.is_empty() {
            return Err(VaultError::KeyNeedsPassphrase);
        }
        return decrypt_pkcs8(pem, pass.as_bytes());
    }

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
        if let Ok(sk) = p521::SecretKey::from_sec1_pem(pem) {
            return Ok(parse_ec_p521(sk));
        }
        return Err(VaultError::Crypto(
            "Unsupported EC curve (only P-256, P-384 and P-521 are supported)".into(),
        ));
    }

    // PKCS#8: "BEGIN PRIVATE KEY" (RSA, EC, or Ed25519).
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
        if let Ok(sk) = p521::SecretKey::from_pkcs8_pem(pem) {
            return Ok(parse_ec_p521(sk));
        }
        if let Some(seed) = try_extract_ed25519_seed(pem) {
            let keypair = ssh_key::private::Ed25519Keypair::from_seed(&seed);
            return Ok(PrivateKey::from(keypair));
        }
        return Err(VaultError::Crypto(
            "Unsupported PKCS#8 key type (supported: RSA, ECDSA P-256/P-384/P-521, Ed25519)".into(),
        ));
    }

    Err(VaultError::Crypto("Unrecognized PEM format".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_tlv_short_form() {
        // SEQUENCE len=3 { 02 01 00 }, an INTEGER(0).
        let buf = [0x30, 0x03, 0x02, 0x01, 0x00];
        let body = read_tlv(&buf, 0x30).unwrap();
        assert_eq!(body, &[0x02, 0x01, 0x00]);
        let (inner, rest) = take_tlv(body, 0x02).unwrap();
        assert_eq!(inner, &[0x00]);
        assert!(rest.is_empty());
    }

    #[test]
    fn der_tlv_long_form() {
        // OCTET STRING len=130 (0x82), long-form length encoding.
        let mut buf = vec![0x04, 0x81, 0x82];
        buf.extend(std::iter::repeat_n(0xAA, 130));
        let body = read_tlv(&buf, 0x04).unwrap();
        assert_eq!(body.len(), 130);
    }

    #[test]
    fn extract_ed25519_from_pkcs8() {
        // Minimal Ed25519 PKCS#8 with a known seed (all zeros for simplicity).
        let seed = [0u8; 32];
        let der: Vec<u8> = vec![
            0x30, 0x2e, // SEQUENCE len 46
            0x02, 0x01, 0x00, // INTEGER 0
            0x30, 0x05, // SEQUENCE len 5
            0x06, 0x03, 0x2b, 0x65, 0x70, // OID 1.3.101.112
            0x04, 0x22, // OCTET STRING len 34
            0x04, 0x20, // inner OCTET STRING len 32
        ];
        let mut full = der;
        full.extend_from_slice(&seed);
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&full);
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            b64
        );
        let got = try_extract_ed25519_seed(&pem).unwrap();
        assert_eq!(got, seed);
    }

    #[test]
    fn evp_bytes_to_key_first_blocks_match_md5() {
        use md5::{Digest, Md5};
        let pw = b"password";
        let salt = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let key = evp_bytes_to_key(pw, &salt, 32);
        // D1 = MD5(password || salt)
        let mut h = Md5::new();
        h.update(pw);
        h.update(salt);
        let d1 = h.finalize();
        assert_eq!(&key[..16], d1.as_slice());
        // D2 = MD5(D1 || password || salt)
        let mut h = Md5::new();
        h.update(d1);
        h.update(pw);
        h.update(salt);
        let d2 = h.finalize();
        assert_eq!(&key[16..32], d2.as_slice());
    }

    #[test]
    fn parse_p256_pkcs8_round_trips() {
        // Exercises the `to_sec1_point` public-key path shared by all
        // three ECDSA curves (the P-521 test below covers the same code
        // for the 66-byte variant).
        use p256::elliptic_curve::Generate;
        use pkcs8::EncodePrivateKey;
        let sk = p256::SecretKey::generate_from_rng(&mut rand::rng());
        let pem = sk.to_pkcs8_pem(pkcs8::LineEnding::LF).unwrap();
        let key = parse(&pem, None).unwrap();
        assert!(matches!(
            key.algorithm(),
            ssh_key::Algorithm::Ecdsa {
                curve: ssh_key::EcdsaCurve::NistP256
            }
        ));
        assert!(key
            .public_key()
            .to_openssh()
            .unwrap()
            .starts_with("ecdsa-sha2-nistp256 "));
    }

    #[test]
    fn parse_p521_pkcs8_round_trips() {
        use p521::elliptic_curve::Generate;
        use pkcs8::EncodePrivateKey;
        let sk = p521::SecretKey::generate_from_rng(&mut rand::rng());
        let pem = sk.to_pkcs8_pem(pkcs8::LineEnding::LF).unwrap();
        let key = parse(&pem, None).unwrap();
        assert!(matches!(
            key.algorithm(),
            ssh_key::Algorithm::Ecdsa {
                curve: ssh_key::EcdsaCurve::NistP521
            }
        ));
        // The recovered key re-encodes to a valid P-521 OpenSSH public key.
        assert!(key
            .public_key()
            .to_openssh()
            .unwrap()
            .starts_with("ecdsa-sha2-nistp521 "));
    }

    #[test]
    fn parse_legacy_encrypted_pkcs1_pem_round_trips() {
        use base64::Engine;
        use cbc::cipher::block_padding::Pkcs7;
        use cbc::cipher::{BlockEncryptMut, KeyIvInit};
        use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};

        // Fresh 1024-bit RSA key, no embedded secret. 1024 keeps the test
        // fast; the path under test is format/KDF, not key strength.
        let rsa_key = rsa::RsaPrivateKey::new(&mut rand::rng(), 1024).unwrap();
        let want_fp = parse(&rsa_key.to_pkcs1_pem(LineEnding::LF).unwrap(), None)
            .unwrap()
            .fingerprint(ssh_key::HashAlg::Sha256)
            .to_string();

        // Encrypt the PKCS#1 DER exactly as OpenSSL would: EVP_BytesToKey
        // /MD5 (salt = first 8 IV bytes) + AES-128-CBC + PKCS#7.
        let der = rsa_key.to_pkcs1_der().unwrap();
        let iv: [u8; 16] = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF,
        ];
        let pass = b"correct horse battery";
        let key = evp_bytes_to_key(pass, &iv[..8], 16);
        let plain = der.as_bytes();
        let mut buf = plain.to_vec();
        let n = buf.len();
        buf.resize(n + 16, 0);
        let ct = cbc::Encryptor::<aes::Aes128>::new_from_slices(&key, &iv)
            .unwrap()
            .encrypt_padded_mut::<Pkcs7>(&mut buf, n)
            .unwrap()
            .to_vec();

        let b64 = base64::engine::general_purpose::STANDARD.encode(&ct);
        let mut body = String::new();
        for chunk in b64.as_bytes().chunks(64) {
            body.push_str(std::str::from_utf8(chunk).unwrap());
            body.push('\n');
        }
        let iv_hex: String = iv.iter().map(|b| format!("{:02X}", b)).collect();
        let pem = format!(
            "-----BEGIN RSA PRIVATE KEY-----\n\
             Proc-Type: 4,ENCRYPTED\n\
             DEK-Info: AES-128-CBC,{iv_hex}\n\n\
             {body}-----END RSA PRIVATE KEY-----\n"
        );

        // Missing passphrase prompts; wrong passphrase fails; correct
        // passphrase recovers the identical key.
        assert!(matches!(
            parse(&pem, None),
            Err(VaultError::KeyNeedsPassphrase)
        ));
        assert!(parse(&pem, Some("wrong")).is_err());
        let got = parse(&pem, Some("correct horse battery")).unwrap();
        assert_eq!(
            got.fingerprint(ssh_key::HashAlg::Sha256).to_string(),
            want_fp
        );
    }

    /// The real-world case from a user: an OpenSSL-legacy PEM encrypted with
    /// DES-EDE3-CBC (3des). Exercises the 24-byte key derivation (two MD5
    /// blocks, vs one for AES-128) and the 8-byte IV/block path.
    #[test]
    fn parse_legacy_encrypted_3des_pem_round_trips() {
        use base64::Engine;
        use cbc::cipher::block_padding::Pkcs7;
        use cbc::cipher::{BlockEncryptMut, KeyIvInit};
        use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding};

        let rsa_key = rsa::RsaPrivateKey::new(&mut rand::rng(), 1024).unwrap();
        let want_fp = parse(&rsa_key.to_pkcs1_pem(LineEnding::LF).unwrap(), None)
            .unwrap()
            .fingerprint(ssh_key::HashAlg::Sha256)
            .to_string();

        // DES-EDE3-CBC: 24-byte key, 8-byte IV (and 8-byte salt = the IV).
        let der = rsa_key.to_pkcs1_der().unwrap();
        let iv: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
        let pass = b"hunter2 hunter2";
        let key = evp_bytes_to_key(pass, &iv, 24);
        let plain = der.as_bytes();
        let mut buf = plain.to_vec();
        let n = buf.len();
        buf.resize(n + 8, 0);
        let ct = cbc::Encryptor::<des::TdesEde3>::new_from_slices(&key, &iv)
            .unwrap()
            .encrypt_padded_mut::<Pkcs7>(&mut buf, n)
            .unwrap()
            .to_vec();

        let b64 = base64::engine::general_purpose::STANDARD.encode(&ct);
        let mut body = String::new();
        for chunk in b64.as_bytes().chunks(64) {
            body.push_str(std::str::from_utf8(chunk).unwrap());
            body.push('\n');
        }
        let iv_hex: String = iv.iter().map(|b| format!("{:02X}", b)).collect();
        let pem = format!(
            "-----BEGIN RSA PRIVATE KEY-----\n\
             Proc-Type: 4,ENCRYPTED\n\
             DEK-Info: DES-EDE3-CBC,{iv_hex}\n\n\
             {body}-----END RSA PRIVATE KEY-----\n"
        );

        assert!(parse(&pem, Some("wrong")).is_err());
        let got = parse(&pem, Some("hunter2 hunter2")).unwrap();
        assert_eq!(
            got.fingerprint(ssh_key::HashAlg::Sha256).to_string(),
            want_fp
        );
    }
}

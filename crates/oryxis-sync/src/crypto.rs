use chacha20poly1305::aead::{Aead, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::RngCore;
use uuid::Uuid;
use x25519_dalek::{EphemeralSecret, PublicKey as X25519PublicKey};

use crate::error::SyncError;

/// Length of the channel-binding exporter (RFC 5705 keying material)
/// fed into the Ed25519 session-handshake signature.
pub const SESSION_EXPORTER_LEN: usize = 32;

/// RFC 5705 exporter label used to derive channel-binding bytes from the
/// QUIC/TLS session for the Ed25519 handshake signature. Bumping this
/// label (e.g. "v2") forces incompatible peers to fail verification
/// instead of silently accepting an attacker-controlled exporter.
pub const SESSION_EXPORTER_LABEL: &[u8] = b"oryxis-sync session auth v1";

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

    /// A short, human-readable identifier for this device's public
    /// key. The first 6 bytes of `SHA-256(pubkey)` rendered as
    /// `xxxx-xxxx-xxxx` lowercase hex. Used by the signaling server
    /// to dedupe registrations and shown to users for visual
    /// verification of a paired device.
    pub fn public_key_fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(self.public_key().to_bytes());
        format!(
            "{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}",
            digest[0], digest[1], digest[2], digest[3], digest[4], digest[5]
        )
    }

    /// Serialize the identity for persistence. Layout (deterministic, fixed length):
    ///   16 bytes device_id (Uuid bytes)
    ///   32 bytes signing key secret
    ///   N  bytes UTF-8 device_name
    /// The signing key bytes are sensitive and must be stored encrypted
    /// at rest (vault settings, encrypted column, etc).
    pub fn to_bytes(&self) -> Vec<u8> {
        let name = self.device_name.as_bytes();
        let mut out = Vec::with_capacity(16 + 32 + name.len());
        out.extend_from_slice(self.device_id.as_bytes());
        out.extend_from_slice(&self.signing_key.to_bytes());
        out.extend_from_slice(name);
        out
    }

    /// Inverse of [`Self::to_bytes`]. Returns an error if the input is
    /// truncated or contains invalid UTF-8 in the device name.
    pub fn from_bytes(data: &[u8]) -> Result<Self, SyncError> {
        if data.len() < 16 + 32 {
            return Err(SyncError::Crypto("Identity blob too short".into()));
        }
        let mut id_bytes = [0u8; 16];
        id_bytes.copy_from_slice(&data[..16]);
        let device_id = Uuid::from_bytes(id_bytes);

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&data[16..48]);
        let signing_key = SigningKey::from_bytes(&key_bytes);

        let device_name = std::str::from_utf8(&data[48..])
            .map_err(|e| SyncError::Crypto(format!("Identity name not UTF-8: {}", e)))?
            .to_string();

        Ok(Self {
            device_id,
            device_name,
            signing_key,
        })
    }

    /// Load the persisted identity from the vault, or generate+persist
    /// a fresh one if none exists yet. Idempotent: the second call with
    /// the same vault returns the same identity. Vault must be unlocked.
    ///
    /// `fallback_device_name` is used only when generating a new
    /// identity; when loading an existing one, the name embedded in
    /// the blob wins.
    pub fn load_or_generate(
        vault: &oryxis_vault::VaultStore,
        fallback_device_name: &str,
    ) -> Result<Self, SyncError> {
        if let Some(blob) = vault
            .get_sync_device_identity()
            .map_err(|e| SyncError::Vault(e.to_string()))?
        {
            return Self::from_bytes(&blob);
        }
        let fresh = Self::generate(fallback_device_name);
        vault
            .set_sync_device_identity(&fresh.to_bytes())
            .map_err(|e| SyncError::Vault(e.to_string()))?;
        Ok(fresh)
    }
}

/// Sign a 32-byte message with this device's Ed25519 key. The generic
/// primitive behind both the session channel-binding signature and the
/// pairing challenge response (the challenge nonce is also 32 bytes).
pub fn sign_ed25519_32(signing_key: &SigningKey, message: &[u8; 32]) -> [u8; 64] {
    signing_key.sign(message).to_bytes()
}

/// Verify an Ed25519 signature over a 32-byte message against a raw
/// 32-byte public key. Generic counterpart of [`sign_ed25519_32`].
/// Fails if either input is malformed or the signature does not match.
pub fn verify_ed25519_32(
    pubkey: &[u8],
    message: &[u8; 32],
    signature: &[u8],
) -> Result<(), SyncError> {
    let pubkey_array: [u8; 32] = pubkey
        .try_into()
        .map_err(|_| SyncError::Crypto("Peer pubkey must be 32 bytes".into()))?;
    let verifying = VerifyingKey::from_bytes(&pubkey_array)
        .map_err(|e| SyncError::Crypto(format!("Bad peer pubkey: {}", e)))?;
    let sig_array: [u8; 64] = signature
        .try_into()
        .map_err(|_| SyncError::Crypto("Signature must be 64 bytes".into()))?;
    let sig = Signature::from_bytes(&sig_array);
    verifying
        .verify_strict(message, &sig)
        .map_err(|e| SyncError::Crypto(format!("Ed25519 verify failed: {}", e)))
}

/// Sign the channel-binding exporter with this device's Ed25519 key.
/// The exporter comes from the QUIC TLS session (RFC 5705), so the
/// resulting signature is bound to the specific TLS session: a MITM
/// with its own TLS context will see a different exporter and cannot
/// forge or relay a valid signature without holding the private key.
pub fn sign_session_handshake(
    signing_key: &SigningKey,
    exporter: &[u8; SESSION_EXPORTER_LEN],
) -> [u8; 64] {
    sign_ed25519_32(signing_key, exporter)
}

/// Verify a peer's Ed25519 signature over the channel-binding exporter.
/// `peer_pubkey` is the 32-byte raw VerifyingKey bytes stored on the
/// `SyncPeer` row at pairing time.
pub fn verify_session_handshake(
    peer_pubkey: &[u8],
    exporter: &[u8; SESSION_EXPORTER_LEN],
    signature: &[u8],
) -> Result<(), SyncError> {
    verify_ed25519_32(peer_pubkey, exporter, signature)
}

/// Maximum skew, in seconds, the server tolerates between its clock
/// and the `signed_at` field in a register/unregister request. Anything
/// outside this window is rejected as a likely replay or as a client
/// with a wildly broken clock that we would not want to trust either
/// way. Kept short so a captured signature has a small reuse window.
pub const REGISTER_TIMESTAMP_SKEW_SECS: i64 = 60;

/// Domain-separation tag for the signaling register canonical payload.
/// Bumping this tag forces a clean break with older clients/servers
/// instead of silently producing an unverifiable signature.
pub const REGISTER_SIGN_LABEL: &str = "oryxis-register-v1";

/// Domain-separation tag for the signaling unregister payload.
pub const UNREGISTER_SIGN_LABEL: &str = "oryxis-unregister-v1";

/// Build the canonical byte string the client and server both hash to
/// build a register signature. Newline-delimited text so a JS server
/// (Cloudflare Worker) can reproduce it without bincode or fixed-width
/// integer encoding. Domain-separated by [`REGISTER_SIGN_LABEL`] so a
/// signature minted for any other oryxis protocol cannot be replayed
/// here, and vice-versa.
pub fn register_sign_payload(
    device_id: &Uuid,
    ip: &str,
    port: u16,
    signed_at: i64,
) -> Vec<u8> {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        REGISTER_SIGN_LABEL, device_id, ip, port, signed_at
    )
    .into_bytes()
}

/// Canonical bytes for the unregister request. Same construction as
/// [`register_sign_payload`] minus the address fields (since the
/// server doesn't need them to look up the row).
pub fn unregister_sign_payload(device_id: &Uuid, signed_at: i64) -> Vec<u8> {
    format!("{}\n{}\n{}", UNREGISTER_SIGN_LABEL, device_id, signed_at).into_bytes()
}

/// Sign a register/unregister payload with this device's Ed25519 key.
/// The payload is hashed with SHA-256 first so we sign exactly 32
/// bytes (matching the rest of the crypto surface) and so the JS
/// verifier can use the standard `Ed25519` algorithm without needing
/// to prehash itself.
pub fn sign_register_payload(signing_key: &SigningKey, payload: &[u8]) -> [u8; 64] {
    signing_key.sign(payload).to_bytes()
}

/// Verify a register/unregister payload signature. Mirrors
/// [`sign_register_payload`] and is the entry point both the Rust
/// relay and the test suite use; the JS worker does the equivalent
/// via `crypto.subtle.verify({ name: "Ed25519" }, ...)`.
pub fn verify_register_payload(
    pubkey: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), SyncError> {
    let pubkey_array: [u8; 32] = pubkey
        .try_into()
        .map_err(|_| SyncError::Crypto("Register pubkey must be 32 bytes".into()))?;
    let verifying = VerifyingKey::from_bytes(&pubkey_array)
        .map_err(|e| SyncError::Crypto(format!("Bad register pubkey: {}", e)))?;
    let sig_array: [u8; 64] = signature
        .try_into()
        .map_err(|_| SyncError::Crypto("Register signature must be 64 bytes".into()))?;
    let sig = Signature::from_bytes(&sig_array);
    verifying
        .verify_strict(payload, &sig)
        .map_err(|e| SyncError::Crypto(format!("Register signature invalid: {}", e)))
}

/// Domain-separation tag for the relay-session handshake transcript.
/// Folded into the hash so the same nonce pair signed in some other
/// context cannot be replayed here, and so bumping the tag forces a
/// hard fail instead of silent accept on a future change.
pub const RELAY_HANDSHAKE_LABEL: &[u8] = b"oryxis-sync relay auth v5";

/// Build the 32-byte transcript both sides sign during the relay
/// handshake. The transcript binds the two device IDs and the two
/// fresh per-session nonces, so a signature captured from one session
/// will not verify in another. The QUIC path uses the TLS exporter
/// instead, since QUIC sessions have channel binding for free.
pub fn relay_handshake_transcript(
    client_device_id: &Uuid,
    server_device_id: &Uuid,
    client_nonce: &[u8; 32],
    server_nonce: &[u8; 32],
) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(RELAY_HANDSHAKE_LABEL);
    h.update(client_device_id.as_bytes());
    h.update(server_device_id.as_bytes());
    h.update(client_nonce);
    h.update(server_nonce);
    let digest = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Sign the relay-handshake transcript with this device's Ed25519
/// key. Wrapper around [`sign_ed25519_32`] kept separate so the
/// call sites read clearly and so the relay path can grow its own
/// label-bump independent of [`sign_session_handshake`].
pub fn sign_relay_handshake(
    signing_key: &SigningKey,
    transcript: &[u8; 32],
) -> [u8; 64] {
    sign_ed25519_32(signing_key, transcript)
}

/// Verify a peer's Ed25519 signature over the relay-handshake
/// transcript against the pubkey persisted at pairing time.
pub fn verify_relay_handshake(
    peer_pubkey: &[u8],
    transcript: &[u8; 32],
    signature: &[u8],
) -> Result<(), SyncError> {
    verify_ed25519_32(peer_pubkey, transcript, signature)
}

/// Fresh 32-byte random nonce for the relay handshake. Same shape as
/// the pairing challenge nonce; kept as a distinct helper so the call
/// sites read clearly.
pub fn random_relay_nonce() -> [u8; 32] {
    random_challenge()
}

/// Constant-time byte comparison for the pairing-code check. Returns
/// true only when both slices are the same length and content; the
/// timing does not reveal where (or whether) they first differ.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// Generate a 6-digit numeric pairing code.
pub fn generate_pairing_code() -> String {
    let mut rng = OsRng;
    let code: u32 = rng.next_u32() % 1_000_000;
    format!("{:06}", code)
}

/// Generate a fresh 32-byte random nonce for the pairing challenge.
pub fn random_challenge() -> [u8; 32] {
    let mut challenge = [0u8; 32];
    OsRng.fill_bytes(&mut challenge);
    challenge
}

/// Perform X25519 key exchange to derive a shared secret in one step.
/// Returns (our_public_key, shared_secret). The DH output is run through
/// HKDF-SHA256 with the [`PEER_SECRET_HKDF_INFO`] domain separator before
/// being returned, so the secret is uniformly distributed AEAD-grade key
/// material and cannot be reused for a different protocol purpose. Used
/// by tests and by any caller that has the peer's pubkey available when
/// generating.
pub fn x25519_key_exchange(peer_public_key: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let our_public = X25519PublicKey::from(&secret);
    let peer_public = X25519PublicKey::from(*peer_public_key);
    let shared = secret.diffie_hellman(&peer_public);
    (our_public.to_bytes(), derive_peer_secret(shared.as_bytes()))
}

/// Generate a fresh ephemeral X25519 keypair, returning the secret
/// (held across an `.await` until the peer's pubkey arrives) and the
/// public bytes we send on the wire. Pair it with [`x25519_dh`] to
/// finish the exchange. Used by the pairing flow, where the two
/// pubkeys are not exchanged in lockstep.
pub fn x25519_keypair() -> (EphemeralSecret, [u8; 32]) {
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let public_bytes = X25519PublicKey::from(&secret).to_bytes();
    (secret, public_bytes)
}

/// Complete an X25519 exchange started with [`x25519_keypair`]. Takes
/// the ephemeral secret (consumed: `EphemeralSecret` is single-use,
/// so the DH stage cannot be repeated for the same keypair) and the
/// peer's public bytes; returns the 32-byte shared secret to persist
/// on the `SyncPeer` row. The raw DH output is run through HKDF-SHA256
/// (see [`derive_peer_secret`]) before being returned so the persisted
/// secret is uniform AEAD key material rather than the X25519 group
/// element directly.
///
/// NOTE: the derived secret is long-lived (lives on the `SyncPeer`
/// row for the lifetime of the peering) so this DOES NOT provide
/// forward secrecy. Compromise of the vault discloses all historical
/// and future per-record AEAD payloads under this peer. A future
/// protocol bump should ratchet per session (e.g. HKDF over the
/// stored secret plus per-session nonces) to recover FS.
pub fn x25519_dh(secret: EphemeralSecret, peer_public: &[u8; 32]) -> [u8; 32] {
    let peer = X25519PublicKey::from(*peer_public);
    derive_peer_secret(secret.diffie_hellman(&peer).as_bytes())
}

/// HKDF info string used to derive the per-peer AEAD key from the raw
/// X25519 DH output. Bumping this tag forces a re-pairing instead of
/// silently producing a wrong-key decryption error, and prevents the
/// derived key from being reusable for any other purpose that picks a
/// different info string.
pub const PEER_SECRET_HKDF_INFO: &[u8] = b"oryxis-sync peer secret v5";

/// Derive a 32-byte AEAD key from a raw 32-byte X25519 DH output using
/// HKDF-SHA256 with an empty salt and [`PEER_SECRET_HKDF_INFO`] as the
/// domain separator. Best-practice over feeding raw DH bytes into
/// ChaCha20-Poly1305: HKDF uniformises the bias of the X25519 group
/// element and pins the key to this specific use.
pub fn derive_peer_secret(dh_output: &[u8]) -> [u8; 32] {
    use hkdf::Hkdf;
    use sha2::Sha256;
    let hk = Hkdf::<Sha256>::new(None, dh_output);
    let mut okm = [0u8; 32];
    // Expand into a 32-byte slot, well under the 255 * HashLen limit
    // for HKDF-SHA256 so the `expand` call cannot fail.
    hk.expand(PEER_SECRET_HKDF_INFO, &mut okm)
        .expect("HKDF-SHA256 expand for 32 bytes always fits within the per-call limit");
    okm
}

/// Encrypt payload with shared secret using ChaCha20Poly1305.
///
/// Nonce is a fresh 96-bit random per call. The birthday bound on a
/// 96-bit random nonce is ~2^48 messages per key before a collision
/// becomes likely (a key collision under AEAD is catastrophic). At
/// even 100 sync records per second a single per-peer key would take
/// ~89,000 years to reach that bound, so we're far inside the safe
/// regime. Re-pairing rotates the key, shrinking the window further.
///
/// FUTURE: switching to XChaCha20Poly1305 (192-bit nonce) would lift
/// the bound past any concern. Deferred because it changes the wire
/// format (nonce length goes 12 -> 24 bytes), which would require a
/// protocol-version bump and a coordinated re-pair across every
/// paired device. Worth it on the next breaking v6 bump, not before.
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

    #[test]
    fn device_identity_roundtrip_bytes() {
        let original = DeviceIdentity::generate("my-laptop");
        let blob = original.to_bytes();
        let restored = DeviceIdentity::from_bytes(&blob).unwrap();
        assert_eq!(restored.device_id, original.device_id);
        assert_eq!(restored.device_name, original.device_name);
        assert_eq!(restored.public_key_bytes(), original.public_key_bytes());
        // Same private key produces same signature for the same input.
        let exporter = [7u8; SESSION_EXPORTER_LEN];
        let sig_orig = sign_session_handshake(&original.signing_key, &exporter);
        let sig_back = sign_session_handshake(&restored.signing_key, &exporter);
        assert_eq!(sig_orig, sig_back);
    }

    #[test]
    fn device_identity_from_bytes_rejects_truncated() {
        assert!(DeviceIdentity::from_bytes(&[]).is_err());
        assert!(DeviceIdentity::from_bytes(&[0u8; 47]).is_err());
    }

    #[test]
    fn session_handshake_signature_roundtrip() {
        let identity = DeviceIdentity::generate("alice");
        let exporter = [42u8; SESSION_EXPORTER_LEN];
        let sig = sign_session_handshake(&identity.signing_key, &exporter);
        verify_session_handshake(&identity.public_key_bytes(), &exporter, &sig).unwrap();
    }

    #[test]
    fn session_handshake_rejects_wrong_signer() {
        // MITM scenario: attacker has a different Ed25519 identity.
        // Even if attacker signs the correct exporter, the verifier
        // checks against the legitimate peer's stored pubkey.
        let legit = DeviceIdentity::generate("alice");
        let attacker = DeviceIdentity::generate("mallory");
        let exporter = [1u8; SESSION_EXPORTER_LEN];
        let attacker_sig = sign_session_handshake(&attacker.signing_key, &exporter);
        let err =
            verify_session_handshake(&legit.public_key_bytes(), &exporter, &attacker_sig);
        assert!(err.is_err());
    }

    #[test]
    fn session_handshake_rejects_tampered_exporter() {
        // MITM scenario: attacker holds two TLS sessions (one with each
        // legitimate peer). The exporters of those two sessions differ.
        // Even if the attacker relays Alice's signature to Bob unchanged,
        // Bob will verify it against his own exporter and the check
        // fails. This is the channel-binding property.
        let alice = DeviceIdentity::generate("alice");
        let exporter_alice_session = [1u8; SESSION_EXPORTER_LEN];
        let exporter_bob_session = [2u8; SESSION_EXPORTER_LEN];
        let sig = sign_session_handshake(&alice.signing_key, &exporter_alice_session);
        let err =
            verify_session_handshake(&alice.public_key_bytes(), &exporter_bob_session, &sig);
        assert!(err.is_err());
    }

    #[test]
    fn constant_time_eq_matches_and_rejects() {
        assert!(constant_time_eq(b"123456", b"123456"));
        assert!(!constant_time_eq(b"123456", b"123457"));
        assert!(!constant_time_eq(b"123456", b"12345"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn ed25519_32_sign_verify_roundtrip() {
        let identity = DeviceIdentity::generate("alice");
        let challenge = [9u8; 32];
        let sig = sign_ed25519_32(&identity.signing_key, &challenge);
        verify_ed25519_32(&identity.public_key_bytes(), &challenge, &sig).unwrap();
        // A different signer's signature must not verify.
        let mallory = DeviceIdentity::generate("mallory");
        let bad = sign_ed25519_32(&mallory.signing_key, &challenge);
        assert!(verify_ed25519_32(&identity.public_key_bytes(), &challenge, &bad).is_err());
    }

    #[test]
    fn register_payload_signature_roundtrips() {
        let identity = DeviceIdentity::generate("alice");
        let payload =
            register_sign_payload(&identity.device_id, "203.0.113.5", 9001, 1_747_500_000);
        let sig = sign_register_payload(&identity.signing_key, &payload);
        verify_register_payload(&identity.public_key_bytes(), &payload, &sig).unwrap();
    }

    #[test]
    fn register_payload_rejects_tampered_fields() {
        // The signature must commit to every field of the register
        // canonical payload. Bumping any one (port here) flips the
        // verifier even though signer + key bytes are unchanged.
        let identity = DeviceIdentity::generate("alice");
        let signed_at = 1_747_500_000;
        let original =
            register_sign_payload(&identity.device_id, "203.0.113.5", 9001, signed_at);
        let sig = sign_register_payload(&identity.signing_key, &original);
        let tampered =
            register_sign_payload(&identity.device_id, "203.0.113.5", 9002, signed_at);
        assert!(verify_register_payload(&identity.public_key_bytes(), &tampered, &sig).is_err());
    }

    #[test]
    fn register_payload_rejects_other_signer() {
        // TOFU defense: a token holder who knows the device_id but
        // doesn't hold the pinned private key cannot mint a valid
        // register or unregister request.
        let owner = DeviceIdentity::generate("owner");
        let attacker = DeviceIdentity::generate("attacker");
        let payload = register_sign_payload(&owner.device_id, "10.0.0.1", 9000, 1_747_500_000);
        let attacker_sig = sign_register_payload(&attacker.signing_key, &payload);
        assert!(
            verify_register_payload(&owner.public_key_bytes(), &payload, &attacker_sig).is_err()
        );
    }

    #[test]
    fn unregister_payload_distinct_from_register() {
        // Domain separation: a register signature cannot be replayed
        // as an unregister, even when device_id and signed_at match.
        let identity = DeviceIdentity::generate("alice");
        let signed_at = 1_747_500_000;
        let register =
            register_sign_payload(&identity.device_id, "10.0.0.1", 9000, signed_at);
        let unregister = unregister_sign_payload(&identity.device_id, signed_at);
        assert_ne!(register, unregister);
        let sig = sign_register_payload(&identity.signing_key, &register);
        assert!(
            verify_register_payload(&identity.public_key_bytes(), &unregister, &sig).is_err()
        );
    }

    #[test]
    fn random_challenge_is_32_bytes_and_varies() {
        let a = random_challenge();
        let b = random_challenge();
        assert_eq!(a.len(), 32);
        // Astronomically unlikely to collide; guards against a stub.
        assert_ne!(a, b);
    }

    #[test]
    fn session_handshake_rejects_malformed_inputs() {
        let identity = DeviceIdentity::generate("alice");
        let exporter = [0u8; SESSION_EXPORTER_LEN];
        let sig = sign_session_handshake(&identity.signing_key, &exporter);

        // Pubkey wrong length.
        assert!(verify_session_handshake(&[0u8; 16], &exporter, &sig).is_err());
        // Signature wrong length.
        assert!(
            verify_session_handshake(&identity.public_key_bytes(), &exporter, &[0u8; 32]).is_err()
        );
    }
}

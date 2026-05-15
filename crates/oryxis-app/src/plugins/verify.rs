//! Ed25519 signature verification for downloaded plugin binaries.
//!
//! Verify-only: this module never holds a private key. The dev signs
//! release binaries with `oryxis-plugin-signer` (a separate tool, a
//! later PR); the public key is baked in here as a `const` so a
//! tampered binary can't slip a matching key alongside it.
//!
//! Two trust anchors:
//!
//! - [`PROD_PUBKEY`], the production key, kept in a CI secret. The
//!   only key release builds trust.
//! - [`DEV_PUBKEY`], a development key committed to the repo so the
//!   plugin pipeline can be exercised locally. Trusted *only* in
//!   `debug_assertions` builds.
//!
//! Both are placeholder all-zero keys until the signer tool and
//! keygen script land. An all-zero key is treated as "not
//! provisioned" and dropped from the trust set, so until then the
//! download path verifies nothing and refuses every binary, which is
//! the correct inert state. The dev loop (decision B) runs the
//! plugin straight out of `target/debug/` and never touches this
//! path.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};

use super::PluginError;

/// Production plugin-signing public key. The matching private half
/// lives in the `ORYXIS_SIGNING_KEY` repository secret;
/// `oryxis-plugin-signer` (invoked by `.github/workflows/release-aws.yml`)
/// reads it from there to sign every released plugin binary.
pub const PROD_PUBKEY: [u8; 32] = [
    0x19, 0x33, 0x99, 0xc1, 0xad, 0x91, 0x4b, 0x07, 0xa1, 0x4b, 0xe8, 0xee, 0x7a, 0xec, 0x94,
    0x31, 0xa9, 0x7e, 0x1d, 0xc0, 0xa6, 0x78, 0xdb, 0x38, 0x54, 0xc4, 0x5c, 0x2e, 0xaa, 0xe2,
    0xcd, 0x25,
];

/// Seed for the development signing keypair. Re-exported from the
/// protocol crate (where the signer also reads it) so the dev sign
/// + verify halves are mathematically guaranteed to match.
pub(crate) const DEV_SEED: [u8; 32] =
    oryxis_plugin_protocol::DEV_PLUGIN_SIGNING_SEED;

/// Public half of the dev signing keypair, derived from [`DEV_SEED`]
/// at runtime. Avoids the chicken-and-egg of baking a const that
/// can only be computed at runtime; the seed is statically known
/// and ed25519's keygen is fast enough that the once-per-call
/// derivation is invisible.
fn dev_pubkey() -> [u8; 32] {
    SigningKey::from_bytes(&DEV_SEED).verifying_key().to_bytes()
}

/// The trust anchors active for *this* build: prod always, dev only
/// in debug builds. The placeholder (all-zero) prod key is filtered
/// out so an un-provisioned build trusts nothing rather than
/// appearing to trust a null key.
fn active_pubkeys() -> Vec<[u8; 32]> {
    let mut keys = vec![PROD_PUBKEY];
    if cfg!(debug_assertions) {
        keys.push(dev_pubkey());
    }
    keys.retain(|k| k != &[0u8; 32]);
    keys
}

/// Verify a binary's detached signature against this build's trust
/// anchors. `signature_b64` is the base64 Ed25519 signature straight
/// from the manifest's `signature` field.
///
/// Succeeds if *any* active key validates the signature; fails when
/// none do (or when no keys are provisioned).
pub fn verify(data: &[u8], signature_b64: &str) -> Result<(), PluginError> {
    verify_with_keys(data, signature_b64, &active_pubkeys())
}

/// Core verification against an explicit key set. Split out from
/// [`verify`] so tests can supply a generated key without touching
/// the baked-in trust anchors.
pub fn verify_with_keys(
    data: &[u8],
    signature_b64: &str,
    pubkeys: &[[u8; 32]],
) -> Result<(), PluginError> {
    let sig_bytes = STANDARD
        .decode(signature_b64.trim())
        .map_err(|e| PluginError::Integrity(format!("signature is not valid base64: {e}")))?;
    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|e| PluginError::Integrity(format!("malformed Ed25519 signature: {e}")))?;

    for key_bytes in pubkeys {
        // `verify_strict` rejects low-order keys / malleable
        // signatures, the right choice for a security gate.
        if let Ok(vk) = VerifyingKey::from_bytes(key_bytes)
            && vk.verify_strict(data, &signature).is_ok()
        {
            return Ok(());
        }
    }
    Err(PluginError::Integrity(
        "Ed25519 signature did not match any trusted plugin signing key".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// Build a deterministic keypair from a fixed seed, no RNG
    /// dependency needed for the test.
    fn keypair(seed: u8) -> (SigningKey, [u8; 32]) {
        let sk = SigningKey::from_bytes(&[seed; 32]);
        let pk = sk.verifying_key().to_bytes();
        (sk, pk)
    }

    #[test]
    fn valid_signature_passes() {
        let (sk, pk) = keypair(7);
        let data = b"plugin binary contents";
        let sig = STANDARD.encode(sk.sign(data).to_bytes());
        assert!(verify_with_keys(data, &sig, &[pk]).is_ok());
    }

    #[test]
    fn tampered_data_fails() {
        let (sk, pk) = keypair(7);
        let sig = STANDARD.encode(sk.sign(b"original").to_bytes());
        assert!(verify_with_keys(b"tampered", &sig, &[pk]).is_err());
    }

    #[test]
    fn wrong_key_fails() {
        let (sk, _) = keypair(7);
        let (_, other_pk) = keypair(9);
        let data = b"plugin binary contents";
        let sig = STANDARD.encode(sk.sign(data).to_bytes());
        assert!(verify_with_keys(data, &sig, &[other_pk]).is_err());
    }

    #[test]
    fn any_matching_key_in_set_passes() {
        let (sk, pk) = keypair(7);
        let (_, decoy) = keypair(9);
        let data = b"plugin binary contents";
        let sig = STANDARD.encode(sk.sign(data).to_bytes());
        // Order shouldn't matter, decoy first then the real key.
        assert!(verify_with_keys(data, &sig, &[decoy, pk]).is_ok());
    }

    #[test]
    fn malformed_signature_is_integrity_error() {
        let err = verify_with_keys(b"data", "not%%base64", &[[1u8; 32]]).unwrap_err();
        assert!(matches!(err, PluginError::Integrity(_)));
    }

    #[test]
    fn active_pubkeys_match_build_profile() {
        let keys = active_pubkeys();
        // The prod key is real now; both build profiles trust it.
        assert!(keys.contains(&PROD_PUBKEY), "prod pubkey must be active");
        if cfg!(debug_assertions) {
            // Debug additionally trusts the dev seed for local signing.
            assert_eq!(keys.len(), 2);
            assert!(keys.contains(&dev_pubkey()));
        } else {
            // Release trusts only the prod key.
            assert_eq!(keys.len(), 1);
        }
    }

    /// End-to-end check: a signature made with the dev seed must
    /// pass the real `verify` path in debug builds.
    #[test]
    fn dev_seed_signature_passes_real_verify() {
        if !cfg!(debug_assertions) {
            return;
        }
        let sk = SigningKey::from_bytes(&DEV_SEED);
        let data = b"plugin binary contents";
        let sig = STANDARD.encode(sk.sign(data).to_bytes());
        assert!(verify(data, &sig).is_ok(), "dev-seed sig should verify");
    }
}

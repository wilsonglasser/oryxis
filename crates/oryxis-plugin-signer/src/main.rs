//! `oryxis-plugin-signer`, sign a plugin binary with the Ed25519
//! plugin-signing key and compute the SHA-256 the manifest needs.
//!
//! Two subcommands:
//!
//! - `keygen` generates a fresh ed25519 keypair. Use this once to
//!   produce the production keypair; bake the public half into
//!   `oryxis-app::plugins::verify::PROD_PUBKEY` and store the
//!   private half in the CI secret `ORYXIS_SIGNING_KEY`.
//! - `sign <binary>` signs a binary and prints the SHA-256, the
//!   base64 Ed25519 signature, and the size in bytes, the three
//!   fields the plugin manifest needs. The signing key comes from
//!   `ORYXIS_SIGNING_KEY` (32-byte hex) by default; pass `--dev`
//!   to sign with the committed development seed instead (accepted
//!   only by debug builds of the app).

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use oryxis_plugin_protocol::DEV_PLUGIN_SIGNING_SEED;

const USAGE: &str = "\
usage:
  oryxis-plugin-signer keygen
  oryxis-plugin-signer sign <binary> [--dev]

`sign` reads the signing key from the ORYXIS_SIGNING_KEY environment
variable (32 bytes hex, 64 chars). Pass --dev to use the committed
development seed instead; the dev key is only accepted by debug
builds of the app.";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let Some(cmd) = args.get(1) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match cmd.as_str() {
        "keygen" => {
            keygen();
            ExitCode::SUCCESS
        }
        "sign" => match sign(&args[2..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
        "--help" | "-h" | "help" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("unknown subcommand: {cmd}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Generate a fresh production keypair and print both halves so the
/// caller can bake the public half + store the private half in CI.
fn keygen() {
    let mut rng = rand::rngs::OsRng;
    let sk = SigningKey::generate(&mut rng);
    let pk = sk.verifying_key();
    println!("# Fresh ed25519 plugin-signing keypair.");
    println!("#");
    println!("# Bake the public half into PROD_PUBKEY in");
    println!("# crates/oryxis-app/src/plugins/verify.rs:");
    println!("public_hex   = {}", hex(&pk.to_bytes()));
    println!("public_bytes = {}", rust_array(&pk.to_bytes()));
    println!("#");
    println!("# Store the private half in the ORYXIS_SIGNING_KEY CI secret.");
    println!("# Keep it OUT of source control.");
    println!("private_hex  = {}", hex(&sk.to_bytes()));
}

fn sign(args: &[String]) -> Result<(), String> {
    let Some(binary) = args.first() else {
        return Err("missing <binary> argument".into());
    };
    let dev = args.iter().any(|a| a == "--dev");

    let sk = if dev {
        SigningKey::from_bytes(&DEV_PLUGIN_SIGNING_SEED)
    } else {
        let key_hex = env::var("ORYXIS_SIGNING_KEY").map_err(|_| {
            "ORYXIS_SIGNING_KEY not set; pass --dev to sign with the development seed".to_string()
        })?;
        let bytes = decode_hex(&key_hex)
            .ok_or_else(|| "ORYXIS_SIGNING_KEY must be hex (only 0-9 a-f)".to_string())?;
        if bytes.len() != 32 {
            return Err(format!(
                "ORYXIS_SIGNING_KEY: expected 32 bytes, got {}",
                bytes.len()
            ));
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&bytes);
        SigningKey::from_bytes(&seed)
    };

    let path = Path::new(binary);
    let data = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let signature = sk.sign(&data);
    let digest = Sha256::digest(&data);

    // Three fields the manifest needs, one per line so a workflow can
    // grep them out without parsing.
    println!("sha256        = {}", hex(&digest));
    println!("signature_b64 = {}", STANDARD.encode(signature.to_bytes()));
    println!("size          = {}", data.len());
    Ok(())
}

/// Lowercase hex encoding, rolled by hand to avoid a `hex` crate dep.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Render `bytes` as a Rust array literal `[0xAA, 0xBB, ...]` for
/// pasting straight into a `const [u8; 32]`.
fn rust_array(bytes: &[u8]) -> String {
    let mut s = String::from("[");
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!("0x{b:02x}"));
    }
    s.push(']');
    s
}

/// Decode lowercase or uppercase hex into raw bytes. `None` on any
/// invalid character or odd length.
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        assert_eq!(hex(&[0xab, 0x10, 0x00, 0xff]), "ab1000ff");
        assert_eq!(decode_hex("ab1000ff"), Some(vec![0xab, 0x10, 0x00, 0xff]));
        assert_eq!(decode_hex(""), Some(vec![]));
    }

    #[test]
    fn decode_hex_rejects_garbage() {
        assert!(decode_hex("xyz").is_none()); // non-hex chars
        assert!(decode_hex("abc").is_none()); // odd length
    }

    #[test]
    fn rust_array_format() {
        assert_eq!(rust_array(&[0x01, 0xff]), "[0x01, 0xff]");
        assert_eq!(rust_array(&[]), "[]");
    }
}

//! Full-vault snapshot encode/decode for the SFTP sync transport.
//!
//! P2P sync negotiates a delta over a live QUIC/relay session. The SFTP
//! transport has no peer to talk to, only a file: it treats that file as
//! a "virtual peer" and exchanges the whole vault state every round. The
//! reconciliation itself is identical to P2P, this module just reuses the
//! manifest bricks (`build_manifest` / `collect_records` / `apply_records`)
//! to turn the vault into one sealed blob and back.
//!
//! A round on a device is: download the remote blob, [`merge_snapshot`]
//! it into the local vault (LWW + tombstones, same as a `DeltaPush`),
//! then [`build_full_snapshot`] the now-merged vault and upload it. Each
//! device keeps its own local state and tombstones, so a lost upload race
//! self-heals on the next round.

use std::sync::Arc;
use std::sync::Mutex;

use oryxis_vault::VaultStore;

use crate::crypto;
use crate::engine::{apply_records, build_manifest, collect_records};
use crate::error::SyncError;
use crate::protocol::{DeltaRef, SyncRecord};

/// Header prefixing a sealed snapshot. A wrong-format or truncated file
/// then fails on the magic/version check instead of being fed to the
/// AEAD and surfacing as an opaque decrypt error.
const SNAPSHOT_MAGIC: &[u8; 6] = b"ORXSNP";
const SNAPSHOT_VERSION: u16 = 1;
const HEADER_LEN: usize = SNAPSHOT_MAGIC.len() + 2;

/// Serialize the entire vault into one encrypted snapshot blob.
///
/// The manifest covers every live entity plus every tombstone, so the
/// snapshot carries deletions the same way a P2P delta does. Each
/// `SyncRecord` payload is already AEAD-sealed per entity by
/// [`collect_records`]; the outer seal here also covers the record list
/// itself so entity ids, types and timestamps don't sit in clear on the
/// remote host.
pub fn build_full_snapshot(
    vault: &Arc<Mutex<VaultStore>>,
    secret: &[u8; 32],
) -> Result<Vec<u8>, SyncError> {
    let manifest = build_manifest(vault)?;
    let needed: Vec<DeltaRef> = manifest
        .iter()
        .map(|e| DeltaRef {
            entity_type: e.entity_type,
            entity_id: e.entity_id,
        })
        .collect();
    let records = collect_records(vault, &needed, Some(secret))?;
    let json = serde_json::to_vec(&records)
        .map_err(|e| SyncError::Protocol(format!("snapshot encode: {e}")))?;
    let sealed = crypto::encrypt_payload(&json, secret)?;

    let mut out = Vec::with_capacity(HEADER_LEN + sealed.len());
    out.extend_from_slice(SNAPSHOT_MAGIC);
    out.extend_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Decrypt a snapshot blob and merge its records into the local vault via
/// the same LWW path as an incoming `DeltaPush`. Returns the number of
/// records carried by the snapshot (not all are necessarily applied, the
/// defensive LWW in `apply_records` skips any record that isn't strictly
/// newer than the local copy).
///
/// A decrypt failure (wrong passphrase, corrupt file) returns an error
/// and leaves the vault untouched, so a caller must NOT push a fresh
/// snapshot after a failed merge or it would clobber good remote data
/// with a vault that never absorbed the remote state.
pub fn merge_snapshot(
    vault: &Arc<Mutex<VaultStore>>,
    blob: &[u8],
    secret: &[u8; 32],
) -> Result<usize, SyncError> {
    let body = parse_header(blob)?;
    let json = crypto::decrypt_payload(body, secret)?;
    let records: Vec<SyncRecord> = serde_json::from_slice(&json)
        .map_err(|e| SyncError::Protocol(format!("snapshot decode: {e}")))?;
    let count = records.len();
    apply_records(vault, &records, Some(secret))?;
    Ok(count)
}

/// Validate the snapshot header and return the sealed body that follows
/// it. A short or wrong-magic buffer is a hard error; an unknown version
/// is rejected rather than guessed at.
fn parse_header(blob: &[u8]) -> Result<&[u8], SyncError> {
    if blob.len() < HEADER_LEN {
        return Err(SyncError::Protocol("snapshot too short".into()));
    }
    if &blob[..SNAPSHOT_MAGIC.len()] != SNAPSHOT_MAGIC {
        return Err(SyncError::Protocol("snapshot bad magic".into()));
    }
    let version = u16::from_le_bytes([blob[6], blob[7]]);
    if version != SNAPSHOT_VERSION {
        return Err(SyncError::Protocol(format!(
            "snapshot version {version} unsupported"
        )));
    }
    Ok(&blob[HEADER_LEN..])
}

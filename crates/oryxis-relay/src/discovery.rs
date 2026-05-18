//! Signaling table: `device_id -> (ip, port, public_key_fp)`.
//!
//! Mirrors the `/register`, `/lookup/:id`, `/register/:id` endpoints
//! the Cloudflare Worker serves. Entries TTL out after
//! `REGISTER_TTL` (5min); clients re-register from `engine::start`
//! every 3min so a healthy peer is always present.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

pub const REGISTER_TTL: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub device_id: Uuid,
    #[serde(default)]
    pub public_key_fp: String,
    pub ip: String,
    pub port: u16,
    /// ISO-8601; sent back to clients verbatim.
    pub registered_at: String,
}

struct StoredRecord {
    record: DeviceRecord,
    inserted_at: Instant,
    /// 32-byte Ed25519 verifying key pinned at first register (TOFU).
    /// Any later register or unregister for `device_id` must arrive
    /// signed by the same key; otherwise the request is rejected as
    /// a hijack attempt by another bearer-token holder.
    pinned_pubkey: [u8; 32],
}

/// Outcome of a `register` call against the TOFU device table.
#[derive(Debug, PartialEq, Eq)]
pub enum RegisterOutcome {
    /// Either a fresh device_id, or a re-register whose pubkey matches
    /// the previously stored one.
    Accepted,
    /// `device_id` already exists with a different pinned pubkey. The
    /// caller should respond 403 and surface a warning in metrics.
    PubkeyMismatch,
}

#[derive(Default, Clone)]
pub struct DeviceTable {
    inner: Arc<Mutex<HashMap<Uuid, StoredRecord>>>,
}

impl DeviceTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a device with TOFU pubkey pinning. The first register
    /// for a given `device_id` stores `pubkey`; later registers that
    /// arrive with a different pubkey are rejected with
    /// [`RegisterOutcome::PubkeyMismatch`] so a token holder cannot
    /// hijack someone else's `device_id`. An expired entry behaves the
    /// same as no entry: the registration is accepted and re-pins.
    pub async fn register(&self, record: DeviceRecord, pubkey: [u8; 32]) -> RegisterOutcome {
        let mut map = self.inner.lock().await;
        if let Some(existing) = map.get(&record.device_id)
            && existing.inserted_at.elapsed() <= REGISTER_TTL
            && existing.pinned_pubkey != pubkey
        {
            return RegisterOutcome::PubkeyMismatch;
        }
        map.insert(record.device_id, StoredRecord {
            record,
            inserted_at: Instant::now(),
            pinned_pubkey: pubkey,
        });
        RegisterOutcome::Accepted
    }

    pub async fn lookup(&self, device_id: &Uuid) -> Option<DeviceRecord> {
        let mut map = self.inner.lock().await;
        // Expire lazily on read so a stale entry never escapes.
        if let Some(stored) = map.get(device_id)
            && stored.inserted_at.elapsed() > REGISTER_TTL
        {
            map.remove(device_id);
            return None;
        }
        map.get(device_id).map(|s| s.record.clone())
    }

    /// Return the pinned pubkey for a live entry, or `None` if the
    /// device has no current registration. Used by the unregister
    /// handler to verify the caller signed with the pinned key.
    pub async fn pinned_pubkey(&self, device_id: &Uuid) -> Option<[u8; 32]> {
        let map = self.inner.lock().await;
        map.get(device_id).map(|s| s.pinned_pubkey)
    }

    pub async fn unregister(&self, device_id: &Uuid) {
        self.inner.lock().await.remove(device_id);
    }

    pub fn spawn_sweeper(self, interval: Duration) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(
                tokio::time::MissedTickBehavior::Delay,
            );
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let mut map = self.inner.lock().await;
                map.retain(|_, stored| {
                    stored.inserted_at.elapsed() <= REGISTER_TTL
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_lookup_unregister() {
        let table = DeviceTable::new();
        let id = Uuid::new_v4();
        let pk = [7u8; 32];
        let outcome = table
            .register(
                DeviceRecord {
                    device_id: id,
                    public_key_fp: "fp".into(),
                    ip: "203.0.113.5".into(),
                    port: 9001,
                    registered_at: "2026-05-14T00:00:00Z".into(),
                },
                pk,
            )
            .await;
        assert_eq!(outcome, RegisterOutcome::Accepted);
        let r = table.lookup(&id).await.unwrap();
        assert_eq!(r.port, 9001);
        assert_eq!(table.pinned_pubkey(&id).await, Some(pk));

        table.unregister(&id).await;
        assert!(table.lookup(&id).await.is_none());
    }

    #[tokio::test]
    async fn re_register_with_different_pubkey_is_rejected() {
        let table = DeviceTable::new();
        let id = Uuid::new_v4();
        let pk_a = [1u8; 32];
        let pk_b = [2u8; 32];
        let rec = || DeviceRecord {
            device_id: id,
            public_key_fp: "fp".into(),
            ip: "203.0.113.5".into(),
            port: 9001,
            registered_at: "2026-05-14T00:00:00Z".into(),
        };
        assert_eq!(table.register(rec(), pk_a).await, RegisterOutcome::Accepted);
        assert_eq!(
            table.register(rec(), pk_b).await,
            RegisterOutcome::PubkeyMismatch
        );
        // Re-register with the same key still succeeds.
        assert_eq!(table.register(rec(), pk_a).await, RegisterOutcome::Accepted);
    }
}

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
}

#[derive(Default, Clone)]
pub struct DeviceTable {
    inner: Arc<Mutex<HashMap<Uuid, StoredRecord>>>,
}

impl DeviceTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, record: DeviceRecord) {
        let mut map = self.inner.lock().await;
        map.insert(record.device_id, StoredRecord {
            record,
            inserted_at: Instant::now(),
        });
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
        table.register(DeviceRecord {
            device_id: id,
            public_key_fp: "fp".into(),
            ip: "203.0.113.5".into(),
            port: 9001,
            registered_at: "2026-05-14T00:00:00Z".into(),
        }).await;
        let r = table.lookup(&id).await.unwrap();
        assert_eq!(r.port, 9001);

        table.unregister(&id).await;
        assert!(table.lookup(&id).await.is_none());
    }
}

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownHost {
    pub id: Uuid,
    pub hostname: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub first_seen: chrono::DateTime<chrono::Utc>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

impl KnownHost {
    pub fn new(hostname: &str, port: u16, key_type: &str, fingerprint: &str) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            hostname: hostname.into(),
            port,
            key_type: key_type.into(),
            fingerprint: fingerprint.into(),
            first_seen: now,
            last_seen: now,
        }
    }
}

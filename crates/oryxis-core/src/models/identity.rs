use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub id: Uuid,
    pub label: String,
    pub username: Option<String>,
    // password is NOT stored here — it lives encrypted in the vault DB
    pub key_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Identity {
    pub fn new(label: &str) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.to_string(),
            username: None,
            key_id: None,
            created_at: now,
            updated_at: now,
        }
    }
}

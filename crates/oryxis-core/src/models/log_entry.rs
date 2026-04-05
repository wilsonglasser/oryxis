use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: Uuid,
    pub connection_label: String,
    pub hostname: String,
    pub event: LogEvent,
    pub message: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogEvent {
    Connected,
    Disconnected,
    AuthFailed,
    Error,
}

impl std::fmt::Display for LogEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected => write!(f, "Connected"),
            Self::Disconnected => write!(f, "Disconnected"),
            Self::AuthFailed => write!(f, "Auth Failed"),
            Self::Error => write!(f, "Error"),
        }
    }
}

impl LogEntry {
    pub fn new(label: &str, hostname: &str, event: LogEvent, message: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            connection_label: label.into(),
            hostname: hostname.into(),
            event,
            message: message.into(),
            timestamp: chrono::Utc::now(),
        }
    }
}

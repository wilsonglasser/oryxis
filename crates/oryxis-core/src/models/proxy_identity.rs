use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::connection::ProxyType;

/// A reusable proxy configuration that can be attached to multiple
/// hosts via `Connection.proxy_identity_id`. Mirrors the `Identity`
/// type for credentials — same lifecycle (create, edit, delete with
/// cascade null), same encryption strategy (password lives in a
/// dedicated encrypted column, never in serialized JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyIdentity {
    pub id: Uuid,
    pub label: String,
    pub proxy_type: ProxyType,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    // password is NOT stored here — it lives encrypted in the vault DB
    // (`proxy_identities.password` column).
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl ProxyIdentity {
    pub fn new(label: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            // SOCKS5 is the most common default for end-user proxies;
            // the editor lets the user change it before saving.
            proxy_type: ProxyType::Socks5,
            host: String::new(),
            port: 1080,
            username: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_proxy_identity_defaults() {
        let pi = ProxyIdentity::new("home-bastion");
        assert_eq!(pi.label, "home-bastion");
        assert_eq!(pi.proxy_type, ProxyType::Socks5);
        assert_eq!(pi.port, 1080);
        assert!(pi.host.is_empty());
        assert!(pi.username.is_none());
    }

    #[test]
    fn proxy_identity_serialization_roundtrip() {
        let mut pi = ProxyIdentity::new("corp-http");
        pi.proxy_type = ProxyType::Http;
        pi.host = "proxy.corp.local".into();
        pi.port = 8080;
        pi.username = Some("alice".into());

        let json = serde_json::to_string(&pi).unwrap();
        let de: ProxyIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(de.label, "corp-http");
        assert_eq!(de.proxy_type, ProxyType::Http);
        assert_eq!(de.host, "proxy.corp.local");
        assert_eq!(de.port, 8080);
        assert_eq!(de.username.as_deref(), Some("alice"));
    }
}

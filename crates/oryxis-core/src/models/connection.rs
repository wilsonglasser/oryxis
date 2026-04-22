use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: Uuid,
    pub label: String,
    pub hostname: String,
    pub port: u16,
    pub username: Option<String>,
    pub auth_method: AuthMethod,
    pub key_id: Option<Uuid>,
    pub identity_id: Option<Uuid>,
    pub group_id: Option<Uuid>,
    pub jump_chain: Vec<Uuid>,
    pub proxy: Option<ProxyConfig>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub port_forwards: Vec<PortForward>,
    pub mcp_enabled: bool,
    pub last_used: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Detected remote OS id — populated the first time we successfully SSH
    /// into this host and the OS-detection setting is enabled. Values are
    /// lowercase `ID=` from `/etc/os-release` for Linux (ubuntu / debian /
    /// alpine / rhel / fedora / arch / amzn / centos / rocky / alma / suse)
    /// or `uname -s` lowercased for non-Linux (darwin / freebsd / openbsd /
    /// netbsd). `None` means unknown — show the generic server icon.
    #[serde(default)]
    pub detected_os: Option<String>,
    /// User-chosen icon id (overrides the auto-detected one). When present,
    /// the OS-detection probe is skipped and the stored icon / color are
    /// used verbatim on host cards / tabs / editor.
    #[serde(default)]
    pub custom_icon: Option<String>,
    /// User-chosen icon-background color as a hex string (e.g. `#E95420`).
    /// Paired with `custom_icon`.
    #[serde(default)]
    pub custom_color: Option<String>,
}

impl Connection {
    pub fn new(label: impl Into<String>, hostname: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            hostname: hostname.into(),
            port: 22,
            username: None,
            auth_method: AuthMethod::Auto,
            key_id: None,
            identity_id: None,
            group_id: None,
            jump_chain: Vec::new(),
            port_forwards: Vec::new(),
            proxy: None,
            tags: Vec::new(),
            notes: None,
            color: None,
            mcp_enabled: true,
            last_used: None,
            created_at: now,
            updated_at: now,
            detected_os: None,
            custom_icon: None,
            custom_color: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum AuthMethod {
    #[default]
    Auto,
    Password,
    Key,
    Agent,
    Interactive,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortForward {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub proxy_type: ProxyType,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProxyType {
    Socks5,
    Socks4,
    Http,
    Command(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_connection_defaults() {
        let conn = Connection::new("test", "10.0.0.1");
        assert_eq!(conn.label, "test");
        assert_eq!(conn.hostname, "10.0.0.1");
        assert_eq!(conn.port, 22);
        assert_eq!(conn.auth_method, AuthMethod::Auto);
        assert!(conn.username.is_none());
        assert!(conn.jump_chain.is_empty());
        assert!(conn.proxy.is_none());
    }

    #[test]
    fn connection_serialization_roundtrip() {
        let mut conn = Connection::new("prod", "server.example.com");
        conn.username = Some("deploy".into());
        conn.auth_method = AuthMethod::Key;
        conn.tags = vec!["production".into(), "web".into()];

        let json = serde_json::to_string(&conn).unwrap();
        let deserialized: Connection = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.label, "prod");
        assert_eq!(deserialized.hostname, "server.example.com");
        assert_eq!(deserialized.username, Some("deploy".into()));
        assert_eq!(deserialized.auth_method, AuthMethod::Key);
        assert_eq!(deserialized.tags.len(), 2);
    }

    #[test]
    fn proxy_config_serialization() {
        let proxy = ProxyConfig {
            proxy_type: ProxyType::Socks5,
            host: "proxy.local".into(),
            port: 1080,
            username: Some("user".into()),
        };

        let json = serde_json::to_string(&proxy).unwrap();
        let de: ProxyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(de.proxy_type, ProxyType::Socks5);
        assert_eq!(de.port, 1080);
    }

    #[test]
    fn auth_method_variants() {
        assert_eq!(serde_json::to_string(&AuthMethod::Auto).unwrap(), "\"Auto\"");
        assert_eq!(serde_json::to_string(&AuthMethod::Password).unwrap(), "\"Password\"");
        assert_eq!(serde_json::to_string(&AuthMethod::Key).unwrap(), "\"Key\"");
        assert_eq!(serde_json::to_string(&AuthMethod::Agent).unwrap(), "\"Agent\"");
        assert_eq!(serde_json::to_string(&AuthMethod::Interactive).unwrap(), "\"Interactive\"");
    }
}

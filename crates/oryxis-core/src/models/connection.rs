use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::cloud::CloudRef;

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
    /// Reference to a saved `ProxyIdentity`. When set, takes precedence
    /// over the inline `proxy` field — the SSH engine resolves the
    /// identity (via the vault) and ignores `proxy`. `None` falls back
    /// to inline. Cleared on cascade when the identity is deleted.
    #[serde(default)]
    pub proxy_identity_id: Option<Uuid>,
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
    /// Forward the local ssh-agent socket to the remote shell. When
    /// enabled, after the session channel is open we send an
    /// `auth-agent-req@openssh.com` request; sshd then sets
    /// `SSH_AUTH_SOCK` on the remote side and tunnels back any reads
    /// from that socket through this SSH transport. Lets the user
    /// `ssh hostB` from inside hostA without staging keys remotely.
    #[serde(default)]
    pub agent_forwarding: bool,
    /// Per-host terminal palette override. When set, takes precedence
    /// over the global `terminal_theme_override` setting and the app
    /// theme fallback. Stored as `TerminalTheme::name()` (e.g.
    /// "Dracula", "Monokai") so the value survives palette additions
    /// without a migration. `None` falls through to the global pick.
    #[serde(default)]
    pub terminal_theme: Option<String>,
    /// Set on hosts imported from a cloud profile (EC2 in v0.6). Carries
    /// the stable resource handle so the connect path can re-resolve
    /// hostname / pick the right transport on each session. `None` for
    /// manually-added hosts.
    #[serde(default)]
    pub cloud_ref: Option<CloudRef>,
    /// Sent to the remote shell right after the session opens. Used to
    /// escape minimal entry shells (`exec bash` on ECS / distroless) or
    /// to drop into a specific working directory. `None` skips the step.
    #[serde(default)]
    pub initial_command: Option<String>,
    /// Per-host SSH keepalive override (seconds). `None` inherits the
    /// global `keepalive_interval` setting. `Some(0)` explicitly disables
    /// keepalive on this host even when the global default is non-zero.
    /// `Some(n)` overrides the global with `n` seconds.
    #[serde(default)]
    pub keepalive_interval: Option<u32>,
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
            proxy_identity_id: None,
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
            agent_forwarding: false,
            terminal_theme: None,
            cloud_ref: None,
            initial_command: None,
            keepalive_interval: None,
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
    /// Proxy password. Hydrated in-memory by the vault
    /// (`get_proxy_password`) right before connect. Marked `serde(skip)`
    /// so it never lands in the `proxy` column (which is plaintext JSON)
    /// — the credential lives in the encrypted `proxy_password` column.
    #[serde(skip)]
    pub password: Option<String>,
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
            password: None,
        };

        let json = serde_json::to_string(&proxy).unwrap();
        let de: ProxyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(de.proxy_type, ProxyType::Socks5);
        assert_eq!(de.port, 1080);
        assert_eq!(de.username.as_deref(), Some("user"));
        assert!(de.password.is_none());
    }

    /// `password` is `serde(skip)` — it must not appear in serialized
    /// JSON nor be read back. This guards against credential leaks via
    /// the plaintext `proxy` column.
    #[test]
    fn proxy_config_password_is_not_serialized() {
        let proxy = ProxyConfig {
            proxy_type: ProxyType::Http,
            host: "proxy.local".into(),
            port: 8080,
            username: Some("u".into()),
            password: Some("topsecret".into()),
        };

        let json = serde_json::to_string(&proxy).unwrap();
        assert!(
            !json.contains("topsecret"),
            "password leaked into ProxyConfig JSON: {json}"
        );
        assert!(
            !json.contains("password"),
            "password key should not appear at all: {json}"
        );

        let de: ProxyConfig = serde_json::from_str(&json).unwrap();
        assert!(de.password.is_none());
    }

    /// Legacy peers (sync wire) and old portable exports never carried
    /// the `keepalive_interval` field. Receiving such a payload must
    /// deserialize cleanly with the field defaulting to `None` (= inherit
    /// global). Without `#[serde(default)]` on the field, this would
    /// regress the moment a v1 peer talks to a v2 peer.
    #[test]
    fn keepalive_interval_legacy_payload_defaults_to_none() {
        let conn = Connection::new("legacy", "10.0.0.1");
        let mut value = serde_json::to_value(&conn).unwrap();
        // Simulate a payload from a peer that never knew about the field.
        value.as_object_mut().unwrap().remove("keepalive_interval");
        let de: Connection = serde_json::from_value(value).unwrap();
        assert_eq!(de.keepalive_interval, None);
    }

    #[test]
    fn keepalive_interval_round_trip() {
        let mut conn = Connection::new("h", "1.2.3.4");
        conn.keepalive_interval = Some(45);
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(de.keepalive_interval, Some(45));

        // Explicit zero must round-trip distinctly from None — they have
        // different semantics (per-host disable vs. inherit global).
        conn.keepalive_interval = Some(0);
        let json = serde_json::to_string(&conn).unwrap();
        let de: Connection = serde_json::from_str(&json).unwrap();
        assert_eq!(de.keepalive_interval, Some(0));
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

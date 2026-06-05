use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Direction of an SSH port forward, mirroring the `ssh -L/-R/-D` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForwardKind {
    /// `-L`: a local listener tunnels to a destination reached from the server.
    Local,
    /// `-R`: a server-side listener tunnels back to a destination reached from
    /// this client.
    Remote,
    /// `-D`: a local SOCKS5 listener; the destination is chosen per-connection
    /// by the SOCKS client.
    Dynamic,
}

impl ForwardKind {
    /// All variants, in editor display order.
    pub const ALL: [ForwardKind; 3] = [
        ForwardKind::Local,
        ForwardKind::Remote,
        ForwardKind::Dynamic,
    ];

    /// Short text token used as the on-disk and on-wire representation.
    pub fn as_token(self) -> &'static str {
        match self {
            ForwardKind::Local => "local",
            ForwardKind::Remote => "remote",
            ForwardKind::Dynamic => "dynamic",
        }
    }

    /// Parse the token written by [`as_token`]. Unknown tokens fall back to
    /// `Local` so a corrupt row never breaks the whole list.
    pub fn from_token(s: &str) -> ForwardKind {
        match s {
            "remote" => ForwardKind::Remote,
            "dynamic" => ForwardKind::Dynamic,
            _ => ForwardKind::Local,
        }
    }

    /// Dynamic forwards have no fixed target (the SOCKS client picks it).
    pub fn has_target(self) -> bool {
        !matches!(self, ForwardKind::Dynamic)
    }
}

impl fmt::Display for ForwardKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ForwardKind::Local => "Local (-L)",
            ForwardKind::Remote => "Remote (-R)",
            ForwardKind::Dynamic => "Dynamic (-D)",
        };
        f.write_str(s)
    }
}

/// A standalone port forward, independent of any terminal session. Turning a
/// rule on opens a dedicated SSH connection (no PTY) that holds the tunnel
/// until it is turned off. Persisted in the vault; the on/off runtime state is
/// not (it lives only in app memory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForwardRule {
    pub id: Uuid,
    pub label: String,
    pub kind: ForwardKind,
    /// The `Connection` whose auth + transport carries this forward.
    pub host_id: Uuid,
    /// Bind interface for the listener. For `Local`/`Dynamic` this is local
    /// (`127.0.0.1` or `0.0.0.0`); for `Remote` it is the server-side bind
    /// (`0.0.0.0` needs `GatewayPorts yes` on the remote `sshd`).
    pub listen_host: String,
    pub listen_port: u16,
    /// Destination host. `Local`: reached from the server. `Remote`: reached
    /// from this client. `Dynamic`: unused.
    pub target_host: String,
    /// Destination port. Unused for `Dynamic`.
    pub target_port: u16,
    /// Start this rule automatically on app boot.
    pub auto_start: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl PortForwardRule {
    pub fn new(label: impl Into<String>, kind: ForwardKind, host_id: Uuid) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            kind,
            host_id,
            listen_host: "127.0.0.1".to_string(),
            listen_port: 0,
            target_host: String::new(),
            target_port: 0,
            auto_start: false,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_kind_token_round_trip() {
        for kind in ForwardKind::ALL {
            assert_eq!(ForwardKind::from_token(kind.as_token()), kind);
        }
        // Unknown tokens fall back to Local rather than panicking.
        assert_eq!(ForwardKind::from_token("bogus"), ForwardKind::Local);
    }

    #[test]
    fn only_dynamic_has_no_target() {
        assert!(ForwardKind::Local.has_target());
        assert!(ForwardKind::Remote.has_target());
        assert!(!ForwardKind::Dynamic.has_target());
    }
}

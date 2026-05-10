//! Persisted cloud-related types embedded in `Connection` / `Group`.
//!
//! These live in `oryxis-core` (not `oryxis-cloud`) so the vault can
//! serialize them without taking on the cloud-provider trait surface.
//! The `oryxis-cloud` crate re-uses these types for the `CloudProvider`
//! trait while keeping its discovery-result + registry types to itself.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How an interactive session is opened to a cloud-managed resource.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TransportKind {
    /// Plain TCP + SSH handshake. Identical to a manual host.
    Ssh,
    /// AWS EC2 Instance Connect — push public key via API, then plain SSH.
    InstanceConnect,
    /// AWS SSM Session Manager. Wraps `session-manager-plugin` over a
    /// streaming WebSocket; PTY bytes flow through the plugin process.
    Ssm,
    /// AWS ECS Exec — same SSM streaming protocol underneath, but
    /// targeted at a specific container in a running task.
    EcsExec,
    /// Kubernetes pod exec via the kube-apiserver. In-process via the
    /// `kube` crate, no subprocess.
    KubectlExec,
}

impl TransportKind {
    /// Whether SFTP / file transfer is meaningful on this transport.
    /// SSM / ECS / kubectl exec deliver a raw PTY only — no SFTP layer.
    pub fn supports_sftp(self) -> bool {
        matches!(self, Self::Ssh | Self::InstanceConnect)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum CloudResourceType {
    Ec2,
}

/// Stable handle to a cloud-managed resource that backs a `Connection`.
///
/// Only set on imported EC2 hosts in v0.6 — ECS tasks and K8s pods are
/// ephemeral and live as transient children of dynamic groups instead
/// (see `Group.cloud_query`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudRef {
    pub profile_id: Uuid,
    pub resource_type: CloudResourceType,
    /// Provider-stable id (EC2: `i-…`).
    pub resource_id: String,
    pub region: Option<String>,
    pub transport_pref: TransportKind,
    /// Re-resolve the public/private hostname from the cloud right
    /// before each connect. Useful when the instance's public IP can
    /// change across stop/start.
    #[serde(default)]
    pub auto_refresh_hostname: bool,
}

/// Backing query of a dynamic `Group`. Children are re-resolved on each
/// expand by calling `CloudProvider::resolve_query`. Children are
/// transient — they never touch the vault. User customization lives on
/// the group's `template` so it applies to every resolved child.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudQuery {
    pub profile_id: Uuid,
    pub kind: CloudQueryKind,
    pub template: ConnectionTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CloudQueryKind {
    EcsTasks {
        cluster: String,
        service: String,
        container: String,
    },
    K8sPods {
        context: String,
        namespace: String,
        selector: PodSelector,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PodSelector {
    /// Free-form `key=value` selector (`app=nginx,tier=frontend`).
    Labels(BTreeMap<String, String>),
    /// Match every pod owned by a Deployment by name.
    Deployment(String),
    /// Match every pod owned by a StatefulSet by name.
    StatefulSet(String),
    /// Single named pod — escape hatch for one-off pinning.
    Name(String),
}

/// Configuration applied to every transient child a dynamic group
/// resolves. Fields mirror their counterparts on `Connection`; an empty
/// `None` means "fall through to whatever default the connect path uses
/// for that field".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionTemplate {
    /// Default username used when the resource doesn't expose one.
    #[serde(default)]
    pub username: Option<String>,
    /// Sent right after the shell opens. `exec bash` is a typical value
    /// for ECS tasks whose default entry shell is `/bin/sh`.
    #[serde(default)]
    pub initial_command: Option<String>,
    /// Forced transport. Always set for dynamic groups (e.g. `EcsExec`
    /// for ECS, `KubectlExec` for K8s) since these resources have no
    /// other reasonable entry point.
    pub transport: TransportKind,
    /// Per-host terminal palette override — same semantics as
    /// `Connection.terminal_theme`.
    #[serde(default)]
    pub terminal_theme: Option<String>,
}

impl ConnectionTemplate {
    pub fn new(transport: TransportKind) -> Self {
        Self {
            username: None,
            initial_command: None,
            transport,
            terminal_theme: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_ref_roundtrip() {
        let r = CloudRef {
            profile_id: Uuid::new_v4(),
            resource_type: CloudResourceType::Ec2,
            resource_id: "i-0abcdef".into(),
            region: Some("us-east-1".into()),
            transport_pref: TransportKind::InstanceConnect,
            auto_refresh_hostname: true,
        };
        let j = serde_json::to_string(&r).unwrap();
        let back: CloudRef = serde_json::from_str(&j).unwrap();
        assert_eq!(back.resource_id, "i-0abcdef");
        assert_eq!(back.transport_pref, TransportKind::InstanceConnect);
        assert!(back.auto_refresh_hostname);
    }

    #[test]
    fn cloud_query_ecs_roundtrip() {
        let q = CloudQuery {
            profile_id: Uuid::new_v4(),
            kind: CloudQueryKind::EcsTasks {
                cluster: "payments".into(),
                service: "api-svc".into(),
                container: "api".into(),
            },
            template: ConnectionTemplate {
                username: None,
                initial_command: Some("exec bash".into()),
                transport: TransportKind::EcsExec,
                terminal_theme: None,
            },
        };
        let j = serde_json::to_string(&q).unwrap();
        let back: CloudQuery = serde_json::from_str(&j).unwrap();
        assert_eq!(back.template.transport, TransportKind::EcsExec);
        assert_eq!(back.template.initial_command.as_deref(), Some("exec bash"));
    }

    #[test]
    fn pod_selector_labels_roundtrip() {
        let mut m = BTreeMap::new();
        m.insert("app".into(), "nginx".into());
        let s = PodSelector::Labels(m);
        let j = serde_json::to_string(&s).unwrap();
        let back: PodSelector = serde_json::from_str(&j).unwrap();
        match back {
            PodSelector::Labels(m) => assert_eq!(m.get("app").map(|s| s.as_str()), Some("nginx")),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn transport_supports_sftp() {
        assert!(TransportKind::Ssh.supports_sftp());
        assert!(TransportKind::InstanceConnect.supports_sftp());
        assert!(!TransportKind::Ssm.supports_sftp());
        assert!(!TransportKind::EcsExec.supports_sftp());
        assert!(!TransportKind::KubectlExec.supports_sftp());
    }
}

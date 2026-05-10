use serde::{Deserialize, Serialize};

/// Result of a one-shot wizard discovery — the user picks a subset and
/// imports it. Discovered EC2s become individual hosts; discovered ECS
/// services / K8s workloads become dynamic groups.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub ec2: Vec<DiscoveredEc2>,
    pub ecs_services: Vec<DiscoveredEcsService>,
    pub k8s_workloads: Vec<DiscoveredK8sWorkload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredEc2 {
    pub instance_id: String,
    pub region: String,
    pub name: Option<String>,
    pub public_dns: Option<String>,
    pub private_dns: Option<String>,
    pub public_ip: Option<String>,
    pub private_ip: Option<String>,
    pub state: String,
    /// Default username inferred from the AMI when known (`ec2-user`,
    /// `ubuntu`, `admin`…). The editor lets the user override.
    pub default_username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredEcsService {
    pub region: String,
    pub cluster: String,
    pub service: String,
    pub container: String,
    /// Number of currently running tasks — purely informational, shown
    /// in the wizard so the user can tell empty services from active ones.
    pub running_task_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredK8sWorkload {
    pub context: String,
    pub namespace: String,
    /// `Deployment(name)` / `StatefulSet(name)` / `DaemonSet(name)`.
    pub kind: String,
    pub name: String,
    pub container: String,
    pub running_pod_count: u32,
}

/// Resolved live host returned by `resolve_query` — used by dynamic
/// groups to render their current children.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredHost {
    /// Display label rendered in the sidebar tree.
    pub label: String,
    /// Identifier the transport needs to actually connect (taskId for
    /// ECS, podName for K8s, instance-id for EC2).
    pub resource_id: String,
    /// Optional per-child overrides surfaced by the provider (zone,
    /// node, etc.) — shown as a subtitle on the row.
    pub subtitle: Option<String>,
}

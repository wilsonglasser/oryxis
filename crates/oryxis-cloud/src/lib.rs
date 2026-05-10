//! Pluggable cloud provider abstraction.
//!
//! A `CloudProvider` exposes two orthogonal capabilities to the rest of
//! Oryxis: discovery (list resources in the upstream cloud) and transport
//! (open an interactive channel to a resource — SSH, SSM, ECS Exec,
//! kubectl exec, etc.).
//!
//! This crate intentionally carries no SDK dependency. Concrete
//! providers live in sibling crates (`oryxis-cloud-aws`, `oryxis-cloud-k8s`)
//! that pull in the heavy SDKs only when their cargo feature is enabled.
//!
//! Persisted types (`CloudRef`, `CloudQuery`, `TransportKind`, …) live
//! in `oryxis-core::models` so the vault can serialize them without
//! depending on this crate.

pub mod error;
pub mod registry;
pub mod resource;

pub use error::CloudError;
pub use registry::{CloudProviderRegistry, RegisteredProvider};
pub use resource::{
    DiscoveredEc2, DiscoveredEcsService, DiscoveredHost, DiscoveredK8sWorkload, DiscoveryResult,
};

// Re-export the persisted types from core so providers don't have to
// pull `oryxis-core` directly for the common case.
pub use oryxis_core::models::{
    CloudProfile, CloudQuery, CloudQueryKind, CloudRef, CloudResourceType, ConnectionTemplate,
    PodSelector, TransportKind,
};

use async_trait::async_trait;

/// Abstraction over a cloud backend (AWS, Kubernetes, GCP, etc.).
///
/// Discovery is split per resource family because the user wires each
/// family into the UI differently — EC2 instances become individual
/// `Connection` rows (manual import), while ECS services and K8s
/// workloads back dynamic `Group`s that re-resolve their children on
/// each expand.
#[async_trait]
pub trait CloudProvider: Send + Sync {
    /// Stable identifier ("aws", "k8s", ...). Used in `CloudProfile.provider`.
    fn id(&self) -> &'static str;

    /// Validate credentials by issuing a single cheap call (`STS GetCallerIdentity`,
    /// `kubectl version`, etc.). Used by the wizard's "Test" step.
    async fn test_credentials(&self, profile: &CloudProfile) -> Result<(), CloudError>;

    /// One-shot discovery for the wizard's "Discover & pick" step. Returns
    /// every supported resource family in the regions / contexts the
    /// profile is scoped to.
    async fn discover(&self, profile: &CloudProfile) -> Result<DiscoveryResult, CloudError>;

    /// Resolve a `CloudQuery` into the current set of live children.
    /// Called on every dynamic-group expand (subject to caller-side cache).
    async fn resolve_query(
        &self,
        profile: &CloudProfile,
        query: &CloudQuery,
    ) -> Result<Vec<DiscoveredHost>, CloudError>;

    /// Transports this provider can open. The UI uses this to populate
    /// the per-host "Transport" picker on cloud-aware editors.
    fn supported_transports(&self, resource_type: CloudResourceType) -> Vec<TransportKind>;
}

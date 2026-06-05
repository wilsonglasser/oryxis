//! `PluginProvider`, a `CloudProvider` backed by a plugin subprocess.
//!
//! This is the bridge between the app's existing `CloudProvider`
//! trait surface and the JSON-RPC plugin world. Every trait method
//! turns into a typed `PluginHost::call`; the host owns the
//! subprocess lifecycle (lazy spawn, idle teardown, restart on
//! crash). The app only ever sees `PluginProvider`, the concrete
//! `AwsProvider` now lives inside the `oryxis-cloud-aws-plugin`
//! binary.

use std::path::PathBuf;

use async_trait::async_trait;
use oryxis_cloud::{
    CloudError, CloudProfile, CloudProvider, CloudQuery, CloudResourceType, DiscoveredHost,
    DiscoveryResult, SessionPayload, TransportKind,
};
use oryxis_plugin_protocol::{
    Discover, ProfileParams, PushInstanceConnectKey, PushInstanceConnectKeyParams, ResolveQuery,
    ResolveQueryParams, StartEcsExec, StartEcsExecParams, StartSsmSession, StartSsmSessionParams,
    TestCredentials,
};

use super::cache;
use super::host::PluginHost;
use super::PluginError;

/// A cloud provider whose operations run in a plugin subprocess.
pub struct PluginProvider {
    /// Provider id. Leaked to a `&'static str` so it satisfies
    /// `CloudProvider::id()`, one tiny, bounded leak per provider
    /// for the whole app lifetime, which beats rippling an owned
    /// `String` through the registry and every call site.
    id: &'static str,
    host: PluginHost,
}

impl PluginProvider {
    /// Build a provider for `provider_id`, resolving its binary and
    /// preparing (but not spawning) the host.
    ///
    // The binary path is resolved once, here. When the update flow
    // flips the cache's `current` pointer, the host needs rebuilding
    // (or a `Fn() -> PathBuf` resolver). Fine for now: both the dev
    // loop and a stable install keep the same path for the process
    // lifetime.
    pub fn new(provider_id: &str) -> Self {
        let id: &'static str = Box::leak(provider_id.to_string().into_boxed_str());
        let binary = resolve_binary(provider_id);
        Self {
            id,
            host: PluginHost::new(binary, provider_id),
        }
    }

    /// Re-resolve the cached binary and repoint the host at it. Called
    /// from the install / update completion path so a fresh version
    /// gets used on the next call without recreating the registered
    /// provider Arc.
    pub async fn rebind(&self) {
        let binary = resolve_binary(self.id);
        self.host.rebind(binary).await;
    }
}

/// Resolve the plugin binary path for a provider.
///
/// Debug builds prefer a freshly-built binary next to the app
/// executable (`target/debug/`), so `cargo build` + restart picks up
/// plugin edits without going through the cache (decision B).
/// Otherwise it's the active cached version; when nothing is
/// installed yet the *expected* cache path is returned so the host's
/// spawn failure reads as a clear `BinaryNotFound`.
fn resolve_binary(provider_id: &str) -> PathBuf {
    #[cfg(debug_assertions)]
    {
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            let dev = dir.join(cache::binary_name(provider_id));
            if dev.exists() {
                return dev;
            }
        }
    }
    cache::current_binary(provider_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            cache::provider_dir(provider_id)
                .map(|d| d.join(cache::binary_name(provider_id)))
                .unwrap_or_else(|_| PathBuf::from(cache::binary_name(provider_id)))
        })
}

/// Collapse a `PluginError` into the `CloudError` the dispatch layer
/// already renders. A provider error passes through with its variant
/// intact; everything else (spawn failed, timeout, protocol drift)
/// becomes `Other` with a message that names the cause.
fn plugin_err_to_cloud(e: PluginError) -> CloudError {
    match e {
        PluginError::Provider(c) => c,
        other => CloudError::Other(format!("plugin: {other}")),
    }
}

#[async_trait]
impl CloudProvider for PluginProvider {
    fn id(&self) -> &'static str {
        self.id
    }

    async fn test_credentials(&self, profile: &CloudProfile) -> Result<(), CloudError> {
        self.host
            .call::<TestCredentials>(ProfileParams {
                profile: profile.clone(),
            })
            .await
            .map_err(plugin_err_to_cloud)
    }

    async fn discover(&self, profile: &CloudProfile) -> Result<DiscoveryResult, CloudError> {
        self.host
            .call::<Discover>(ProfileParams {
                profile: profile.clone(),
            })
            .await
            .map_err(plugin_err_to_cloud)
    }

    async fn resolve_query(
        &self,
        profile: &CloudProfile,
        query: &CloudQuery,
    ) -> Result<Vec<DiscoveredHost>, CloudError> {
        self.host
            .call::<ResolveQuery>(ResolveQueryParams {
                profile: profile.clone(),
                query: query.clone(),
            })
            .await
            .map_err(plugin_err_to_cloud)
    }

    fn supported_transports(&self, resource_type: CloudResourceType) -> Vec<TransportKind> {
        // Static UI metadata, not a live call, the trait method is
        // sync and the app never actually invokes it on this path
        // yet. Answered from the provider id so it stays correct if
        // PR 5 starts using it.
        match (self.id, resource_type) {
            ("aws", CloudResourceType::Ec2) => vec![
                TransportKind::Ssh,
                TransportKind::InstanceConnect,
                TransportKind::Ssm,
            ],
            ("k8s", _) => vec![TransportKind::KubectlExec],
            _ => Vec::new(),
        }
    }

    async fn start_ssm_session(
        &self,
        profile: &CloudProfile,
        region: &str,
        instance_id: &str,
    ) -> Result<SessionPayload, CloudError> {
        self.host
            .call::<StartSsmSession>(StartSsmSessionParams {
                profile: profile.clone(),
                region: region.to_string(),
                instance_id: instance_id.to_string(),
            })
            .await
            .map_err(plugin_err_to_cloud)
    }

    async fn start_ecs_exec(
        &self,
        profile: &CloudProfile,
        region: &str,
        cluster: &str,
        task_id: &str,
        container: &str,
        command: &str,
    ) -> Result<SessionPayload, CloudError> {
        self.host
            .call::<StartEcsExec>(StartEcsExecParams {
                profile: profile.clone(),
                region: region.to_string(),
                cluster: cluster.to_string(),
                task_id: task_id.to_string(),
                container: container.to_string(),
                command: command.to_string(),
            })
            .await
            .map_err(plugin_err_to_cloud)
    }

    async fn push_instance_connect_key(
        &self,
        profile: &CloudProfile,
        region: &str,
        instance_id: &str,
        os_user: &str,
        public_key: &str,
    ) -> Result<(), CloudError> {
        self.host
            .call::<PushInstanceConnectKey>(PushInstanceConnectKeyParams {
                profile: profile.clone(),
                region: region.to_string(),
                instance_id: instance_id.to_string(),
                os_user: os_user.to_string(),
                public_key: public_key.to_string(),
            })
            .await
            .map_err(plugin_err_to_cloud)
    }
}

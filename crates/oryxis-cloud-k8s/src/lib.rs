//! Kubernetes cloud provider for Oryxis, driven entirely through the
//! `kubectl` CLI (no native `kube`/`k8s-openapi` dependency).
//!
//! Discovery and resolve shell out to `kubectl get ... -o json` and parse
//! the JSON; the interactive pod shell is opened by the app spawning
//! `kubectl exec -it` in a local PTY (so it never round-trips through this
//! provider). The provider honours the profile's optional `kubeconfig` path
//! and `context`, mapping every failure into a `CloudError`.
//!
//! `kubectl` must be on PATH; a missing binary surfaces as
//! `CloudError::InvalidConfig` so the UI can tell the user to install it.

mod discover;
mod resolve;

use async_trait::async_trait;
use serde::Deserialize;

use oryxis_cloud::{
    CloudError, CloudProfile, CloudProvider, CloudQuery, CloudQueryKind, CloudResourceType,
    DiscoveryResult, DiscoveredHost, TransportKind,
};

/// Parsed `CloudProfile.config` for a Kubernetes account.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct K8sConfig {
    /// Path to a kubeconfig file. `None`/empty = kubectl's default
    /// (`$KUBECONFIG` or `~/.kube/config`).
    #[serde(default)]
    pub kubeconfig: Option<String>,
    /// Context to select within the kubeconfig. `None`/empty = the
    /// kubeconfig's current-context.
    #[serde(default)]
    pub context: Option<String>,
}

impl K8sConfig {
    /// Parse the profile's JSON `config`. A blank / malformed config is
    /// treated as "all defaults" rather than an error, so a half-filled
    /// profile still talks to the default cluster.
    pub fn from_profile(profile: &CloudProfile) -> Self {
        if profile.config.trim().is_empty() {
            return Self::default();
        }
        serde_json::from_str(&profile.config).unwrap_or_default()
    }
}

/// Build the `kubectl` argument list: global `--kubeconfig` / `--context`
/// flags from the config, followed by the subcommand args. Pure + tested.
pub(crate) fn kubectl_args(cfg: &K8sConfig, sub: &[&str]) -> Vec<String> {
    let mut args = Vec::with_capacity(sub.len() + 4);
    if let Some(kc) = cfg.kubeconfig.as_deref().filter(|s| !s.trim().is_empty()) {
        args.push("--kubeconfig".to_string());
        args.push(kc.to_string());
    }
    if let Some(ctx) = cfg.context.as_deref().filter(|s| !s.trim().is_empty()) {
        args.push("--context".to_string());
        args.push(ctx.to_string());
    }
    args.extend(sub.iter().map(|s| s.to_string()));
    args
}

/// Map a failed `kubectl` invocation's stderr into the closest
/// `CloudError` variant so the UI can colour / phrase it sensibly.
pub(crate) fn classify_kubectl_error(stderr: &str) -> CloudError {
    let s = stderr.to_lowercase();
    if s.contains("unauthorized")
        || s.contains("forbidden")
        || s.contains("error loading config")
        || s.contains("no configuration has been provided")
        || s.contains("context") && s.contains("does not exist")
    {
        CloudError::Auth(stderr.trim().to_string())
    } else if s.contains("refused")
        || s.contains("unable to connect")
        || s.contains("i/o timeout")
        || s.contains("no route to host")
        || s.contains("dial tcp")
    {
        CloudError::Network(stderr.trim().to_string())
    } else {
        CloudError::Upstream(stderr.trim().to_string())
    }
}

/// Run `kubectl <flags> <sub...>` and return stdout bytes on success.
pub(crate) async fn run_kubectl(cfg: &K8sConfig, sub: &[&str]) -> Result<Vec<u8>, CloudError> {
    let args = kubectl_args(cfg, sub);
    let output = tokio::process::Command::new("kubectl")
        .args(&args)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                CloudError::InvalidConfig(
                    "kubectl was not found on PATH. Install kubectl to use Kubernetes.".into(),
                )
            } else {
                CloudError::Other(format!("failed to run kubectl: {e}"))
            }
        })?;
    if !output.status.success() {
        return Err(classify_kubectl_error(&String::from_utf8_lossy(
            &output.stderr,
        )));
    }
    Ok(output.stdout)
}

/// Kubernetes provider. Stateless, every call re-derives config from the
/// profile and shells out to `kubectl`.
#[derive(Debug, Default, Clone, Copy)]
pub struct K8sProvider;

impl K8sProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CloudProvider for K8sProvider {
    fn id(&self) -> &'static str {
        "k8s"
    }

    async fn test_credentials(&self, profile: &CloudProfile) -> Result<(), CloudError> {
        let cfg = K8sConfig::from_profile(profile);
        // Hit the apiserver's /version endpoint directly: needs both
        // reachability and valid auth, and doesn't require list rights on
        // any particular resource.
        run_kubectl(&cfg, &["get", "--raw", "/version"]).await?;
        Ok(())
    }

    async fn discover(&self, profile: &CloudProfile) -> Result<DiscoveryResult, CloudError> {
        let cfg = K8sConfig::from_profile(profile);
        discover::discover_workloads(&cfg, profile).await
    }

    async fn resolve_query(
        &self,
        profile: &CloudProfile,
        query: &CloudQuery,
    ) -> Result<Vec<DiscoveredHost>, CloudError> {
        let CloudQueryKind::K8sPods {
            context,
            namespace,
            selector,
        } = &query.kind
        else {
            return Err(CloudError::Unsupported(
                "k8s provider received a non-Kubernetes query".into(),
            ));
        };
        // The group captured its own context at import; it wins over the
        // profile's default so a profile can back groups across contexts.
        let mut cfg = K8sConfig::from_profile(profile);
        if !context.trim().is_empty() {
            cfg.context = Some(context.clone());
        }
        resolve::resolve_pods(&cfg, namespace, selector).await
    }

    fn supported_transports(&self, _resource_type: CloudResourceType) -> Vec<TransportKind> {
        vec![TransportKind::KubectlExec]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(kubeconfig: Option<&str>, context: Option<&str>) -> K8sConfig {
        K8sConfig {
            kubeconfig: kubeconfig.map(str::to_string),
            context: context.map(str::to_string),
        }
    }

    #[test]
    fn args_with_no_config_are_just_the_subcommand() {
        assert_eq!(
            kubectl_args(&cfg(None, None), &["get", "ns"]),
            vec!["get", "ns"]
        );
    }

    #[test]
    fn args_include_kubeconfig_and_context_when_set() {
        let a = kubectl_args(&cfg(Some("/tmp/kc"), Some("prod")), &["get", "pods"]);
        assert_eq!(
            a,
            vec!["--kubeconfig", "/tmp/kc", "--context", "prod", "get", "pods"]
        );
    }

    #[test]
    fn blank_config_fields_are_skipped() {
        let a = kubectl_args(&cfg(Some("  "), Some("")), &["version"]);
        assert_eq!(a, vec!["version"]);
    }

    #[test]
    fn classify_picks_auth_network_or_upstream() {
        assert!(matches!(
            classify_kubectl_error("error: You must be logged in (Unauthorized)"),
            CloudError::Auth(_)
        ));
        assert!(matches!(
            classify_kubectl_error("The connection to the server 1.2.3.4 was refused"),
            CloudError::Network(_)
        ));
        assert!(matches!(
            classify_kubectl_error("something else entirely"),
            CloudError::Upstream(_)
        ));
    }
}

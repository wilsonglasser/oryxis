//! Workload discovery via `kubectl get deploy,sts,ds -A -o json`, parsed
//! into `DiscoveredK8sWorkload` rows.

use std::collections::BTreeMap;

use serde::Deserialize;

use oryxis_cloud::{CloudError, CloudProfile, DiscoveredK8sWorkload, DiscoveryResult};

use crate::{run_kubectl, K8sConfig};

/// Minimal shape of the `kubectl get ... -o json` List we read. Only the
/// fields we surface are deserialized; everything else is ignored.
#[derive(Debug, Deserialize)]
struct ItemList {
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Debug, Deserialize)]
struct Item {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    metadata: Meta,
    #[serde(default)]
    spec: Spec,
    #[serde(default)]
    status: Status,
}

#[derive(Debug, Default, Deserialize)]
struct Meta {
    #[serde(default)]
    namespace: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct Spec {
    #[serde(default)]
    selector: Selector,
    #[serde(default)]
    template: Template,
}

#[derive(Debug, Default, Deserialize)]
struct Selector {
    #[serde(default, rename = "matchLabels")]
    match_labels: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct Template {
    #[serde(default)]
    spec: PodSpec,
}

#[derive(Debug, Default, Deserialize)]
struct PodSpec {
    #[serde(default)]
    containers: Vec<Container>,
}

#[derive(Debug, Default, Deserialize)]
struct Container {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct Status {
    // Deployments / StatefulSets report `readyReplicas`; DaemonSets report
    // `numberReady`. Either may be absent (zero ready).
    #[serde(default, rename = "readyReplicas")]
    ready_replicas: Option<u32>,
    #[serde(default, rename = "numberReady")]
    number_ready: Option<u32>,
}

/// Parse a `kubectl get ... -o json` List into workload rows. Pure +
/// tested. `context` is stamped onto each row so the dynamic group it
/// becomes knows which context to resolve against.
fn parse_workloads(json: &[u8], context: &str) -> Result<Vec<DiscoveredK8sWorkload>, CloudError> {
    let list: ItemList = serde_json::from_slice(json)
        .map_err(|e| CloudError::Other(format!("failed to parse kubectl output: {e}")))?;
    let mut out = Vec::with_capacity(list.items.len());
    for item in list.items {
        let container = item
            .spec
            .template
            .spec
            .containers
            .first()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let running = item.status.ready_replicas.or(item.status.number_ready).unwrap_or(0);
        out.push(DiscoveredK8sWorkload {
            context: context.to_string(),
            namespace: item.metadata.namespace,
            kind: item.kind,
            name: item.metadata.name,
            container,
            running_pod_count: running,
            match_labels: item.spec.selector.match_labels,
        });
    }
    Ok(out)
}

pub(crate) async fn discover_workloads(
    cfg: &K8sConfig,
    _profile: &CloudProfile,
) -> Result<DiscoveryResult, CloudError> {
    let json = run_kubectl(cfg, &["get", "deploy,sts,ds", "-A", "-o", "json"]).await?;
    // The active context is what the dynamic group will resolve against;
    // an explicit `--context` wins, else the kubeconfig's current-context.
    let context = match cfg.context.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(c) => c.to_string(),
        None => current_context(cfg).await.unwrap_or_default(),
    };
    let k8s_workloads = parse_workloads(&json, &context)?;
    Ok(DiscoveryResult {
        ec2: Vec::new(),
        ecs_services: Vec::new(),
        k8s_workloads,
    })
}

/// Best-effort `kubectl config current-context`. Returns `None` on any
/// failure (the workload rows just carry an empty context, which the
/// resolve path treats as "kubeconfig default").
async fn current_context(cfg: &K8sConfig) -> Option<String> {
    let out = run_kubectl(cfg, &["config", "current-context"]).await.ok()?;
    let ctx = String::from_utf8_lossy(&out).trim().to_string();
    (!ctx.is_empty()).then_some(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_workload_list() {
        let json = br#"{
          "items": [
            {
              "kind": "Deployment",
              "metadata": { "namespace": "default", "name": "nginx" },
              "spec": {
                "selector": { "matchLabels": { "app": "nginx" } },
                "template": { "spec": { "containers": [ { "name": "nginx" } ] } }
              },
              "status": { "readyReplicas": 3 }
            },
            {
              "kind": "DaemonSet",
              "metadata": { "namespace": "kube-system", "name": "fluentd" },
              "spec": {
                "selector": { "matchLabels": { "k8s-app": "fluentd" } },
                "template": { "spec": { "containers": [ { "name": "fluentd" } ] } }
              },
              "status": { "numberReady": 5 }
            }
          ]
        }"#;
        let rows = parse_workloads(json, "prod").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, "Deployment");
        assert_eq!(rows[0].name, "nginx");
        assert_eq!(rows[0].namespace, "default");
        assert_eq!(rows[0].container, "nginx");
        assert_eq!(rows[0].running_pod_count, 3);
        assert_eq!(rows[0].context, "prod");
        assert_eq!(rows[0].match_labels.get("app").map(String::as_str), Some("nginx"));
        // DaemonSet ready count comes from numberReady.
        assert_eq!(rows[1].running_pod_count, 5);
        assert_eq!(rows[1].kind, "DaemonSet");
    }

    #[test]
    fn empty_list_is_ok() {
        assert!(parse_workloads(b"{\"items\":[]}", "c").unwrap().is_empty());
    }

    #[test]
    fn missing_ready_counts_default_to_zero() {
        let json = br#"{"items":[{"kind":"StatefulSet","metadata":{"namespace":"db","name":"pg"},"spec":{"selector":{"matchLabels":{"app":"pg"}},"template":{"spec":{"containers":[{"name":"postgres"}]}}},"status":{}}]}"#;
        let rows = parse_workloads(json, "c").unwrap();
        assert_eq!(rows[0].running_pod_count, 0);
        assert_eq!(rows[0].container, "postgres");
    }
}

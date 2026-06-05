//! Pod resolution for a `K8sPods` dynamic group, via
//! `kubectl get pods -n NS [-l SELECTOR] -o json`.

use std::collections::BTreeMap;

use serde::Deserialize;

use oryxis_cloud::{CloudError, DiscoveredHost, PodSelector};

use crate::{run_kubectl, K8sConfig};

/// Build the `key=value,key=value` label selector string from a label map.
/// Pure + tested.
pub(crate) fn labels_to_selector(labels: &BTreeMap<String, String>) -> String {
    labels
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Debug, Deserialize)]
struct PodList {
    #[serde(default)]
    items: Vec<Pod>,
}

#[derive(Debug, Deserialize)]
struct Pod {
    #[serde(default)]
    metadata: PodMeta,
    #[serde(default)]
    spec: PodSpec,
    #[serde(default)]
    status: PodStatus,
}

#[derive(Debug, Default, Deserialize)]
struct PodMeta {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct PodSpec {
    #[serde(default, rename = "nodeName")]
    node_name: Option<String>,
    #[serde(default)]
    containers: Vec<Container>,
}

#[derive(Debug, Default, Deserialize)]
struct Container {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct PodStatus {
    #[serde(default)]
    phase: Option<String>,
    #[serde(default, rename = "podIP")]
    pod_ip: Option<String>,
    #[serde(default, rename = "startTime")]
    start_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// Parse `kubectl get pods -o json` into resolved hosts. Pure + tested.
pub(crate) fn parse_pods(json: &[u8]) -> Result<Vec<DiscoveredHost>, CloudError> {
    let list: PodList = serde_json::from_slice(json)
        .map_err(|e| CloudError::Other(format!("failed to parse pod list: {e}")))?;
    Ok(list
        .items
        .into_iter()
        .map(|p| {
            let container = p.spec.containers.first().map(|c| c.name.clone());
            DiscoveredHost {
                label: p.metadata.name.clone(),
                resource_id: p.metadata.name,
                subtitle: p.spec.node_name.clone(),
                container_name: container,
                task_definition: None,
                status: p.status.phase,
                started_at: p.status.start_time,
                private_ip: p.status.pod_ip,
                availability_zone: p.spec.node_name,
                region: None,
            }
        })
        .collect())
}

/// Read a workload's `spec.selector.matchLabels` so a `Deployment` /
/// `StatefulSet` selector can be turned into a pod label query.
async fn workload_match_labels(
    cfg: &K8sConfig,
    namespace: &str,
    kind: &str,
    name: &str,
) -> Result<BTreeMap<String, String>, CloudError> {
    #[derive(Deserialize)]
    struct Wl {
        #[serde(default)]
        spec: WlSpec,
    }
    #[derive(Default, Deserialize)]
    struct WlSpec {
        #[serde(default)]
        selector: WlSelector,
    }
    #[derive(Default, Deserialize)]
    struct WlSelector {
        #[serde(default, rename = "matchLabels")]
        match_labels: BTreeMap<String, String>,
    }
    let json = run_kubectl(cfg, &[kind, name, "-n", namespace, "-o", "json"]).await?;
    let wl: Wl = serde_json::from_slice(&json)
        .map_err(|e| CloudError::Other(format!("failed to parse {kind}/{name}: {e}")))?;
    Ok(wl.spec.selector.match_labels)
}

/// Resolve the pods matching a `K8sPods` selector in a namespace.
pub(crate) async fn resolve_pods(
    cfg: &K8sConfig,
    namespace: &str,
    selector: &PodSelector,
) -> Result<Vec<DiscoveredHost>, CloudError> {
    let json = match selector {
        PodSelector::Labels(labels) => {
            let sel = labels_to_selector(labels);
            run_kubectl(cfg, &["get", "pods", "-n", namespace, "-l", &sel, "-o", "json"]).await?
        }
        PodSelector::Name(name) => {
            run_kubectl(
                cfg,
                &[
                    "get",
                    "pods",
                    "-n",
                    namespace,
                    "--field-selector",
                    &format!("metadata.name={name}"),
                    "-o",
                    "json",
                ],
            )
            .await?
        }
        PodSelector::Deployment(name) => {
            let labels = workload_match_labels(cfg, namespace, "deployment", name).await?;
            let sel = labels_to_selector(&labels);
            run_kubectl(cfg, &["get", "pods", "-n", namespace, "-l", &sel, "-o", "json"]).await?
        }
        PodSelector::StatefulSet(name) => {
            let labels = workload_match_labels(cfg, namespace, "statefulset", name).await?;
            let sel = labels_to_selector(&labels);
            run_kubectl(cfg, &["get", "pods", "-n", namespace, "-l", &sel, "-o", "json"]).await?
        }
    };
    parse_pods(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_string_is_sorted_and_comma_joined() {
        let mut m = BTreeMap::new();
        m.insert("tier".to_string(), "frontend".to_string());
        m.insert("app".to_string(), "nginx".to_string());
        // BTreeMap iterates sorted, so `app` precedes `tier`.
        assert_eq!(labels_to_selector(&m), "app=nginx,tier=frontend");
    }

    #[test]
    fn parses_pods_into_hosts() {
        let json = br#"{"items":[
          {"metadata":{"name":"nginx-abc","namespace":"default"},
           "spec":{"nodeName":"node-1","containers":[{"name":"nginx"},{"name":"sidecar"}]},
           "status":{"phase":"Running","podIP":"10.1.2.3","startTime":"2026-01-02T03:04:05Z"}}
        ]}"#;
        let hosts = parse_pods(json).unwrap();
        assert_eq!(hosts.len(), 1);
        let h = &hosts[0];
        assert_eq!(h.resource_id, "nginx-abc");
        assert_eq!(h.label, "nginx-abc");
        assert_eq!(h.container_name.as_deref(), Some("nginx"));
        assert_eq!(h.status.as_deref(), Some("Running"));
        assert_eq!(h.private_ip.as_deref(), Some("10.1.2.3"));
        assert_eq!(h.availability_zone.as_deref(), Some("node-1"));
        assert!(h.started_at.is_some());
    }

    #[test]
    fn empty_pod_list_is_ok() {
        assert!(parse_pods(b"{\"items\":[]}").unwrap().is_empty());
    }
}

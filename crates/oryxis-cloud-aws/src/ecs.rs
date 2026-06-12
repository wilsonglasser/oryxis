//! ECS discovery, list services per cluster, expand each to its
//! container names so the wizard can offer a checkbox per
//! `(cluster, service, container)` triple.
//!
//! ECS API shape we walk:
//!   1. `ListClusters`, cluster ARNs
//!   2. `ListServices(cluster)`, service ARNs
//!   3. `DescribeServices(cluster, service)`, running task count + task
//!      definition ARN
//!   4. `DescribeTaskDefinition(taskDefArn)`, container definitions
//!      (so we know which containers exist on each task)
//!
//! The cost is one call per unique task definition plus a couple per
//! cluster. Regions fan out in parallel and clusters / task-def
//! describes run with bounded concurrency, so even wide accounts
//! resolve in interactive time. ECS Exec / dynamic-group resolution
//! happens later, in a different call site.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use aws_config::{Region, SdkConfig};
use aws_sdk_ecs::Client as EcsClient;
use oryxis_cloud::{CloudError, CloudProfile, DiscoveredEcsService, DiscoveredHost};

use crate::auth::{build_sdk_config, AwsConfigJson};

/// How many clusters (per region) and task definitions (per cluster)
/// we describe concurrently. Bounded so a profile with dozens of
/// clusters doesn't open an unbounded burst of API calls and trip
/// IAM rate limits.
const BATCH_CONCURRENCY: usize = 8;

/// Memoized container lists keyed by task-definition ARN, shared by
/// every cluster task within one discovery pass. Many services point
/// at the same task definition, so this collapses the dominant
/// per-service DescribeTaskDefinition cost.
type TaskDefCache = Arc<Mutex<HashMap<String, Vec<String>>>>;

/// Derive a per-region ECS client from an already-loaded `SdkConfig`,
/// overriding only the region. Avoids re-running the full credential
/// chain (profile files, SSO cache, IMDS, ...) once per region.
fn region_client(sdk: &SdkConfig, region: &str) -> EcsClient {
    let conf = aws_sdk_ecs::config::Builder::from(sdk)
        .region(Region::new(region.to_string()))
        .build();
    EcsClient::from_conf(conf)
}

/// Run discovery against every region the profile configured. Per-region
/// failures are logged and skipped (same approach as EC2). Regions run
/// concurrently against one shared credential chain.
pub async fn discover_ecs_services(
    profile: &CloudProfile,
) -> Result<Vec<DiscoveredEcsService>, CloudError> {
    let cfg = AwsConfigJson::parse(profile)?;
    let mut regions: Vec<String> = if cfg.regions.is_empty() {
        cfg.region.clone().into_iter().collect()
    } else {
        cfg.regions.clone()
    };
    regions.sort();
    regions.dedup();

    // Load the credential chain once and share it across regions. A
    // failed load degrades to "every region skipped" (one warn instead
    // of N), the same empty-result contract the serial code had.
    let sdk = match build_sdk_config(profile, None, None).await {
        Ok(sdk) => sdk,
        Err(err) => {
            tracing::warn!(
                target = "oryxis_cloud_aws",
                error = %err,
                "ECS discovery failed to build the AWS SDK config, skipping all regions"
            );
            return Ok(Vec::new());
        }
    };

    // Fan out all regions concurrently; results are awaited in the
    // sorted-region order so output stays deterministic.
    let handles: Vec<_> = regions
        .iter()
        .map(|region| {
            let client = region_client(&sdk, region);
            let region = region.clone();
            tokio::spawn(async move { list_one_region(&client, &region).await })
        })
        .collect();

    let mut out = Vec::new();
    for (region, handle) in regions.iter().zip(handles) {
        let result = match handle.await {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!(
                    target = "oryxis_cloud_aws",
                    region = %region,
                    error = %err,
                    "ECS discovery task panicked for region, skipping"
                );
                continue;
            }
        };
        match result {
            Ok(mut found) => out.append(&mut found),
            Err(err) => {
                tracing::warn!(
                    target = "oryxis_cloud_aws",
                    region = %region,
                    error = %err,
                    "ECS discovery failed for region, skipping"
                );
            }
        }
    }
    Ok(out)
}

async fn list_one_region(
    client: &EcsClient,
    region: &str,
) -> Result<Vec<DiscoveredEcsService>, CloudError> {
    let cluster_arns = list_all_clusters(client, region).await?;
    let task_def_cache: TaskDefCache = TaskDefCache::default();

    // Walk clusters in bounded-concurrency batches; a failing cluster
    // is logged and skipped, never kills the region (same contract as
    // the serial loop).
    let mut out = Vec::new();
    for batch in cluster_arns.chunks(BATCH_CONCURRENCY) {
        let handles: Vec<_> = batch
            .iter()
            .map(|cluster_arn| {
                // Friendly name = the slash-tail of the ARN.
                // `arn:aws:ecs:us-east-1:123:cluster/my-cluster` → `my-cluster`.
                let cluster_name = cluster_arn
                    .rsplit('/')
                    .next()
                    .unwrap_or(cluster_arn)
                    .to_string();
                let client = client.clone();
                let region = region.to_string();
                let cache = Arc::clone(&task_def_cache);
                tokio::spawn(async move {
                    let result =
                        list_services_in_cluster(&client, &cluster_name, &region, &cache).await;
                    (cluster_name, result)
                })
            })
            .collect();

        for handle in handles {
            match handle.await {
                Ok((_, Ok(mut services))) => out.append(&mut services),
                Ok((cluster_name, Err(err))) => {
                    tracing::warn!(
                        target = "oryxis_cloud_aws",
                        region = %region,
                        cluster = %cluster_name,
                        error = %err,
                        "ECS service listing failed for cluster, skipping"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        target = "oryxis_cloud_aws",
                        region = %region,
                        error = %err,
                        "ECS cluster listing task panicked, skipping"
                    );
                }
            }
        }
    }
    Ok(out)
}

async fn list_all_clusters(client: &EcsClient, region: &str) -> Result<Vec<String>, CloudError> {
    let mut next_token: Option<String> = None;
    let mut arns = Vec::new();
    loop {
        let mut req = client.list_clusters();
        if let Some(t) = next_token.take() {
            req = req.next_token(t);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| CloudError::Upstream(format!("ecs:ListClusters {region}: {e}")))?;
        for arn in resp.cluster_arns() {
            arns.push(arn.to_string());
        }
        match resp.next_token() {
            Some(t) if !t.is_empty() => next_token = Some(t.to_string()),
            _ => break,
        }
    }
    Ok(arns)
}

async fn list_services_in_cluster(
    client: &EcsClient,
    cluster: &str,
    region: &str,
    task_def_cache: &TaskDefCache,
) -> Result<Vec<DiscoveredEcsService>, CloudError> {
    // 1) List service ARNs.
    let mut next_token: Option<String> = None;
    let mut service_arns: Vec<String> = Vec::new();
    loop {
        let mut req = client.list_services().cluster(cluster);
        if let Some(t) = next_token.take() {
            req = req.next_token(t);
        }
        let resp = req.send().await.map_err(|e| {
            CloudError::Upstream(format!("ecs:ListServices {region}/{cluster}: {e}"))
        })?;
        for arn in resp.service_arns() {
            service_arns.push(arn.to_string());
        }
        match resp.next_token() {
            Some(t) if !t.is_empty() => next_token = Some(t.to_string()),
            _ => break,
        }
    }

    if service_arns.is_empty() {
        return Ok(Vec::new());
    }

    // 2) DescribeServices in batches of 10 (the API hard limit),
    // collecting (service, task-def, running) rows for the container
    // expansion below.
    let mut rows: Vec<(String, String, u32)> = Vec::new();
    for chunk in service_arns.chunks(10) {
        let resp = client
            .describe_services()
            .cluster(cluster)
            .set_services(Some(chunk.to_vec()))
            .send()
            .await
            .map_err(|e| {
                CloudError::Upstream(format!(
                    "ecs:DescribeServices {region}/{cluster}: {e}"
                ))
            })?;

        for svc in resp.services() {
            let service_name = svc.service_name().unwrap_or_default().to_string();
            let task_def_arn = svc.task_definition().unwrap_or_default().to_string();
            let running = svc.running_count().max(0) as u32;
            rows.push((service_name, task_def_arn, running));
        }
    }

    // 3) DescribeTaskDefinition to enumerate containers. Tasks may
    // have multiple containers, surface each one as a separate
    // (cluster, service, container) entry so the user picks exactly
    // which container to exec into. Resolved once per unique ARN via
    // the shared cache (many services reuse the same task def), the
    // misses fanned out with bounded concurrency. A failing describe
    // yields an empty container list for its services, same skip
    // behaviour the serial per-service loop had.
    let pending: Vec<String> = {
        let cache = task_def_cache.lock().unwrap();
        let mut seen: HashSet<&str> = HashSet::new();
        rows.iter()
            .filter(|(_, arn, _)| {
                !arn.is_empty() && !cache.contains_key(arn) && seen.insert(arn.as_str())
            })
            .map(|(_, arn, _)| arn.clone())
            .collect()
    };
    for batch in pending.chunks(BATCH_CONCURRENCY) {
        let handles: Vec<_> = batch
            .iter()
            .map(|arn| {
                let client = client.clone();
                let arn = arn.clone();
                let region = region.to_string();
                tokio::spawn(async move {
                    let result = describe_task_def_containers(&client, &arn, &region).await;
                    (arn, result)
                })
            })
            .collect();

        for handle in handles {
            match handle.await {
                Ok((arn, Ok(containers))) => {
                    task_def_cache.lock().unwrap().insert(arn, containers);
                }
                Ok((arn, Err(err))) => {
                    tracing::warn!(
                        target = "oryxis_cloud_aws",
                        region = %region,
                        cluster = %cluster,
                        task_definition = %arn,
                        error = %err,
                        "ECS task-def describe failed, skipping containers for its services"
                    );
                    // Cache the miss as "no containers" so other
                    // services on the same ARN don't retry this pass.
                    task_def_cache.lock().unwrap().insert(arn, Vec::new());
                }
                Err(err) => {
                    tracing::warn!(
                        target = "oryxis_cloud_aws",
                        region = %region,
                        cluster = %cluster,
                        error = %err,
                        "ECS task-def describe task panicked, skipping"
                    );
                }
            }
        }
    }

    let mut out = Vec::new();
    for (service_name, task_def_arn, running) in rows {
        let containers = task_def_cache
            .lock()
            .unwrap()
            .get(&task_def_arn)
            .cloned()
            .unwrap_or_default();
        for container in containers {
            out.push(DiscoveredEcsService {
                region: region.to_string(),
                cluster: cluster.to_string(),
                service: service_name.clone(),
                container,
                running_task_count: running,
            });
        }
    }
    Ok(out)
}

/// Live-task resolution for an ECS dynamic group. Called every time
/// the user expands the group; result feeds the transient host list
/// the dashboard renders.
///
/// We have to find the *actual* region the cluster lives in, the
/// profile may have a default region that doesn't match (single
/// account spread across regions is normal). Strategy: try the
/// configured `regions` set; for each, look up the cluster by name;
/// the first one that returns a match wins. For one-region profiles
/// this is a single API call.
pub async fn resolve_ecs_tasks(
    profile: &CloudProfile,
    cluster: &str,
    service: &str,
    container: &str,
) -> Result<Vec<DiscoveredHost>, CloudError> {
    let cfg = AwsConfigJson::parse(profile)?;
    let mut regions: Vec<String> = if cfg.regions.is_empty() {
        cfg.region.clone().into_iter().collect()
    } else {
        cfg.regions.clone()
    };
    regions.sort();
    regions.dedup();

    if regions.is_empty() {
        return Err(CloudError::InvalidConfig(
            "no region configured: set `region` or `regions` in the profile config".into(),
        ));
    }

    // Load the credential chain once; per-region failure degrades to
    // "skip that region" exactly like the serial probe did, so a
    // failed load just means every region is skipped.
    let sdk = match build_sdk_config(profile, None, None).await {
        Ok(sdk) => sdk,
        Err(err) => {
            tracing::warn!(
                target = "oryxis_cloud_aws",
                cluster = %cluster,
                service = %service,
                error = %err,
                "ECS resolve_query failed to build the AWS SDK config, skipping all regions"
            );
            return Ok(Vec::new());
        }
    };

    // Probe every candidate region in parallel, then pick the winner
    // in sorted-region order so the result stays deterministic when
    // (improbably) more than one region matches. For one-region
    // profiles this is still a single API call.
    let handles: Vec<_> = regions
        .iter()
        .map(|region| {
            let client = region_client(&sdk, region);
            let region = region.clone();
            let cluster = cluster.to_string();
            let service = service.to_string();
            let container = container.to_string();
            tokio::spawn(async move {
                list_running_tasks(&client, &region, &cluster, &service, &container).await
            })
        })
        .collect();

    for (region, handle) in regions.iter().zip(handles) {
        let result = match handle.await {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!(
                    target = "oryxis_cloud_aws",
                    region = %region,
                    cluster = %cluster,
                    service = %service,
                    error = %err,
                    "ECS resolve_query task panicked for region, trying the next"
                );
                continue;
            }
        };
        match result {
            Ok(tasks) if !tasks.is_empty() => return Ok(tasks),
            Ok(_) => continue, // empty in this region, try the next
            Err(err) => {
                tracing::warn!(
                    target = "oryxis_cloud_aws",
                    region = %region,
                    cluster = %cluster,
                    service = %service,
                    error = %err,
                    "ECS resolve_query failed for region, trying the next"
                );
            }
        }
    }
    Ok(Vec::new())
}

async fn list_running_tasks(
    client: &EcsClient,
    region: &str,
    cluster: &str,
    service: &str,
    container: &str,
) -> Result<Vec<DiscoveredHost>, CloudError> {
    // 1) ListTasks, RUNNING tasks for this service.
    let mut next_token: Option<String> = None;
    let mut task_arns: Vec<String> = Vec::new();
    loop {
        let mut req = client
            .list_tasks()
            .cluster(cluster)
            .service_name(service)
            .desired_status(aws_sdk_ecs::types::DesiredStatus::Running);
        if let Some(t) = next_token.take() {
            req = req.next_token(t);
        }
        let resp = req.send().await.map_err(|e| {
            CloudError::Upstream(format!(
                "ecs:ListTasks {region}/{cluster}/{service}: {e}"
            ))
        })?;
        for arn in resp.task_arns() {
            task_arns.push(arn.to_string());
        }
        match resp.next_token() {
            Some(t) if !t.is_empty() => next_token = Some(t.to_string()),
            _ => break,
        }
    }
    if task_arns.is_empty() {
        return Ok(Vec::new());
    }

    // 2) DescribeTasks in batches of 100 (the API ceiling), surface
    // each task as a host with the container's network details (IP,
    // availability zone) when present.
    let mut out = Vec::new();
    for chunk in task_arns.chunks(100) {
        let resp = client
            .describe_tasks()
            .cluster(cluster)
            .set_tasks(Some(chunk.to_vec()))
            .send()
            .await
            .map_err(|e| {
                CloudError::Upstream(format!(
                    "ecs:DescribeTasks {region}/{cluster}: {e}"
                ))
            })?;

        for task in resp.tasks() {
            let task_arn = task.task_arn().unwrap_or_default();
            // Friendly id = the slash-tail of the ARN.
            // `arn:…:task/<cluster>/<task-id>` → `<task-id>`.
            let task_id = task_arn
                .rsplit('/')
                .next()
                .unwrap_or(task_arn)
                .to_string();
            let zone_raw = task.availability_zone().unwrap_or_default().to_string();
            let zone = if zone_raw.is_empty() { None } else { Some(zone_raw.clone()) };
            let status = task.last_status().map(|s| s.to_string());
            let started_at = task
                .started_at()
                .and_then(|t| chrono::DateTime::<chrono::Utc>::from_timestamp(t.secs(), 0));
            // Task definition: `family:revision` extracted from the
            // full ARN. ARN format:
            // `arn:aws:ecs:<region>:<acct>:task-definition/family:rev`.
            let task_definition = task.task_definition_arn().and_then(|arn| {
                arn.rsplit('/').next().map(|s| s.to_string())
            });

            // Pick the containers to emit. Empty `container` filter
            // = all (Lens-style nesting: one row per container in
            // each task). Otherwise filter to the single named
            // container, matching the v0.6 behaviour.
            let task_containers: Vec<_> = if container.is_empty() {
                task.containers()
                    .iter()
                    .filter_map(|c| c.name().map(|n| (n.to_string(), c)))
                    .collect()
            } else {
                task.containers()
                    .iter()
                    .find(|c| c.name() == Some(container))
                    .map(|c| (container.to_string(), c))
                    .into_iter()
                    .collect()
            };

            for (cname, c) in task_containers {
                let ip: Option<String> = c
                    .network_interfaces()
                    .first()
                    .and_then(|net| net.private_ipv4_address())
                    .filter(|p| !p.is_empty())
                    .map(|p| p.to_string());

                let label = match (&ip, &zone_raw.as_str()) {
                    (Some(addr), z) if !z.is_empty() => {
                        format!("{task_id}  ({addr})  {z}")
                    }
                    (Some(addr), _) => format!("{task_id}  ({addr})"),
                    (None, z) if !z.is_empty() => format!("{task_id}  {z}"),
                    _ => task_id.clone(),
                };

                out.push(DiscoveredHost {
                    label,
                    resource_id: task_id.clone(),
                    subtitle: ip.clone(),
                    container_name: Some(cname),
                    task_definition: task_definition.clone(),
                    status: status.clone(),
                    started_at,
                    private_ip: ip,
                    availability_zone: zone.clone(),
                    region: Some(region.to_string()),
                });
            }
        }
    }
    Ok(out)
}

async fn describe_task_def_containers(
    client: &EcsClient,
    task_def_arn: &str,
    region: &str,
) -> Result<Vec<String>, CloudError> {
    if task_def_arn.is_empty() {
        return Ok(Vec::new());
    }
    let resp = client
        .describe_task_definition()
        .task_definition(task_def_arn)
        .send()
        .await
        .map_err(|e| {
            CloudError::Upstream(format!("ecs:DescribeTaskDefinition {region}: {e}"))
        })?;
    let names = resp
        .task_definition()
        .map(|td| {
            td.container_definitions()
                .iter()
                .filter_map(|c| c.name().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(names)
}

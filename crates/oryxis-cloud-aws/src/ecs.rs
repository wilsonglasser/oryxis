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
//! The cost is one call per service plus one per cluster, discovery
//! is interactive (opened by the user), so the wizard already tolerates
//! a couple of seconds of latency. ECS Exec / dynamic-group resolution
//! happens later, in a different call site.

use aws_sdk_ecs::Client as EcsClient;
use oryxis_cloud::{CloudError, CloudProfile, DiscoveredEcsService, DiscoveredHost};

use crate::auth::{build_sdk_config, AwsConfigJson};

/// Run discovery against every region the profile configured. Per-region
/// failures are logged and skipped (same approach as EC2).
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

    let mut out = Vec::new();
    for region in &regions {
        match list_one_region(profile, region).await {
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
    profile: &CloudProfile,
    region: &str,
) -> Result<Vec<DiscoveredEcsService>, CloudError> {
    let sdk = build_sdk_config(profile, Some(region), None).await?;
    let client = EcsClient::new(&sdk);

    let cluster_arns = list_all_clusters(&client, region).await?;
    let mut out = Vec::new();
    for cluster_arn in cluster_arns {
        // Friendly name = the slash-tail of the ARN.
        // `arn:aws:ecs:us-east-1:123:cluster/my-cluster` → `my-cluster`.
        let cluster_name = cluster_arn
            .rsplit('/')
            .next()
            .unwrap_or(&cluster_arn)
            .to_string();
        match list_services_in_cluster(&client, &cluster_name, region).await {
            Ok(mut services) => out.append(&mut services),
            Err(err) => {
                tracing::warn!(
                    target = "oryxis_cloud_aws",
                    region = %region,
                    cluster = %cluster_name,
                    error = %err,
                    "ECS service listing failed for cluster, skipping"
                );
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

    // 2) DescribeServices in batches of 10 (the API hard limit).
    let mut out = Vec::new();
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

            // 3) DescribeTaskDefinition to enumerate containers.
            // Tasks may have multiple containers, surface each one
            // as a separate (cluster, service, container) entry so
            // the user picks exactly which container to exec into.
            let containers = describe_task_def_containers(client, &task_def_arn, region)
                .await
                .unwrap_or_else(|err| {
                    tracing::warn!(
                        target = "oryxis_cloud_aws",
                        region = %region,
                        cluster = %cluster,
                        service = %service_name,
                        error = %err,
                        "ECS task-def describe failed, skipping containers for this service"
                    );
                    Vec::new()
                });

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

    // Probe regions in parallel only if there are several, otherwise
    // a sequential loop keeps the call simple and avoids spinning up
    // tokio joins for nothing.
    for region in &regions {
        match list_running_tasks(profile, region, cluster, service, container).await {
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
    profile: &CloudProfile,
    region: &str,
    cluster: &str,
    service: &str,
    container: &str,
) -> Result<Vec<DiscoveredHost>, CloudError> {
    let sdk = build_sdk_config(profile, Some(region), None).await?;
    let client = EcsClient::new(&sdk);

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
            let zone = task.availability_zone().unwrap_or_default().to_string();

            // Pull the IP from the matching container, services with
            // multiple containers may have different IPs per
            // container, so we filter by name to be safe.
            let mut ip: Option<String> = None;
            for c in task.containers() {
                if c.name() != Some(container) {
                    continue;
                }
                if let Some(net) = c.network_interfaces().first()
                    && let Some(p) = net.private_ipv4_address()
                    && !p.is_empty()
                {
                    ip = Some(p.to_string());
                }
                break;
            }

            let label = match (&ip, &zone) {
                (Some(addr), z) if !z.is_empty() => {
                    format!("{task_id}  ({addr})  {z}")
                }
                (Some(addr), _) => format!("{task_id}  ({addr})"),
                (None, z) if !z.is_empty() => format!("{task_id}  {z}"),
                _ => task_id.clone(),
            };

            out.push(DiscoveredHost {
                label,
                resource_id: task_id,
                subtitle: ip.clone(),
            });
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

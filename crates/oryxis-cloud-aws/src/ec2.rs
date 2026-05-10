//! EC2 discovery — `DescribeInstances` per region, mapped to
//! `oryxis_cloud::DiscoveredEc2`.

use aws_sdk_ec2::Client as Ec2Client;
use oryxis_cloud::{CloudError, CloudProfile, DiscoveredEc2};

use crate::auth::{build_sdk_config, AwsConfigJson};

/// Run discovery against every region the profile lists. When no region
/// list is configured, falls back to the profile's default region. The
/// caller is responsible for surfacing partial failures (one bad region
/// shouldn't kill the whole call), so each region's error is logged
/// rather than bubbled up — the returned vec contains whatever did work.
pub async fn discover_ec2(profile: &CloudProfile) -> Result<Vec<DiscoveredEc2>, CloudError> {
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

    let mut out = Vec::new();
    for region in &regions {
        match describe_one_region(profile, region).await {
            Ok(mut found) => out.append(&mut found),
            Err(err) => {
                tracing::warn!(
                    target = "oryxis_cloud_aws",
                    region = %region,
                    error = %err,
                    "EC2 discovery failed for region — skipping"
                );
            }
        }
    }
    Ok(out)
}

async fn describe_one_region(
    profile: &CloudProfile,
    region: &str,
) -> Result<Vec<DiscoveredEc2>, CloudError> {
    let sdk = build_sdk_config(profile, Some(region), None).await?;
    let client = Ec2Client::new(&sdk);

    let mut out = Vec::new();
    let mut next_token: Option<String> = None;
    loop {
        let mut req = client.describe_instances();
        if let Some(t) = next_token.take() {
            req = req.next_token(t);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| CloudError::Upstream(format!("ec2:DescribeInstances {region}: {e}")))?;

        for reservation in resp.reservations() {
            for instance in reservation.instances() {
                if let Some(d) = map_instance(instance, region) {
                    out.push(d);
                }
            }
        }

        match resp.next_token() {
            Some(t) if !t.is_empty() => next_token = Some(t.to_string()),
            _ => break,
        }
    }
    Ok(out)
}

/// Map an EC2 `Instance` row to our wire type. Returns `None` only when
/// the instance has no `instance_id` (which the SDK guarantees is never
/// the case in practice — defensive).
fn map_instance(
    instance: &aws_sdk_ec2::types::Instance,
    region: &str,
) -> Option<DiscoveredEc2> {
    let instance_id = instance.instance_id()?.to_string();

    let name = instance
        .tags()
        .iter()
        .find(|t| t.key() == Some("Name"))
        .and_then(|t| t.value())
        .map(str::to_string);

    let state = instance
        .state()
        .and_then(|s| s.name())
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| "unknown".into());

    let default_username = instance
        .image_id()
        .and_then(default_username_for_ami);

    Some(DiscoveredEc2 {
        instance_id,
        region: region.to_string(),
        name,
        public_dns: non_empty(instance.public_dns_name()),
        private_dns: non_empty(instance.private_dns_name()),
        public_ip: non_empty(instance.public_ip_address()),
        private_ip: non_empty(instance.private_ip_address()),
        state,
        default_username,
    })
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.filter(|v| !v.is_empty()).map(str::to_string)
}

/// Best-effort mapping AMI id → conventional default username. EC2 has
/// no API to look this up — every consumer recreates this table from
/// the docs. We only cover the AMIs we recognise; unknown AMIs return
/// `None` and the user fills `username` themselves in the editor.
///
/// Heuristic is deliberately on the AMI id prefix only; querying
/// `DescribeImages` per instance would multiply the API cost of
/// discovery without giving us much more signal.
fn default_username_for_ami(ami_id: &str) -> Option<String> {
    // The id alone doesn't tell us the OS; we'd need DescribeImages to
    // read the platform / description. For now return `None` — the
    // editor surfaces the field as a free text input. A follow-up PR
    // can do a single batched DescribeImages per region and cache the
    // owner+description heuristic. Keeping this stub here so the call
    // site doesn't change once the heuristic lands.
    let _ = ami_id;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_empty_filters_blanks() {
        assert_eq!(non_empty(Some("hello")).as_deref(), Some("hello"));
        assert_eq!(non_empty(Some("")), None);
        assert_eq!(non_empty(None), None);
    }

    #[test]
    fn default_username_unknown_ami_returns_none() {
        // Stub for now; once the DescribeImages-backed heuristic lands
        // this test grows into a per-platform table check.
        assert_eq!(default_username_for_ami("ami-0123456789abcdef0"), None);
    }
}

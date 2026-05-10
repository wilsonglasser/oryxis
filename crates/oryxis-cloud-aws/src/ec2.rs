//! EC2 discovery, `DescribeInstances` per region, mapped to
//! `oryxis_cloud::DiscoveredEc2`.

use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_ec2instanceconnect::Client as InstanceConnectClient;
use oryxis_cloud::{CloudError, CloudProfile, DiscoveredEc2};

use crate::auth::{build_sdk_config, AwsConfigJson};

/// Run discovery against every region the profile lists. When no region
/// list is configured, falls back to the profile's default region. The
/// caller is responsible for surfacing partial failures (one bad region
/// shouldn't kill the whole call), so each region's error is logged
/// rather than bubbled up, the returned vec contains whatever did work.
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
                    "EC2 discovery failed for region, skipping"
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

    let mut raw_instances: Vec<aws_sdk_ec2::types::Instance> = Vec::new();
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
                raw_instances.push(instance.clone());
            }
        }

        match resp.next_token() {
            Some(t) if !t.is_empty() => next_token = Some(t.to_string()),
            _ => break,
        }
    }

    // Batch-resolve AMI → default-username via DescribeImages so we
    // pick "ubuntu" for Ubuntu AMIs, "admin" for Debian, "ec2-user"
    // for Amazon Linux / RHEL / CentOS / SUSE, etc. Single API call
    // per region with up to 100 unique AMI ids; failure here just
    // falls back to the stub (no username), the importer plugs
    // "ec2-user" as the universal default downstream.
    let unique_amis: Vec<String> = {
        let mut s: std::collections::HashSet<String> = std::collections::HashSet::new();
        for inst in &raw_instances {
            if let Some(ami) = inst.image_id() {
                s.insert(ami.to_string());
            }
        }
        s.into_iter().collect()
    };
    let ami_user_map: std::collections::HashMap<String, String> = if unique_amis.is_empty() {
        std::collections::HashMap::new()
    } else {
        match describe_images_username_map(&client, region, &unique_amis).await {
            Ok(m) => m,
            Err(err) => {
                tracing::warn!(
                    target = "oryxis_cloud_aws",
                    region = %region,
                    error = %err,
                    "ec2:DescribeImages failed, falling back to no per-AMI username inference"
                );
                std::collections::HashMap::new()
            }
        }
    };

    let mut out = Vec::new();
    for instance in &raw_instances {
        if let Some(d) = map_instance(instance, region, &ami_user_map) {
            out.push(d);
        }
    }
    Ok(out)
}

/// Call `DescribeImages` in batches (API ceiling: 1000 ImageIds per
/// request, but we keep batches under 100 to stay below typical IAM
/// rate-limit budgets) and return a map AMI-id → inferred username.
/// AMIs that don't match any heuristic are simply absent from the map.
async fn describe_images_username_map(
    client: &Ec2Client,
    region: &str,
    ami_ids: &[String],
) -> Result<std::collections::HashMap<String, String>, CloudError> {
    let mut map = std::collections::HashMap::new();
    for chunk in ami_ids.chunks(100) {
        let resp = client
            .describe_images()
            .set_image_ids(Some(chunk.to_vec()))
            .send()
            .await
            .map_err(|e| {
                CloudError::Upstream(format!("ec2:DescribeImages {region}: {e}"))
            })?;
        for image in resp.images() {
            let Some(id) = image.image_id() else { continue };
            // Description is the most reliable signal; name is a
            // fallback because some custom AMIs leave description
            // blank. Both lowercased so the matcher is
            // case-insensitive.
            let mut hay = String::new();
            if let Some(desc) = image.description() {
                hay.push_str(desc);
                hay.push(' ');
            }
            if let Some(name) = image.name() {
                hay.push_str(name);
            }
            let user = infer_default_username(&hay.to_lowercase());
            if let Some(u) = user {
                map.insert(id.to_string(), u);
            }
        }
    }
    Ok(map)
}

/// Map an EC2 `Instance` row to our wire type. Returns `None` only when
/// the instance has no `instance_id` (which the SDK guarantees is never
/// the case in practice, defensive).
fn map_instance(
    instance: &aws_sdk_ec2::types::Instance,
    region: &str,
    ami_user_map: &std::collections::HashMap<String, String>,
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
        .and_then(|ami| ami_user_map.get(ami).cloned());

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

/// Match a lowercased AMI description / name against the conventional
/// default-username table for popular distros on AWS. Order matters:
/// "amazon linux" must be checked before plain "linux", "ubuntu pro"
/// before "ubuntu", etc. Returns `None` for unknown images so the
/// caller can fall back to "ec2-user".
fn infer_default_username(haystack_lower: &str) -> Option<String> {
    let h = haystack_lower;
    // Bitnami images ship with a `bitnami` user pre-baked.
    if h.contains("bitnami") {
        return Some("bitnami".into());
    }
    if h.contains("ubuntu") {
        return Some("ubuntu".into());
    }
    // Debian official AMIs use `admin`, not `debian`. The other way
    // around is a common mistake, be explicit.
    if h.contains("debian") {
        return Some("admin".into());
    }
    if h.contains("amazon linux") || h.contains("amzn") {
        return Some("ec2-user".into());
    }
    if h.contains("rhel") || h.contains("red hat") {
        return Some("ec2-user".into());
    }
    if h.contains("rocky") || h.contains("alma") {
        return Some("rocky".into());
    }
    if h.contains("centos") {
        // Newer official CentOS AMIs (8+) use `centos`, very old
        // ones used `root`. `centos` is the safe default.
        return Some("centos".into());
    }
    if h.contains("suse") {
        return Some("ec2-user".into());
    }
    if h.contains("freebsd") {
        return Some("ec2-user".into());
    }
    if h.contains("fedora") {
        return Some("fedora".into());
    }
    if h.contains("kali") {
        return Some("kali".into());
    }
    None
}

/// Push an SSH public key to an EC2 instance via Instance Connect.
///
/// AWS holds the key in the instance's `~/.ssh/authorized_keys` for
/// ~60 seconds; the caller has that window to open a regular SSH
/// session with the matching private key. Idempotent, safe to call
/// multiple times if the user reconnects within the same minute.
///
/// `region` should come from the resource's `CloudRef.region` so a
/// cross-region account dispatches to the right Instance Connect
/// endpoint (each region runs its own).
pub async fn push_instance_connect_key(
    profile: &CloudProfile,
    region: &str,
    instance_id: &str,
    os_user: &str,
    public_key: &str,
) -> Result<(), CloudError> {
    if region.is_empty() {
        return Err(CloudError::InvalidConfig(
            "Instance Connect requires the resource's region (`cloud_ref.region`) to be set"
                .into(),
        ));
    }
    if public_key.trim().is_empty() {
        return Err(CloudError::InvalidConfig(
            "Instance Connect needs a public key, link an SSH key to this host first".into(),
        ));
    }

    tracing::info!(
        target = "oryxis_cloud_aws",
        region = %region,
        instance_id = %instance_id,
        os_user = %os_user,
        "EC2 Instance Connect: pushing temporary public key"
    );
    let sdk = build_sdk_config(profile, Some(region), None).await?;
    let client = InstanceConnectClient::new(&sdk);
    client
        .send_ssh_public_key()
        .instance_id(instance_id)
        .instance_os_user(os_user)
        .ssh_public_key(public_key.trim())
        .send()
        .await
        .map(|_| {
            tracing::info!(
                target = "oryxis_cloud_aws",
                region = %region,
                instance_id = %instance_id,
                os_user = %os_user,
                "EC2 Instance Connect: SendSSHPublicKey ok (key valid for ~60s)"
            );
        })
        .map_err(|e| {
            CloudError::Upstream(format!(
                "ec2-instance-connect:SendSSHPublicKey {region}/{instance_id} as {os_user}: {e}"
            ))
        })
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
    fn infer_default_username_table() {
        // Substring match against the lowercased AMI description.
        // Order in the matcher matters: "amazon linux" must beat
        // "linux", "ubuntu pro" must beat "ubuntu pro debian
        // backport"-style noise (no real AMI names like that, but
        // be defensive).
        assert_eq!(
            infer_default_username("ubuntu server 22.04 lts amd64").as_deref(),
            Some("ubuntu")
        );
        assert_eq!(
            infer_default_username("amazon linux 2023 ami").as_deref(),
            Some("ec2-user")
        );
        assert_eq!(
            infer_default_username("debian 12 bookworm").as_deref(),
            Some("admin")
        );
        assert_eq!(
            infer_default_username("rhel 9 x86_64").as_deref(),
            Some("ec2-user")
        );
        assert_eq!(
            infer_default_username("rocky linux 9").as_deref(),
            Some("rocky")
        );
        assert_eq!(
            infer_default_username("bitnami wordpress 6 on debian").as_deref(),
            Some("bitnami")
        );
        assert_eq!(
            infer_default_username("custom homemade image").as_deref(),
            None,
        );
    }
}

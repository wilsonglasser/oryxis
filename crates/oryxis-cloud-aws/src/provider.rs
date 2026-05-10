//! `CloudProvider` impl for AWS — wires `auth` + `ec2` modules.

use async_trait::async_trait;
use aws_sdk_sts::Client as StsClient;
use oryxis_cloud::{
    CloudError, CloudProfile, CloudProvider, CloudQuery, CloudResourceType, DiscoveredHost,
    DiscoveryResult, TransportKind,
};

use crate::auth::build_sdk_config;
use crate::ec2::discover_ec2;
use crate::ecs::discover_ecs_services;

/// Walk `std::error::Error::source()` and concatenate every layer's
/// display into one chain. The AWS SDK wraps the actual cause (TLS
/// handshake, missing profile, DNS, 4xx body) under several layers of
/// generic enum variants, so the top-level `Display` is usually just
/// "dispatch failure" / "service error" — useless on its own.
fn error_chain<E: std::error::Error>(top: &E) -> String {
    let mut parts = vec![top.to_string()];
    let mut current: Option<&dyn std::error::Error> = top.source();
    while let Some(src) = current {
        parts.push(src.to_string());
        current = src.source();
    }
    parts.join(" -> ")
}

#[derive(Default)]
pub struct AwsProvider;

impl AwsProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CloudProvider for AwsProvider {
    fn id(&self) -> &'static str {
        "aws"
    }

    async fn test_credentials(&self, profile: &CloudProfile) -> Result<(), CloudError> {
        // STS GetCallerIdentity is the canonical "are these creds
        // alive" check — tiny payload, no IAM permission required for
        // the caller, ~150ms typical latency.
        let sdk = build_sdk_config(profile, None, None).await?;
        let sts = StsClient::new(&sdk);
        sts.get_caller_identity()
            .send()
            .await
            .map(|_| ())
            .map_err(|e| {
                // The SDK's `Display` collapses the entire error
                // chain into a single short string ("dispatch
                // failure") that hides whether it was TLS, DNS, a
                // missing profile, or a 4xx. Walk `Error::source()`
                // explicitly so the wizard surfaces something
                // actionable instead of `dispatch failure`.
                CloudError::Auth(format!(
                    "sts:GetCallerIdentity: {}",
                    error_chain(&e)
                ))
            })
    }

    async fn discover(&self, profile: &CloudProfile) -> Result<DiscoveryResult, CloudError> {
        // Run EC2 + ECS discovery concurrently — they hit different
        // SDK clients and have no shared state, so paying both
        // round-trips in parallel halves wall-clock latency on big
        // accounts. K8s isn't an AWS resource family; it ships in
        // the standalone `oryxis-cloud-k8s` provider.
        let (ec2_res, ecs_res) =
            tokio::join!(discover_ec2(profile), discover_ecs_services(profile));
        Ok(DiscoveryResult {
            ec2: ec2_res?,
            ecs_services: ecs_res?,
            k8s_workloads: Vec::new(),
        })
    }

    async fn resolve_query(
        &self,
        _profile: &CloudProfile,
        _query: &CloudQuery,
    ) -> Result<Vec<DiscoveredHost>, CloudError> {
        // Dynamic groups ride on ECS / K8s — both arrive in later PRs.
        // Returning Unsupported here (rather than empty Vec) so a
        // mistakenly-attached query surfaces loudly instead of looking
        // like an empty group.
        Err(CloudError::Unsupported(
            "AWS provider does not resolve dynamic-group queries yet (ECS support pending)".into(),
        ))
    }

    fn supported_transports(&self, resource_type: CloudResourceType) -> Vec<TransportKind> {
        match resource_type {
            CloudResourceType::Ec2 => vec![
                TransportKind::Ssh,
                TransportKind::InstanceConnect,
                TransportKind::Ssm,
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_is_stable() {
        assert_eq!(AwsProvider::new().id(), "aws");
    }

    #[test]
    fn ec2_supports_three_transports() {
        let p = AwsProvider::new();
        let t = p.supported_transports(CloudResourceType::Ec2);
        assert_eq!(t.len(), 3);
        assert!(t.contains(&TransportKind::Ssh));
        assert!(t.contains(&TransportKind::InstanceConnect));
        assert!(t.contains(&TransportKind::Ssm));
    }
}

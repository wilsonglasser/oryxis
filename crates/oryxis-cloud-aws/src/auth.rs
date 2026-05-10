//! AWS auth — parse `CloudProfile.config` and build an `aws_config::SdkConfig`.
//!
//! Each `auth_kind` is parsed strictly: an unknown variant returns
//! `CloudError::Unsupported` instead of silently falling back, so a
//! profile written by a newer Oryxis doesn't get authenticated against
//! the wrong path on an older build.

use aws_config::{BehaviorVersion, Region, SdkConfig};
use oryxis_cloud::{CloudError, CloudProfile};
use serde::{Deserialize, Serialize};

/// Non-secret slice of the AWS-flavoured `CloudProfile.config` JSON.
/// Optional fields all default to `None` so older configs (or hand-edited
/// ones missing keys) still parse.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AwsConfigJson {
    /// Named AWS CLI profile (`~/.aws/config`). `None` falls back to
    /// the default profile resolution chain.
    pub profile_name: Option<String>,
    /// Default region used by single-region operations. Discovery may
    /// fan out to `regions` instead when set.
    pub region: Option<String>,
    /// Region whitelist for discovery. Empty / absent means "use
    /// `region` only" (no implicit fan-out — the user controls scope).
    pub regions: Vec<String>,
    /// For `auth_kind = "access_key"`: the access key *id* (the secret
    /// half lives in the encrypted `cloud_profiles.secret` column).
    pub access_key_id: Option<String>,
    /// For `auth_kind = "sso"`: SSO start URL (e.g.
    /// `https://acme.awsapps.com/start`).
    pub sso_start_url: Option<String>,
    /// For `auth_kind = "sso"`: SSO region (where the SSO instance lives,
    /// not the workload region).
    pub sso_region: Option<String>,
    /// For `auth_kind = "sso"`: target account id and role.
    pub sso_account_id: Option<String>,
    pub sso_role_name: Option<String>,
}

impl AwsConfigJson {
    pub fn parse(profile: &CloudProfile) -> Result<Self, CloudError> {
        if profile.provider != "aws" {
            return Err(CloudError::InvalidConfig(format!(
                "expected provider=\"aws\", got \"{}\"",
                profile.provider
            )));
        }
        serde_json::from_str::<Self>(&profile.config)
            .map_err(|e| CloudError::InvalidConfig(format!("config json: {e}")))
    }
}

/// Build an `aws_config::SdkConfig` for a given profile, optionally
/// overriding the region (used by per-region discovery fan-out).
///
/// `secret` carries the decrypted blob from `cloud_profiles.secret`
/// when needed (access-key auth). For `profile` and `sso` it can be
/// `None`.
pub async fn build_sdk_config(
    profile: &CloudProfile,
    region_override: Option<&str>,
    _secret: Option<&str>,
) -> Result<SdkConfig, CloudError> {
    let cfg = AwsConfigJson::parse(profile)?;

    let region = region_override
        .map(str::to_string)
        .or_else(|| cfg.region.clone());

    let mut loader = aws_config::defaults(BehaviorVersion::latest());
    if let Some(r) = region {
        loader = loader.region(Region::new(r));
    }

    match profile.auth_kind.as_str() {
        // Default credential chain — picks up the named profile from
        // `~/.aws/config` / `~/.aws/credentials`. Falls through to env
        // vars + container/IMDS providers when `profile_name` is unset,
        // matching what `aws CLI` does.
        "profile" => {
            if let Some(name) = cfg.profile_name.as_deref() {
                loader = loader.profile_name(name);
            }
        }
        // Access key auth + SSO are stubbed for this PR — they land in
        // the next batch of PRs once the wizard surfaces them. We don't
        // silently downgrade to the default chain because that would
        // authenticate as the wrong identity.
        "access_key" | "sso" => {
            return Err(CloudError::Unsupported(format!(
                "AWS auth_kind \"{}\" is not implemented yet",
                profile.auth_kind
            )));
        }
        other => {
            return Err(CloudError::Unsupported(format!(
                "AWS auth_kind \"{other}\" is not recognised"
            )));
        }
    }

    Ok(loader.load().await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(config: &str, auth_kind: &str) -> CloudProfile {
        let mut p = CloudProfile::new("test", "aws");
        p.auth_kind = auth_kind.into();
        p.config = config.into();
        p
    }

    #[test]
    fn parses_profile_config() {
        let p = profile(
            r#"{"profile_name":"prod","region":"us-east-1","regions":["us-east-1","eu-west-1"]}"#,
            "profile",
        );
        let cfg = AwsConfigJson::parse(&p).unwrap();
        assert_eq!(cfg.profile_name.as_deref(), Some("prod"));
        assert_eq!(cfg.region.as_deref(), Some("us-east-1"));
        assert_eq!(cfg.regions.len(), 2);
    }

    #[test]
    fn empty_config_parses_to_defaults() {
        let p = profile("{}", "profile");
        let cfg = AwsConfigJson::parse(&p).unwrap();
        assert!(cfg.profile_name.is_none());
        assert!(cfg.regions.is_empty());
    }

    #[test]
    fn rejects_non_aws_provider() {
        let mut p = CloudProfile::new("k8s", "k8s");
        p.config = "{}".into();
        let err = AwsConfigJson::parse(&p).unwrap_err();
        match err {
            CloudError::InvalidConfig(_) => {}
            _ => panic!("wrong error variant: {err:?}"),
        }
    }

    #[test]
    fn rejects_malformed_json() {
        let p = profile("{not-json", "profile");
        assert!(matches!(
            AwsConfigJson::parse(&p),
            Err(CloudError::InvalidConfig(_))
        ));
    }

    #[tokio::test]
    async fn build_sdk_config_rejects_unknown_auth_kind() {
        let p = profile(r#"{"region":"us-east-1"}"#, "magic-auth");
        let err = build_sdk_config(&p, None, None).await.unwrap_err();
        assert!(matches!(err, CloudError::Unsupported(_)));
    }

    #[tokio::test]
    async fn build_sdk_config_rejects_access_key_for_now() {
        let p = profile(r#"{"region":"us-east-1"}"#, "access_key");
        let err = build_sdk_config(&p, None, None).await.unwrap_err();
        match err {
            CloudError::Unsupported(msg) => assert!(msg.contains("access_key")),
            _ => panic!("wrong error variant: {err:?}"),
        }
    }
}

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A configured cloud account that providers can authenticate against
/// and discover resources in. Mirrors the `Identity` / `ProxyIdentity`
/// shape: a non-secret model row plus an encrypted `secret` BLOB column
/// in the vault, hydrated only when a provider needs it.
///
/// `provider` and `auth_kind` are stored as plain strings instead of
/// enums so a provider released after this row was written still
/// deserializes (forward compatibility for sync).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudProfile {
    pub id: Uuid,
    pub label: String,
    /// `"aws"`, `"k8s"`, …, matches `CloudProvider::id()`.
    pub provider: String,
    /// Provider-specific auth strategy. AWS: `"profile"`, `"access_key"`,
    /// `"sso"`. K8s: `"kubeconfig"`.
    pub auth_kind: String,
    /// JSON-encoded non-secret config (region, profile name, kubeconfig
    /// path, SSO start URL, …). Provider crates own the schema.
    pub config: String,
    /// Optional last-discovery timestamp the UI shows next to the
    /// profile. Updated by call sites; not used for any logic.
    pub last_discovered: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,

    /// Decrypted secret payload, never persisted (skip serde). The
    /// dispatcher populates this from `vault.get_cloud_profile_secret`
    /// right before handing the profile to a `CloudProvider` call so
    /// the AWS-side code can read it without a vault dependency.
    /// `None` when no secret is set OR before the dispatcher hydrated.
    #[serde(skip, default)]
    pub secret: Option<String>,
}

impl CloudProfile {
    pub fn new(label: impl Into<String>, provider: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            provider: provider.into(),
            auth_kind: String::new(),
            config: "{}".into(),
            last_discovered: None,
            created_at: now,
            updated_at: now,
            secret: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_cloud_profile_defaults() {
        let p = CloudProfile::new("prod", "aws");
        assert_eq!(p.label, "prod");
        assert_eq!(p.provider, "aws");
        assert_eq!(p.config, "{}");
        assert!(p.last_discovered.is_none());
    }

    #[test]
    fn cloud_profile_serialization_roundtrip() {
        let mut p = CloudProfile::new("prod", "aws");
        p.auth_kind = "profile".into();
        p.config = r#"{"profile_name":"production","region":"us-east-1"}"#.into();

        let json = serde_json::to_string(&p).unwrap();
        let de: CloudProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(de.label, "prod");
        assert_eq!(de.provider, "aws");
        assert_eq!(de.auth_kind, "profile");
        assert!(de.config.contains("us-east-1"));
    }
}

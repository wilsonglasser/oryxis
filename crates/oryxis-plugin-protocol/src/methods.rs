//! Typed request/response schema for every plugin operation.
//!
//! The contract is 8 JSON-RPC methods: `initialize` plus the 7
//! provider/transport operations. (The original exploration counted
//! 9, but `plugin_invocation` is pure argv formatting, not a
//! roundtrip, it lives here as the free function [`plugin_invocation`]
//! and the host calls it locally on whatever [`SessionPayload`] came
//! back from `start_ecs_exec` / `start_ssm_session`.)
//!
//! Each operation is a zero-sized type implementing [`Method`], which
//! ties the method-name string to its `Params` and `Result` types so
//! the host can issue calls without stringly-typed plumbing.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use oryxis_cloud::{DiscoveredHost, DiscoveryResult, SessionPayload};
use oryxis_core::models::{CloudProfile, CloudQuery, CloudResourceType, TransportKind};

/// JSON-RPC method-name strings. Namespaced (`provider.*`,
/// `transport.*`) so future operation families can be added without
/// renaming the existing ones.
pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const TEST_CREDENTIALS: &str = "provider.test_credentials";
    pub const DISCOVER: &str = "provider.discover";
    pub const RESOLVE_QUERY: &str = "provider.resolve_query";
    pub const SUPPORTED_TRANSPORTS: &str = "provider.supported_transports";
    pub const START_ECS_EXEC: &str = "transport.start_ecs_exec";
    pub const START_SSM_SESSION: &str = "transport.start_ssm_session";
    pub const PUSH_INSTANCE_CONNECT_KEY: &str = "transport.push_instance_connect_key";
}

/// Links a JSON-RPC method name to its typed params and result.
///
/// Implemented by a zero-sized marker type per operation so the host
/// can write `host.call::<Discover>(params)` and get a
/// `DiscoveryResult` back without naming the method string or the
/// result type twice.
pub trait Method {
    const NAME: &'static str;
    type Params: Serialize + DeserializeOwned;
    type Result: Serialize + DeserializeOwned;
}

// ---------------------------------------------------------------------------
// initialize, capability negotiation
// ---------------------------------------------------------------------------

/// Sent by the host as the first frame on every plugin connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// Protocol versions the host understands. Order is irrelevant,
    /// the plugin picks the max of the intersection.
    pub supported_versions: Vec<u32>,
}

/// The plugin's reply to `initialize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// Protocol version the plugin selected: the highest value
    /// present in both the host's `supported_versions` and the
    /// plugin's own set. The host aborts the connection if this
    /// isn't a version it offered.
    pub protocol_version: u32,
    /// Stable provider id, matches `CloudProvider::id()` and
    /// `CloudProfile.provider` (`"aws"`, ...).
    pub provider_id: String,
    /// Human-readable plugin build version (`"0.4.2"`), shown in the
    /// plugins UI. Never used for protocol decisions.
    pub plugin_version: String,
    /// Method names this plugin actually implements. The host greys
    /// out transports whose method isn't listed, this is how a
    /// provider that doesn't support, say, ECS Exec advertises that
    /// without the host hardcoding per-provider knowledge.
    pub capabilities: Vec<String>,
}

/// `initialize`, protocol + capability handshake.
pub struct Initialize;
impl Method for Initialize {
    const NAME: &'static str = method::INITIALIZE;
    type Params = InitializeParams;
    type Result = InitializeResult;
}

// ---------------------------------------------------------------------------
// provider.test_credentials
// ---------------------------------------------------------------------------

/// Params for any operation that only needs the hydrated profile.
///
/// The profile carries its decrypted `secret` inline, plugins are
/// stateless and never touch the vault, so every call ships the full
/// credential. The host hydrates `CloudProfile.secret` from the
/// vault immediately before serializing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileParams {
    pub profile: CloudProfile,
}

/// `provider.test_credentials`, one cheap call to validate creds.
pub struct TestCredentials;
impl Method for TestCredentials {
    const NAME: &'static str = method::TEST_CREDENTIALS;
    type Params = ProfileParams;
    /// Success carries no payload, the absence of an error *is* the
    /// result.
    type Result = ();
}

// ---------------------------------------------------------------------------
// provider.discover
// ---------------------------------------------------------------------------

/// `provider.discover`, one-shot discovery for the wizard.
pub struct Discover;
impl Method for Discover {
    const NAME: &'static str = method::DISCOVER;
    type Params = ProfileParams;
    type Result = DiscoveryResult;
}

// ---------------------------------------------------------------------------
// provider.resolve_query
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveQueryParams {
    pub profile: CloudProfile,
    pub query: CloudQuery,
}

/// `provider.resolve_query`, re-resolve a dynamic group's children.
pub struct ResolveQuery;
impl Method for ResolveQuery {
    const NAME: &'static str = method::RESOLVE_QUERY;
    type Params = ResolveQueryParams;
    type Result = Vec<DiscoveredHost>;
}

// ---------------------------------------------------------------------------
// provider.supported_transports
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportedTransportsParams {
    pub resource_type: CloudResourceType,
}

/// `provider.supported_transports`, which transports a resource
/// family can open. Pure metadata, no profile needed.
pub struct SupportedTransports;
impl Method for SupportedTransports {
    const NAME: &'static str = method::SUPPORTED_TRANSPORTS;
    type Params = SupportedTransportsParams;
    type Result = Vec<TransportKind>;
}

// ---------------------------------------------------------------------------
// Shared transport result, the session-manager-plugin payload
// ---------------------------------------------------------------------------

// `SessionPayload` is canonical in `oryxis-cloud` (the `CloudProvider`
// trait's transport methods return it). It's re-exported from this
// crate's `lib.rs` so plugin + host reach for one path.

/// Format the 6-positional-arg invocation `session-manager-plugin`
/// expects.
///
/// This is pure formatting, not a JSON-RPC operation, the host calls
/// it locally on whatever [`SessionPayload`] came back from
/// `start_ecs_exec` / `start_ssm_session`. Keeping it host-side
/// avoids a useless roundtrip on every connect.
pub fn plugin_invocation(session: &SessionPayload) -> Vec<String> {
    vec![
        session.session_json.clone(),
        session.region.clone(),
        "StartSession".to_string(),
        session.profile_name.clone(),
        session.start_session_request.clone(),
        session.endpoint.clone(),
    ]
}

// ---------------------------------------------------------------------------
// transport.start_ecs_exec
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartEcsExecParams {
    pub profile: CloudProfile,
    pub region: String,
    pub cluster: String,
    pub task_id: String,
    pub container: String,
    /// Interactive command run inside the container (`/bin/sh`,
    /// `bash -i`, ...). The host resolves the default before calling.
    pub command: String,
}

/// `transport.start_ecs_exec`, open an ECS Exec session.
pub struct StartEcsExec;
impl Method for StartEcsExec {
    const NAME: &'static str = method::START_ECS_EXEC;
    type Params = StartEcsExecParams;
    type Result = SessionPayload;
}

// ---------------------------------------------------------------------------
// transport.start_ssm_session
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartSsmSessionParams {
    pub profile: CloudProfile,
    pub region: String,
    pub instance_id: String,
}

/// `transport.start_ssm_session`, open an SSM Session against an EC2
/// instance.
pub struct StartSsmSession;
impl Method for StartSsmSession {
    const NAME: &'static str = method::START_SSM_SESSION;
    type Params = StartSsmSessionParams;
    type Result = SessionPayload;
}

// ---------------------------------------------------------------------------
// transport.push_instance_connect_key
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushInstanceConnectKeyParams {
    pub profile: CloudProfile,
    pub region: String,
    pub instance_id: String,
    pub os_user: String,
    pub public_key: String,
}

/// `transport.push_instance_connect_key`, push a temporary SSH key
/// via EC2 Instance Connect.
pub struct PushInstanceConnectKey;
impl Method for PushInstanceConnectKey {
    const NAME: &'static str = method::PUSH_INSTANCE_CONNECT_KEY;
    type Params = PushInstanceConnectKeyParams;
    /// Success carries no payload, the host follows up with a plain
    /// SSH connect inside the ~60s key window.
    type Result = ();
}

#[cfg(test)]
mod tests {
    use super::*;
    use oryxis_core::models::CloudProfile;

    #[test]
    fn profile_params_roundtrip() {
        let mut profile = CloudProfile::new("prod", "aws");
        profile.secret = Some("decrypted-secret".into());
        let params = ProfileParams { profile };
        let json = serde_json::to_string(&params).unwrap();
        let back: ProfileParams = serde_json::from_str(&json).unwrap();
        assert_eq!(back.profile.label, "prod");
        // `secret` is `#[serde(skip)]` on `CloudProfile`, it must NOT
        // survive a roundtrip, the host re-hydrates it per call from
        // the vault rather than trusting wire data.
        assert_eq!(back.profile.secret, None);
    }

    #[test]
    fn session_payload_roundtrip_and_invocation() {
        let payload = SessionPayload {
            session_json: r#"{"SessionId":"s-1"}"#.into(),
            region: "us-east-1".into(),
            profile_name: "prod".into(),
            start_session_request: r#"{"Target":"i-0abc"}"#.into(),
            endpoint: "https://ssm.us-east-1.amazonaws.com".into(),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let back: SessionPayload = serde_json::from_str(&json).unwrap();
        let argv = plugin_invocation(&back);
        assert_eq!(argv.len(), 6);
        assert_eq!(argv[2], "StartSession");
        assert_eq!(argv[1], "us-east-1");
    }

    #[test]
    fn method_names_are_namespaced_and_unique() {
        let names = [
            Initialize::NAME,
            TestCredentials::NAME,
            Discover::NAME,
            ResolveQuery::NAME,
            SupportedTransports::NAME,
            StartEcsExec::NAME,
            StartSsmSession::NAME,
            PushInstanceConnectKey::NAME,
        ];
        let mut seen = std::collections::HashSet::new();
        for n in names {
            assert!(seen.insert(n), "duplicate method name: {n}");
        }
    }
}

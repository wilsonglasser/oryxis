//! Transport session payload.
//!
//! SSM Session and ECS Exec don't speak SSH, they hand off to the
//! AWS `session-manager-plugin`, a separate binary that owns the
//! streaming WebSocket protocol. `SessionPayload` is everything that
//! plugin needs to attach: the provider builds it, the caller feeds
//! it to `oryxis_plugin_protocol::plugin_invocation` to format the
//! subprocess argv.
//!
//! It lives here, not in a provider crate, because the
//! `CloudProvider` trait's transport methods return it: every
//! consumer (the in-process trait, the JSON-RPC plugin wire, the
//! protocol crate's `plugin_invocation`) sees the one type.

use serde::{Deserialize, Serialize};

/// Everything `session-manager-plugin` needs to attach to an SSM /
/// ECS Exec session. Field names mirror what the AWS CLI passes the
/// plugin, keep them exact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPayload {
    /// The `{SessionId, StreamUrl, TokenValue}` JSON object the
    /// plugin reads as its first positional arg. Already serialized,
    /// the plugin wants a string, not an object.
    pub session_json: String,
    /// AWS region the session lives in. Same region the SSM endpoint
    /// resolves to.
    pub region: String,
    /// AWS CLI profile name used when starting the session. Empty
    /// for env-var / SSO / access-key auth, where the plugin doesn't
    /// need a profile name to refresh short-lived credentials.
    pub profile_name: String,
    /// SSM `StartSession` request as JSON. Encodes the target string
    /// (`ecs:cluster_task_runtimeId` for ECS Exec, the bare instance
    /// id for SSM) and the document to run.
    pub start_session_request: String,
    /// Region-specific SSM endpoint URL. The plugin uses it for any
    /// follow-up control calls (e.g. terminate-session).
    pub endpoint: String,
}

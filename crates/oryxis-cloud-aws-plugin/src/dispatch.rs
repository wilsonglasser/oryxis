//! Map JSON-RPC method calls onto `oryxis-cloud-aws`.
//!
//! Each operation deserializes its typed params, calls into the AWS
//! provider library, and serializes the result. Two error shapes
//! come back to the host:
//!
//! - [`error_codes::INVALID_PARAMS`], the params didn't deserialize,
//!   a contract / framing problem.
//! - [`error_codes::PROVIDER_ERROR`], the AWS call itself failed; the
//!   `CloudError` is serialized into the envelope's `data` field so
//!   the host can rebuild the exact variant.

use oryxis_cloud::CloudProvider;
use oryxis_cloud_aws::AwsProvider;
use oryxis_plugin_protocol::{
    cloud_error_to_rpc, error_codes, method, negotiate_version, CloudError, InitializeParams,
    InitializeResult, JsonRpcResponse, ProfileParams, PushInstanceConnectKeyParams,
    ResolveQueryParams, StartEcsExecParams, StartSsmSessionParams, SupportedTransportsParams,
    SUPPORTED_PROTOCOL_VERSIONS,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// Provider id this plugin serves. Matches `CloudProvider::id()` and
/// `CloudProfile.provider`.
const PROVIDER_ID: &str = "aws";

/// Failure modes of a single operation, before they're turned into a
/// JSON-RPC error frame.
enum OpError {
    /// Params didn't deserialize, or a result wouldn't serialize, a
    /// contract-level problem, not the provider's fault.
    BadParams(String),
    /// The AWS provider call returned a `CloudError`.
    Provider(CloudError),
}

/// Dispatch one request to the matching operation and turn the
/// outcome into a response frame.
pub async fn handle(method_name: &str, id: Value, params: Option<Value>) -> JsonRpcResponse {
    let result: Result<Value, OpError> = match method_name {
        method::INITIALIZE => initialize(params),
        method::TEST_CREDENTIALS => test_credentials(params).await,
        method::DISCOVER => discover(params).await,
        method::RESOLVE_QUERY => resolve_query(params).await,
        method::SUPPORTED_TRANSPORTS => supported_transports(params),
        method::START_ECS_EXEC => start_ecs_exec(params).await,
        method::START_SSM_SESSION => start_ssm_session(params).await,
        method::PUSH_INSTANCE_CONNECT_KEY => push_instance_connect_key(params).await,
        other => {
            return JsonRpcResponse::error(
                id,
                error_codes::METHOD_NOT_FOUND,
                format!("method not found: {other}"),
            );
        }
    };
    into_response(id, result)
}

/// Turn an operation outcome into a JSON-RPC response frame.
fn into_response(id: Value, result: Result<Value, OpError>) -> JsonRpcResponse {
    match result {
        Ok(value) => JsonRpcResponse::success(id, value),
        Err(OpError::BadParams(msg)) => {
            JsonRpcResponse::error(id, error_codes::INVALID_PARAMS, msg)
        }
        // Carry the provider error through with its variant intact.
        Err(OpError::Provider(err)) => {
            JsonRpcResponse::error_with_data(id, cloud_error_to_rpc(&err))
        }
    }
}

/// Deserialize typed params, mapping a failure to `BadParams`.
fn parse_params<P: DeserializeOwned>(params: Option<Value>) -> Result<P, OpError> {
    serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|e| OpError::BadParams(format!("invalid params: {e}")))
}

/// Serialize a result value, mapping a failure to `BadParams` (a
/// non-serializable result is a bug in our own types).
fn ok_value<T: Serialize>(value: T) -> Result<Value, OpError> {
    serde_json::to_value(value).map_err(|e| OpError::BadParams(format!("serialize result: {e}")))
}

// --- initialize -------------------------------------------------------------

fn initialize(params: Option<Value>) -> Result<Value, OpError> {
    let p: InitializeParams = parse_params(params)?;
    // Pick the highest protocol version both sides understand.
    let protocol_version = negotiate_version(&p.supported_versions, SUPPORTED_PROTOCOL_VERSIONS)
        .ok_or_else(|| {
            OpError::BadParams(format!(
                "no common protocol version: host {:?}, plugin {:?}",
                p.supported_versions, SUPPORTED_PROTOCOL_VERSIONS
            ))
        })?;
    ok_value(InitializeResult {
        protocol_version,
        provider_id: PROVIDER_ID.to_string(),
        plugin_version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: capabilities(),
    })
}

/// Method names this plugin implements. AWS covers all 7 ops, a
/// future provider with a thinner surface would list fewer here.
fn capabilities() -> Vec<String> {
    vec![
        method::TEST_CREDENTIALS.to_string(),
        method::DISCOVER.to_string(),
        method::RESOLVE_QUERY.to_string(),
        method::SUPPORTED_TRANSPORTS.to_string(),
        method::START_ECS_EXEC.to_string(),
        method::START_SSM_SESSION.to_string(),
        method::PUSH_INSTANCE_CONNECT_KEY.to_string(),
    ]
}

// --- provider operations ----------------------------------------------------

async fn test_credentials(params: Option<Value>) -> Result<Value, OpError> {
    let p: ProfileParams = parse_params(params)?;
    AwsProvider::new()
        .test_credentials(&p.profile)
        .await
        .map_err(OpError::Provider)?;
    ok_value(())
}

async fn discover(params: Option<Value>) -> Result<Value, OpError> {
    let p: ProfileParams = parse_params(params)?;
    let result = AwsProvider::new()
        .discover(&p.profile)
        .await
        .map_err(OpError::Provider)?;
    ok_value(result)
}

async fn resolve_query(params: Option<Value>) -> Result<Value, OpError> {
    let p: ResolveQueryParams = parse_params(params)?;
    let hosts = AwsProvider::new()
        .resolve_query(&p.profile, &p.query)
        .await
        .map_err(OpError::Provider)?;
    ok_value(hosts)
}

fn supported_transports(params: Option<Value>) -> Result<Value, OpError> {
    let p: SupportedTransportsParams = parse_params(params)?;
    let transports = AwsProvider::new().supported_transports(p.resource_type);
    ok_value(transports)
}

// --- transport operations ---------------------------------------------------
//
// All three go through the `CloudProvider` trait rather than the
// `oryxis-cloud-aws` free functions, the trait is the contract, and
// it already returns the wire `SessionPayload` directly.

async fn start_ecs_exec(params: Option<Value>) -> Result<Value, OpError> {
    let p: StartEcsExecParams = parse_params(params)?;
    let session = AwsProvider::new()
        .start_ecs_exec(
            &p.profile,
            &p.region,
            &p.cluster,
            &p.task_id,
            &p.container,
            &p.command,
        )
        .await
        .map_err(OpError::Provider)?;
    ok_value(session)
}

async fn start_ssm_session(params: Option<Value>) -> Result<Value, OpError> {
    let p: StartSsmSessionParams = parse_params(params)?;
    let session = AwsProvider::new()
        .start_ssm_session(&p.profile, &p.region, &p.instance_id)
        .await
        .map_err(OpError::Provider)?;
    ok_value(session)
}

async fn push_instance_connect_key(params: Option<Value>) -> Result<Value, OpError> {
    let p: PushInstanceConnectKeyParams = parse_params(params)?;
    AwsProvider::new()
        .push_instance_connect_key(
            &p.profile,
            &p.region,
            &p.instance_id,
            &p.os_user,
            &p.public_key,
        )
        .await
        .map_err(OpError::Provider)?;
    ok_value(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn initialize_returns_provider_metadata() {
        let resp = handle(
            method::INITIALIZE,
            Value::from(1),
            Some(serde_json::json!({ "supported_versions": [1] })),
        )
        .await;
        assert!(resp.error.is_none(), "initialize errored: {:?}", resp.error);
        let init: InitializeResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(init.provider_id, "aws");
        assert_eq!(init.protocol_version, 1);
        // AWS implements all 7 operations.
        assert_eq!(init.capabilities.len(), 7);
        assert!(init.capabilities.contains(&method::DISCOVER.to_string()));
    }

    #[tokio::test]
    async fn initialize_rejects_disjoint_protocol_versions() {
        let resp = handle(
            method::INITIALIZE,
            Value::from(1),
            Some(serde_json::json!({ "supported_versions": [999] })),
        )
        .await;
        let err = resp.error.expect("should reject");
        assert_eq!(err.code, error_codes::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn unknown_method_is_method_not_found() {
        let resp = handle("provider.bogus", Value::from(1), None).await;
        let err = resp.error.expect("should reject");
        assert_eq!(err.code, error_codes::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn bad_params_is_invalid_params() {
        // `discover` needs a `profile` object; a bare string can't
        // deserialize into `ProfileParams`.
        let resp = handle(
            method::DISCOVER,
            Value::from(1),
            Some(serde_json::json!("not a profile")),
        )
        .await;
        let err = resp.error.expect("should reject");
        assert_eq!(err.code, error_codes::INVALID_PARAMS);
    }
}

//! Bridge between provider-level `CloudError` and the JSON-RPC error
//! envelope.
//!
//! `CloudError` stays canonical in `oryxis-cloud`, this module only
//! does the round-trip through the envelope's `data` field. A plugin
//! that catches a `CloudError` calls [`cloud_error_to_rpc`] before
//! writing the response; the host calls [`rpc_error_to_cloud`] to
//! recover the exact variant.

use oryxis_cloud::CloudError;

use crate::jsonrpc::{error_codes, JsonRpcError};

/// Wrap a provider-level `CloudError` as a JSON-RPC error frame. The
/// error is serialized into `data` so the host can reconstruct the
/// exact variant; `message` carries the human-readable `Display` for
/// logs and for hosts that don't bother to parse `data`.
pub fn cloud_error_to_rpc(err: &CloudError) -> JsonRpcError {
    JsonRpcError {
        code: error_codes::PROVIDER_ERROR,
        message: err.to_string(),
        // Serialization of `CloudError` cannot fail (it's a flat enum
        // of `String`s), but stay defensive: a `None` here just
        // means the host falls back to `CloudError::Other(message)`.
        data: serde_json::to_value(err).ok(),
    }
}

/// Recover the original `CloudError` from a JSON-RPC error produced
/// by [`cloud_error_to_rpc`].
///
/// Falls back to `CloudError::Other(message)` when `data` is absent
/// or unparseable, which is the right behaviour for errors that
/// originated in the JSON-RPC framing layer (parse error, method not
/// found) rather than in the provider itself.
pub fn rpc_error_to_cloud(err: &JsonRpcError) -> CloudError {
    err.data
        .as_ref()
        .and_then(|d| serde_json::from_value::<CloudError>(d.clone()).ok())
        .unwrap_or_else(|| CloudError::Other(err.message.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_error_roundtrips_through_envelope() {
        let original = CloudError::Auth("sts:GetCallerIdentity failed".into());
        let rpc = cloud_error_to_rpc(&original);
        assert_eq!(rpc.code, error_codes::PROVIDER_ERROR);
        assert_eq!(rpc.message, original.to_string());
        assert_eq!(rpc_error_to_cloud(&rpc), original);
    }

    #[test]
    fn framing_error_without_data_falls_back_to_other() {
        // A method-not-found error never carries a serialized
        // CloudError, the host should still get a usable value.
        let rpc = JsonRpcError {
            code: error_codes::METHOD_NOT_FOUND,
            message: "method not found: bogus".into(),
            data: None,
        };
        assert_eq!(
            rpc_error_to_cloud(&rpc),
            CloudError::Other("method not found: bogus".into())
        );
    }
}

//! JSON-RPC 2.0 framing types.
//!
//! Ported from `oryxis-mcp::protocol`, with one structural change:
//! the MCP server only ever *receives* requests and *sends*
//! responses, so its types were one-directional. A plugin host both
//! sends requests and receives responses, so both types here
//! round-trip (`Serialize + Deserialize`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A JSON-RPC 2.0 request frame.
///
/// `id` is `None` only for notifications (no response expected).
/// The plugin host always sets an id; notifications are reserved for
/// a future streaming-progress channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    /// Build a request frame with the `"2.0"` version string filled
    /// in. `params` should already be a serialized params struct
    /// (`serde_json::to_value(MyParams { .. })`).
    pub fn new(id: impl Into<Value>, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: Some(id.into()),
            method: method.into(),
            params: Some(params),
        }
    }

    /// Build a notification frame (no `id`, so the plugin must not reply).
    /// Pass `Value::Null` for a parameterless notification.
    pub fn notification(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: None,
            method: method.into(),
            params: if params.is_null() { None } else { Some(params) },
        }
    }

    /// True when this frame is a notification (no `id`) and therefore
    /// must not receive a response.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// A JSON-RPC 2.0 response frame. Exactly one of `result` / `error`
/// is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// Error response carrying a structured `data` payload, used by
    /// plugins to ship a serialized `CloudError` back to the host.
    pub fn error_with_data(id: Value, err: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(err),
        }
    }
}

/// The `error` member of a failed [`JsonRpcResponse`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    /// Structured payload. For provider failures this is the
    /// serialized `CloudError` (see [`crate::error`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 standard error codes plus the plugin-specific
/// `PROVIDER_ERROR` range.
pub mod error_codes {
    /// Invalid JSON was received.
    pub const PARSE_ERROR: i32 = -32700;
    /// The JSON sent is not a valid request object.
    pub const INVALID_REQUEST: i32 = -32600;
    /// The method does not exist on this plugin.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid method parameters.
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal plugin error not attributable to the provider call.
    pub const INTERNAL_ERROR: i32 = -32603;
    /// The call reached the provider and the provider returned a
    /// `CloudError`. The `data` field carries the serialized error
    /// so the host can rebuild the exact variant.
    pub const PROVIDER_ERROR: i32 = -32000;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip_omits_none_fields() {
        let req = JsonRpcRequest::new(7, "provider.discover", serde_json::json!({"a": 1}));
        let json = serde_json::to_string(&req).unwrap();
        let back: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.jsonrpc, "2.0");
        assert_eq!(back.method, "provider.discover");
        assert_eq!(back.id, Some(Value::from(7)));
        assert!(!back.is_notification());
    }

    #[test]
    fn notification_has_no_id() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/progress"}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert!(req.is_notification());
    }

    #[test]
    fn response_roundtrip_success() {
        let resp = JsonRpcResponse::success(Value::from(1), serde_json::json!({"ok": true}));
        let json = serde_json::to_string(&resp).unwrap();
        let back: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        assert!(back.error.is_none());
        assert_eq!(back.result, Some(serde_json::json!({"ok": true})));
    }
}

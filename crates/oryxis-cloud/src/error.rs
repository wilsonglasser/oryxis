use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Provider-level error. `Serialize`/`Deserialize` so it round-trips
/// across the plugin JSON-RPC pipe: the plugin serializes the variant
/// into the error envelope's `data` field and the host reconstructs
/// the exact variant rather than re-parsing a flattened string.
/// `Clone` + `PartialEq` exist for the same reason (host-side caching
/// and the round-trip unit test).
#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloudError {
    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("provider returned: {0}")]
    Upstream(String),

    #[error("resource not found: {0}")]
    NotFound(String),

    #[error("operation not supported by this provider: {0}")]
    Unsupported(String),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant must survive a JSON round-trip unchanged, the
    /// plugin pipe relies on it to ferry provider errors back to the
    /// host without collapsing them into a flat string.
    #[test]
    fn cloud_error_json_roundtrip_every_variant() {
        let variants = [
            CloudError::Auth("bad creds".into()),
            CloudError::Network("timeout".into()),
            CloudError::Upstream("503".into()),
            CloudError::NotFound("i-0abc".into()),
            CloudError::Unsupported("k8s on aws".into()),
            CloudError::InvalidConfig("missing region".into()),
            CloudError::Other("misc".into()),
        ];
        for original in variants {
            let json = serde_json::to_string(&original).unwrap();
            let back: CloudError = serde_json::from_str(&json).unwrap();
            assert_eq!(original, back, "round-trip changed {original:?}");
        }
    }
}

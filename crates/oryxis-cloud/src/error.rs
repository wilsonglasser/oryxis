use thiserror::Error;

#[derive(Debug, Error)]
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

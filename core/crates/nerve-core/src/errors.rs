use thiserror::Error;

#[derive(Debug, Error)]
pub enum NerveError {
    #[error("not implemented on this platform: {0}")]
    Unsupported(String),

    #[error("missing OS permission: {0}")]
    PermissionDenied(String),

    #[error("rejected by safety policy: {0}")]
    SafetyRejected(String),

    #[error("rate limit exceeded ({allowed}/min)")]
    RateLimited { allowed: u32 },

    #[error("emergency stop is engaged")]
    EmergencyStopped,

    #[error("element not found")]
    ElementNotFound,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("backend error: {0}")]
    Backend(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, NerveError>;

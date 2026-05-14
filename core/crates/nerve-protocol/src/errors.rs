//! Stable error codes and protocol version negotiation.
//!
//! These are *part of the wire contract* — once shipped in a release, codes
//! never change meaning. Adding new codes is fine; renaming or repurposing
//! them is a breaking change.

use serde::{Deserialize, Serialize};

/// Stable error code surfaced to clients. The numeric `code` is what SDKs
/// pattern-match on; the `message` is human-readable and can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Catch-all for unexpected internal failures.
    Internal,
    /// Request shape was malformed JSON or violated the schema.
    BadRequest,
    /// Binary frames or other features that aren't supported on this build.
    Unsupported,
    /// Daemon refused the action because no session was started.
    NoSession,
    /// Session id was supplied but no matching session is active.
    SessionNotFound,
    /// Token auth failed at `session_start`.
    AuthRequired,
    /// Token auth was attempted but the token was wrong.
    AuthInvalid,
    /// Protocol version mismatch detected during handshake.
    VersionMismatch,
    /// Safety engine refused the action.
    SafetyRejected,
    /// Safety rate limit hit.
    RateLimited,
    /// Emergency stop is engaged.
    EmergencyStopped,
    /// Compiler could not locate a target element.
    ElementNotFound,
    /// Platform backend reported an OS-level failure.
    BackendFailure,
    /// Daemon is missing a permission required for this action.
    PermissionDenied,
    /// Action was deduplicated because an idempotency key was seen before.
    Idempotent,
    /// Reading or writing the audit log failed.
    LogIoError,
    /// Replay requested a session whose log can't be read.
    ReplayUnavailable,
}

impl ErrorCode {
    /// Whether a retry has any chance of succeeding.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::Internal
                | ErrorCode::RateLimited
                | ErrorCode::BackendFailure
                | ErrorCode::LogIoError
        )
    }

    /// Recommended retry delay in milliseconds, or `None` if the client should
    /// not retry.
    pub fn retry_after_ms(self) -> Option<u64> {
        match self {
            ErrorCode::RateLimited => Some(1_000),
            ErrorCode::BackendFailure | ErrorCode::Internal => Some(250),
            ErrorCode::LogIoError => Some(100),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Internal => "internal",
            ErrorCode::BadRequest => "bad_request",
            ErrorCode::Unsupported => "unsupported",
            ErrorCode::NoSession => "no_session",
            ErrorCode::SessionNotFound => "session_not_found",
            ErrorCode::AuthRequired => "auth_required",
            ErrorCode::AuthInvalid => "auth_invalid",
            ErrorCode::VersionMismatch => "version_mismatch",
            ErrorCode::SafetyRejected => "safety_rejected",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::EmergencyStopped => "emergency_stopped",
            ErrorCode::ElementNotFound => "element_not_found",
            ErrorCode::BackendFailure => "backend_failure",
            ErrorCode::PermissionDenied => "permission_denied",
            ErrorCode::Idempotent => "idempotent",
            ErrorCode::LogIoError => "log_io_error",
            ErrorCode::ReplayUnavailable => "replay_unavailable",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Semantic version parts that the daemon advertises in `hello`.
///
/// Clients should refuse to talk to a daemon whose `major` differs from theirs.
/// `minor` bumps are additive (new optional fields, new message kinds the
/// client can ignore). `patch` bumps are bug-fix only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ProtocolVersion {
    pub const CURRENT: Self = Self { major: 0, minor: 1, patch: 0 };

    pub fn compatible_with(self, other: Self) -> bool {
        // For 0.x, treat minor as breaking too — semver convention.
        if self.major != other.major {
            return false;
        }
        if self.major == 0 {
            return self.minor == other.minor;
        }
        true
    }
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

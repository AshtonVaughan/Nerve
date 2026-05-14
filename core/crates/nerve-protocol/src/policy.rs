//! Safety and policy types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyPolicy {
    /// If true, the daemon executes nothing but still produces ActionResults
    /// with `method: NoOp` and `safety_decision: DryRun`.
    pub dry_run: bool,

    /// If true, the daemon waits for a human to acknowledge each action via
    /// `confirm_action` before executing it.
    pub require_confirmation: bool,

    /// If true, all execute_action calls are blocked until the human releases
    /// control.
    pub human_takeover: bool,

    /// If non-empty, only actions touching one of these apps proceed.
    pub app_allowlist: Vec<String>,

    /// Actions that would touch any of these apps are rejected.
    pub app_blocklist: Vec<String>,

    /// Rolling rate limit. 0 disables the limit.
    pub max_actions_per_minute: u32,

    /// Absolute session timeout in seconds. 0 disables the limit.
    pub max_session_seconds: u64,

    /// Patterns to redact from logs and from text that gets typed back to the
    /// agent (e.g. seed phrases, API keys).
    pub redact_patterns: Vec<String>,

    /// If true, `type_text` actions whose target field looks like a password
    /// input are rejected unless explicitly confirmed.
    pub block_password_fields: bool,

    /// If true, payment-shaped fields trigger confirmation.
    pub confirm_payment_fields: bool,
}

impl Default for SafetyPolicy {
    fn default() -> Self {
        Self {
            dry_run: false,
            require_confirmation: false,
            human_takeover: false,
            app_allowlist: Vec::new(),
            app_blocklist: Vec::new(),
            max_actions_per_minute: 600,
            max_session_seconds: 0,
            redact_patterns: Vec::new(),
            block_password_fields: true,
            confirm_payment_fields: true,
        }
    }
}

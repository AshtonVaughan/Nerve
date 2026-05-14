//! WebSocket message envelopes.
//!
//! Every message in either direction is a JSON object with a `kind`
//! discriminator plus a payload. Requests carry an `id` so async responses can
//! be correlated.

use serde::{Deserialize, Serialize};

use crate::action::{ActionEnvelope, ActionResult, AuditEntry};
use crate::observation::{Capabilities, Observation};
use crate::policy::SafetyPolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClientMessage {
    SessionStart {
        request_id: String,
        client_name: Option<String>,
        client_version: Option<String>,
        /// Optional explicit session id to resume an existing audit log.
        session_id: Option<String>,
        policy: Option<SafetyPolicy>,
    },
    SessionStop { request_id: String },
    GetCapabilities { request_id: String },
    GetObservation {
        request_id: String,
        include_screenshot: Option<bool>,
        include_ui_tree: Option<bool>,
    },
    SubscribeObservations {
        request_id: String,
        interval_ms: u64,
        include_screenshot: Option<bool>,
    },
    UnsubscribeObservations { request_id: String },
    ExecuteAction {
        request_id: String,
        action: ActionEnvelope,
    },
    ExecuteActionBatch {
        request_id: String,
        actions: Vec<ActionEnvelope>,
        stop_on_error: bool,
    },
    GetActionLog {
        request_id: String,
        session_id: Option<String>,
        limit: Option<usize>,
    },
    ReplaySession {
        request_id: String,
        session_id: String,
        speed: Option<f32>,
    },
    SetSafetyPolicy {
        request_id: String,
        policy: SafetyPolicy,
    },
    EmergencyStop { request_id: String },
    ConfirmAction {
        request_id: String,
        action_id: String,
        allow: bool,
    },
    /// Heartbeat. The daemon echoes back `Pong` with the same nonce.
    Ping { request_id: String, nonce: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerMessage {
    Hello {
        protocol_version: String,
        daemon_version: String,
        platform: crate::observation::Platform,
        session_id: String,
    },
    SessionStarted {
        request_id: String,
        session_id: String,
        capabilities: Capabilities,
    },
    SessionStopped { request_id: String, session_id: String },
    Capabilities { request_id: String, capabilities: Capabilities },
    Observation {
        request_id: Option<String>,
        observation: Observation,
    },
    ActionResult {
        request_id: String,
        result: ActionResult,
    },
    BatchResult {
        request_id: String,
        results: Vec<ActionResult>,
    },
    ActionLog {
        request_id: String,
        entries: Vec<AuditEntry>,
    },
    PolicyUpdated {
        request_id: String,
        policy: SafetyPolicy,
    },
    EmergencyStopped { request_id: Option<String> },
    ConfirmationRequired {
        action_id: String,
        action: ActionEnvelope,
        reason: String,
    },
    ReplayProgress {
        request_id: String,
        step: usize,
        total: usize,
        entry: AuditEntry,
    },
    ReplayComplete { request_id: String, session_id: String },
    Pong { request_id: String, nonce: u64 },
    Error {
        request_id: Option<String>,
        code: String,
        message: String,
    },
}

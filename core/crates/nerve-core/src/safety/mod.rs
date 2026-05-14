//! Safety policy engine.
//!
//! The engine is consulted on every action and produces a [`SafetyDecision`].
//! Decisions are deliberately blunt — the daemon either allows, blocks,
//! confirms, dry-runs, or hard-stops — because subtle policy is brittle in a
//! shared control surface.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tracing::warn;

use nerve_protocol::{
    ActionEnvelope, AnyAction, LowLevelAction, SafetyDecision, SafetyPolicy, SemanticAction,
};

pub mod redact;

pub use redact::Redactor;

pub struct SafetyEngine {
    policy: RwLock<SafetyPolicy>,
    emergency_stop: RwLock<bool>,
    /// Recent action timestamps for rate limiting.
    recent: RwLock<VecDeque<Instant>>,
    /// Compiled redaction patterns.
    redactor: RwLock<Redactor>,
    /// Pending confirmations, by action id.
    pending: dashmap::DashMap<String, ConfirmationState>,
}

#[derive(Debug)]
pub struct ConfirmationState {
    /// `Some(true)` = approved, `Some(false)` = denied, `None` = waiting.
    pub decision: Option<bool>,
}

impl SafetyEngine {
    pub fn new(policy: SafetyPolicy) -> Arc<Self> {
        let redactor = Redactor::compile(&policy.redact_patterns);
        Arc::new(Self {
            policy: RwLock::new(policy),
            emergency_stop: RwLock::new(false),
            recent: RwLock::new(VecDeque::new()),
            redactor: RwLock::new(redactor),
            pending: dashmap::DashMap::new(),
        })
    }

    pub fn policy(&self) -> SafetyPolicy { self.policy.read().clone() }

    pub fn set_policy(&self, policy: SafetyPolicy) {
        let redactor = Redactor::compile(&policy.redact_patterns);
        *self.redactor.write() = redactor;
        *self.policy.write() = policy;
    }

    pub fn redactor(&self) -> Redactor { self.redactor.read().clone() }

    pub fn engage_emergency_stop(&self) {
        *self.emergency_stop.write() = true;
        warn!("emergency stop engaged");
    }

    pub fn release_emergency_stop(&self) {
        *self.emergency_stop.write() = false;
    }

    pub fn is_emergency_stopped(&self) -> bool { *self.emergency_stop.read() }

    /// Pre-execution check. Returns the safety decision; if the decision is
    /// `Confirmed`, the caller must wait on [`Self::await_confirmation`].
    pub fn evaluate(
        &self,
        env: &ActionEnvelope,
        active_app: Option<&str>,
    ) -> SafetyDecision {
        if self.is_emergency_stopped() {
            return SafetyDecision::EmergencyStopped;
        }
        let policy = self.policy.read().clone();

        if policy.human_takeover {
            // Get-observation / screenshot actions are always safe to run
            // because they only read state, but mutating actions are blocked.
            if !is_read_only(&env.action) {
                return SafetyDecision::Blocked;
            }
        }

        // App allow / block list checks. We trust whatever app name was
        // reported by the platform backend.
        if let Some(app) = active_app {
            if !policy.app_allowlist.is_empty() && !policy.app_allowlist.iter().any(|a| a.eq_ignore_ascii_case(app)) {
                return SafetyDecision::Blocked;
            }
            if policy.app_blocklist.iter().any(|a| a.eq_ignore_ascii_case(app)) {
                return SafetyDecision::Blocked;
            }
        }

        // Rate limit.
        if policy.max_actions_per_minute > 0 {
            let mut recent = self.recent.write();
            let now = Instant::now();
            while let Some(front) = recent.front() {
                if now.duration_since(*front) > Duration::from_secs(60) {
                    recent.pop_front();
                } else {
                    break;
                }
            }
            if recent.len() as u32 >= policy.max_actions_per_minute {
                return SafetyDecision::RateLimited;
            }
            recent.push_back(now);
        }

        if policy.dry_run {
            return SafetyDecision::DryRun;
        }

        // Confirmation step.
        if policy.require_confirmation && !is_read_only(&env.action) {
            self.pending.insert(env.id.clone(), ConfirmationState { decision: None });
            return SafetyDecision::Confirmed;
        }

        SafetyDecision::Allowed
    }

    pub fn register_confirmation(&self, action_id: &str, allow: bool) {
        if let Some(mut entry) = self.pending.get_mut(action_id) {
            entry.decision = Some(allow);
        }
    }

    /// Block until a `confirm_action` arrives. Returns true if approved.
    pub async fn await_confirmation(&self, action_id: &str, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if let Some(entry) = self.pending.get(action_id) {
                if let Some(d) = entry.decision { return d; }
            } else {
                return false;
            }
            if start.elapsed() >= timeout {
                self.pending.remove(action_id);
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    pub fn clear_confirmation(&self, action_id: &str) {
        self.pending.remove(action_id);
    }
}

fn is_read_only(action: &AnyAction) -> bool {
    match action {
        AnyAction::Low(low) => matches!(
            low,
            LowLevelAction::GetObservation { .. }
                | LowLevelAction::Screenshot
                | LowLevelAction::ClipboardGet
                | LowLevelAction::Wait { .. }
                | LowLevelAction::EmergencyStop
        ),
        AnyAction::Semantic(s) => matches!(
            s,
            SemanticAction::FindTextOnScreen { .. }
                | SemanticAction::VerifyTextPresent { .. }
                | SemanticAction::VerifyWindowActive { .. }
                | SemanticAction::WaitForText { .. }
                | SemanticAction::WaitForWindow { .. }
        ),
    }
}


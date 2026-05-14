//! Action execution.
//!
//! The executor:
//!
//! 1. takes an [`ActionEnvelope`],
//! 2. consults the safety engine to get a [`SafetyDecision`],
//! 3. if the action is semantic, runs the [`crate::compiler`] to lower it,
//! 4. dispatches the resulting low-level primitive against the platform
//!    backend,
//! 5. snapshots active window and screenshot hashes before and after,
//! 6. writes an [`AuditEntry`] to the log,
//! 7. returns an [`ActionResult`] to the caller.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tracing::{info, warn};

use nerve_protocol::{
    ActionEnvelope, ActionResult, AnyAction, AuditEntry, ExecutionMethod, LowLevelAction,
    SafetyDecision,
};

use crate::audit::AuditLog;
use crate::compiler::Compiler;
use crate::errors::{NerveError, Result};
use crate::observation::sha256_hex;
use crate::platform::PlatformBackend;
use crate::session::Session;

pub struct Executor {
    backend: Arc<dyn PlatformBackend>,
    audit: Arc<AuditLog>,
    compiler: Compiler,
}

impl Executor {
    pub fn new(backend: Arc<dyn PlatformBackend>, audit: Arc<AuditLog>) -> Self {
        let compiler = Compiler::new(backend.clone());
        Self { backend, audit, compiler }
    }

    pub fn backend(&self) -> Arc<dyn PlatformBackend> { self.backend.clone() }

    pub async fn execute(
        &self,
        session: &Arc<Session>,
        env: ActionEnvelope,
    ) -> Result<ActionResult> {
        let active_app_before = with_timeout(self.backend.active_window(), 1500)
            .await
            .ok()
            .flatten()
            .map(|w| w.app_name);
        let safety_decision = session.safety.evaluate(&env, active_app_before.as_deref());

        // Cheap screenshot hashes for audit. Skip them for read-only actions
        // (no state to compare against) and for short-circuit safety decisions
        // (dry-run / blocked / rate-limited / e-stop) so we don't pay the
        // capture cost just to record "we did nothing".
        let mut screenshot_before: Option<String> = None;
        let mut screenshot_after: Option<String> = None;
        let want_hashes = !is_read_only(&env.action)
            && matches!(
                safety_decision,
                SafetyDecision::Allowed | SafetyDecision::Confirmed
            );
        if want_hashes {
            if let Ok(cap) = self.backend.capture_primary_screen().await {
                screenshot_before = Some(sha256_hex(&cap.png_bytes));
            }
        }

        let result = match safety_decision {
            SafetyDecision::EmergencyStopped => Err(NerveError::EmergencyStopped),
            SafetyDecision::Blocked => Err(NerveError::SafetyRejected(
                "blocked by allow/blocklist or human takeover".into(),
            )),
            SafetyDecision::RateLimited => Err(NerveError::RateLimited {
                allowed: session.safety.policy().max_actions_per_minute,
            }),
            SafetyDecision::DryRun => Ok(self.dry_run_result(&env).await),
            SafetyDecision::Confirmed => {
                // Wait up to 30s for human confirmation.
                let approved = session
                    .safety
                    .await_confirmation(&env.id, Duration::from_secs(30))
                    .await;
                session.safety.clear_confirmation(&env.id);
                if approved {
                    self.execute_inner(&env).await
                } else {
                    Err(NerveError::SafetyRejected("human did not approve action".into()))
                }
            }
            SafetyDecision::Allowed => self.execute_inner(&env).await,
        };

        if want_hashes {
            if let Ok(cap) = self.backend.capture_primary_screen().await {
                screenshot_after = Some(sha256_hex(&cap.png_bytes));
            }
        }

        let active_app_after = with_timeout(self.backend.active_window(), 1500)
            .await
            .ok()
            .flatten()
            .map(|w| w.app_name);

        let cursor = with_timeout(self.backend.cursor_position(), 1500).await.ok();
        let mut action_result = match result {
            Ok(mut r) => {
                r.cursor = cursor;
                r.active_window = active_app_after.clone();
                r.screenshot_before = screenshot_before.clone();
                r.screenshot_after = screenshot_after.clone();
                r
            }
            Err(e) => ActionResult {
                id: env.id.clone(),
                ok: false,
                timestamp: Utc::now(),
                method: match e {
                    NerveError::EmergencyStopped
                    | NerveError::SafetyRejected(_)
                    | NerveError::RateLimited { .. } => ExecutionMethod::NoOp,
                    _ => ExecutionMethod::NoOp,
                },
                cursor,
                active_window: active_app_after.clone(),
                error: Some(e.to_string()),
                data: None,
                screenshot_before: screenshot_before.clone(),
                screenshot_after: screenshot_after.clone(),
                compiled: None,
            },
        };

        // Stash the last action name on the session for observation streaming.
        *session.meta.last_action.write() = Some(describe_action(&env.action));

        // Redact note before persisting.
        let note = env
            .note
            .as_ref()
            .map(|n| session.safety.redactor().redact(n));

        let entry = AuditEntry {
            session_id: session.meta.id.clone(),
            action_id: env.id.clone(),
            timestamp: action_result.timestamp,
            action: env.action.clone(),
            result: action_result.clone(),
            active_window_before: active_app_before,
            active_window_after: active_app_after,
            safety_decision,
            note,
        };
        if let Err(e) = self.audit.append(&entry) {
            warn!("failed to append audit entry: {e}");
        }

        // Make sure the daemon-side cursor field on the result is correct.
        // (cursor was already populated above; nothing more to do.)
        let _ = &mut action_result;
        Ok(action_result)
    }

    async fn dry_run_result(&self, env: &ActionEnvelope) -> ActionResult {
        ActionResult {
            id: env.id.clone(),
            ok: true,
            timestamp: Utc::now(),
            method: ExecutionMethod::NoOp,
            cursor: None,
            active_window: None,
            error: None,
            data: Some(serde_json::json!({"dry_run": true, "action": env.action})),
            screenshot_before: None,
            screenshot_after: None,
            compiled: None,
        }
    }

    async fn execute_inner(&self, env: &ActionEnvelope) -> Result<ActionResult> {
        match &env.action {
            AnyAction::Low(low) => self.execute_low(&env.id, low).await,
            AnyAction::Semantic(sem) => {
                let plan = self.compiler.compile(sem).await?;
                info!(
                    action_id = %env.id,
                    method = ?plan.method,
                    "semantic action compiled"
                );
                let primitive = plan
                    .primitive
                    .clone()
                    .ok_or_else(|| NerveError::ElementNotFound)?;
                let mut result = self.execute_low(&env.id, &primitive).await?;
                // Override the reported method with the compiler's choice so
                // we don't claim "coordinate_click" when AX did the work.
                result.method = plan.method;
                result.compiled = Some(plan);
                Ok(result)
            }
        }
    }

    async fn execute_low(&self, id: &str, action: &LowLevelAction) -> Result<ActionResult> {
        let started = Utc::now();
        let (method, data) = match action {
            LowLevelAction::GetObservation { .. } | LowLevelAction::Screenshot => {
                (ExecutionMethod::Capture, None)
            }
            LowLevelAction::MoveMouse { x, y } => {
                self.backend.move_mouse(*x, *y).await?;
                (ExecutionMethod::CoordinateClick, None)
            }
            LowLevelAction::Click { x, y, button } => {
                self.backend.click(*x, *y, *button).await?;
                (ExecutionMethod::CoordinateClick, None)
            }
            LowLevelAction::DoubleClick { x, y } => {
                self.backend.double_click(*x, *y).await?;
                (ExecutionMethod::CoordinateClick, None)
            }
            LowLevelAction::RightClick { x, y } => {
                self.backend
                    .click(*x, *y, nerve_protocol::MouseButton::Right)
                    .await?;
                (ExecutionMethod::CoordinateClick, None)
            }
            LowLevelAction::Drag { from_x, from_y, to_x, to_y, button } => {
                self.backend
                    .drag((*from_x, *from_y), (*to_x, *to_y), *button)
                    .await?;
                (ExecutionMethod::CoordinateClick, None)
            }
            LowLevelAction::Scroll { x, y, delta_x, delta_y } => {
                self.backend.scroll(*x, *y, *delta_x, *delta_y).await?;
                (ExecutionMethod::CoordinateClick, None)
            }
            LowLevelAction::TypeText { text, delay_ms } => {
                self.backend.type_text(text, *delay_ms).await?;
                (ExecutionMethod::Keyboard, None)
            }
            LowLevelAction::KeyPress { key } => {
                self.backend.key_press(key).await?;
                (ExecutionMethod::Keyboard, None)
            }
            LowLevelAction::Hotkey { keys } => {
                self.backend.hotkey(keys).await?;
                (ExecutionMethod::Keyboard, None)
            }
            LowLevelAction::ClipboardGet => {
                let text = self.backend.clipboard_get().await?;
                (ExecutionMethod::Clipboard, Some(serde_json::json!({"text": text})))
            }
            LowLevelAction::ClipboardSet { text } => {
                self.backend.clipboard_set(text).await?;
                (ExecutionMethod::Clipboard, None)
            }
            LowLevelAction::Wait { ms } => {
                tokio::time::sleep(Duration::from_millis(*ms)).await;
                (ExecutionMethod::Wait, None)
            }
            LowLevelAction::EmergencyStop => {
                // The runtime escalates this separately. Here we just record
                // it as a no-op for completeness.
                (ExecutionMethod::NoOp, None)
            }
        };
        Ok(ActionResult {
            id: id.to_string(),
            ok: true,
            timestamp: started,
            method,
            cursor: None,
            active_window: None,
            error: None,
            data,
            screenshot_before: None,
            screenshot_after: None,
            compiled: None,
        })
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
        AnyAction::Semantic(_) => false,
    }
}

/// Run a backend future with a hard timeout. Returns `Err` on timeout so the
/// caller can fall back instead of stalling the runtime.
async fn with_timeout<F, T>(fut: F, ms: u64) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    match tokio::time::timeout(std::time::Duration::from_millis(ms), fut).await {
        Ok(v) => v,
        Err(_) => Err(NerveError::Backend(format!("platform call timed out after {ms}ms"))),
    }
}

fn describe_action(action: &AnyAction) -> String {
    match action {
        AnyAction::Low(LowLevelAction::Click { x, y, .. }) => format!("click({x},{y})"),
        AnyAction::Low(LowLevelAction::TypeText { text, .. }) => {
            let preview: String = text.chars().take(32).collect();
            format!("type({})", preview)
        }
        AnyAction::Low(other) => format!("{:?}", std::mem::discriminant(other)),
        AnyAction::Semantic(s) => format!("{:?}", std::mem::discriminant(s)),
    }
}

//! Semantic action compiler.
//!
//! Turns a [`SemanticAction`] like "click the Save button in TextEdit" into a
//! concrete [`LowLevelAction`]. The compiler walks a priority ladder:
//!
//! 1. Accessibility action (preferred — direct invocation, no pixels involved).
//! 2. Native OS UI action (e.g. press the menu shortcut directly).
//! 3. Browser DOM adapter (reserved, no-op for MVP).
//! 4. OCR / element bounding-box click.
//! 5. Coordinate click fallback if a `bounds` hint was supplied.
//!
//! Strategies that the platform backend doesn't support (e.g. AX on Linux
//! Wayland) are silently skipped, and the compiler logs the attempted ladder
//! in the returned [`CompiledPlan`] so the audit log makes the decision
//! reproducible.

use std::sync::Arc;

use tracing::debug;

use nerve_protocol::{
    Bounds, CompiledPlan, ExecutionMethod, LowLevelAction, MouseButton, SemanticAction, UiNode,
};

use crate::errors::{NerveError, Result};
use crate::platform::PlatformBackend;

pub struct Compiler {
    backend: Arc<dyn PlatformBackend>,
}

impl Compiler {
    pub fn new(backend: Arc<dyn PlatformBackend>) -> Self {
        Self { backend }
    }

    pub async fn compile(&self, action: &SemanticAction) -> Result<CompiledPlan> {
        let mut trace = Vec::new();
        let mut attempted = Vec::new();

        match action {
            SemanticAction::ClickElement { target } => {
                self.compile_click(
                    target.text.as_deref(),
                    target.role.as_deref(),
                    target.app.as_deref(),
                    target.bounds.clone(),
                    target.index.unwrap_or(0),
                    &mut trace,
                    &mut attempted,
                )
                .await
            }
            SemanticAction::ClickElementByText { text, app } => {
                self.compile_click(
                    Some(text),
                    None,
                    app.as_deref(),
                    None,
                    0,
                    &mut trace,
                    &mut attempted,
                )
                .await
            }
            SemanticAction::ClickElementByRole { role, app } => {
                self.compile_click(
                    None,
                    Some(role),
                    app.as_deref(),
                    None,
                    0,
                    &mut trace,
                    &mut attempted,
                )
                .await
            }
            SemanticAction::PressButtonNamed { name, app } => {
                self.compile_click(
                    Some(name),
                    Some("button"),
                    app.as_deref(),
                    None,
                    0,
                    &mut trace,
                    &mut attempted,
                )
                .await
            }
            SemanticAction::FocusWindow { title, app } => {
                self.compile_focus_window(title.as_deref(), app.as_deref(), &mut trace, &mut attempted).await
            }
            SemanticAction::SelectMenuItem { path, app } => {
                trace.push(format!(
                    "select_menu_item attempted via keyboard fallback path={:?} app={:?}",
                    path, app
                ));
                attempted.push(ExecutionMethod::AccessibilityAction);
                // Fallback: simulate clicking by name on the first path entry
                // and then arrow-keying through the rest. Real macOS / Windows
                // implementations should use AXMenuItem / UIA InvokePattern.
                let name = path.last().cloned().unwrap_or_default();
                self.compile_click(
                    Some(&name),
                    Some("menuitem"),
                    app.as_deref(),
                    None,
                    0,
                    &mut trace,
                    &mut attempted,
                )
                .await
            }
            SemanticAction::TypeIntoFocusedElement { text } => {
                // If the text contains code points outside the ASCII range, use
                // the clipboard-paste path so we don't depend on the user's
                // keyboard layout.
                let needs_unicode = text.chars().any(|c| !c.is_ascii());
                Ok(CompiledPlan {
                    method: if needs_unicode {
                        ExecutionMethod::Clipboard
                    } else {
                        ExecutionMethod::Keyboard
                    },
                    primitive: Some(LowLevelAction::TypeText {
                        text: text.clone(),
                        delay_ms: None,
                        unicode_paste: needs_unicode,
                    }),
                    attempted: vec![ExecutionMethod::AccessibilityAction, ExecutionMethod::Keyboard],
                    trace: vec![format!(
                        "type into focused element, {} chars, unicode_paste={}",
                        text.len(),
                        needs_unicode
                    )],
                })
            }
            SemanticAction::FindTextOnScreen { text } => Ok(CompiledPlan {
                method: ExecutionMethod::OcrBoundingBox,
                primitive: Some(LowLevelAction::Screenshot),
                attempted: vec![ExecutionMethod::OcrBoundingBox],
                trace: vec![format!("OCR scan for '{}': MVP returns a screenshot for the caller to OCR", text)],
            }),
            SemanticAction::VerifyTextPresent { text, .. } => Ok(CompiledPlan {
                method: ExecutionMethod::OcrBoundingBox,
                primitive: Some(LowLevelAction::Screenshot),
                attempted: vec![ExecutionMethod::AccessibilityAction, ExecutionMethod::OcrBoundingBox],
                trace: vec![format!("verify_text_present('{}') -> screenshot for caller-side check", text)],
            }),
            SemanticAction::VerifyWindowActive { app, title } => Ok(CompiledPlan {
                method: ExecutionMethod::NoOp,
                primitive: Some(LowLevelAction::GetObservation { include_screenshot: Some(false) }),
                attempted: vec![ExecutionMethod::AccessibilityAction],
                trace: vec![format!("verify_window_active app={:?} title={:?}", app, title)],
            }),
            SemanticAction::WaitForText { text, timeout_ms } => Ok(CompiledPlan {
                method: ExecutionMethod::Wait,
                primitive: Some(LowLevelAction::Wait { ms: *timeout_ms }),
                attempted: vec![ExecutionMethod::Wait],
                trace: vec![format!("wait_for_text('{}', {}ms)", text, timeout_ms)],
            }),
            SemanticAction::WaitForWindow { timeout_ms, app, title } => Ok(CompiledPlan {
                method: ExecutionMethod::Wait,
                primitive: Some(LowLevelAction::Wait { ms: *timeout_ms }),
                attempted: vec![ExecutionMethod::Wait],
                trace: vec![format!("wait_for_window app={:?} title={:?}", app, title)],
            }),
            SemanticAction::CloseWindow { app, title } => {
                trace.push(format!("close_window app={:?} title={:?} -> Cmd/Ctrl+W", app, title));
                let keys = if cfg!(target_os = "macos") {
                    vec!["meta".to_string(), "w".to_string()]
                } else {
                    vec!["ctrl".to_string(), "w".to_string()]
                };
                Ok(CompiledPlan {
                    method: ExecutionMethod::Keyboard,
                    primitive: Some(LowLevelAction::Hotkey { keys }),
                    attempted: vec![ExecutionMethod::AccessibilityAction, ExecutionMethod::Keyboard],
                    trace,
                })
            }
            SemanticAction::OpenApp { name } => {
                trace.push(format!("open_app({}) via platform launcher", name));
                self.backend.open_app(name).await?;
                Ok(CompiledPlan {
                    method: ExecutionMethod::NativeUiAction,
                    primitive: Some(LowLevelAction::Wait { ms: 800 }),
                    attempted: vec![ExecutionMethod::NativeUiAction],
                    trace,
                })
            }
        }
    }

    async fn compile_click(
        &self,
        text: Option<&str>,
        role: Option<&str>,
        app: Option<&str>,
        bounds_hint: Option<Bounds>,
        index: usize,
        trace: &mut Vec<String>,
        attempted: &mut Vec<ExecutionMethod>,
    ) -> Result<CompiledPlan> {
        // 1. Accessibility tree search.
        attempted.push(ExecutionMethod::AccessibilityAction);
        let ui_tree = self.backend.ui_tree().await.unwrap_or_default();
        if !ui_tree.is_empty() {
            if let Some(bounds) = find_in_tree(&ui_tree, text, role, index) {
                let (cx, cy) = center(&bounds);
                trace.push(format!(
                    "accessibility match text={:?} role={:?} bounds={:?}",
                    text, role, bounds
                ));
                return Ok(CompiledPlan {
                    method: ExecutionMethod::AccessibilityAction,
                    primitive: Some(LowLevelAction::Click {
                        x: cx,
                        y: cy,
                        button: MouseButton::Left,
                    }),
                    attempted: attempted.clone(),
                    trace: trace.clone(),
                });
            }
            trace.push("accessibility tree present but no match".into());
        } else {
            trace.push("accessibility tree unavailable".into());
        }

        // 2. Native UI action — placeholder. Future: app-specific hooks
        //    (TextEdit menu items, browser DOM via WebDriver, etc).
        attempted.push(ExecutionMethod::NativeUiAction);
        debug!(?app, "native UI action layer is a no-op in MVP");

        // 3. Browser DOM adapter — placeholder.
        attempted.push(ExecutionMethod::BrowserDomAdapter);

        // 4. OCR / bounds hint.
        if let Some(b) = bounds_hint {
            attempted.push(ExecutionMethod::OcrBoundingBox);
            let (cx, cy) = center(&b);
            trace.push(format!("using caller-supplied bounds hint {:?}", b));
            return Ok(CompiledPlan {
                method: ExecutionMethod::OcrBoundingBox,
                primitive: Some(LowLevelAction::Click {
                    x: cx,
                    y: cy,
                    button: MouseButton::Left,
                }),
                attempted: attempted.clone(),
                trace: trace.clone(),
            });
        }

        // 5. Coordinate fallback. Without bounds we cannot click sensibly.
        attempted.push(ExecutionMethod::CoordinateClick);
        Err(NerveError::ElementNotFound)
    }

    async fn compile_focus_window(
        &self,
        title: Option<&str>,
        app: Option<&str>,
        trace: &mut Vec<String>,
        attempted: &mut Vec<ExecutionMethod>,
    ) -> Result<CompiledPlan> {
        attempted.push(ExecutionMethod::AccessibilityAction);
        trace.push(format!("focus_window app={:?} title={:?}", app, title));
        // The MVP falls back to Alt-Tab style window cycling on Windows/Linux,
        // and Cmd-Tab on macOS. A real implementation should call
        // SetForegroundWindow / AXWindow / wmctrl.
        let keys = if cfg!(target_os = "macos") {
            vec!["meta".to_string(), "tab".to_string()]
        } else {
            vec!["alt".to_string(), "tab".to_string()]
        };
        Ok(CompiledPlan {
            method: ExecutionMethod::Keyboard,
            primitive: Some(LowLevelAction::Hotkey { keys }),
            attempted: attempted.clone(),
            trace: trace.clone(),
        })
    }
}

fn find_in_tree<'a>(
    nodes: &'a [UiNode],
    text: Option<&str>,
    role: Option<&str>,
    mut index: usize,
) -> Option<Bounds> {
    fn walk<'b>(
        nodes: &'b [UiNode],
        text: Option<&str>,
        role: Option<&str>,
        index: &mut usize,
    ) -> Option<Bounds> {
        for n in nodes {
            let role_match = role.map(|r| n.role.eq_ignore_ascii_case(r)).unwrap_or(true);
            let text_match = text
                .map(|t| {
                    n.label.as_deref().map(|l| l.eq_ignore_ascii_case(t)).unwrap_or(false)
                        || n.value.as_deref().map(|v| v.eq_ignore_ascii_case(t)).unwrap_or(false)
                })
                .unwrap_or(true);
            if role_match && text_match {
                if *index == 0 {
                    return n.bounds.clone();
                }
                *index -= 1;
            }
            if let Some(b) = walk(&n.children, text, role, index) {
                return Some(b);
            }
        }
        None
    }
    walk(nodes, text, role, &mut index)
}

fn center(b: &Bounds) -> (i32, i32) {
    (b.x + b.width / 2, b.y + b.height / 2)
}

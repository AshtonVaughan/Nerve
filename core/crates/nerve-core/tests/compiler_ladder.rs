//! End-to-end test of the action compiler's lowering ladder.
//!
//! We plug a hand-rolled fake backend into the `Compiler` and verify that
//! each rung (AX → caller bounds → OCR → ElementNotFound) fires the way the
//! audit trace says it does. This catches regressions where someone
//! reorders the rungs or short-circuits coordinate fallback.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use nerve_core::compiler::Compiler;
use nerve_core::errors::Result;
use nerve_core::platform::{CapturedScreen, PlatformBackend};
use nerve_protocol::{
    ActiveWindow, Backends, Bounds, Capabilities, CursorPosition, ElementTarget, ExecutionMethod,
    LowLevelAction, Monitor, MouseButton, Platform, SemanticAction, UiNode,
};

/// Configurable fake backend. Each method returns what was injected via the
/// `FakeBackend::new(...)` builder or the per-test setters.
#[derive(Default)]
struct FakeState {
    ui_tree: Vec<UiNode>,
    screen_png: Vec<u8>,
}

struct FakeBackend {
    state: Mutex<FakeState>,
}

impl FakeBackend {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(FakeState::default()),
        })
    }

    fn set_ui_tree(&self, tree: Vec<UiNode>) {
        self.state.lock().unwrap().ui_tree = tree;
    }
}

#[async_trait]
impl PlatformBackend for FakeBackend {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn platform(&self) -> Platform {
        Platform::Linux
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            platform: Platform::Linux,
            screen_capture: false,
            input_control: false,
            accessibility_tree: true,
            clipboard: false,
            semantic_actions: true,
            ocr: false,
            wayland_limited: false,
            missing_permissions: vec![],
            backends: self.backends(),
            version: "test".into(),
        }
    }
    fn backends(&self) -> Backends {
        Backends {
            screen_capture: "fake".into(),
            input: "fake".into(),
            accessibility: "fake".into(),
            clipboard: "fake".into(),
        }
    }
    async fn capture_primary_screen(&self) -> Result<CapturedScreen> {
        Ok(CapturedScreen {
            width: 800,
            height: 600,
            scale_factor: 1.0,
            png_bytes: self.state.lock().unwrap().screen_png.clone(),
        })
    }
    async fn monitors(&self) -> Result<Vec<Monitor>> {
        Ok(Vec::new())
    }
    async fn cursor_position(&self) -> Result<CursorPosition> {
        Ok(CursorPosition { x: 0, y: 0 })
    }
    async fn active_window(&self) -> Result<Option<ActiveWindow>> {
        Ok(None)
    }
    async fn ui_tree(&self) -> Result<Vec<UiNode>> {
        Ok(self.state.lock().unwrap().ui_tree.clone())
    }
    async fn move_mouse(&self, _: i32, _: i32) -> Result<()> {
        Ok(())
    }
    async fn click(&self, _: i32, _: i32, _: MouseButton) -> Result<()> {
        Ok(())
    }
    async fn double_click(&self, _: i32, _: i32) -> Result<()> {
        Ok(())
    }
    async fn drag(&self, _: (i32, i32), _: (i32, i32), _: MouseButton) -> Result<()> {
        Ok(())
    }
    async fn scroll(&self, _: i32, _: i32, _: i32, _: i32) -> Result<()> {
        Ok(())
    }
    async fn type_text(&self, _: &str, _: Option<u64>) -> Result<()> {
        Ok(())
    }
    async fn key_press(&self, _: &str) -> Result<()> {
        Ok(())
    }
    async fn hotkey(&self, _: &[String]) -> Result<()> {
        Ok(())
    }
    async fn clipboard_get(&self) -> Result<String> {
        Ok(String::new())
    }
    async fn clipboard_set(&self, _: &str) -> Result<()> {
        Ok(())
    }
    async fn open_app(&self, _: &str) -> Result<()> {
        Ok(())
    }
    async fn missing_permissions(&self) -> Vec<String> {
        vec![]
    }
}

fn target(text: &str) -> ElementTarget {
    ElementTarget {
        text: Some(text.into()),
        role: Some("button".into()),
        app: None,
        bounds: None,
        index: None,
    }
}

fn node(role: &str, label: &str, b: (i32, i32, i32, i32)) -> UiNode {
    UiNode {
        role: role.into(),
        label: Some(label.into()),
        value: None,
        bounds: Some(Bounds {
            x: b.0,
            y: b.1,
            width: b.2,
            height: b.3,
        }),
        enabled: true,
        focused: false,
        children: Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ax_rung_fires_when_tree_has_match() {
    let backend = FakeBackend::new();
    backend.set_ui_tree(vec![node("button", "Save", (10, 20, 60, 30))]);
    let compiler = Compiler::new(backend as Arc<dyn PlatformBackend>);

    let plan = compiler
        .compile(&SemanticAction::ClickElement {
            target: target("Save"),
        })
        .await
        .expect("AX rung");

    assert_eq!(plan.method, ExecutionMethod::AccessibilityAction);
    match plan.primitive {
        Some(LowLevelAction::Click { x, y, .. }) => {
            // Centre of (10, 20, 60, 30) is (40, 35).
            assert_eq!((x, y), (40, 35));
        }
        other => panic!("expected click primitive, got {:?}", other),
    }
    // Trace records which rung resolved the match.
    let trace = plan.trace.join("\n");
    assert!(trace.contains("accessibility match"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounds_hint_rung_fires_when_ax_misses() {
    let backend = FakeBackend::new();
    // Empty UI tree → AX misses.
    let compiler = Compiler::new(backend as Arc<dyn PlatformBackend>);

    let mut tgt = target("Save");
    tgt.bounds = Some(Bounds {
        x: 100,
        y: 100,
        width: 20,
        height: 20,
    });
    let plan = compiler
        .compile(&SemanticAction::ClickElement { target: tgt })
        .await
        .expect("bounds hint rung");
    assert_eq!(plan.method, ExecutionMethod::OcrBoundingBox);
    match plan.primitive {
        Some(LowLevelAction::Click { x, y, .. }) => assert_eq!((x, y), (110, 110)),
        other => panic!("expected click, got {:?}", other),
    }
    assert!(plan.trace.join("\n").contains("caller-supplied bounds hint"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_target_returns_element_not_found_not_silent_coord_click() {
    let backend = FakeBackend::new();
    let compiler = Compiler::new(backend as Arc<dyn PlatformBackend>);
    // No UI tree, no bounds hint, no OCR feature → must error, never fall
    // through to a fabricated coordinate click.
    let err = compiler
        .compile(&SemanticAction::ClickElement {
            target: target("Save"),
        })
        .await
        .expect_err("expected ElementNotFound");
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("not found") || msg.to_lowercase().contains("element"),
        "unexpected error message: {msg}"
    );
}

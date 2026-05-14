/**
 * Type definitions mirroring the Nerve wire protocol (`nerve-protocol`).
 *
 * The Rust crate is the source of truth; these types are kept in sync
 * by hand for the MVP. Once the protocol stabilises, this file should be
 * generated from a shared JSON schema.
 */

export type Platform = "macos" | "windows" | "linux" | "unknown";
export type MouseButton = "left" | "right" | "middle";

export interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface CursorPosition {
  x: number;
  y: number;
}

export interface Screen {
  width: number;
  height: number;
  scale_factor: number;
  screenshot_base64?: string;
  screenshot_format: string;
  screenshot_hash?: string;
}

export interface ActiveWindow {
  title: string;
  app_name: string;
  process_name: string;
  pid?: number;
  bounds: Bounds;
}

export interface UiNode {
  role: string;
  label?: string;
  value?: string;
  bounds?: Bounds;
  enabled: boolean;
  focused: boolean;
  children: UiNode[];
}

export interface SafetyState {
  agent_active: boolean;
  dry_run: boolean;
  human_takeover: boolean;
  emergency_stopped: boolean;
  confirmation_required: boolean;
}

export interface Observation {
  session_id: string;
  timestamp: string;
  platform: Platform;
  screen: Screen;
  cursor: CursorPosition;
  active_window: ActiveWindow | null;
  ui_tree: UiNode[];
  ocr: unknown[];
  focused_element: UiNode | null;
  last_action: string | null;
  visual_diff: unknown;
  safety_state: SafetyState;
}

export interface Capabilities {
  platform: Platform;
  screen_capture: boolean;
  input_control: boolean;
  accessibility_tree: boolean;
  clipboard: boolean;
  semantic_actions: boolean;
  ocr: boolean;
  wayland_limited: boolean;
  missing_permissions: string[];
  backends: {
    screen_capture: string;
    input: string;
    accessibility: string;
    clipboard: string;
  };
  version: string;
}

export interface ElementTarget {
  text?: string;
  role?: string;
  app?: string;
  bounds?: Bounds;
  index?: number;
}

export interface SafetyPolicy {
  dry_run?: boolean;
  require_confirmation?: boolean;
  human_takeover?: boolean;
  app_allowlist?: string[];
  app_blocklist?: string[];
  max_actions_per_minute?: number;
  max_session_seconds?: number;
  redact_patterns?: string[];
  block_password_fields?: boolean;
  confirm_payment_fields?: boolean;
}

export type LowLevelAction =
  | { type: "get_observation"; include_screenshot?: boolean | null }
  | { type: "screenshot" }
  | { type: "move_mouse"; x: number; y: number }
  | { type: "click"; x: number; y: number; button?: MouseButton }
  | { type: "double_click"; x: number; y: number }
  | { type: "right_click"; x: number; y: number }
  | { type: "drag"; from_x: number; from_y: number; to_x: number; to_y: number; button?: MouseButton }
  | { type: "scroll"; x: number; y: number; delta_x: number; delta_y: number }
  | { type: "type_text"; text: string; delay_ms?: number | null }
  | { type: "key_press"; key: string }
  | { type: "hotkey"; keys: string[] }
  | { type: "clipboard_get" }
  | { type: "clipboard_set"; text: string }
  | { type: "wait"; ms: number }
  | { type: "emergency_stop" };

export type SemanticAction =
  | { type: "click_element"; target: ElementTarget }
  | { type: "click_element_by_text"; text: string; app?: string }
  | { type: "click_element_by_role"; role: string; app?: string }
  | { type: "press_button_named"; name: string; app?: string }
  | { type: "focus_window"; title?: string; app?: string }
  | { type: "select_menu_item"; path: string[]; app?: string }
  | { type: "type_into_focused_element"; text: string }
  | { type: "find_text_on_screen"; text: string }
  | { type: "verify_text_present"; text: string; timeout_ms?: number }
  | { type: "verify_window_active"; app?: string; title?: string }
  | { type: "wait_for_text"; text: string; timeout_ms: number }
  | { type: "wait_for_window"; app?: string; title?: string; timeout_ms: number }
  | { type: "close_window"; app?: string; title?: string }
  | { type: "open_app"; name: string };

export type AnyAction = LowLevelAction | SemanticAction;

export interface ActionEnvelope {
  id: string;
  action: AnyAction;
  note?: string;
}

export type ExecutionMethod =
  | "accessibility_action"
  | "native_ui_action"
  | "browser_dom_adapter"
  | "ocr_bounding_box"
  | "coordinate_click"
  | "keyboard"
  | "clipboard"
  | "wait"
  | "capture"
  | "no_op";

export interface CompiledPlan {
  method: ExecutionMethod;
  primitive: LowLevelAction | null;
  attempted: ExecutionMethod[];
  trace: string[];
}

export interface ActionResult {
  id: string;
  ok: boolean;
  timestamp: string;
  method: ExecutionMethod;
  cursor: CursorPosition | null;
  active_window: string | null;
  error: string | null;
  data: Record<string, unknown> | null;
  screenshot_before: string | null;
  screenshot_after: string | null;
  compiled: CompiledPlan | null;
}

export type SafetyDecision =
  | "allowed"
  | "dry_run"
  | "confirmed"
  | "blocked"
  | "rate_limited"
  | "emergency_stopped";

export interface AuditEntry {
  session_id: string;
  action_id: string;
  timestamp: string;
  action: AnyAction;
  result: ActionResult;
  active_window_before: string | null;
  active_window_after: string | null;
  safety_decision: SafetyDecision;
  note: string | null;
}

export type ClientMessage =
  | { kind: "session_start"; request_id: string; client_name?: string; client_version?: string; session_id?: string; policy?: SafetyPolicy }
  | { kind: "session_stop"; request_id: string }
  | { kind: "get_capabilities"; request_id: string }
  | { kind: "get_observation"; request_id: string; include_screenshot?: boolean; include_ui_tree?: boolean }
  | { kind: "subscribe_observations"; request_id: string; interval_ms: number; include_screenshot?: boolean }
  | { kind: "unsubscribe_observations"; request_id: string }
  | { kind: "execute_action"; request_id: string; action: ActionEnvelope }
  | { kind: "execute_action_batch"; request_id: string; actions: ActionEnvelope[]; stop_on_error: boolean }
  | { kind: "get_action_log"; request_id: string; session_id?: string; limit?: number }
  | { kind: "replay_session"; request_id: string; session_id: string; speed?: number }
  | { kind: "set_safety_policy"; request_id: string; policy: SafetyPolicy }
  | { kind: "emergency_stop"; request_id: string }
  | { kind: "confirm_action"; request_id: string; action_id: string; allow: boolean }
  | { kind: "ping"; request_id: string; nonce: number };

export type ServerMessage =
  | { kind: "hello"; protocol_version: string; daemon_version: string; platform: Platform; session_id: string }
  | { kind: "session_started"; request_id: string; session_id: string; capabilities: Capabilities }
  | { kind: "session_stopped"; request_id: string; session_id: string }
  | { kind: "capabilities"; request_id: string; capabilities: Capabilities }
  | { kind: "observation"; request_id: string | null; observation: Observation }
  | { kind: "action_result"; request_id: string; result: ActionResult }
  | { kind: "batch_result"; request_id: string; results: ActionResult[] }
  | { kind: "action_log"; request_id: string; entries: AuditEntry[] }
  | { kind: "policy_updated"; request_id: string; policy: SafetyPolicy }
  | { kind: "emergency_stopped"; request_id: string | null }
  | { kind: "confirmation_required"; action_id: string; action: ActionEnvelope; reason: string }
  | { kind: "replay_progress"; request_id: string; step: number; total: number; entry: AuditEntry }
  | { kind: "replay_complete"; request_id: string; session_id: string }
  | { kind: "pong"; request_id: string; nonce: number }
  | { kind: "error"; request_id: string | null; code: string; message: string };

//! HTTP + WebSocket server.
//!
//! Built on `axum`. The same listener serves:
//!
//! * `GET /` and `/dashboard.*` — the bundled dashboard.
//! * `GET /api/...` — small JSON endpoints used by the dashboard.
//! * `GET /` with an `Upgrade: websocket` header — the agent control plane.
//!
//! All routes are bound to `127.0.0.1` by default; the daemon refuses
//! non-loopback binds unless `loopback_only = false` is set.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use ::metrics::{counter, gauge, histogram};
use tokio::sync::mpsc;
use tracing::{debug, error, info};
use uuid::Uuid;

use nerve_protocol::{ClientMessage, ErrorCode, ProtocolVersion, ServerMessage, PROTOCOL_VERSION};

use crate::observation::{observe, ObserveOpts};
use crate::runtime::{Runtime, RuntimeEvent};
use crate::session::Session;

pub struct WsServer {
    runtime: Runtime,
    sessions: Arc<DashMap<String, Arc<Session>>>,
    subscribers: Arc<DashMap<String, mpsc::Sender<ServerMessage>>>,
    dashboard_state: Arc<DashboardSnapshot>,
}

#[derive(Default)]
pub struct DashboardSnapshot {
    pub last_action: RwLock<Option<String>>,
    pub last_safety_decision: RwLock<Option<String>>,
}

#[derive(Clone)]
struct AppState {
    server: Arc<WsServer>,
}

impl WsServer {
    pub fn new(runtime: Runtime) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            sessions: Arc::new(DashMap::new()),
            subscribers: Arc::new(DashMap::new()),
            dashboard_state: Arc::new(DashboardSnapshot::default()),
        })
    }

    pub async fn serve(self: Arc<Self>) -> Result<()> {
        let bind = self.runtime.config.bind;
        if self.runtime.config.loopback_only && !bind.ip().is_loopback() {
            anyhow::bail!(
                "refusing to bind to non-loopback {bind}: set loopback_only=false to override"
            );
        }
        let state = AppState { server: self.clone() };

        let app = Router::new()
            .route("/", get(root_or_upgrade))
            .route("/dashboard.js", get(dashboard_js))
            .route("/dashboard.css", get(dashboard_css))
            .route("/api/capabilities", get(api_capabilities))
            .route("/api/sessions", get(api_sessions))
            .route("/api/version", get(api_version))
            .route("/healthz", get(healthz))
            .route("/metrics", get(metrics_endpoint))
            .fallback(not_found)
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(bind).await?;
        info!(%bind, "nerve listening");
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .await?;
        Ok(())
    }
}

// --- handlers ---------------------------------------------------------------

async fn root_or_upgrade(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    ws: Option<WebSocketUpgrade>,
) -> Response {
    if let Some(ws) = ws {
        debug!(%addr, "ws upgrade");
        let server = state.server.clone();
        return ws.on_upgrade(move |socket| async move {
            server.handle_socket(socket, addr).await;
        });
    }
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        crate::server::dashboard::INDEX_HTML,
    )
        .into_response()
}

async fn dashboard_js() -> Response {
    (
        [(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
        crate::server::dashboard::APP_JS,
    )
        .into_response()
}

async fn dashboard_css() -> Response {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        crate::server::dashboard::APP_CSS,
    )
        .into_response()
}

async fn api_capabilities(State(state): State<AppState>) -> Response {
    axum::Json(state.server.runtime.capabilities()).into_response()
}

async fn api_sessions(State(state): State<AppState>) -> Response {
    let list = state.server.runtime.audit.list_sessions().unwrap_or_default();
    axum::Json(list).into_response()
}

async fn api_version() -> Response {
    axum::Json(serde_json::json!({
        "protocol": PROTOCOL_VERSION,
        "daemon": crate::DAEMON_VERSION,
    }))
    .into_response()
}

async fn healthz() -> Response {
    (StatusCode::OK, "ok").into_response()
}

async fn metrics_endpoint() -> Response {
    match crate::metrics::render_prometheus() {
        Some(body) => (
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
            body,
        )
            .into_response(),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "metrics not installed (set telemetry.prometheus = true)",
        )
            .into_response(),
    }
}

async fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

// --- WS session loop --------------------------------------------------------

impl WsServer {
    async fn handle_socket(self: Arc<Self>, socket: WebSocket, addr: SocketAddr) {
        info!(%addr, "websocket client connected");
        counter!("nerve_ws_connections_total").increment(1);
        *self.runtime.bus.connected_clients.write() += 1;
        gauge!("nerve_ws_connections_active")
            .set(*self.runtime.bus.connected_clients.read() as f64);
        let _ = self.runtime.bus.events.send(RuntimeEvent::ClientConnected);

        let (mut sink, mut stream) = socket.split();
        let (out_tx, mut out_rx) = mpsc::channel::<ServerMessage>(128);
        let connection_id = format!("conn_{}", Uuid::new_v4().simple());

        out_tx
            .send(ServerMessage::Hello {
                protocol_version: PROTOCOL_VERSION.to_string(),
                protocol_version_struct: Some(ProtocolVersion::CURRENT),
                daemon_version: crate::DAEMON_VERSION.to_string(),
                platform: self.runtime.backend.platform(),
                session_id: connection_id.clone(),
                auth_required: self.runtime.config.auth_token.is_some(),
            })
            .await
            .ok();

        self.subscribers.insert(connection_id.clone(), out_tx.clone());

        // Pump runtime events to the client.
        let mut runtime_events = self.runtime.bus.events.subscribe();
        let out_tx_evt = out_tx.clone();
        let evt_task = tokio::spawn(async move {
            while let Ok(evt) = runtime_events.recv().await {
                if matches!(evt, RuntimeEvent::EmergencyStop) {
                    let _ = out_tx_evt
                        .send(ServerMessage::EmergencyStopped { request_id: None })
                        .await;
                }
            }
        });

        // Writer task: drains out_rx into the websocket.
        let writer = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                let json = match serde_json::to_string(&msg) {
                    Ok(j) => j,
                    Err(e) => {
                        error!("serialise outgoing: {e}");
                        continue;
                    }
                };
                if sink.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
            // Send close frame for a clean handshake.
            let _ = sink.close().await;
        });

        let mut active_session: Option<Arc<Session>> = None;
        while let Some(msg) = stream.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    debug!("ws recv: {e}");
                    break;
                }
            };
            match msg {
                Message::Text(t) => match serde_json::from_str::<ClientMessage>(&t) {
                    Ok(cm) => {
                        self.dispatch(&mut active_session, &out_tx, cm).await;
                    }
                    Err(e) => {
                        let _ = out_tx
                            .send(ServerMessage::error(
                                None,
                                ErrorCode::BadRequest,
                                format!("parse: {e}"),
                            ))
                            .await;
                    }
                },
                Message::Binary(_) => {
                    let _ = out_tx
                        .send(ServerMessage::error(
                            None,
                            ErrorCode::Unsupported,
                            "binary frames not supported",
                        ))
                        .await;
                }
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => {
                    // axum handles ping/pong itself.
                }
            }
        }

        evt_task.abort();
        // Drop our sender so the writer task drains and exits.
        drop(out_tx);
        let _ = writer.await;

        self.subscribers.remove(&connection_id);
        *self.runtime.bus.connected_clients.write() = self
            .runtime
            .bus
            .connected_clients
            .read()
            .saturating_sub(1);
        gauge!("nerve_ws_connections_active")
            .set(*self.runtime.bus.connected_clients.read() as f64);
        let _ = self.runtime.bus.events.send(RuntimeEvent::ClientDisconnected);
        info!(%addr, "websocket client disconnected");
    }

    async fn dispatch(
        &self,
        active_session: &mut Option<Arc<Session>>,
        out_tx: &mpsc::Sender<ServerMessage>,
        msg: ClientMessage,
    ) {
        let runtime = &self.runtime;
        let sessions = &self.sessions;
        let dashboard = &self.dashboard_state;
        match msg {
            ClientMessage::Ping { request_id, nonce } => {
                let _ = out_tx.send(ServerMessage::Pong { request_id, nonce }).await;
            }
            ClientMessage::SessionStart {
                request_id,
                client_name,
                client_version,
                client_protocol_version,
                auth_token,
                session_id,
                policy,
            } => {
                // Version negotiation. If the client sent a structured version,
                // refuse incompatible ones.
                if let Some(cpv) = client_protocol_version {
                    if !ProtocolVersion::CURRENT.compatible_with(cpv) {
                        let _ = out_tx
                            .send(ServerMessage::error(
                                Some(request_id),
                                ErrorCode::VersionMismatch,
                                format!(
                                    "client {cpv} incompatible with daemon {}",
                                    ProtocolVersion::CURRENT
                                ),
                            ))
                            .await;
                        return;
                    }
                }
                // Token auth.
                if let Some(expected) = runtime.config.auth_token.as_deref() {
                    match auth_token.as_deref() {
                        None => {
                            let _ = out_tx
                                .send(ServerMessage::error(
                                    Some(request_id),
                                    ErrorCode::AuthRequired,
                                    "daemon requires auth_token",
                                ))
                                .await;
                            return;
                        }
                        Some(tok) if !constant_time_eq(tok.as_bytes(), expected.as_bytes()) => {
                            let _ = out_tx
                                .send(ServerMessage::error(
                                    Some(request_id),
                                    ErrorCode::AuthInvalid,
                                    "invalid auth_token",
                                ))
                                .await;
                            return;
                        }
                        Some(_) => {}
                    }
                }
                let pol = policy.unwrap_or_else(|| runtime.config.default_policy.clone());
                let session = match session_id {
                    Some(id) => Session::with_id(id, pol),
                    None => Session::new(client_name, client_version, pol),
                };
                sessions.insert(session.meta.id.clone(), session.clone());
                *active_session = Some(session.clone());
                let _ = out_tx
                    .send(ServerMessage::SessionStarted {
                        request_id,
                        session_id: session.meta.id.clone(),
                        capabilities: runtime.capabilities(),
                    })
                    .await;
            }
            ClientMessage::SessionStop { request_id } => {
                if let Some(s) = active_session.clone() {
                    sessions.remove(&s.meta.id);
                    let _ = out_tx
                        .send(ServerMessage::SessionStopped {
                            request_id,
                            session_id: s.meta.id.clone(),
                        })
                        .await;
                    *active_session = None;
                } else {
                    no_session(out_tx, Some(request_id)).await;
                }
            }
            ClientMessage::GetCapabilities { request_id } => {
                let _ = out_tx
                    .send(ServerMessage::Capabilities {
                        request_id,
                        capabilities: runtime.capabilities(),
                    })
                    .await;
            }
            ClientMessage::GetObservation {
                request_id,
                include_screenshot,
                include_ui_tree,
            } => {
                let session = match ensure_session(active_session, out_tx, Some(request_id.clone())).await {
                    Some(s) => s,
                    None => return,
                };
                let opts = ObserveOpts {
                    include_screenshot: include_screenshot.unwrap_or(true),
                    include_ui_tree: include_ui_tree.unwrap_or(true),
                };
                let obs = observe(&runtime.backend, &session.safety, &session.meta.id, opts).await;
                let _ = out_tx
                    .send(ServerMessage::Observation {
                        request_id: Some(request_id),
                        observation: obs,
                    })
                    .await;
            }
            ClientMessage::SubscribeObservations {
                request_id,
                interval_ms,
                include_screenshot,
                cursor_only,
                delta_frames,
            } => {
                let session = match ensure_session(active_session, out_tx, Some(request_id.clone())).await {
                    Some(s) => s,
                    None => return,
                };
                let tx = out_tx.clone();
                let backend = runtime.backend.clone();
                let session_id = session.meta.id.clone();
                let safety = session.safety.clone();
                let req_id = request_id.clone();
                if cursor_only {
                    // High-frequency cursor stream: just cursor + active window.
                    // Use ~60Hz by default unless the caller picked something tighter.
                    let interval = std::cmp::max(interval_ms, 16);
                    tokio::spawn(async move {
                        loop {
                            let cursor = backend.cursor_position().await.unwrap_or_default();
                            let active_window = backend
                                .active_window()
                                .await
                                .ok()
                                .flatten()
                                .map(|w| w.app_name);
                            // Skip a tick if the channel is full so we don't lock up.
                            if let Err(e) = tx.try_send(ServerMessage::CursorTick {
                                request_id: Some(req_id.clone()),
                                timestamp: chrono::Utc::now(),
                                cursor,
                                active_window,
                            }) {
                                if matches!(e, tokio::sync::mpsc::error::TrySendError::Closed(_)) {
                                    break;
                                }
                                // Full channel: drop frame, the client is behind.
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(interval)).await;
                        }
                    });
                } else {
                    let opts = ObserveOpts {
                        include_screenshot: include_screenshot.unwrap_or(false),
                        include_ui_tree: false,
                    };
                    let frame_cache = if delta_frames {
                        Some(crate::diff::FrameCache::new())
                    } else {
                        None
                    };
                    tokio::spawn(async move {
                        loop {
                            let mut obs = observe(&backend, &safety, &session_id, opts).await;
                            counter!("nerve_observations_total").increment(1);
                            if obs.screen.screenshot_base64.is_some() {
                                counter!("nerve_screenshots_total").increment(1);
                            }
                            // Delta-frame computation.
                            if let Some(cache) = frame_cache.as_ref() {
                                if let Ok(cap) = backend.capture_primary_screen().await {
                                    let dirty = cache.diff_and_replace(
                                        // xcap returns RGBA bytes; we only have PNG here.
                                        // For a real fast path the backend should expose
                                        // raw RGBA — for the MVP we hash the PNG bytes
                                        // tile-wise as a stand-in.
                                        &cap.png_bytes,
                                        cap.width,
                                        cap.height,
                                        cap.png_bytes.clone(),
                                    );
                                    obs.dirty_tiles = crate::diff::coalesce(dirty);
                                }
                            }
                            // Backpressure: try_send so a slow client never blocks the daemon.
                            match tx.try_send(ServerMessage::Observation {
                                request_id: Some(req_id.clone()),
                                observation: obs,
                            }) {
                                Ok(()) => {}
                                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                    // Drop this frame.
                                }
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                    break;
                                }
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(
                                interval_ms.max(50),
                            ))
                            .await;
                        }
                    });
                }
            }
            ClientMessage::UnsubscribeObservations { request_id: _ } => {
                // The MVP runs one task per subscription and lets it die when
                // the channel closes (i.e. when the websocket terminates).
                // Cancelling mid-stream is left for the multi-stream upgrade.
            }
            ClientMessage::ExecuteAction { request_id, action } => {
                let session = match ensure_session(active_session, out_tx, Some(request_id.clone())).await {
                    Some(s) => s,
                    None => return,
                };
                if let Some(key) = action.idempotency_key.as_ref() {
                    if let Some(cached) = session.idempotency.get(key) {
                        counter!("nerve_actions_total", "method" => "idempotent").increment(1);
                        let _ = out_tx
                            .send(ServerMessage::ActionResult {
                                request_id,
                                result: cached,
                            })
                            .await;
                        return;
                    }
                }
                let idempotency_key = action.idempotency_key.clone();
                let started = std::time::Instant::now();
                match runtime.executor.execute(&session, action).await {
                    Ok(result) => {
                        let method = format!("{:?}", result.method);
                        *dashboard.last_action.write() = Some(method.clone());
                        counter!("nerve_actions_total", "method" => method.clone()).increment(1);
                        if !result.ok {
                            counter!("nerve_actions_failed_total", "method" => method).increment(1);
                        }
                        histogram!("nerve_action_latency_ms")
                            .record(started.elapsed().as_secs_f64() * 1000.0);
                        if let Some(k) = idempotency_key {
                            session.idempotency.insert(k, result.clone());
                        }
                        let _ = out_tx
                            .send(ServerMessage::ActionResult { request_id, result })
                            .await;
                    }
                    Err(e) => {
                        counter!("nerve_actions_failed_total", "method" => "error").increment(1);
                        let _ = out_tx
                            .send(ServerMessage::error(
                                Some(request_id),
                                error_code_for(&e),
                                e.to_string(),
                            ))
                            .await;
                    }
                }
            }
            ClientMessage::ExecuteActionBatch {
                request_id,
                actions,
                stop_on_error,
            } => {
                let session = match ensure_session(active_session, out_tx, Some(request_id.clone())).await {
                    Some(s) => s,
                    None => return,
                };
                let mut results = Vec::with_capacity(actions.len());
                for a in actions {
                    // Idempotency for batch members.
                    if let Some(key) = a.idempotency_key.as_ref() {
                        if let Some(cached) = session.idempotency.get(key) {
                            results.push(cached);
                            continue;
                        }
                    }
                    let idempotency_key = a.idempotency_key.clone();
                    match runtime.executor.execute(&session, a).await {
                        Ok(r) => {
                            let ok = r.ok;
                            if let Some(k) = idempotency_key {
                                session.idempotency.insert(k, r.clone());
                            }
                            results.push(r);
                            if !ok && stop_on_error {
                                break;
                            }
                        }
                        Err(e) => {
                            let _ = out_tx
                                .send(ServerMessage::error(
                                    Some(request_id.clone()),
                                    error_code_for(&e),
                                    e.to_string(),
                                ))
                                .await;
                            if stop_on_error {
                                return;
                            }
                        }
                    }
                }
                let _ = out_tx
                    .send(ServerMessage::BatchResult { request_id, results })
                    .await;
            }
            ClientMessage::GetActionLog {
                request_id,
                session_id,
                limit,
            } => {
                let sid = session_id.unwrap_or_else(|| {
                    active_session
                        .as_ref()
                        .map(|s| s.meta.id.clone())
                        .unwrap_or_default()
                });
                match runtime.audit.read_session(&sid, limit) {
                    Ok(entries) => {
                        let _ = out_tx
                            .send(ServerMessage::ActionLog { request_id, entries })
                            .await;
                    }
                    Err(e) => {
                        let _ = out_tx
                            .send(ServerMessage::error(
                                Some(request_id),
                                ErrorCode::LogIoError,
                                e.to_string(),
                            ))
                            .await;
                    }
                }
            }
            ClientMessage::ReplaySession {
                request_id,
                session_id,
                speed,
            } => {
                let speed = speed.unwrap_or(1.0).max(0.01);
                let entries = match runtime.audit.read_session(&session_id, None) {
                    Ok(e) => e,
                    Err(e) => {
                        let _ = out_tx
                            .send(ServerMessage::error(
                                Some(request_id),
                                ErrorCode::ReplayUnavailable,
                                e.to_string(),
                            ))
                            .await;
                        return;
                    }
                };
                let total = entries.len();
                for (i, entry) in entries.into_iter().enumerate() {
                    let _ = out_tx
                        .send(ServerMessage::ReplayProgress {
                            request_id: request_id.clone(),
                            step: i,
                            total,
                            entry,
                        })
                        .await;
                    tokio::time::sleep(std::time::Duration::from_millis((100.0 / speed) as u64)).await;
                }
                let _ = out_tx
                    .send(ServerMessage::ReplayComplete {
                        request_id,
                        session_id,
                    })
                    .await;
            }
            ClientMessage::SetSafetyPolicy { request_id, policy } => {
                if let Some(session) = active_session.as_ref() {
                    session.safety.set_policy(policy.clone());
                    let _ = out_tx
                        .send(ServerMessage::PolicyUpdated { request_id, policy })
                        .await;
                } else {
                    no_session(out_tx, Some(request_id)).await;
                }
            }
            ClientMessage::EmergencyStop { request_id } => {
                if let Some(session) = active_session.as_ref() {
                    session.safety.engage_emergency_stop();
                }
                runtime.engage_emergency_stop();
                let _ = out_tx
                    .send(ServerMessage::EmergencyStopped {
                        request_id: Some(request_id),
                    })
                    .await;
            }
            ClientMessage::ConfirmAction {
                request_id,
                action_id,
                allow,
            } => {
                if let Some(session) = active_session.as_ref() {
                    session.safety.register_confirmation(&action_id, allow);
                    let _ = out_tx
                        .send(ServerMessage::PolicyUpdated {
                            request_id,
                            policy: session.safety.policy(),
                        })
                        .await;
                } else {
                    no_session(out_tx, Some(request_id)).await;
                }
            }
        }
    }
}

async fn ensure_session(
    slot: &mut Option<Arc<Session>>,
    out_tx: &mpsc::Sender<ServerMessage>,
    request_id: Option<String>,
) -> Option<Arc<Session>> {
    if let Some(s) = slot.clone() {
        return Some(s);
    }
    no_session(out_tx, request_id).await;
    None
}

async fn no_session(out_tx: &mpsc::Sender<ServerMessage>, request_id: Option<String>) {
    let _ = out_tx
        .send(ServerMessage::error(
            request_id,
            ErrorCode::NoSession,
            "call session_start before invoking actions",
        ))
        .await;
}

fn error_code_for(e: &crate::errors::NerveError) -> ErrorCode {
    use crate::errors::NerveError as NE;
    match e {
        NE::Unsupported(_) => ErrorCode::Unsupported,
        NE::PermissionDenied(_) => ErrorCode::PermissionDenied,
        NE::SafetyRejected(_) => ErrorCode::SafetyRejected,
        NE::RateLimited { .. } => ErrorCode::RateLimited,
        NE::EmergencyStopped => ErrorCode::EmergencyStopped,
        NE::ElementNotFound => ErrorCode::ElementNotFound,
        NE::Io(_) => ErrorCode::LogIoError,
        NE::Serde(_) => ErrorCode::BadRequest,
        NE::Backend(_) | NE::Other(_) => ErrorCode::BackendFailure,
    }
}

mod dashboard {
    pub const INDEX_HTML: &str = include_str!("../../../../dashboard/static/index.html");
    pub const APP_JS: &str = include_str!("../../../../dashboard/static/dashboard.js");
    pub const APP_CSS: &str = include_str!("../../../../dashboard/static/dashboard.css");
}

/// Timing-safe byte slice equality.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

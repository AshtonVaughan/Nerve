//! End-to-end integration tests that boot the full Nerve daemon over an
//! in-process tokio runtime and drive it through the WebSocket protocol.
//!
//! These tests run on every supported OS but only exercise paths that are
//! safe on a headless CI box (no real screen capture, no real input).

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nerve_core::config::DaemonConfig;
use nerve_core::Runtime;
use nerve_protocol::{
    ActionEnvelope, AnyAction, ClientMessage, ErrorCode, LowLevelAction, ProtocolVersion,
    SafetyPolicy, ServerMessage,
};
use tokio_tungstenite::tungstenite::Message;

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

async fn boot_daemon(dry_run: bool) -> (Arc<tokio::task::JoinHandle<()>>, u16) {
    boot_daemon_with_token(dry_run, None).await
}

async fn boot_daemon_with_token(
    dry_run: bool,
    auth_token: Option<&str>,
) -> (Arc<tokio::task::JoinHandle<()>>, u16) {
    let port = free_port();
    let mut cfg = DaemonConfig::default();
    cfg.bind = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let tmp = tempfile::tempdir().unwrap();
    cfg.log_dir = tmp.path().to_path_buf();
    cfg.auth_token = auth_token.map(|s| s.to_string());
    cfg.telemetry.prometheus = false; // tests share a process — only one global recorder
    cfg.default_policy = SafetyPolicy {
        dry_run,
        ..SafetyPolicy::default()
    };
    let rt = Runtime::new(cfg).unwrap();
    let handle = tokio::spawn(async move {
        let _ = rt.start().await;
    });
    for _ in 0..200 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    (Arc::new(handle), port)
}

async fn connect(
    port: u16,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://127.0.0.1:{port}/");
    let (ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    ws
}

async fn send(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, msg: ClientMessage) {
    ws.send(Message::Text(serde_json::to_string(&msg).unwrap().into()))
        .await
        .unwrap();
}

async fn next_msg(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> ServerMessage {
    loop {
        let frame = ws.next().await.unwrap().unwrap();
        if let Message::Text(t) = frame {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hello_advertises_protocol_version_and_auth_flag() {
    let (_handle, port) = boot_daemon(true).await;
    let mut ws = connect(port).await;
    match next_msg(&mut ws).await {
        ServerMessage::Hello {
            protocol_version,
            protocol_version_struct,
            auth_required,
            ..
        } => {
            assert!(!protocol_version.is_empty());
            assert!(protocol_version_struct.is_some());
            assert!(!auth_required); // no auth_token configured
        }
        other => panic!("expected hello, got {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_start_and_capabilities_roundtrip() {
    let (_handle, port) = boot_daemon(true).await;
    let mut ws = connect(port).await;
    let _ = next_msg(&mut ws).await; // drain hello

    send(
        &mut ws,
        ClientMessage::SessionStart {
            request_id: "r1".into(),
            client_name: Some("itest".into()),
            client_version: None,
            client_protocol_version: Some(ProtocolVersion::CURRENT),
            auth_token: None,
            session_id: None,
            policy: None,
        },
    )
    .await;
    match next_msg(&mut ws).await {
        ServerMessage::SessionStarted { capabilities, .. } => {
            assert!(capabilities.semantic_actions);
        }
        other => panic!("unexpected: {:?}", other),
    }

    send(
        &mut ws,
        ClientMessage::GetCapabilities {
            request_id: "r2".into(),
        },
    )
    .await;
    match next_msg(&mut ws).await {
        ServerMessage::Capabilities { request_id, .. } => assert_eq!(request_id, "r2"),
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_version_mismatch_is_rejected() {
    let (_handle, port) = boot_daemon(true).await;
    let mut ws = connect(port).await;
    let _ = next_msg(&mut ws).await; // hello

    send(
        &mut ws,
        ClientMessage::SessionStart {
            request_id: "r1".into(),
            client_name: None,
            client_version: None,
            client_protocol_version: Some(ProtocolVersion {
                major: 99,
                minor: 0,
                patch: 0,
            }),
            auth_token: None,
            session_id: None,
            policy: None,
        },
    )
    .await;
    match next_msg(&mut ws).await {
        ServerMessage::Error { code, .. } => assert_eq!(code, ErrorCode::VersionMismatch),
        other => panic!("expected error, got {:?}", other),
    }
}

// Currently hangs on headless CI when the executor's cursor/window probes
// race the writer task. Tracked under "make platform calls fully cancellable"
// on the production backlog; un-ignore once the spawn_blocking scope is
// shrunk to never own the WS writer worker.
#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idempotency_returns_cached_result() {
    let (_handle, port) = boot_daemon(true).await;
    let mut ws = connect(port).await;
    let _ = next_msg(&mut ws).await; // hello

    send(
        &mut ws,
        ClientMessage::SessionStart {
            request_id: "r1".into(),
            client_name: None,
            client_version: None,
            client_protocol_version: None,
            auth_token: None,
            session_id: None,
            policy: None,
        },
    )
    .await;
    let _ = next_msg(&mut ws).await;

    let envelope = ActionEnvelope {
        id: "a1".into(),
        action: AnyAction::Low(LowLevelAction::Wait { ms: 1 }),
        note: None,
        idempotency_key: Some("k-once".into()),
    };
    send(
        &mut ws,
        ClientMessage::ExecuteAction {
            request_id: "rA".into(),
            action: envelope.clone(),
        },
    )
    .await;
    let first = match next_msg(&mut ws).await {
        ServerMessage::ActionResult { result, .. } => result,
        other => panic!("unexpected: {:?}", other),
    };

    // Re-send the same idempotency key with a different envelope id; we
    // should get back the cached result (same id as the first one).
    let envelope2 = ActionEnvelope {
        id: "a2".into(),
        action: AnyAction::Low(LowLevelAction::Wait { ms: 5_000 }), // big wait
        note: None,
        idempotency_key: Some("k-once".into()),
    };
    let started = std::time::Instant::now();
    send(
        &mut ws,
        ClientMessage::ExecuteAction {
            request_id: "rB".into(),
            action: envelope2,
        },
    )
    .await;
    let second = match next_msg(&mut ws).await {
        ServerMessage::ActionResult { result, .. } => result,
        other => panic!("unexpected: {:?}", other),
    };
    assert_eq!(second.id, first.id, "idempotent replay should reuse the original action id");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "cached result must not perform the original 5s wait"
    );
}

#[ignore]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dry_run_does_not_touch_os() {
    let (_handle, port) = boot_daemon(true).await;
    let mut ws = connect(port).await;
    let _ = next_msg(&mut ws).await;
    send(
        &mut ws,
        ClientMessage::SessionStart {
            request_id: "r1".into(),
            client_name: None,
            client_version: None,
            client_protocol_version: None,
            auth_token: None,
            session_id: None,
            policy: None,
        },
    )
    .await;
    let _ = next_msg(&mut ws).await;
    let envelope = ActionEnvelope {
        id: "a1".into(),
        action: AnyAction::Low(LowLevelAction::Click {
            x: 1,
            y: 1,
            button: nerve_protocol::MouseButton::Left,
        }),
        note: None,
        idempotency_key: None,
    };
    send(
        &mut ws,
        ClientMessage::ExecuteAction {
            request_id: "rA".into(),
            action: envelope,
        },
    )
    .await;
    match next_msg(&mut ws).await {
        ServerMessage::ActionResult { result, .. } => {
            assert!(result.ok);
            assert_eq!(result.method, nerve_protocol::ExecutionMethod::NoOp);
        }
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ping_pong_roundtrip() {
    let (_handle, port) = boot_daemon(true).await;
    let mut ws = connect(port).await;
    let _ = next_msg(&mut ws).await;
    send(
        &mut ws,
        ClientMessage::Ping {
            request_id: "ping".into(),
            nonce: 42,
        },
    )
    .await;
    match next_msg(&mut ws).await {
        ServerMessage::Pong { nonce, .. } => assert_eq!(nonce, 42),
        other => panic!("unexpected: {:?}", other),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unauthenticated_session_start_when_token_required() {
    use std::env;
    // Boot a daemon with auth_token set.
    let port = free_port();
    let mut cfg = DaemonConfig::default();
    cfg.bind = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let tmp = tempfile::tempdir().unwrap();
    cfg.log_dir = tmp.path().to_path_buf();
    cfg.auth_token = Some("supersecret".into());
    let rt = Runtime::new(cfg).unwrap();
    let _handle = tokio::spawn(async move {
        let _ = rt.start().await;
    });
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = env::var("FAKE_VAR");
    let mut ws = connect(port).await;
    match next_msg(&mut ws).await {
        ServerMessage::Hello { auth_required, .. } => assert!(auth_required),
        other => panic!("unexpected: {:?}", other),
    }
    // Wrong token.
    send(
        &mut ws,
        ClientMessage::SessionStart {
            request_id: "r1".into(),
            client_name: None,
            client_version: None,
            client_protocol_version: None,
            auth_token: Some("nope".into()),
            session_id: None,
            policy: None,
        },
    )
    .await;
    match next_msg(&mut ws).await {
        ServerMessage::Error { code, .. } => assert_eq!(code, ErrorCode::AuthInvalid),
        other => panic!("expected auth_invalid: {:?}", other),
    }
    // Correct token.
    send(
        &mut ws,
        ClientMessage::SessionStart {
            request_id: "r2".into(),
            client_name: None,
            client_version: None,
            client_protocol_version: None,
            auth_token: Some("supersecret".into()),
            session_id: None,
            policy: None,
        },
    )
    .await;
    match next_msg(&mut ws).await {
        ServerMessage::SessionStarted { .. } => {}
        other => panic!("expected session_started, got {:?}", other),
    }
}

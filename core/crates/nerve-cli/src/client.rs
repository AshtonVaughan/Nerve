//! Small WebSocket client used by every non-`start` CLI command.

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use nerve_protocol::{
    ActionEnvelope, ActionResult, AnyAction, AuditEntry, Capabilities, ClientMessage, Observation,
    ServerMessage,
};

pub struct CliClient {
    ws: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
}

impl CliClient {
    pub async fn connect(host: &str, port: u16) -> Result<Self> {
        let url = format!("ws://{}:{}/", host, port);
        let (ws, _resp) = tokio_tungstenite::connect_async(url).await?;
        let mut me = Self { ws };
        // Drain the daemon's `Hello` so callers can ignore it.
        let _ = me.next().await?;
        Ok(me)
    }

    pub async fn send(&mut self, msg: ClientMessage) -> Result<()> {
        let json = serde_json::to_string(&msg)?;
        self.ws.send(Message::Text(json.into())).await?;
        Ok(())
    }

    pub async fn next(&mut self) -> Result<ServerMessage> {
        loop {
            let msg = self
                .ws
                .next()
                .await
                .ok_or_else(|| anyhow!("daemon closed the connection"))??;
            match msg {
                Message::Text(t) => return Ok(serde_json::from_str::<ServerMessage>(&t)?),
                Message::Binary(_) => return Err(anyhow!("unexpected binary frame")),
                Message::Close(_) => return Err(anyhow!("daemon closed the connection")),
                _ => continue,
            }
        }
    }

    pub async fn request<F>(&mut self, build: F) -> Result<ServerMessage>
    where
        F: FnOnce(String) -> ClientMessage,
    {
        let id = format!("cli_{}", Uuid::new_v4().simple());
        self.send(build(id.clone())).await?;
        loop {
            let msg = self.next().await?;
            if matches_request_id(&msg, &id) {
                return Ok(msg);
            }
        }
    }

    pub async fn session_start(&mut self) -> Result<String> {
        let token = std::env::var("NERVE_AUTH_TOKEN").ok();
        let resp = self
            .request(|request_id| ClientMessage::SessionStart {
                request_id,
                client_name: Some("nerve-cli".into()),
                client_version: Some(env!("CARGO_PKG_VERSION").into()),
                client_protocol_version: Some(nerve_protocol::ProtocolVersion::CURRENT),
                auth_token: token,
                session_id: None,
                policy: None,
            })
            .await?;
        match resp {
            ServerMessage::SessionStarted { session_id, .. } => Ok(session_id),
            ServerMessage::Error { code, message, .. } => Err(anyhow!("{code}: {message}")),
            other => Err(anyhow!("unexpected response to session_start: {:?}", other)),
        }
    }

    pub async fn capabilities(&mut self) -> Result<Capabilities> {
        let resp = self
            .request(|request_id| ClientMessage::GetCapabilities { request_id })
            .await?;
        match resp {
            ServerMessage::Capabilities { capabilities, .. } => Ok(capabilities),
            ServerMessage::Error { code, message, .. } => Err(anyhow!("{code}: {message}")),
            other => Err(anyhow!("unexpected response: {:?}", other)),
        }
    }

    pub async fn observation(&mut self, include_screenshot: bool, include_ui_tree: bool) -> Result<Observation> {
        let resp = self
            .request(|request_id| ClientMessage::GetObservation {
                request_id,
                include_screenshot: Some(include_screenshot),
                include_ui_tree: Some(include_ui_tree),
            })
            .await?;
        match resp {
            ServerMessage::Observation { observation, .. } => Ok(observation),
            ServerMessage::Error { code, message, .. } => Err(anyhow!("{code}: {message}")),
            other => Err(anyhow!("unexpected response: {:?}", other)),
        }
    }

    pub async fn execute(&mut self, action: AnyAction) -> Result<ActionResult> {
        let action_id = format!("act_{}", Uuid::new_v4().simple());
        let env = ActionEnvelope {
            id: action_id,
            action,
            note: None,
            idempotency_key: None,
        };
        let resp = self
            .request(|request_id| ClientMessage::ExecuteAction {
                request_id,
                action: env,
            })
            .await?;
        match resp {
            ServerMessage::ActionResult { result, .. } => Ok(result),
            ServerMessage::Error { code, message, .. } => Err(anyhow!("{code}: {message}")),
            other => Err(anyhow!("unexpected response: {:?}", other)),
        }
    }

    pub async fn action_log(&mut self, session_id: Option<String>, limit: Option<usize>) -> Result<Vec<AuditEntry>> {
        let resp = self
            .request(|request_id| ClientMessage::GetActionLog { request_id, session_id, limit })
            .await?;
        match resp {
            ServerMessage::ActionLog { entries, .. } => Ok(entries),
            ServerMessage::Error { code, message, .. } => Err(anyhow!("{code}: {message}")),
            other => Err(anyhow!("unexpected response: {:?}", other)),
        }
    }
}

fn matches_request_id(msg: &ServerMessage, id: &str) -> bool {
    match msg {
        ServerMessage::SessionStarted { request_id, .. }
        | ServerMessage::SessionStopped { request_id, .. }
        | ServerMessage::Capabilities { request_id, .. }
        | ServerMessage::ActionResult { request_id, .. }
        | ServerMessage::BatchResult { request_id, .. }
        | ServerMessage::ActionLog { request_id, .. }
        | ServerMessage::PolicyUpdated { request_id, .. }
        | ServerMessage::ReplayProgress { request_id, .. }
        | ServerMessage::ReplayComplete { request_id, .. }
        | ServerMessage::Pong { request_id, .. } => request_id == id,
        ServerMessage::Observation { request_id, .. } => request_id.as_deref() == Some(id),
        ServerMessage::CursorTick { request_id, .. } => request_id.as_deref() == Some(id),
        ServerMessage::Error { request_id, .. } => request_id.as_deref() == Some(id),
        ServerMessage::Hello { .. }
        | ServerMessage::EmergencyStopped { .. }
        | ServerMessage::ConfirmationRequired { .. } => false,
    }
}

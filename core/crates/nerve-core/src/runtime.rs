//! Runtime glue.
//!
//! Wires together the platform backend, audit log, executor, observation
//! gatherer, and WebSocket server. The runtime is the long-lived object that
//! the daemon's `main` keeps alive.

use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{info, warn};

use nerve_protocol::{Capabilities, Platform};

use crate::actions::Executor;
use crate::audit::AuditLog;
use crate::config::DaemonConfig;
use crate::platform::{self, PlatformBackend};
use crate::server::WsServer;

#[derive(Clone)]
pub struct Runtime {
    pub config: Arc<DaemonConfig>,
    pub backend: Arc<dyn PlatformBackend>,
    pub audit: Arc<AuditLog>,
    pub executor: Arc<Executor>,
    pub bus: Arc<RuntimeBus>,
}

#[derive(Debug)]
pub struct RuntimeBus {
    /// Broadcast channel for runtime-wide events (e.g. emergency stop).
    pub events: broadcast::Sender<RuntimeEvent>,
    /// Tracks connected client count for dashboard reporting.
    pub connected_clients: RwLock<usize>,
}

impl Default for RuntimeBus {
    fn default() -> Self {
        let (tx, _rx) = broadcast::channel(64);
        Self {
            events: tx,
            connected_clients: RwLock::new(0),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    EmergencyStop,
    ClientConnected,
    ClientDisconnected,
}

impl Runtime {
    pub fn new(config: DaemonConfig) -> std::io::Result<Self> {
        let backend = platform::detect();
        let audit = AuditLog::open(&config.log_dir)?;
        let executor = Arc::new(Executor::new(backend.clone(), audit.clone()));
        Ok(Self {
            config: Arc::new(config),
            backend,
            audit,
            executor,
            bus: Arc::new(RuntimeBus::default()),
        })
    }

    pub fn capabilities(&self) -> Capabilities {
        let mut caps = self.backend.capabilities();
        caps.platform = self.backend.platform();
        caps
    }

    pub async fn start(self) -> anyhow::Result<()> {
        info!(
            platform = ?Platform::current(),
            bind = %self.config.bind,
            log_dir = %self.config.log_dir.display(),
            "starting nerve daemon"
        );
        let server = WsServer::new(self);
        server.serve().await
    }

    pub fn engage_emergency_stop(&self) {
        let _ = self.bus.events.send(RuntimeEvent::EmergencyStop);
        warn!("runtime: broadcasting emergency stop");
    }
}

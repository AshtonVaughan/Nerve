//! Sessions.
//!
//! A session bundles together: a session id, the safety engine for that
//! session, and a handle for streaming observations. The runtime keeps a map
//! of active sessions and tears them down on `session_stop` or when the
//! underlying WebSocket closes.

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use uuid::Uuid;

use nerve_protocol::SafetyPolicy;

use crate::safety::SafetyEngine;

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub last_action: RwLockArc<Option<String>>,
}

#[derive(Debug, Default, Clone)]
pub struct RwLockArc<T>(pub std::sync::Arc<RwLock<T>>);

impl<T: Default> RwLockArc<T> {
    pub fn new() -> Self { Self(std::sync::Arc::new(RwLock::new(T::default()))) }
}

impl<T> RwLockArc<T> {
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, T> { self.0.read() }
    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, T> { self.0.write() }
}

pub struct Session {
    pub meta: SessionMeta,
    pub safety: Arc<SafetyEngine>,
    pub started: Instant,
}

impl Session {
    pub fn new(client_name: Option<String>, client_version: Option<String>, policy: SafetyPolicy) -> Arc<Self> {
        let id = format!("sess_{}", Uuid::new_v4().simple());
        Arc::new(Self {
            meta: SessionMeta {
                id,
                started_at: Utc::now(),
                client_name,
                client_version,
                last_action: RwLockArc::new(),
            },
            safety: SafetyEngine::new(policy),
            started: Instant::now(),
        })
    }

    pub fn with_id(id: String, policy: SafetyPolicy) -> Arc<Self> {
        Arc::new(Self {
            meta: SessionMeta {
                id,
                started_at: Utc::now(),
                client_name: None,
                client_version: None,
                last_action: RwLockArc::new(),
            },
            safety: SafetyEngine::new(policy),
            started: Instant::now(),
        })
    }
}

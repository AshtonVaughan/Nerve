//! Sessions.
//!
//! A session bundles together: a session id, the safety engine for that
//! session, and a handle for streaming observations. The runtime keeps a map
//! of active sessions and tears them down on `session_stop` or when the
//! underlying WebSocket closes.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use uuid::Uuid;

use nerve_protocol::{ActionResult, SafetyPolicy};

use crate::safety::SafetyEngine;

/// LRU-ish cache of recent (idempotency_key -> result) pairs.
///
/// Bounded to keep memory predictable. When the cache is full we evict the
/// oldest entry — clients should not depend on entries living longer than the
/// session, but in practice retries happen within seconds.
#[derive(Debug)]
pub struct IdempotencyCache {
    inner: Mutex<IdempotencyCacheInner>,
    capacity: usize,
    ttl: Duration,
}

#[derive(Debug, Default)]
struct IdempotencyCacheInner {
    order: VecDeque<(String, Instant)>,
    map: std::collections::HashMap<String, ActionResult>,
}

impl IdempotencyCache {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(IdempotencyCacheInner::default()),
            capacity,
            ttl,
        }
    }

    pub fn get(&self, key: &str) -> Option<ActionResult> {
        let mut guard = self.inner.lock();
        // Drop expired entries first.
        let now = Instant::now();
        while let Some((_, ts)) = guard.order.front() {
            if now.duration_since(*ts) > self.ttl {
                if let Some((evicted, _)) = guard.order.pop_front() {
                    guard.map.remove(&evicted);
                }
            } else {
                break;
            }
        }
        guard.map.get(key).cloned()
    }

    pub fn insert(&self, key: String, result: ActionResult) {
        let mut guard = self.inner.lock();
        if guard.map.contains_key(&key) {
            return;
        }
        while guard.order.len() >= self.capacity {
            if let Some((evicted, _)) = guard.order.pop_front() {
                guard.map.remove(&evicted);
            }
        }
        guard.order.push_back((key.clone(), Instant::now()));
        guard.map.insert(key, result);
    }
}

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
    pub idempotency: IdempotencyCache,
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
            idempotency: IdempotencyCache::new(2048, Duration::from_secs(600)),
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
            idempotency: IdempotencyCache::new(2048, Duration::from_secs(600)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use nerve_protocol::{ActionResult, ExecutionMethod};

    fn dummy_result(id: &str) -> ActionResult {
        ActionResult {
            id: id.into(),
            ok: true,
            timestamp: Utc::now(),
            method: ExecutionMethod::NoOp,
            cursor: None,
            active_window: None,
            error: None,
            data: None,
            screenshot_before: None,
            screenshot_after: None,
            compiled: None,
        }
    }

    #[test]
    fn idempotency_cache_round_trips() {
        let cache = IdempotencyCache::new(8, Duration::from_secs(60));
        assert!(cache.get("k1").is_none());
        cache.insert("k1".into(), dummy_result("a1"));
        let r = cache.get("k1").unwrap();
        assert_eq!(r.id, "a1");
    }

    #[test]
    fn idempotency_cache_evicts_when_full() {
        let cache = IdempotencyCache::new(2, Duration::from_secs(60));
        cache.insert("k1".into(), dummy_result("a1"));
        cache.insert("k2".into(), dummy_result("a2"));
        cache.insert("k3".into(), dummy_result("a3"));
        assert!(cache.get("k1").is_none());
        assert!(cache.get("k2").is_some());
        assert!(cache.get("k3").is_some());
    }
}

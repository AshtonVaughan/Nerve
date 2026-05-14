//! Append-only audit log.
//!
//! Logs are JSONL. Each session lives in its own file
//! `<log_dir>/<session_id>.jsonl`, plus an index file
//! `<log_dir>/index.jsonl` that tracks session metadata for fast listing.
//!
//! The log is the source of truth for replay: every replayed step is a literal
//! re-execution of an [`AuditEntry`].

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use tracing::warn;

use nerve_protocol::AuditEntry;

#[derive(Debug)]
pub struct AuditLog {
    root: PathBuf,
    open_writers: Mutex<dashmap::DashMap<String, Arc<Mutex<std::fs::File>>>>,
}

impl AuditLog {
    pub fn open(root: impl Into<PathBuf>) -> std::io::Result<Arc<Self>> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Arc::new(Self {
            root,
            open_writers: Mutex::new(dashmap::DashMap::new()),
        }))
    }

    pub fn root(&self) -> &Path { &self.root }

    pub fn append(&self, entry: &AuditEntry) -> std::io::Result<()> {
        let path = self.session_path(&entry.session_id);
        let map = self.open_writers.lock();
        let file = map
            .entry(entry.session_id.clone())
            .or_insert_with(|| {
                let f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .expect("open session log");
                Arc::new(Mutex::new(f))
            })
            .clone();
        drop(map);
        let json = serde_json::to_string(entry)?;
        let mut f = file.lock();
        writeln!(f, "{}", json)?;
        f.flush()?;
        Ok(())
    }

    pub fn read_session(&self, session_id: &str, limit: Option<usize>) -> std::io::Result<Vec<AuditEntry>> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return Ok(vec![]);
        }
        let file = std::fs::File::open(&path)?;
        let reader = BufReader::new(file);
        let mut entries: Vec<AuditEntry> = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() { continue; }
            match serde_json::from_str::<AuditEntry>(&line) {
                Ok(e) => entries.push(e),
                Err(e) => warn!("skipping malformed audit line: {e}"),
            }
        }
        if let Some(limit) = limit {
            if entries.len() > limit {
                let from = entries.len() - limit;
                entries.drain(0..from);
            }
        }
        Ok(entries)
    }

    pub fn list_sessions(&self) -> std::io::Result<Vec<String>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        let safe: String = session_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        self.root.join(format!("{}.jsonl", safe))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use nerve_protocol::{ActionResult, AnyAction, ExecutionMethod, LowLevelAction, SafetyDecision};

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::open(dir.path()).unwrap();
        let entry = AuditEntry {
            session_id: "test".into(),
            action_id: "a1".into(),
            timestamp: Utc::now(),
            action: AnyAction::Low(LowLevelAction::Screenshot),
            result: ActionResult {
                id: "a1".into(),
                ok: true,
                timestamp: Utc::now(),
                method: ExecutionMethod::Capture,
                cursor: None,
                active_window: None,
                error: None,
                data: None,
                screenshot_before: None,
                screenshot_after: None,
                compiled: None,
            },
            active_window_before: None,
            active_window_after: None,
            safety_decision: SafetyDecision::Allowed,
            note: None,
        };
        log.append(&entry).unwrap();
        log.append(&entry).unwrap();
        let back = log.read_session("test", None).unwrap();
        assert_eq!(back.len(), 2);
    }
}

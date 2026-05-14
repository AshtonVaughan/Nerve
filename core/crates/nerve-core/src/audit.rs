//! Append-only audit log.
//!
//! Logs are JSONL. Each session lives in its own file
//! `<log_dir>/<session_id>.jsonl`, plus an index file
//! `<log_dir>/index.jsonl` that tracks session metadata for fast listing.
//!
//! The log is the source of truth for replay: every replayed step is a literal
//! re-execution of an [`AuditEntry`].

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use flate2::write::GzEncoder;
use flate2::Compression;
use parking_lot::Mutex;
use tracing::{debug, warn};

use nerve_protocol::AuditEntry;

use crate::config::AuditConfig;

#[derive(Debug)]
pub struct AuditLog {
    root: PathBuf,
    open_writers: Mutex<dashmap::DashMap<String, Arc<Mutex<WriterState>>>>,
    rotation: AuditConfig,
}

#[derive(Debug)]
struct WriterState {
    file: std::fs::File,
    path: PathBuf,
    bytes_written: u64,
}

impl AuditLog {
    pub fn open(root: impl Into<PathBuf>) -> std::io::Result<Arc<Self>> {
        Self::open_with_rotation(root, AuditConfig::default())
    }

    pub fn open_with_rotation(
        root: impl Into<PathBuf>,
        rotation: AuditConfig,
    ) -> std::io::Result<Arc<Self>> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Arc::new(Self {
            root,
            open_writers: Mutex::new(dashmap::DashMap::new()),
            rotation,
        }))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn append(&self, entry: &AuditEntry) -> std::io::Result<()> {
        let path = self.session_path(&entry.session_id);
        let map = self.open_writers.lock();
        let state = map
            .entry(entry.session_id.clone())
            .or_insert_with(|| {
                let f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .expect("open session log");
                let bytes_written = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                Arc::new(Mutex::new(WriterState {
                    file: f,
                    path: path.clone(),
                    bytes_written,
                }))
            })
            .clone();
        drop(map);
        let json = serde_json::to_string(entry)?;
        let line_bytes = json.len() as u64 + 1;
        let mut guard = state.lock();
        writeln!(guard.file, "{}", json)?;
        guard.file.flush()?;
        guard.bytes_written += line_bytes;

        // Rotate if needed.
        if self.rotation.max_bytes_per_file > 0
            && guard.bytes_written >= self.rotation.max_bytes_per_file
        {
            let path = guard.path.clone();
            // Close the current file.
            drop(guard);
            self.open_writers.lock().remove(&entry.session_id);
            if let Err(e) = self.rotate_file(&path) {
                warn!("audit log rotation failed: {e}");
            }
        }
        Ok(())
    }

    pub fn read_session(
        &self,
        session_id: &str,
        limit: Option<usize>,
    ) -> std::io::Result<Vec<AuditEntry>> {
        let live = self.session_path(session_id);
        let mut entries: Vec<AuditEntry> = Vec::new();

        // Read rolled shards in chronological order first, then live file.
        let mut rolled: Vec<PathBuf> = Vec::new();
        if let Ok(dir) = std::fs::read_dir(&self.root) {
            let stem = sanitised(session_id);
            for entry in dir.flatten() {
                let p = entry.path();
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or_default();
                if name.starts_with(&format!("{stem}.")) && name != format!("{stem}.jsonl") {
                    rolled.push(p);
                }
            }
        }
        rolled.sort();

        for shard in rolled.into_iter().chain(std::iter::once(live)) {
            if !shard.exists() {
                continue;
            }
            match shard.extension().and_then(|s| s.to_str()) {
                Some("gz") => {
                    let f = std::fs::File::open(&shard)?;
                    let gz = flate2::read::GzDecoder::new(f);
                    let reader = BufReader::new(gz);
                    read_lines_into(reader, &mut entries);
                }
                _ => {
                    let f = std::fs::File::open(&shard)?;
                    let reader = BufReader::new(f);
                    read_lines_into(reader, &mut entries);
                }
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

    fn rotate_file(&self, path: &Path) -> std::io::Result<()> {
        let parent = path.parent().unwrap_or(Path::new("."));
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("session");
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3f");
        let rolled_name = format!("{}.{ts}.jsonl", stem);
        let rolled_path = parent.join(&rolled_name);
        std::fs::rename(path, &rolled_path)?;
        debug!("rotated audit log {:?} -> {:?}", path, rolled_path);

        // Compress if configured.
        if self.rotation.compress_rolled {
            let gz_path = parent.join(format!("{rolled_name}.gz"));
            let mut input = std::fs::File::open(&rolled_path)?;
            let output = std::fs::File::create(&gz_path)?;
            let mut encoder = GzEncoder::new(output, Compression::default());
            std::io::copy(&mut input, &mut encoder)?;
            encoder.finish()?;
            std::fs::remove_file(&rolled_path)?;
        }

        // Evict oldest if we exceed the cap.
        if self.rotation.max_rolled_files > 0 {
            self.evict_old(parent, stem)?;
        }
        Ok(())
    }

    fn evict_old(&self, dir: &Path, stem: &str) -> std::io::Result<()> {
        let prefix = format!("{stem}.");
        let mut rolled: Vec<PathBuf> = std::fs::read_dir(dir)?
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or_default();
                if name.starts_with(&prefix) && name != format!("{stem}.jsonl") {
                    Some(p)
                } else {
                    None
                }
            })
            .collect();
        rolled.sort();
        while rolled.len() > self.rotation.max_rolled_files {
            let oldest = rolled.remove(0);
            let _ = std::fs::remove_file(oldest);
        }
        Ok(())
    }

    /// Force-flush all open writers — call before shutdown.
    pub fn flush(&self) -> std::io::Result<()> {
        for entry in self.open_writers.lock().iter() {
            let mut guard = entry.value().lock();
            guard.file.flush()?;
        }
        Ok(())
    }

    pub fn list_sessions(&self) -> std::io::Result<Vec<String>> {
        let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default();
            // Live (no timestamp shard).
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if name.ends_with(".jsonl") && !stem.contains('.') {
                    out.insert(stem.to_string());
                    continue;
                }
                // Rolled shards have form `<stem>.<ts>.jsonl[.gz]`. Strip
                // both extensions to recover the session id.
                let mut s: &str = stem;
                if s.ends_with(".jsonl") {
                    s = &s[..s.len() - 6];
                }
                if let Some((session, _ts)) = s.rsplit_once('.') {
                    out.insert(session.to_string());
                }
            }
        }
        Ok(out.into_iter().collect())
    }

    fn session_path(&self, session_id: &str) -> PathBuf {
        self.root.join(format!("{}.jsonl", sanitised(session_id)))
    }
}

fn sanitised(session_id: &str) -> String {
    session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn read_lines_into<R: Read>(reader: BufReader<R>, entries: &mut Vec<AuditEntry>) {
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                warn!("audit log read error: {e}");
                return;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<AuditEntry>(&line) {
            Ok(e) => entries.push(e),
            Err(e) => warn!("skipping malformed audit line: {e}"),
        }
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

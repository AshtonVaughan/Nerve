//! Runtime configuration.
//!
//! Loaded from (in order of precedence):
//!
//! 1. `--config <path>` CLI flag,
//! 2. `$NERVE_CONFIG` environment variable,
//! 3. `~/.config/nerve/config.toml` on Unix / `%APPDATA%\nerve\config.toml` on Windows,
//! 4. compiled-in [`DaemonConfig::default()`].
//!
//! Values from the chosen file override defaults; environment variables can
//! still override individual fields (e.g. `NERVE_BIND`, `NERVE_AUTH_TOKEN`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use nerve_protocol::SafetyPolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub bind: SocketAddr,
    /// Directory for audit logs.
    pub log_dir: PathBuf,
    /// Maximum JPEG quality / PNG compression level. Currently advisory.
    pub screenshot_format: String,
    pub default_policy: SafetyPolicy,
    /// If true, the daemon refuses connections from non-loopback addresses
    /// regardless of `bind`.
    pub loopback_only: bool,
    /// Required shared secret. When set, clients must send it as
    /// `auth_token` in `session_start`. None means accept any local
    /// connection.
    pub auth_token: Option<String>,
    /// TLS settings. None disables TLS (loopback HTTP/WS only).
    pub tls: Option<TlsConfig>,
    /// Audit log rotation policy.
    pub audit: AuditConfig,
    /// Telemetry / metrics export policy.
    pub telemetry: TelemetryConfig,
    /// Optional Prometheus exporter bind address (e.g. "127.0.0.1:9464").
    pub prometheus_bind: Option<SocketAddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TlsConfig {
    /// PEM-encoded certificate chain file.
    pub cert_path: PathBuf,
    /// PEM-encoded private key file.
    pub key_path: PathBuf,
    /// If true, generate a self-signed cert on first launch and store it at
    /// the configured paths. Useful for local-only daemons.
    pub auto_self_signed: bool,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            cert_path: PathBuf::from(""),
            key_path: PathBuf::from(""),
            auto_self_signed: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditConfig {
    /// Maximum size per session log file before we roll to a new shard.
    pub max_bytes_per_file: u64,
    /// Number of rolled shards to keep before the oldest is deleted.
    pub max_rolled_files: usize,
    /// If true, compress rolled shards with gzip.
    pub compress_rolled: bool,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            // 64 MiB — large enough that small sessions never roll, small
            // enough that hostile or runaway sessions don't fill disk silently.
            max_bytes_per_file: 64 * 1024 * 1024,
            max_rolled_files: 16,
            compress_rolled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    /// Emit Prometheus metrics.
    pub prometheus: bool,
    /// Emit OpenTelemetry traces to the configured endpoint.
    pub otel_endpoint: Option<String>,
    /// Anonymous crash count opt-in. Off by default.
    pub crash_reports: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            prometheus: true,
            otel_endpoint: None,
            crash_reports: false,
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8765),
            log_dir: default_log_dir(),
            screenshot_format: "png".to_string(),
            default_policy: SafetyPolicy::default(),
            loopback_only: true,
            auth_token: None,
            tls: None,
            audit: AuditConfig::default(),
            telemetry: TelemetryConfig::default(),
            prometheus_bind: None,
        }
    }
}

impl DaemonConfig {
    /// Resolve the config: try the explicit `path`, then `$NERVE_CONFIG`,
    /// then the platform default location, falling back to in-memory
    /// defaults.
    pub fn resolve(path: Option<&Path>) -> (Self, Option<PathBuf>) {
        // 1. Explicit path.
        if let Some(p) = path {
            if p.exists() {
                if let Ok(s) = std::fs::read_to_string(p) {
                    if let Ok(cfg) = toml::from_str::<DaemonConfig>(&s) {
                        return (cfg.with_env_overrides(), Some(p.to_path_buf()));
                    }
                }
            }
        }
        // 2. $NERVE_CONFIG.
        if let Ok(p) = std::env::var("NERVE_CONFIG") {
            let p = PathBuf::from(p);
            if p.exists() {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if let Ok(cfg) = toml::from_str::<DaemonConfig>(&s) {
                        return (cfg.with_env_overrides(), Some(p));
                    }
                }
            }
        }
        // 3. Platform default.
        let default_path = default_config_path();
        if let Some(p) = &default_path {
            if p.exists() {
                if let Ok(s) = std::fs::read_to_string(p) {
                    if let Ok(cfg) = toml::from_str::<DaemonConfig>(&s) {
                        return (cfg.with_env_overrides(), Some(p.clone()));
                    }
                }
            }
        }
        // 4. Built-in defaults.
        (Self::default().with_env_overrides(), None)
    }

    /// Apply selected env-var overrides on top of a config.
    pub fn with_env_overrides(mut self) -> Self {
        if let Ok(s) = std::env::var("NERVE_BIND") {
            if let Ok(addr) = s.parse::<SocketAddr>() {
                self.bind = addr;
            }
        }
        if let Ok(s) = std::env::var("NERVE_AUTH_TOKEN") {
            if !s.is_empty() {
                self.auth_token = Some(s);
            }
        }
        if let Ok(s) = std::env::var("NERVE_LOG_DIR") {
            self.log_dir = PathBuf::from(s);
        }
        self
    }

    /// Render this config back to TOML for `nerve config show / write`.
    pub fn to_toml_pretty(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

pub fn default_log_dir() -> PathBuf {
    dirs::data_local_dir()
        .map(|p| p.join("nerve").join("logs"))
        .unwrap_or_else(|| PathBuf::from("./.nerve/logs"))
}

pub fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("nerve").join("config.toml"))
}

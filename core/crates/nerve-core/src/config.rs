//! Runtime configuration.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use nerve_protocol::SafetyPolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Required shared secret. When set, clients must send it in `client_name`
    /// during `session_start`. None means accept any local connection.
    pub auth_token: Option<String>,
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
        }
    }
}

pub fn default_log_dir() -> PathBuf {
    dirs::data_local_dir()
        .map(|p| p.join("nerve").join("logs"))
        .unwrap_or_else(|| PathBuf::from("./.nerve/logs"))
}

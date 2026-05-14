//! Daemon-wide metrics.
//!
//! Wired through the `metrics` crate so consumers can plug in any
//! exporter — we default to a Prometheus pull endpoint via
//! `metrics-exporter-prometheus`. The endpoint is mounted by `WsServer` at
//! `GET /metrics`.

use std::net::SocketAddr;
use std::sync::OnceLock;

use anyhow::Result;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the global metrics recorder once per process. Subsequent calls are
/// no-ops — useful for tests that build multiple [`Runtime`](crate::Runtime)
/// instances.
pub fn install_prometheus(_bind: Option<SocketAddr>) -> Result<()> {
    if HANDLE.get().is_some() {
        return Ok(());
    }
    let builder = PrometheusBuilder::new();
    let handle = builder
        .install_recorder()
        .map_err(|e| anyhow::anyhow!("install metrics recorder: {e}"))?;
    let _ = HANDLE.set(handle);
    // Register all known metric names so they appear in /metrics output even
    // if their value is zero.
    metrics::describe_counter!("nerve_actions_total", "Total actions executed");
    metrics::describe_counter!(
        "nerve_actions_failed_total",
        "Total actions that returned ok=false"
    );
    metrics::describe_histogram!(
        "nerve_action_latency_ms",
        "Per-action wall-clock latency in milliseconds"
    );
    metrics::describe_counter!(
        "nerve_observations_total",
        "Total observations served (snapshot + streamed)"
    );
    metrics::describe_counter!(
        "nerve_screenshots_total",
        "Total screenshots returned in observations"
    );
    metrics::describe_counter!(
        "nerve_safety_decisions_total",
        "Total safety decisions by kind"
    );
    metrics::describe_counter!(
        "nerve_ws_connections_total",
        "Total WebSocket connections accepted"
    );
    metrics::describe_gauge!(
        "nerve_ws_connections_active",
        "Active WebSocket connections right now"
    );
    metrics::describe_counter!("nerve_audit_writes_total", "Audit log entries appended");
    metrics::describe_counter!(
        "nerve_audit_rotations_total",
        "Number of audit log files rotated"
    );
    Ok(())
}

/// Render the current Prometheus snapshot if metrics are installed.
pub fn render_prometheus() -> Option<String> {
    HANDLE.get().map(|h| h.render())
}

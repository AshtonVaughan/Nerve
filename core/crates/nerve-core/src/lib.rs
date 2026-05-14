//! Nerve daemon library.
//!
//! The binary `nerve` is a thin wrapper that:
//!
//! 1. parses CLI flags,
//! 2. calls [`runtime::Runtime::start`] to spin up a WebSocket server,
//! 3. forwards a Ctrl-C into the runtime's emergency-stop channel.
//!
//! Everything else (platform backends, action execution, safety, audit log,
//! semantic action compiler) lives here so it can be reused by tests,
//! benchmarks, and future host integrations.

pub mod actions;
pub mod audit;
pub mod browser;
pub mod compiler;
pub mod config;
pub mod diff;
pub mod errors;
pub mod metrics;
pub mod observation;
pub mod ocr;
pub mod platform;
pub mod runtime;
pub mod safety;
pub mod server;
pub mod session;
pub mod tls;

pub use errors::NerveError;
pub use runtime::Runtime;

pub const DAEMON_VERSION: &str = env!("CARGO_PKG_VERSION");

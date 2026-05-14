//! Nerve wire protocol.
//!
//! Types here are the source of truth for the JSON exchanged between the
//! daemon, SDKs, CLI, and dashboard. Anything that crosses the WebSocket
//! boundary lives in this crate so that consumers can depend on it without
//! pulling in the rest of the daemon.

pub mod action;
pub mod errors;
pub mod observation;
pub mod policy;
pub mod ws;

pub use action::*;
pub use errors::*;
pub use observation::*;
pub use policy::*;
pub use ws::*;

/// Current wire-protocol version. Bump on breaking changes.
pub const PROTOCOL_VERSION: &str = "0.1.0";

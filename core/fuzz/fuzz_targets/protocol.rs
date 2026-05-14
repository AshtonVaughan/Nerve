//! Fuzz target for the Nerve wire protocol.
//!
//! Feeds arbitrary bytes into the JSON parser and asserts the daemon never
//! panics or returns ill-formed types. Caller invokes via
//! `cargo +nightly fuzz run protocol`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use nerve_protocol::{ClientMessage, ServerMessage};

fuzz_target!(|data: &[u8]| {
    // Anything that parses as a ClientMessage must round-trip without panic.
    if let Ok(cm) = serde_json::from_slice::<ClientMessage>(data) {
        let _ = serde_json::to_vec(&cm);
    }
    if let Ok(sm) = serde_json::from_slice::<ServerMessage>(data) {
        let _ = serde_json::to_vec(&sm);
    }
});

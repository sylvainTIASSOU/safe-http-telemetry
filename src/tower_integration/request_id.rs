//! Bounded request ID generation and optional incoming-ID propagation.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use http::HeaderMap;

const MAX_INCOMING_REQUEST_ID_BYTES: usize = 128;
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn resolve(headers: &HeaderMap, trust_incoming: bool) -> String {
    if trust_incoming {
        if let Some(request_id) = headers
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .filter(|value| is_valid_incoming(value))
        {
            return request_id.to_owned();
        }
    }

    generate()
}

fn is_valid_incoming(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_INCOMING_REQUEST_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._:".contains(&byte))
}

fn generate() -> String {
    let timestamp_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);

    // A request ID is an observability identifier, not a credential.
    // Time, process ID and a process-local sequence provide bounded IDs
    // without adding another runtime dependency.
    format!(
        "req-{:08x}-{:032x}-{:016x}",
        std::process::id(),
        timestamp_ns,
        sequence
    )
}

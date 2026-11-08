//! Ports `packages/shared/src/ids.ts`: unique identifiers (UUID v4).

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Milliseconds since the Unix epoch (`Date.now()`).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

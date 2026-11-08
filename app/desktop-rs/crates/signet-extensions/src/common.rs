//! Shared plumbing: API errors, callers, the event emitter trait and a small JSON file store.
//! Port of `apis/types.ts` and the `AtomicWriter` of the Electron app.

use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Error shown to the extension as `runtime.lastError` or a rejected promise.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ApiError(pub String);

impl ApiError {
    pub fn new(message: impl Into<String>) -> Self {
        ApiError(message.into())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// Answer to a call; errors travel as data so the message reaches the extension untouched.
#[derive(Debug, Clone, PartialEq)]
pub enum CrxResult {
    Ok(Option<Value>),
    Err(String),
}

impl CrxResult {
    pub fn ok(value: Value) -> Self {
        CrxResult::Ok(Some(value))
    }
    pub fn err(error: impl Into<String>) -> Self {
        CrxResult::Err(error.into())
    }
    pub fn is_ok(&self) -> bool {
        matches!(self, CrxResult::Ok(_))
    }
    /// `{ ok: true, value? }` or `{ ok: false, error }`, the shape the page-side helper expects.
    pub fn to_json(&self) -> Value {
        match self {
            CrxResult::Ok(Some(v)) if !v.is_null() => serde_json::json!({ "ok": true, "value": v }),
            CrxResult::Ok(_) => serde_json::json!({ "ok": true }),
            CrxResult::Err(e) => serde_json::json!({ "ok": false, "error": e }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextKind {
    Worker,
    Page,
}

/// The exact context that called (a worker run, or one page): several can exist for one extension.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApiContext {
    pub kind: ContextKind,
    /// Unique while the context lives (`worker-<n>`, `page-<n>`).
    pub id: String,
    pub url: String,
}

impl ApiContext {
    /// Context used when a caller does not give one (tests, internal calls).
    pub fn unknown() -> Self {
        ApiContext { kind: ContextKind::Worker, id: "worker-unknown".into(), url: String::new() }
    }
}

/// Who is calling: derived from the sender (worker scope or page URL) by the web layer, never from the arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub extension_id: String,
    pub context: ApiContext,
}

impl Caller {
    pub fn new(extension_id: &str) -> Self {
        Caller { extension_id: extension_id.to_string(), context: ApiContext::unknown() }
    }
}

/// One event to deliver to an extension (its worker and pages).
#[derive(Debug, Clone, PartialEq)]
pub struct OutEvent {
    pub extension_id: String,
    pub name: String,
    pub args: Vec<Value>,
    /// Start a stopped service worker for it (alarms, a setting taken over).
    pub wake: bool,
    /// Keep and retry when the worker cannot start (false for time-sensitive events).
    pub queue: bool,
}

/// What a feature needs to reach extensions.
pub trait Emitter: Send + Sync {
    fn emit(&self, extension_id: &str, name: &str, args: Vec<Value>, wake: bool, queue: bool);
    /// Extensions loaded that hold the permission (manifest or granted).
    fn extensions_with(&self, permission: &str) -> Vec<String>;
}

/// Emitter that records events (the host drains it; tests read it).
#[derive(Default)]
pub struct Outbox {
    events: Mutex<Vec<OutEvent>>,
    with_permission: Mutex<Vec<(String, Vec<String>)>>,
}

impl Outbox {
    pub fn new() -> Self {
        Self::default()
    }
    /// Takes the events recorded so far.
    pub fn drain(&self) -> Vec<OutEvent> {
        std::mem::take(&mut *self.events.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn peek(&self) -> Vec<OutEvent> {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    /// Declares which extensions hold a permission (the host sets it from its registry).
    pub fn set_holders(&self, permission: &str, ids: Vec<String>) {
        let mut guard = self.with_permission.lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|(p, _)| p != permission);
        guard.push((permission.to_string(), ids));
    }
}

impl Emitter for Outbox {
    fn emit(&self, extension_id: &str, name: &str, args: Vec<Value>, wake: bool, queue: bool) {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).push(OutEvent {
            extension_id: extension_id.to_string(),
            name: name.to_string(),
            args,
            wake,
            queue,
        });
    }
    fn extensions_with(&self, permission: &str) -> Vec<String> {
        let guard = self.with_permission.lock().unwrap_or_else(|e| e.into_inner());
        guard.iter().find(|(p, _)| p == permission).map(|(_, v)| v.clone()).unwrap_or_default()
    }
}

/// Writes a JSON file atomically (temp file then rename). `request` marks it dirty, `flush` writes when dirty.
pub struct JsonStore {
    path: PathBuf,
    dirty: bool,
}

impl JsonStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        JsonStore { path: path.into(), dirty: false }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Reads and parses the file; None when missing or corrupt.
    pub fn read(&self) -> Option<Value> {
        let text = fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&text).ok()
    }
    pub fn request(&mut self) {
        self.dirty = true;
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    /// Writes `value` when something was requested (or `force`). Returns true when the file is up to date.
    pub fn flush(&mut self, value: &Value) -> bool {
        if !self.dirty {
            return true;
        }
        match write_atomic(&self.path, serde_json::to_string(value).unwrap_or_default().as_bytes()) {
            Ok(()) => {
                self.dirty = false;
                true
            }
            Err(_) => false,
        }
    }
}

/// Atomic file write: sibling temp file, fsync, rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

pub fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|s| s.as_str().map(String::from)).collect(),
        _ => Vec::new(),
    }
}

pub fn uniq(list: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in list {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

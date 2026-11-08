//! Small shared helpers: error type, JSON files, atomic writes, HTML escaping.
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Msg(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Msg(msg.into()))
}

/// Reads and parses a JSON file; `None` when missing or invalid.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Atomic write: unique tmp file, fsync, rename.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let res = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res.map_err(Error::from)
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    write_atomic(path, &serde_json::to_vec(value)?)
}

pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Truncates to at most `max` chars (JS `slice` on UTF-16 differs only for astral chars).
pub fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Translation callback given by the app: key and `{name}` parameters to text (backed by signet-core i18n).
pub type Translate<'a> = &'a dyn Fn(&str, &[(&str, String)]) -> String;

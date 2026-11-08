//! Ports `app/desktop/src/main/core/atomic.ts`: durable and atomic file writes (tmp file + fsync + rename).
//!
//! Unlike the TS version, writes are synchronous: the app decides when to call them (see `store` for the timing).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes a file and fsyncs it.
pub fn write_file_durable(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut f = OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
    f.write_all(data)?;
    f.sync_all()
}

/// Atomic write (tmp + fsync + rename) of a file whose content is built on demand.
/// An identical payload to the last one written is skipped.
#[derive(Debug)]
pub struct AtomicWriter {
    file: PathBuf,
    last: Option<String>,
}

impl AtomicWriter {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        AtomicWriter { file: file.into(), last: None }
    }

    pub fn path(&self) -> &Path {
        &self.file
    }

    /// Forgets the last written payload (the next write always goes to disk).
    pub fn invalidate(&mut self) {
        self.last = None;
    }

    /// Writes `payload` (None = nothing to do). True if the file is up to date (written, identical or nothing to
    /// write); false if the write failed.
    pub fn write(&mut self, payload: Option<String>) -> bool {
        let Some(payload) = payload else { return true };
        if self.last.as_deref() == Some(payload.as_str()) {
            return true;
        }
        let mut tmp = self.file.clone().into_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        let result = write_file_durable(&tmp, payload.as_bytes()).and_then(|_| fs::rename(&tmp, &self.file));
        match result {
            Ok(()) => {
                self.last = Some(payload);
                true
            }
            Err(_) => {
                let _ = fs::remove_file(&tmp);
                false
            }
        }
    }
}

/// Reads a whole file as UTF-8 text.
pub fn read_text(path: &Path) -> std::io::Result<String> {
    let mut s = String::new();
    std::io::Read::read_to_string(&mut File::open(path)?, &mut s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_write() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        write_file_durable(&f, b"salut").unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "salut");
    }

    #[test]
    fn writes_without_leaving_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("state.json");
        let mut w = AtomicWriter::new(&f);
        assert!(w.write(Some("{\"a\":1}".into())));
        assert_eq!(fs::read_to_string(&f).unwrap(), "{\"a\":1}");
        assert!(!dir.path().join("state.json.tmp").exists());
    }

    #[test]
    fn nothing_to_write() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.json");
        let mut w = AtomicWriter::new(&f);
        assert!(w.write(None));
        assert!(!f.exists());
    }

    #[test]
    fn identical_content_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.json");
        let mut w = AtomicWriter::new(&f);
        assert!(w.write(Some("donn\u{e9}es".into())));
        fs::write(&f, "modifi\u{e9} ailleurs").unwrap();
        assert!(w.write(Some("donn\u{e9}es".into())));
        assert_eq!(fs::read_to_string(&f).unwrap(), "modifi\u{e9} ailleurs");
        w.invalidate();
        assert!(w.write(Some("donn\u{e9}es".into())));
        assert_eq!(fs::read_to_string(&f).unwrap(), "donn\u{e9}es");
    }

    #[test]
    fn failure_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = AtomicWriter::new(dir.path().join("absent").join("x.json"));
        assert!(!w.write(Some("a".into())));
    }
}

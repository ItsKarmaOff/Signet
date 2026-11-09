//! Data folders and the first-run import from the Electron app.
//!
//! The Rust app keeps its data in `<config dir>/Signet GPUI/` (macOS: `~/Library/Application Support/Signet GPUI/`,
//! Linux: `~/.config/Signet GPUI/`, Windows: `%APPDATA%\Signet GPUI\`), next to the Electron app's `Signet/`.
//! The Electron folder is only ever read.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Folder name of the Rust app.
pub const APP_DIR: &str = "Signet GPUI";
/// Folder name of the Electron app (its `userData`).
pub const ELECTRON_DIR: &str = "Signet";

/// JSON files copied from the Electron folder on first run. `passwords.json` is copied as is: it is encrypted with
/// Electron's `safeStorage` and the importer in `signet-features` decides what to do with it.
pub const IMPORT_FILES: [&str; 8] = ["state.json", "history.json", "tab-history.json", "bookmarks.json", "autofill.json", "passwords.json", "mods.json", "extension-settings.json"];
/// Folders copied recursively on first run (`notes` holds notes and Easels).
pub const IMPORT_DIRS: [&str; 1] = ["notes"];

/// Data folder of the Rust app (`<config dir>/Signet GPUI`), or `SIGNET_DATA_DIR` when set (tests).
pub fn data_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("SIGNET_DATA_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(v));
    }
    dirs::config_dir().map(|d| d.join(APP_DIR))
}

/// `userData` folder of the Electron app (`<config dir>/Signet`).
pub fn electron_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join(ELECTRON_DIR))
}

/// Folder of the CEF profile (`<data>/cef`).
pub fn cef_dir(data: &Path) -> PathBuf {
    data.join("cef")
}

/// What `import_from_electron` did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// True if the import ran (the Rust folder had no `state.json` yet).
    pub ran: bool,
    /// Names copied (files and folders).
    pub copied: Vec<String>,
    /// Names that could not be copied, with the error.
    pub failed: Vec<(String, String)>,
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&src, &dst)?;
        } else if kind.is_file() && !dst.exists() {
            fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// First run: if `data` has no `state.json` and `electron` exists, COPIES the Electron JSON files (and the notes
/// folder) into `data`. Never moves, never writes into `electron`, never overwrites a file already in `data`.
pub fn import_from_electron(electron: &Path, data: &Path) -> ImportReport {
    let mut report = ImportReport::default();
    if data.join("state.json").exists() || !electron.is_dir() {
        return report;
    }
    report.ran = true;
    if let Err(e) = fs::create_dir_all(data) {
        report.failed.push((data.display().to_string(), e.to_string()));
        return report;
    }
    for name in IMPORT_FILES {
        let (src, dst) = (electron.join(name), data.join(name));
        if !src.is_file() || dst.exists() {
            continue;
        }
        match fs::copy(&src, &dst) {
            Ok(_) => report.copied.push(name.to_string()),
            Err(e) => report.failed.push((name.to_string(), e.to_string())),
        }
    }
    for name in IMPORT_DIRS {
        let src = electron.join(name);
        if !src.is_dir() {
            continue;
        }
        match copy_dir(&src, &data.join(name)) {
            Ok(()) => report.copied.push(format!("{name}/")),
            Err(e) => report.failed.push((name.to_string(), e.to_string())),
        }
    }
    report
}

/// Whether the first-run import may run: only for the default data folder (no `SIGNET_DATA_DIR`), or when
/// `SIGNET_IMPORT_ELECTRON=1` opts in. A scratch folder (tests, captures) must never copy the user's real profile.
pub fn import_allowed_for(data_dir_env: Option<&std::ffi::OsStr>, opt_in: Option<&std::ffi::OsStr>) -> bool {
    data_dir_env.is_none_or(|v| v.is_empty()) || opt_in.is_some_and(|v| v == "1")
}

/// `import_allowed_for` with the process environment.
pub fn import_allowed() -> bool {
    import_allowed_for(std::env::var_os("SIGNET_DATA_DIR").as_deref(), std::env::var_os("SIGNET_IMPORT_ELECTRON").as_deref())
}

/// Resolves both folders for this user and runs the first-run import (see `import_allowed`). Returns the data folder to use.
pub fn prepare_data_dir() -> io::Result<(PathBuf, ImportReport)> {
    let data = data_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no config directory"))?;
    let report = match electron_dir() {
        Some(e) if import_allowed() => import_from_electron(&e, &data),
        _ => ImportReport::default(),
    };
    fs::create_dir_all(&data)?;
    Ok((data, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(dir: &Path) {
        fs::write(dir.join("state.json"), "{\"version\":1}").unwrap();
        fs::write(dir.join("history.json"), "[]").unwrap();
        fs::write(dir.join("Cookies"), "binary").unwrap();
        fs::create_dir_all(dir.join("notes").join("sub")).unwrap();
        fs::write(dir.join("notes").join("a.json"), "{}").unwrap();
        fs::write(dir.join("notes").join("sub").join("b.json"), "{}").unwrap();
    }

    #[test]
    fn copies_json_and_notes_only() {
        let e = tempfile::tempdir().unwrap();
        let d = tempfile::tempdir().unwrap();
        fill(e.path());
        let data = d.path().join("Signet GPUI");
        let r = import_from_electron(e.path(), &data);
        assert!(r.ran && r.failed.is_empty());
        assert!(data.join("state.json").is_file() && data.join("history.json").is_file());
        assert!(data.join("notes").join("sub").join("b.json").is_file());
        assert!(!data.join("Cookies").exists());
        // The source is untouched.
        assert!(e.path().join("state.json").is_file() && e.path().join("notes").join("a.json").is_file());
    }

    #[test]
    fn does_nothing_when_state_exists() {
        let e = tempfile::tempdir().unwrap();
        let d = tempfile::tempdir().unwrap();
        fill(e.path());
        fs::write(d.path().join("state.json"), "mine").unwrap();
        let r = import_from_electron(e.path(), d.path());
        assert!(!r.ran);
        assert_eq!(fs::read_to_string(d.path().join("state.json")).unwrap(), "mine");
        assert!(!d.path().join("history.json").exists());
    }

    #[test]
    fn does_nothing_without_electron_folder() {
        let d = tempfile::tempdir().unwrap();
        let r = import_from_electron(&d.path().join("absent"), &d.path().join("data"));
        assert!(!r.ran);
        assert!(!d.path().join("data").exists());
    }

    #[test]
    fn import_only_for_default_dir_or_opt_in() {
        use std::ffi::OsStr;
        assert!(import_allowed_for(None, None));
        assert!(import_allowed_for(Some(OsStr::new("")), None));
        assert!(!import_allowed_for(Some(OsStr::new("/tmp/x")), None));
        assert!(!import_allowed_for(Some(OsStr::new("/tmp/x")), Some(OsStr::new("0"))));
        assert!(import_allowed_for(Some(OsStr::new("/tmp/x")), Some(OsStr::new("1"))));
    }

    #[test]
    fn folder_names() {
        if let (Some(d), Some(e)) = (data_dir(), electron_dir()) {
            assert!(d.ends_with("Signet GPUI") && e.ends_with("Signet"));
            assert_eq!(d.parent(), e.parent());
        }
    }
}

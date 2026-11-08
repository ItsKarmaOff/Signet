//! Development harness: port of `app/desktop/src/main/dev/*` (sample state, options, screenshots), driven by the same
//! `SIGNET_*` variables (docs/DEVELOPMENT.md). Only active when `SIGNET_DATA_DIR` is set, like Electron needs
//! `--user-data-dir`: it can never touch a real profile.
//!
//! | Rust | TypeScript |
//! | --- | --- |
//! | `seed.rs` | `seed.ts` (sample state and history, forced theme) |
//! | `capture.rs` | `capture.ts` and `probes.ts` (options, eval, window capture) |

pub mod capture;
pub mod drive;
pub mod seed;

/// True when the harness may act: the data folder was given explicitly.
pub fn enabled() -> bool {
    std::env::var_os("SIGNET_DATA_DIR").is_some_and(|v| !v.is_empty())
}

/// Value of a `SIGNET_*` variable (non-empty), only when the harness is enabled.
pub fn var(name: &str) -> Option<String> {
    if !enabled() {
        return None;
    }
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `SIGNET_<name>=1`.
pub fn flag(name: &str) -> bool {
    var(name).as_deref() == Some("1")
}

/// Target of `SIGNET_CAPTURE`: `(target, section, file)`.
pub fn capture_spec() -> Option<(String, Option<String>, std::path::PathBuf)> {
    let spec = var("SIGNET_CAPTURE")?;
    let idx = spec.find(":/").or_else(|| spec.find(":\\"))?;
    let (head, file) = (&spec[..idx], &spec[idx + 1..]);
    if !file.ends_with(".png") || !std::path::Path::new(file).is_absolute() {
        return None;
    }
    let mut parts = head.splitn(2, ':');
    let target = parts.next()?.to_string();
    let section = parts.next().map(str::to_string).filter(|s| !s.is_empty());
    const TARGETS: [&str; 10] = ["main", "settings", "library", "little", "private", "palette", "find", "welcome", "passkey", "boost"];
    TARGETS.contains(&target.as_str()).then(|| (target, section, file.into()))
}

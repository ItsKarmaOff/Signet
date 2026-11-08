//! Updates (port of `main/features/updates.ts` and of the parts of electron-updater it uses): the GitHub releases of
//! the public repository are the update feed. A newer version is found in the releases list; on Windows (NSIS) and
//! Linux (AppImage) the `latest*.yml` metadata of the release names the file to download (sha512 and size are
//! checked), elsewhere the app only opens the release page.
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use sha2::{Digest, Sha512};

use serde_json::Value;

use crate::http::Http;

/// Public repository: its GitHub releases are the update feed.
pub const REPO_URL: &str = "https://github.com/ItsKarmaOff/Signet";
pub const RELEASES_API: &str = "https://api.github.com/repos/ItsKarmaOff/Signet/releases";
/// First automatic check a little after launch, then every 6 hours.
pub const FIRST_CHECK: Duration = Duration::from_secs(30);
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// TEST ONLY (end-to-end update test in CI): with `SIGNET_UPDATE_TEST=1` set, `SIGNET_UPDATE_FEED=<base url>` replaces the
/// GitHub releases API with `<base url>/releases`, and the first check runs 2 s after launch. Without
/// `SIGNET_UPDATE_TEST=1` the variables are ignored.
pub fn test_mode() -> bool {
    std::env::var("SIGNET_UPDATE_TEST").is_ok_and(|v| v == "1")
}

/// URL of the releases list: GitHub's, or the test override (see `test_mode`).
pub fn releases_api() -> String {
    if test_mode()
        && let Ok(base) = std::env::var("SIGNET_UPDATE_FEED")
        && !base.is_empty()
    {
        return format!("{}/releases", base.trim_end_matches('/'));
    }
    RELEASES_API.to_string()
}

/// Delay of the first automatic check after launch.
pub fn first_check() -> Duration {
    if test_mode() { Duration::from_secs(2) } else { FIRST_CHECK }
}

/// Whether this copy can download and install an update by itself: Windows (installer) and Linux AppImage.
pub fn can_install(os: crate::importers::sources::Os, appimage_env: Option<&str>) -> bool {
    use crate::importers::sources::Os;
    os == Os::Windows || (os == Os::Linux && appimage_env.is_some_and(|a| !a.is_empty()))
}

/// How this copy gets updated, like Electron's `canInstall` plus the package kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// Windows installed by the NSIS installer: the downloaded setup runs silently.
    Nsis,
    /// Linux AppImage: the file at `$APPIMAGE` is replaced.
    AppImage(PathBuf),
    /// macOS, deb/rpm, unpacked folder: only the release page is opened.
    Manual,
}

/// Detects the package kind. `nsis_installed`: an `Uninstall *.exe` sits next to the executable (written by the
/// NSIS installer, absent from a raw folder).
pub fn install_kind(os: crate::importers::sources::Os, appimage_env: Option<&str>, nsis_installed: bool) -> InstallKind {
    use crate::importers::sources::Os;
    match os {
        Os::Windows if nsis_installed => InstallKind::Nsis,
        Os::Linux => match appimage_env {
            Some(a) if !a.is_empty() => InstallKind::AppImage(PathBuf::from(a)),
            _ => InstallKind::Manual,
        },
        _ => InstallKind::Manual,
    }
}

/// True when the folder holds the NSIS uninstaller (`Uninstall Signet.exe`).
pub fn has_nsis_uninstaller(dir: &Path) -> bool {
    std::fs::read_dir(dir).map(|rd| rd.flatten().any(|e| {
        let n = e.file_name().to_string_lossy().to_lowercase();
        n.starts_with("uninstall") && n.ends_with(".exe")
    })).unwrap_or(false)
}

/// Name of the metadata file in a release, as electron-updater picks it.
pub fn feed_name(os: crate::importers::sources::Os, arm64: bool) -> &'static str {
    use crate::importers::sources::Os;
    match (os, arm64) {
        (Os::Linux, true) => "latest-linux-arm64.yml",
        (Os::Linux, false) => "latest-linux.yml",
        (Os::Mac, _) => "latest-mac.yml",
        (Os::Windows, _) => "latest.yml",
    }
}

/// One entry of the `files` list of a `latest*.yml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedFile {
    pub url: String,
    /// Base64 sha512.
    pub sha512: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Feed {
    pub version: String,
    pub files: Vec<FeedFile>,
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    let b = v.as_bytes();
    if b.len() >= 2 && ((b[0] == b'\'' && b[b.len() - 1] == b'\'') || (b[0] == b'"' && b[b.len() - 1] == b'"')) {
        return v[1..v.len() - 1].to_string();
    }
    v.to_string()
}

/// Parses the small subset of YAML electron-builder writes (`version`, `files` list, `path`, `sha512`).
pub fn parse_feed(text: &str) -> Option<Feed> {
    let mut version = None;
    let (mut path, mut sha) = (None, None);
    let mut files: Vec<FeedFile> = Vec::new();
    let mut cur: Option<(Option<String>, Option<String>, Option<u64>)> = None;
    let mut in_files = false;
    let flush = |cur: &mut Option<(Option<String>, Option<String>, Option<u64>)>, files: &mut Vec<FeedFile>| {
        if let Some((Some(url), Some(sha512), size)) = cur.take() {
            files.push(FeedFile { url, sha512, size });
        }
    };
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && !line.starts_with('-') {
            flush(&mut cur, &mut files);
            let (k, v) = line.split_once(':')?;
            in_files = k == "files";
            match k {
                "version" => version = Some(unquote(v)),
                "path" => path = Some(unquote(v)),
                "sha512" => sha = Some(unquote(v)),
                _ => {}
            }
            continue;
        }
        if !in_files {
            continue;
        }
        let mut body = line.trim_start();
        if let Some(rest) = body.strip_prefix("- ") {
            flush(&mut cur, &mut files);
            cur = Some((None, None, None));
            body = rest;
        }
        let Some(item) = cur.as_mut() else { continue };
        if let Some((k, v)) = body.split_once(':') {
            match k.trim() {
                "url" => item.0 = Some(unquote(v)),
                "sha512" => item.1 = Some(unquote(v)),
                "size" => item.2 = unquote(v).parse().ok(),
                _ => {}
            }
        }
    }
    flush(&mut cur, &mut files);
    // Old metadata without a `files` list: the top-level path and hash.
    if files.is_empty()
        && let (Some(url), Some(sha512)) = (path, sha)
    {
        files.push(FeedFile { url, sha512, size: None });
    }
    Some(Feed { version: version?, files })
}

/// The file to download for this package kind and processor, like electron-updater (`.exe` / `.AppImage`, the one
/// built for this architecture).
pub fn pick_file<'a>(feed: &'a Feed, kind: &InstallKind, arm64: bool) -> Option<&'a FeedFile> {
    let ext = match kind {
        InstallKind::Nsis => ".exe",
        InstallKind::AppImage(_) => ".appimage",
        InstallKind::Manual => return None,
    };
    let mine: &[&str] = if arm64 { &["arm64", "aarch64"] } else { &["x64", "x86_64", "amd64"] };
    let other: &[&str] = if arm64 { &["x64", "x86_64", "amd64"] } else { &["arm64", "aarch64"] };
    let has = |f: &FeedFile, toks: &[&str]| {
        let n = f.url.to_lowercase();
        toks.iter().any(|t| n.contains(t))
    };
    let cands: Vec<&FeedFile> = feed.files.iter().filter(|f| f.url.to_lowercase().ends_with(ext) && !has(f, other)).collect();
    cands.iter().find(|f| has(f, mine)).or(cands.first()).copied()
}

/// A file to download and check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub url: String,
    pub sha512: String,
    pub size: Option<u64>,
}

/// Fetches the release's metadata and resolves the file to download. Asset URLs come from the release's asset list
/// (the metadata only holds file names).
pub fn resolve_package(http: &dyn Http, rel: &Release, os: crate::importers::sources::Os, arm64: bool, kind: &InstallKind) -> Result<Package, String> {
    let feed_url = rel.assets.iter().find(|(n, _)| n == feed_name(os, arm64)).map(|(_, u)| u.clone()).ok_or("no update metadata in the release")?;
    let r = http.get(&feed_url, &[("user-agent", "Signet-update-check")], Duration::from_secs(15), 256 * 1024)?;
    if !r.ok() {
        return Err(format!("metadata: HTTP {}", r.status));
    }
    let feed = parse_feed(&r.text()).ok_or("bad update metadata")?;
    let file = pick_file(&feed, kind, arm64).ok_or("no matching file in the update metadata")?;
    let name = file.url.rsplit('/').next().unwrap_or(&file.url).to_string();
    let url = rel.assets.iter().find(|(n, _)| *n == name).map(|(_, u)| u.clone()).unwrap_or_else(|| format!("{REPO_URL}/releases/download/v{}/{}", rel.version, name));
    Ok(Package { name, url, sha512: file.sha512.clone(), size: file.size })
}

/// Base64 sha512 of a file.
pub fn sha512_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha512::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(h.finalize()))
}

/// Downloads a package to `dest` and checks its size and sha512; a file that fails the check is deleted.
pub fn download_package(http: &dyn Http, pkg: &Package, dest: &Path) -> Result<(), String> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(dest);
    let result = (|| {
        let status = http.download(&pkg.url, dest)?;
        if !(200..300).contains(&status) {
            return Err(format!("download: HTTP {status}"));
        }
        let len = std::fs::metadata(dest).map_err(|e| e.to_string())?.len();
        if pkg.size.is_some_and(|s| s != len) {
            return Err(format!("size mismatch: {len} bytes"));
        }
        if sha512_file(dest).map_err(|e| e.to_string())? != pkg.sha512 {
            return Err("sha512 mismatch".into());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

/// Arguments electron-updater's NsisUpdater gives the installer.
pub fn nsis_args(force_run: bool) -> Vec<&'static str> {
    let mut a = vec!["--updated", "/S"];
    if force_run {
        a.push("--force-run");
    }
    a
}

/// Linux: marks the downloaded AppImage executable and renames it over the running one (atomic; the running
/// process keeps its old inode).
pub fn replace_appimage(downloaded: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(downloaded, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(downloaded, target)
}

/// Release page of a version.
pub fn release_url(version: &str) -> String {
    format!("{REPO_URL}/releases/tag/v{}", version.trim_start_matches('v'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub core: [u64; 3],
    /// Pre-release identifiers (empty for a stable release).
    pub pre: Vec<String>,
}

pub fn parse_version(v: &str) -> Option<Version> {
    let v = v.trim().trim_start_matches('v');
    let v = v.split('+').next().unwrap_or("");
    let (core, pre) = match v.split_once('-') { Some((c, p)) => (c, p), None => (v, "") };
    let mut it = core.split('.');
    let mut parts = [0u64; 3];
    for p in parts.iter_mut() {
        *p = it.next()?.parse().ok()?;
    }
    if it.next().is_some() {
        return None;
    }
    Some(Version { core: parts, pre: if pre.is_empty() { vec![] } else { pre.split('.').map(String::from).collect() } })
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match self.core.cmp(&o.core) {
            Equal => {}
            x => return x,
        }
        match (self.pre.is_empty(), o.pre.is_empty()) {
            (true, true) => return Equal,
            (true, false) => return Greater,
            (false, true) => return Less,
            _ => {}
        }
        for (a, b) in self.pre.iter().zip(o.pre.iter()) {
            let ord = match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(x), Ok(y)) => x.cmp(&y),
                (Ok(_), Err(_)) => Less,
                (Err(_), Ok(_)) => Greater,
                _ => a.cmp(b),
            };
            if ord != Equal {
                return ord;
            }
        }
        self.pre.len().cmp(&o.pre.len())
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

/// True if `candidate` is strictly newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse_version(candidate), parse_version(current)), (Some(a), Some(b)) if a > b)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    /// Downloadable assets: (name, url).
    pub assets: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckResult {
    UpToDate,
    Available(Release),
    Failed,
}

/// Newest non-draft release from the GitHub releases list (pre-releases only if `prerelease_ok`).
pub fn newest_release(data: &Value, prerelease_ok: bool) -> Option<Release> {
    let mut best: Option<(Version, Release)> = None;
    for r in data.as_array()? {
        if r.get("draft").and_then(|d| d.as_bool()) == Some(true) || (!prerelease_ok && r.get("prerelease").and_then(|d| d.as_bool()) == Some(true)) {
            continue;
        }
        let tag = r.get("tag_name").and_then(|t| t.as_str()).unwrap_or("");
        let Some(v) = parse_version(tag) else { continue };
        if best.as_ref().is_some_and(|(b, _)| *b >= v) {
            continue;
        }
        let assets = r.get("assets").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| Some((x.get("name")?.as_str()?.to_string(), x.get("browser_download_url")?.as_str()?.to_string()))).collect()).unwrap_or_default();
        let version = tag.trim_start_matches('v').to_string();
        best = Some((v, Release { url: release_url(&version), version, assets }));
    }
    best.map(|(_, r)| r)
}

/// Looks for a newer release than `current`.
pub fn check(http: &dyn Http, current: &str) -> CheckResult {
    let Ok(r) = http.get(&format!("{}?per_page=20", releases_api()), &[("accept", "application/vnd.github+json"), ("user-agent", "Signet-update-check")], Duration::from_secs(15), 4 * 1024 * 1024) else { return CheckResult::Failed };
    if !r.ok() {
        return CheckResult::Failed;
    }
    let Ok(data) = serde_json::from_slice::<Value>(&r.body) else { return CheckResult::Failed };
    let prerelease_ok = parse_version(current).is_some_and(|v| !v.pre.is_empty());
    match newest_release(&data, prerelease_ok) {
        Some(rel) if is_newer(&rel.version, current) => CheckResult::Available(rel),
        Some(_) | None => CheckResult::UpToDate,
    }
}

/// What the app should do after a check.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Nothing,
    /// Manual check, nothing new.
    ShowUpToDate,
    ShowFailed,
    /// Offer this version (download or open the release page).
    Offer(Release),
}

/// Session bookkeeping: a version already offered is not offered again by automatic checks.
#[derive(Default)]
pub struct Tracker {
    pub checking: bool,
    pub downloading: Option<String>,
    pub downloaded: Option<String>,
    offered: Option<String>,
}

impl Tracker {
    /// Starts a check; false when one is already running or a download is in progress.
    pub fn begin(&mut self) -> bool {
        if self.checking || self.downloading.is_some() || self.downloaded.is_some() {
            return false;
        }
        self.checking = true;
        true
    }

    /// Forgets the offered version (a failed download is offered again by the next automatic check).
    pub fn forget_offer(&mut self) {
        self.offered = None;
    }

    pub fn finish(&mut self, result: CheckResult, manual: bool) -> Outcome {
        self.checking = false;
        match result {
            CheckResult::UpToDate => if manual { Outcome::ShowUpToDate } else { Outcome::Nothing },
            CheckResult::Failed => {
                self.offered = None;
                if manual { Outcome::ShowFailed } else { Outcome::Nothing }
            }
            CheckResult::Available(rel) => {
                if !manual && self.offered.as_deref() == Some(rel.version.as_str()) {
                    return Outcome::Nothing;
                }
                self.offered = Some(rel.version.clone());
                Outcome::Offer(rel)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{resp, FakeHttp};
    use crate::importers::sources::Os;
    use serde_json::json;

    #[test]
    fn install_capability() {
        assert!(can_install(Os::Windows, None));
        assert!(can_install(Os::Linux, Some("/x.AppImage")));
        assert!(!can_install(Os::Linux, None));
        assert!(!can_install(Os::Linux, Some("")));
        assert!(!can_install(Os::Mac, Some("/x")));
    }

    #[test]
    fn version_order() {
        assert!(is_newer("0.5.1", "0.5.0") && is_newer("v1.0.0", "0.9.9") && is_newer("0.10.0", "0.9.0"));
        assert!(!is_newer("0.5.0", "0.5.0") && !is_newer("0.4.9", "0.5.0"));
        assert!(is_newer("1.0.0", "1.0.0-beta.2") && !is_newer("1.0.0-beta.1", "1.0.0"));
        assert!(is_newer("1.0.0-beta.10", "1.0.0-beta.2") && is_newer("1.0.0-rc.1", "1.0.0-beta.9"));
        assert!(!is_newer("garbage", "1.0.0") && !is_newer("1.0.0", "garbage"));
        assert_eq!(release_url("v1.2.3"), format!("{REPO_URL}/releases/tag/v1.2.3"));
        assert!(parse_version("1.2").is_none() && parse_version("1.2.3.4").is_none());
    }

    fn releases() -> Value {
        json!([
            {"tag_name":"v0.6.0-beta.1","prerelease":true,"assets":[]},
            {"tag_name":"v0.5.1","assets":[{"name":"Signet.AppImage","browser_download_url":"https://x/a"}]},
            {"tag_name":"v0.9.0","draft":true},
            {"tag_name":"v0.5.0","assets":[]},
            {"tag_name":"nonsense"}
        ])
    }

    #[test]
    fn newest_release_rules() {
        let r = newest_release(&releases(), false).unwrap();
        assert_eq!((r.version.as_str(), r.assets.len()), ("0.5.1", 1));
        assert_eq!(newest_release(&releases(), true).unwrap().version, "0.6.0-beta.1");
        assert!(newest_release(&json!({}), false).is_none());
    }

    #[test]
    fn check_results() {
        let http = FakeHttp(Box::new(|_| resp(200, &releases().to_string())));
        assert!(matches!(check(&http, "0.5.0"), CheckResult::Available(r) if r.version == "0.5.1"));
        assert_eq!(check(&http, "0.5.1"), CheckResult::UpToDate);
        assert!(matches!(check(&http, "0.5.2-beta.1"), CheckResult::Available(r) if r.version == "0.6.0-beta.1"));
        assert_eq!(check(&FakeHttp(Box::new(|_| resp(403, ""))), "0.5.0"), CheckResult::Failed);
        assert_eq!(check(&FakeHttp(Box::new(|_| resp(200, "not json"))), "0.5.0"), CheckResult::Failed);
        assert_eq!(check(&FakeHttp(Box::new(|_| Err("offline".into()))), "0.5.0"), CheckResult::Failed);
    }

    const WIN: &str = "version: 0.5.0\nfiles:\n  - url: Signet-0.5.0-x64-setup.exe\n    sha512: hZ+dhDMaUh2K3fpbC4ujZ8B25UfoGzqUBYAUrRbWRalXxrsROGtVr+3C065kD/qeMD3I2KbgFw33ZuCKtFxCJA==\n    size: 117305429\npath: Signet-0.5.0-x64-setup.exe\nsha512: hZ+dhDMaUh2K3fpbC4ujZ8B25UfoGzqUBYAUrRbWRalXxrsROGtVr+3C065kD/qeMD3I2KbgFw33ZuCKtFxCJA==\nreleaseDate: '2026-11-02T08:10:55.765Z'\n";
    const LINUX_ARM: &str = "version: 0.5.0\nfiles:\n  - url: Signet-0.5.0-arm64.AppImage\n    sha512: obWVI2qP==\n    size: 130348968\n    blockMapSize: 135924\n  - url: Signet-0.5.0-arm64.deb\n    sha512: emQL==\n    size: 99587116\n  - url: Signet-0.5.0-aarch64.rpm\n    sha512: jtGP==\n    size: 88605808\npath: Signet-0.5.0-arm64.AppImage\nsha512: obWVI2qP==\n";
    const LINUX_X64: &str = "version: 0.5.0\nfiles:\n  - url: Signet-0.5.0-x86_64.AppImage\n    sha512: AfPo==\n    size: 129138074\n  - url: Signet-0.5.0-amd64.deb\n    sha512: rBVx==\n    size: 104695776\n  - url: Signet-0.5.0-x86_64.rpm\n    sha512: D4wt==\n    size: 93309464\n  - url: Signet-0.5.0-arm64.AppImage\n    sha512: zzzz==\n    size: 1\npath: Signet-0.5.0-x86_64.AppImage\n";

    #[test]
    fn parses_real_metadata() {
        let f = parse_feed(WIN).unwrap();
        assert_eq!(f.version, "0.5.0");
        assert_eq!(f.files.len(), 1);
        assert_eq!((f.files[0].url.as_str(), f.files[0].size), ("Signet-0.5.0-x64-setup.exe", Some(117305429)));
        assert!(f.files[0].sha512.ends_with("CJA=="));
        assert_eq!(parse_feed(LINUX_ARM).unwrap().files.len(), 3);
        // Old shape: no files list.
        let old = parse_feed("version: '1.0.0'\npath: a.exe\nsha512: abc\n").unwrap();
        assert_eq!((old.version.as_str(), old.files.len()), ("1.0.0", 1));
        assert!(parse_feed("files:\n  - url: a\n").is_none());
    }

    #[test]
    fn picks_the_right_file() {
        let app = InstallKind::AppImage(PathBuf::from("/x.AppImage"));
        let w = parse_feed(WIN).unwrap();
        assert_eq!(pick_file(&w, &InstallKind::Nsis, false).unwrap().url, "Signet-0.5.0-x64-setup.exe");
        assert!(pick_file(&w, &InstallKind::Nsis, true).is_none());
        let a = parse_feed(LINUX_ARM).unwrap();
        assert_eq!(pick_file(&a, &app, true).unwrap().url, "Signet-0.5.0-arm64.AppImage");
        assert!(pick_file(&a, &app, false).is_none());
        let x = parse_feed(LINUX_X64).unwrap();
        assert_eq!(pick_file(&x, &app, false).unwrap().url, "Signet-0.5.0-x86_64.AppImage");
        assert!(pick_file(&x, &InstallKind::Manual, false).is_none());
    }

    #[test]
    fn feed_names_and_kinds() {
        use crate::importers::sources::Os;
        assert_eq!(feed_name(Os::Windows, false), "latest.yml");
        assert_eq!(feed_name(Os::Linux, false), "latest-linux.yml");
        assert_eq!(feed_name(Os::Linux, true), "latest-linux-arm64.yml");
        assert_eq!(feed_name(Os::Mac, true), "latest-mac.yml");
        assert_eq!(install_kind(Os::Windows, None, true), InstallKind::Nsis);
        assert_eq!(install_kind(Os::Windows, None, false), InstallKind::Manual);
        assert_eq!(install_kind(Os::Linux, Some("/a.AppImage"), false), InstallKind::AppImage("/a.AppImage".into()));
        assert_eq!(install_kind(Os::Linux, Some(""), false), InstallKind::Manual);
        assert_eq!(install_kind(Os::Mac, Some("/a"), true), InstallKind::Manual);
        assert_eq!(nsis_args(true), ["--updated", "/S", "--force-run"]);
        assert_eq!(nsis_args(false), ["--updated", "/S"]);
    }

    #[test]
    fn nsis_uninstaller_detection() {
        let d = tempfile::tempdir().unwrap();
        assert!(!has_nsis_uninstaller(d.path()));
        std::fs::write(d.path().join("Uninstall Signet.exe"), b"x").unwrap();
        assert!(has_nsis_uninstaller(d.path()));
    }

    /// Serves `routes` (path -> body) on a local port, for ever; returns the base URL.
    fn serve(routes: Vec<(String, Vec<u8>)>) -> String {
        use std::io::{BufRead, BufReader, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in l.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                let _ = reader.read_line(&mut first);
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap_or(0) <= 2 {
                        break;
                    }
                }
                let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
                let mut s = stream;
                match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => {
                        let _ = write!(s, "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len());
                        let _ = s.write_all(body);
                    }
                    None => {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                    }
                }
            }
        });
        base
    }

    fn sha_of(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    }

    /// End to end against a local server: releases list, metadata, download and sha512 check (nothing is installed).
    #[test]
    fn feed_download_and_verify_end_to_end() {
        use crate::http::UreqHttp;
        use crate::importers::sources::Os;
        let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let (os, name, kind) = if cfg!(windows) {
            (Os::Windows, "Signet-9.9.9-x64-setup.exe", InstallKind::Nsis)
        } else {
            (Os::Linux, "Signet-9.9.9-x86_64.AppImage", InstallKind::AppImage("/tmp/x.AppImage".into()))
        };
        let feed = |sha: &str| format!("version: 9.9.9\nfiles:\n  - url: {name}\n    sha512: {sha}\n    size: {}\npath: {name}\nsha512: {sha}\n", payload.len());
        // The metadata names the asset; the asset list gives the URLs.
        let base = serve(vec![("/feed".into(), feed(&sha_of(&payload)).into_bytes()), ("/file".into(), payload.clone())]);
        let rel = Release { version: "9.9.9".into(), url: release_url("9.9.9"), assets: vec![(feed_name(os, false).to_string(), format!("{base}/feed")), (name.to_string(), format!("{base}/file"))] };
        let pkg = resolve_package(&UreqHttp, &rel, os, false, &kind).unwrap();
        assert_eq!((pkg.name.as_str(), pkg.url.as_str()), (name, format!("{base}/file").as_str()));
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join(name);
        download_package(&UreqHttp, &pkg, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), payload);
        // A wrong hash and a wrong size are refused and the file is deleted.
        let bad = Package { sha512: sha_of(b"other"), ..pkg.clone() };
        assert!(download_package(&UreqHttp, &bad, &dest).unwrap_err().contains("sha512"));
        assert!(!dest.exists());
        let bad = Package { size: Some(5), ..pkg.clone() };
        assert!(download_package(&UreqHttp, &bad, &dest).unwrap_err().contains("size"));
        assert!(!dest.exists());
        let missing = Package { url: format!("{base}/nope"), ..pkg };
        assert!(download_package(&UreqHttp, &missing, &dest).unwrap_err().contains("404"));
    }

    #[cfg(unix)]
    #[test]
    fn appimage_replacement() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let (cur, new) = (d.path().join("Signet.AppImage"), d.path().join("Signet.AppImage.new"));
        std::fs::write(&cur, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        replace_appimage(&new, &cur).unwrap();
        assert_eq!(std::fs::read(&cur).unwrap(), b"new");
        assert!(!new.exists());
        assert_eq!(std::fs::metadata(&cur).unwrap().permissions().mode() & 0o111, 0o111);
    }

    #[test]
    fn tracker_offers_once_per_session() {
        let rel = || Release { version: "1.1.0".into(), url: release_url("1.1.0"), assets: vec![] };
        let mut t = Tracker::default();
        assert!(t.begin());
        assert!(!t.begin());
        assert_eq!(t.finish(CheckResult::Available(rel()), false), Outcome::Offer(rel()));
        assert!(t.begin());
        assert_eq!(t.finish(CheckResult::Available(rel()), false), Outcome::Nothing);
        assert!(t.begin());
        assert_eq!(t.finish(CheckResult::Available(rel()), true), Outcome::Offer(rel()));
        assert!(t.begin());
        assert_eq!(t.finish(CheckResult::UpToDate, true), Outcome::ShowUpToDate);
        assert!(t.begin());
        assert_eq!(t.finish(CheckResult::UpToDate, false), Outcome::Nothing);
        assert!(t.begin());
        assert_eq!(t.finish(CheckResult::Failed, true), Outcome::ShowFailed);
        // After a failure the version is offered again by the next automatic check.
        assert!(t.begin());
        assert!(matches!(t.finish(CheckResult::Available(rel()), false), Outcome::Offer(_)));
        t.downloading = Some("1.1.0".into());
        assert!(!t.begin());
    }
}

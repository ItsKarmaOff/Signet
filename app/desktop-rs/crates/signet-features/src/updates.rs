//! Updates (port of the check logic of `main/features/updates.ts`): the GitHub releases of the public repository
//! are the update feed. electron-updater is not used; the Rust app only learns that a newer version exists and
//! opens the release page (macOS) or downloads the asset itself (Windows installer, Linux AppImage) if it wants.
use std::time::Duration;

use serde_json::Value;

use crate::http::Http;

/// Public repository: its GitHub releases are the update feed.
pub const REPO_URL: &str = "https://github.com/ItsKarmaOff/Signet";
pub const RELEASES_API: &str = "https://api.github.com/repos/ItsKarmaOff/Signet/releases";
/// First automatic check a little after launch, then every 6 hours.
pub const FIRST_CHECK: Duration = Duration::from_secs(30);
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// Whether this copy can download and install an update by itself: Windows (installer) and Linux AppImage.
pub fn can_install(os: crate::importers::sources::Os, appimage_env: Option<&str>) -> bool {
    use crate::importers::sources::Os;
    os == Os::Windows || (os == Os::Linux && appimage_env.is_some_and(|a| !a.is_empty()))
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
    let Ok(r) = http.get(&format!("{RELEASES_API}?per_page=20"), &[("accept", "application/vnd.github+json"), ("user-agent", "Signet-update-check")], Duration::from_secs(15), 4 * 1024 * 1024) else { return CheckResult::Failed };
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

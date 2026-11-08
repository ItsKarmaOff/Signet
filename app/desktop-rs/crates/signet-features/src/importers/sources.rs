//! Browsers that can be imported from, their locations per platform and the data they offer
//! (port of the discovery part of `importers/index.ts`).
use std::fs;
use std::path::{Path, PathBuf};

use super::chromium::{profile_has, KeyNames};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImportSource {
    Arc,
    Chrome,
    Chromium,
    Brave,
    Edge,
    Safari,
}

impl ImportSource {
    pub fn id(self) -> &'static str {
        match self { Self::Arc => "arc", Self::Chrome => "chrome", Self::Chromium => "chromium", Self::Brave => "brave", Self::Edge => "edge", Self::Safari => "safari" }
    }
    pub fn from_id(id: &str) -> Option<Self> {
        ALL.iter().map(|d| d.id).find(|s| s.id() == id)
    }
}

/// Kinds of data, in display order (`IMPORT_KINDS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ImportKind {
    Spaces,
    Bookmarks,
    Passwords,
    Extensions,
    History,
}

impl ImportKind {
    pub fn id(self) -> &'static str {
        match self { Self::Spaces => "spaces", Self::Bookmarks => "bookmarks", Self::Passwords => "passwords", Self::Extensions => "extensions", Self::History => "history" }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Linux,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") { Os::Mac } else if cfg!(target_os = "windows") { Os::Windows } else { Os::Linux }
    }
}

/// Environment the locations depend on (injectable for tests).
#[derive(Debug, Clone)]
pub struct Env {
    pub os: Os,
    pub home: PathBuf,
    pub local_app_data: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
}

impl Env {
    pub fn current() -> Env {
        Env {
            os: Os::current(),
            home: dirs_home(),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()),
        }
    }
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

pub struct SourceDef {
    pub id: ImportSource,
    pub name: &'static str,
    /// Key of the action label.
    pub title_key: &'static str,
    pub on: &'static [Os],
    pub keys: Option<KeyNames>,
}

const EVERYWHERE: &[Os] = &[Os::Mac, Os::Linux, Os::Windows];

pub const ALL: &[SourceDef] = &[
    SourceDef { id: ImportSource::Arc, name: "Arc", title_key: "import.fromApp", on: &[Os::Mac, Os::Windows], keys: Some(KeyNames { mac: "Arc Safe Storage", linux: &[] }) },
    SourceDef { id: ImportSource::Chrome, name: "Chrome", title_key: "import.bookmarks", on: EVERYWHERE, keys: Some(KeyNames { mac: "Chrome Safe Storage", linux: &["chrome"] }) },
    SourceDef { id: ImportSource::Chromium, name: "Chromium", title_key: "import.bookmarks", on: EVERYWHERE, keys: Some(KeyNames { mac: "Chromium Safe Storage", linux: &["chromium"] }) },
    SourceDef { id: ImportSource::Brave, name: "Brave", title_key: "import.bookmarks", on: EVERYWHERE, keys: Some(KeyNames { mac: "Brave Safe Storage", linux: &["brave"] }) },
    SourceDef { id: ImportSource::Edge, name: "Edge", title_key: "import.bookmarksElided", on: EVERYWHERE, keys: Some(KeyNames { mac: "Microsoft Edge Safe Storage", linux: &["microsoft-edge", "Microsoft Edge"] }) },
    SourceDef { id: ImportSource::Safari, name: "Safari", title_key: "import.bookmarks", on: &[Os::Mac], keys: None },
];

pub fn def(id: ImportSource) -> &'static SourceDef {
    ALL.iter().find(|d| d.id == id).unwrap_or(&ALL[0])
}

/// Sources that exist on this system.
pub fn sources_for(os: Os) -> Vec<&'static SourceDef> {
    ALL.iter().filter(|s| s.on.contains(&os)).collect()
}

/// Display name used for a bookmarks folder (Chromium family only).
pub fn bookmark_folder_name(id: ImportSource) -> Option<&'static str> {
    match id { ImportSource::Chrome => Some("Chrome"), ImportSource::Chromium => Some("Chromium"), ImportSource::Brave => Some("Brave"), ImportSource::Edge => Some("Edge"), _ => None }
}

fn chromium_paths(env: &Env, mac: &str, linux: &[&str], win: &str) -> Vec<PathBuf> {
    match env.os {
        Os::Mac => vec![env.home.join("Library/Application Support").join(mac).join("Default")],
        Os::Windows => vec![env.local_app_data.clone().unwrap_or_else(|| env.home.join("AppData/Local")).join(win).join("User Data/Default")],
        Os::Linux => {
            let config = env.xdg_config_home.clone().unwrap_or_else(|| env.home.join(".config"));
            linux.iter().map(|d| match d.strip_prefix("~/") { Some(rest) => env.home.join(rest), None => config.join(d).join("Default") }).collect()
        }
    }
}

/// Possible locations, in order: the sidebar file (Arc), the bookmarks file (Safari) or the profile folder.
pub fn paths(id: ImportSource, env: &Env) -> Vec<PathBuf> {
    match id {
        ImportSource::Arc => {
            if env.os == Os::Windows {
                let pk = env.local_app_data.clone().unwrap_or_else(|| env.home.join("AppData/Local")).join("Packages");
                fs::read_dir(&pk)
                    .map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("TheBrowserCompany.Arc")).map(|e| e.path().join("LocalCache/Local/Arc/StorableSidebar.json")).collect())
                    .unwrap_or_default()
            } else {
                vec![env.home.join("Library/Application Support/Arc/StorableSidebar.json")]
            }
        }
        ImportSource::Chrome => chromium_paths(env, "Google/Chrome", &["google-chrome", "~/.var/app/com.google.Chrome/config/google-chrome/Default"], "Google/Chrome"),
        ImportSource::Chromium => chromium_paths(env, "Chromium", &["chromium", "~/.var/app/org.chromium.Chromium/config/chromium/Default", "~/snap/chromium/common/chromium/Default"], "Chromium"),
        ImportSource::Brave => chromium_paths(env, "BraveSoftware/Brave-Browser", &["BraveSoftware/Brave-Browser", "~/.var/app/com.brave.Browser/config/BraveSoftware/Brave-Browser/Default"], "BraveSoftware/Brave-Browser"),
        ImportSource::Edge => chromium_paths(env, "Microsoft Edge", &["microsoft-edge", "~/.var/app/com.microsoft.Edge/config/microsoft-edge/Default"], "Microsoft/Edge"),
        ImportSource::Safari => vec![env.home.join("Library/Safari/Bookmarks.plist")],
    }
}

/// Arc keeps its Chromium profile next to the sidebar file.
pub fn profile_of_located(id: ImportSource, located: &Path) -> PathBuf {
    if id == ImportSource::Arc { located.parent().unwrap_or(located).join("User Data/Default") } else { located.to_path_buf() }
}

/// First location present; a permission denial counts as present.
pub fn locate(id: ImportSource, env: &Env) -> Option<PathBuf> {
    paths(id, env).into_iter().find(|p| match fs::metadata(p) {
        Ok(_) => true,
        Err(e) => e.kind() == std::io::ErrorKind::PermissionDenied,
    })
}

/// Chromium profile of a source, if it exists.
pub fn profile_of(id: ImportSource, located: Option<&Path>) -> Option<PathBuf> {
    let located = located?;
    def(id).keys.as_ref()?;
    let dir = profile_of_located(id, located);
    dir.exists().then_some(dir)
}

/// Data the source can provide, in display order.
pub fn kinds_of(id: ImportSource, env: &Env) -> Vec<ImportKind> {
    let Some(located) = locate(id, env) else { return vec![] };
    let mut found = std::collections::BTreeSet::new();
    if id == ImportSource::Arc { found.insert(ImportKind::Spaces); }
    if id == ImportSource::Safari { found.insert(ImportKind::Bookmarks); }
    if let Some(profile) = profile_of(id, Some(&located)) {
        if id != ImportSource::Arc && profile.join("Bookmarks").exists() { found.insert(ImportKind::Bookmarks); }
        let has = profile_has(&profile);
        if has.history { found.insert(ImportKind::History); }
        if has.passwords { found.insert(ImportKind::Passwords); }
        if has.extensions { found.insert(ImportKind::Extensions); }
    }
    found.into_iter().collect()
}

/// Sources present on disk with the data they offer.
pub fn available_sources(env: &Env) -> Vec<(ImportSource, Vec<ImportKind>)> {
    sources_for(env.os).into_iter().map(|d| (d.id, kinds_of(d.id, env))).filter(|(_, k)| !k.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(os: Os, home: &Path) -> Env {
        Env { os, home: home.to_path_buf(), local_app_data: Some(home.join("Local")), xdg_config_home: None }
    }

    #[test]
    fn platform_sources() {
        assert!(sources_for(Os::Linux).iter().all(|s| s.id != ImportSource::Safari && s.id != ImportSource::Arc));
        assert!(sources_for(Os::Mac).iter().any(|s| s.id == ImportSource::Safari));
        assert!(sources_for(Os::Windows).iter().any(|s| s.id == ImportSource::Arc));
    }

    #[test]
    fn locations() {
        let h = Path::new("/h");
        assert_eq!(paths(ImportSource::Chrome, &env(Os::Mac, h)), vec![PathBuf::from("/h/Library/Application Support/Google/Chrome/Default")]);
        assert_eq!(paths(ImportSource::Edge, &env(Os::Windows, h)), vec![PathBuf::from("/h/Local/Microsoft/Edge/User Data/Default")]);
        let l = paths(ImportSource::Chromium, &env(Os::Linux, h));
        assert_eq!(l[0], PathBuf::from("/h/.config/chromium/Default"));
        assert_eq!(l[2], PathBuf::from("/h/snap/chromium/common/chromium/Default"));
        assert_eq!(paths(ImportSource::Safari, &env(Os::Mac, h)), vec![PathBuf::from("/h/Library/Safari/Bookmarks.plist")]);
    }

    #[test]
    fn kinds_found_from_fixture() {
        let d = tempfile::tempdir().unwrap();
        let e = env(Os::Mac, d.path());
        assert!(available_sources(&e).is_empty());
        let prof = d.path().join("Library/Application Support/Google/Chrome/Default");
        fs::create_dir_all(prof.join("Extensions")).unwrap();
        fs::write(prof.join("Bookmarks"), "{}").unwrap();
        fs::write(prof.join("History"), "").unwrap();
        assert_eq!(kinds_of(ImportSource::Chrome, &e), vec![ImportKind::Bookmarks, ImportKind::Extensions, ImportKind::History]);
        let arc = d.path().join("Library/Application Support/Arc");
        fs::create_dir_all(&arc).unwrap();
        fs::write(arc.join("StorableSidebar.json"), "{}").unwrap();
        assert_eq!(kinds_of(ImportSource::Arc, &e), vec![ImportKind::Spaces]);
        assert_eq!(available_sources(&e).len(), 2);
    }

    #[test]
    fn ids() {
        assert_eq!(ImportSource::from_id("brave"), Some(ImportSource::Brave));
        assert_eq!(ImportSource::from_id("x"), None);
    }
}

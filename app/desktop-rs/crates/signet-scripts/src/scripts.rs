//! The page scripts and how they are injected. Mirrors the registrations of `app/desktop/src/main/web/profiles.ts`
//! (`registerPreloadScript`) and of `main/web/gpc.ts`.
//!
//! There is no isolated world and no preload: `signet-web` injects, in every frame at document start, the program built
//! by [`Registry::bundle_for`]. One program per frame holds the bridge and all the scripts that apply to it, so the
//! hidden post function is captured once and every script shares one closure.

use serde::Serialize;
use url::Url;

use crate::bridge::{DISPATCH_FN, POST_FN};

const BRIDGE_JS: &str = include_str!("../js/bridge.js");
const IDENTITY_JS: &str = include_str!("../js/identity.js");
const SWIPE_JS: &str = include_str!("../js/swipe.js");
const PASSWORDS_JS: &str = include_str!("../js/passwords.js");
const AUTOFILL_CORE_JS: &str = include_str!("../js/autofill-core.js");
const AUTOFILL_JS: &str = include_str!("../js/autofill.js");
const NOTES_JS: &str = include_str!("../js/notes.js");
const GPC_JS: &str = include_str!("../js/gpc.js");
const WEBAUTHN_JS: &str = include_str!("../js/webauthn.js");

/// Source of the bridge (a function expression `(names, global) => bridge`), for tests and tooling.
pub const BRIDGE_SOURCE: &str = BRIDGE_JS;
/// Source of the pure autofill helpers (a function expression `() => core`).
pub const AUTOFILL_CORE_SOURCE: &str = AUTOFILL_CORE_JS;

/// In which frames a script runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frames {
    MainOnly,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunAt {
    DocumentStart,
    DocumentEnd,
}

/// Settings a script can depend on (the script is only injected while they hold).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// "Global Privacy Control" setting on (`gpc.ts` registers its script only then).
    GlobalPrivacyControl,
    /// [`webauthn_intercept_enabled`].
    WebAuthnIntercept,
}

/// Which pages a script is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Always,
    /// http and https pages.
    HttpOrHttps,
    /// Document pages: `signet://note/<id>`, `signet://easel/<id>` (and [`ScriptOptions::notes_pages`]).
    AppPages,
    /// URL starting with the prefix.
    UrlPrefix(&'static str),
    /// Only while a setting holds.
    Setting(Gate),
}

/// In which profiles (sessions) a script is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profiles {
    /// Normal and private.
    AllProfiles,
    /// Not in private windows (nothing is saved there).
    NotPrivate,
}

/// A script ready to inject. `source` is a function expression `(bridge, config, global) => void`; it is not meant to
/// run alone: use [`Registry::bundle_for`] or [`bundle`].
#[derive(Debug, Clone)]
pub struct PageScript {
    pub id: &'static str,
    pub source: String,
    pub frames: Frames,
    pub run_at: RunAt,
    pub when: When,
    pub profiles: Profiles,
}

/// An app page (scheme, host with port, path) that may host Notes and Easels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppPage {
    /// With the colon, as `location.protocol`: `signet:`.
    pub protocol: String,
    /// As `location.host`: `app`, `localhost:5173`.
    pub host: String,
    /// As `location.pathname`.
    pub pathname: String,
}

impl AppPage {
    pub fn new(protocol: &str, host: &str, pathname: &str) -> Self {
        Self { protocol: protocol.into(), host: host.into(), pathname: pathname.into() }
    }
}

/// Values the scripts and the validation share. Change them here if the internal pages live elsewhere.
#[derive(Debug, Clone, Default)]
pub struct ScriptOptions {
    /// Extra app pages that host Notes and Easels on a hash route (`#note/<id>`). Default: none, because documents are
    /// served at their own address `signet://note/<id>` and `signet://easel/<id>`, which are always accepted.
    pub notes_pages: Vec<AppPage>,
    /// Logs the swipe decisions to the console (development).
    pub swipe_diag: bool,
}


/// What the browser knows when it injects scripts into a frame.
#[derive(Debug, Clone)]
pub struct FrameContext {
    pub url: String,
    pub is_main_frame: bool,
    pub is_private: bool,
    pub settings: ScriptSettings,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptSettings {
    pub global_privacy_control: bool,
    pub webauthn_intercept: bool,
}

/// Port of `webauthnInterceptEnabled()`: only Linux has no passkey support to lose; elsewhere only on demand
/// (`SIGNET_WEBAUTHN_INTERCEPT=1`), in development (Windows Hello and the macOS keychain keep working).
pub fn webauthn_intercept_enabled_for(linux: bool, packaged: bool, env_flag: bool) -> bool {
    linux || (!packaged && env_flag)
}

/// [`webauthn_intercept_enabled_for`] for this build and environment (`debug_assertions` stands for "not packaged").
pub fn webauthn_intercept_enabled() -> bool {
    webauthn_intercept_enabled_for(
        cfg!(target_os = "linux"),
        !cfg!(debug_assertions),
        std::env::var("SIGNET_WEBAUTHN_INTERCEPT").is_ok_and(|v| v == "1"),
    )
}

fn compose_autofill() -> String {
    format!("(function (bridge, config, g) {{\nconst core = ({AUTOFILL_CORE_JS}\n)();\nreturn ({AUTOFILL_JS}\n)(bridge, config, g, core);\n}})")
}

/// All the page scripts, in registration order (the order of `profiles.ts`).
pub fn all_scripts() -> Vec<PageScript> {
    let script = |id, source: &str, frames, when, profiles| PageScript {
        id,
        source: source.to_owned(),
        frames,
        run_at: RunAt::DocumentStart,
        when,
        profiles,
    };
    vec![
        // `window.chrome` as in Chrome, in all frames (extension pages are skipped by the script itself).
        script("signet-identity", IDENTITY_JS, Frames::All, When::Always, Profiles::AllProfiles),
        // Passkey interception, main frame, only where the platform has nothing native to lose.
        script("signet-webauthn", WEBAUTHN_JS, Frames::MainOnly, When::Setting(Gate::WebAuthnIntercept), Profiles::AllProfiles),
        // Registered for all frames in Electron, acts in the main frame only.
        script("signet-swipe", SWIPE_JS, Frames::MainOnly, When::Always, Profiles::AllProfiles),
        script("signet-passwords", PASSWORDS_JS, Frames::MainOnly, When::HttpOrHttps, Profiles::AllProfiles),
        script("signet-autofill", &compose_autofill(), Frames::MainOnly, When::HttpOrHttps, Profiles::AllProfiles),
        // Notes and Easels: not in private windows.
        script("signet-notes", NOTES_JS, Frames::MainOnly, When::AppPages, Profiles::NotPrivate),
        // Registered by `Gpc` only while the setting is on.
        script("signet-gpc", GPC_JS, Frames::All, When::Setting(Gate::GlobalPrivacyControl), Profiles::AllProfiles),
    ]
}

/// The scripts plus the options, with the logic to pick and assemble what a frame gets.
#[derive(Debug, Clone)]
pub struct Registry {
    pub scripts: Vec<PageScript>,
    pub options: ScriptOptions,
}

impl Registry {
    pub fn new(options: ScriptOptions) -> Self {
        Self { scripts: all_scripts(), options }
    }

    /// Does `script` run in a frame described by `ctx`?
    pub fn applies(&self, script: &PageScript, ctx: &FrameContext) -> bool {
        if script.frames == Frames::MainOnly && !ctx.is_main_frame {
            return false;
        }
        if script.profiles == Profiles::NotPrivate && ctx.is_private {
            return false;
        }
        let url = Url::parse(&ctx.url).ok();
        match script.when {
            When::Always => true,
            When::HttpOrHttps => url.is_some_and(|u| matches!(u.scheme(), "http" | "https")),
            When::AppPages => url.is_some_and(|u| crate::notes::note_of_app_url(u.as_str(), &self.options.notes_pages).is_some()),
            When::UrlPrefix(prefix) => ctx.url.starts_with(prefix),
            When::Setting(Gate::GlobalPrivacyControl) => ctx.settings.global_privacy_control,
            When::Setting(Gate::WebAuthnIntercept) => ctx.settings.webauthn_intercept,
        }
    }

    /// Scripts for a frame, with the run time they need.
    pub fn scripts_for(&self, ctx: &FrameContext, run_at: RunAt) -> Vec<&PageScript> {
        self.scripts.iter().filter(|s| s.run_at == run_at && self.applies(s, ctx)).collect()
    }

    /// The JS to inject in a frame at `run_at`, or None when nothing applies.
    pub fn bundle_for(&self, ctx: &FrameContext, run_at: RunAt) -> Option<String> {
        let list = self.scripts_for(ctx, run_at);
        (!list.is_empty()).then(|| bundle(&list, &self.options))
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new(ScriptOptions::default())
    }
}

/// One program: the bridge, then each script in its own try/catch (a failing script never stops the next).
pub fn bundle(scripts: &[&PageScript], options: &ScriptOptions) -> String {
    let config = serde_json::json!({ "notesPages": options.notes_pages, "swipeDiag": options.swipe_diag });
    let names = serde_json::json!({ "post": POST_FN, "dispatch": DISPATCH_FN });
    let mut out = String::with_capacity(64 * 1024);
    out.push_str("(function () {\n'use strict';\nconst g = globalThis;\n");
    out.push_str(&format!("const config = {config};\n"));
    out.push_str(&format!("const bridge = ({BRIDGE_JS}\n)({names}, g);\n"));
    for s in scripts {
        out.push_str(&format!("try {{ ({}\n)(bridge, config, g); }} catch (e) {{}}\n", s.source));
    }
    out.push_str("})();\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(url: &str, main: bool, private: bool) -> FrameContext {
        FrameContext { url: url.into(), is_main_frame: main, is_private: private, settings: ScriptSettings::default() }
    }

    fn ids(r: &Registry, c: &FrameContext) -> Vec<&'static str> {
        r.scripts_for(c, RunAt::DocumentStart).iter().map(|s| s.id).collect()
    }

    #[test]
    fn registrations_mirror_profiles_ts() {
        let all = all_scripts();
        let names: Vec<_> = all.iter().map(|s| s.id).collect();
        assert_eq!(
            names,
            ["signet-identity", "signet-webauthn", "signet-swipe", "signet-passwords", "signet-autofill", "signet-notes", "signet-gpc"]
        );
        assert!(all.iter().all(|s| s.run_at == RunAt::DocumentStart));
        assert!(all.iter().all(|s| !s.source.trim().is_empty()));
    }

    #[test]
    fn web_page_main_frame() {
        let r = Registry::default();
        assert_eq!(ids(&r, &ctx("https://example.com/", true, false)), ["signet-identity", "signet-swipe", "signet-passwords", "signet-autofill"]);
    }

    #[test]
    fn subframes_only_get_identity() {
        let r = Registry::default();
        assert_eq!(ids(&r, &ctx("https://example.com/", false, false)), ["signet-identity"]);
    }

    #[test]
    fn private_windows_have_no_notes() {
        let r = Registry::default();
        let url = "signet://note/0b9d0f3e-1c1a-4a53-9a6f-2f6d2b1c9e11";
        assert!(ids(&r, &ctx(url, true, false)).contains(&"signet-notes"));
        assert!(!ids(&r, &ctx(url, true, true)).contains(&"signet-notes"));
        assert!(!ids(&r, &ctx("https://example.com/", true, false)).contains(&"signet-notes"));
    }

    #[test]
    fn settings_gate_gpc_and_webauthn() {
        let r = Registry::default();
        let mut c = ctx("http://example.com/", true, true);
        assert!(!ids(&r, &c).contains(&"signet-gpc"));
        c.settings = ScriptSettings { global_privacy_control: true, webauthn_intercept: true };
        let got = ids(&r, &c);
        assert!(got.contains(&"signet-gpc") && got.contains(&"signet-webauthn"));
        // GPC is in all frames, passkeys in the main frame only.
        c.is_main_frame = false;
        let got = ids(&r, &c);
        assert!(got.contains(&"signet-gpc") && !got.contains(&"signet-webauthn"));
    }

    #[test]
    fn internal_and_extension_pages_get_no_password_scripts() {
        let r = Registry::default();
        for url in ["chrome-extension://abc/popup.html", "about:blank", "signet://app/index.html"] {
            let got = ids(&r, &ctx(url, true, false));
            assert!(!got.contains(&"signet-passwords") && !got.contains(&"signet-autofill"), "{url}");
        }
    }

    #[test]
    fn bundle_has_bridge_names_and_every_script() {
        let r = Registry::default();
        let c = ctx("https://example.com/", true, false);
        let js = r.bundle_for(&c, RunAt::DocumentStart).unwrap();
        assert!(js.contains("\"post\":\"__signetPost\"") && js.contains("\"dispatch\":\"__signetDispatch\""));
        for s in r.scripts_for(&c, RunAt::DocumentStart) {
            assert!(js.contains(s.source.trim()), "{}", s.id);
        }
        assert!(r.bundle_for(&c, RunAt::DocumentEnd).is_none());
    }

    #[test]
    fn bridge_names_are_not_hardcoded_in_the_js() {
        for src in [BRIDGE_JS, IDENTITY_JS, SWIPE_JS, PASSWORDS_JS, AUTOFILL_JS, NOTES_JS, GPC_JS, WEBAUTHN_JS] {
            assert!(!src.contains(POST_FN) && !src.contains(DISPATCH_FN));
        }
    }

    #[test]
    fn no_dashes_in_sources() {
        for s in all_scripts() {
            assert!(!s.source.contains('\u{2014}') && !s.source.contains('\u{2013}'), "{}", s.id);
        }
        assert!(!BRIDGE_JS.contains('\u{2014}') && !BRIDGE_JS.contains('\u{2013}'));
    }

    #[test]
    fn webauthn_intercept_rules() {
        assert!(webauthn_intercept_enabled_for(true, true, false));
        assert!(!webauthn_intercept_enabled_for(false, true, true));
        assert!(webauthn_intercept_enabled_for(false, false, true));
        assert!(!webauthn_intercept_enabled_for(false, false, false));
    }
}

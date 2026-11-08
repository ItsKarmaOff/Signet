//! Port of `windows/settings.ts` `update()`/`info()` and the appearance and language broadcasts.

use serde_json::{Map, Value};
use signet_core::api::UiEvent;
use signet_core::core::store::{apply_cleaned, clean_settings, SettingsPatch};
use signet_core::i18n::{init_locale, Locale};
use crate::syslang::system_languages as system_languages_from_env;
use signet_core::types::{AppIconInfo, Appearance, DataCounts, Density, Settings, SettingsInfo, TextSize, TintStrength, APP_ICONS};

use super::Core;
use crate::app::platform::IS_MAC;

/// Plain fields copied from a patch as they are (value validated by the typed `Settings` on the way back).
const PLAIN: [&str; 30] = [
    "peekExternalLinks", "autoPip", "askDownloadPath", "newTabPosition", "confirmQuit", "showBookmarksBar", "swipeNavigation", "tabPreview", "spellcheck",
    "passwordManager", "offerSavePasswords", "autofillAddresses", "autofillCards", "devMode", "devKeepOutOfHistory", "jsonViewer", "secureDns", "secureDnsServer",
    "checkUpdates", "sidebarSide", "theme", "tint", "density", "textSize", "transparency", "adblock", "appIcon", "compactMode", "tabLayout", "onboarded",
];

impl Core {
    pub fn appearance(&self) -> Appearance {
        let s = &self.state().settings;
        Appearance { tint: s.tint.unwrap_or(TintStrength::Normal), density: s.density.unwrap_or(Density::Normal), text: s.text_size.unwrap_or(TextSize::Normal), glass: s.transparency != Some(false) }
    }

    pub fn update_settings(&mut self, patch: SettingsPatch) {
        let before_look = serde_json::to_string(&self.appearance()).unwrap_or_default();
        let before_lang = self.state().settings.ui_language;
        let before_langs = self.state().settings.languages.clone();
        let base = self.state().settings.clone();
        // Validated fields.
        let cleaned = clean_settings(&patch, &base);
        let mut next = base.clone();
        apply_cleaned(&mut next, &cleaned);
        // Plain fields, through JSON so the typed settings reject a wrong value.
        if let Ok(Value::Object(mut map)) = serde_json::to_value(&next) {
            for key in PLAIN {
                match patch.get(key) {
                    Some(Value::Null) => {
                        map.remove(key);
                    }
                    Some(v) => {
                        map.insert(key.to_string(), v.clone());
                    }
                    None => {}
                }
            }
            if let Some(v) = patch.get("uiLanguage") {
                if v == "system" || v.is_null() {
                    map.remove("uiLanguage");
                } else {
                    map.insert("uiLanguage".into(), v.clone());
                }
            }
            for key in ["httpsOnly", "hardwareAcceleration"] {
                if let Some(Value::Bool(b)) = patch.get(key) {
                    if key == "httpsOnly" && *b {
                        map.insert(key.into(), Value::Bool(true));
                    } else if key == "hardwareAcceleration" && !*b {
                        map.insert(key.into(), Value::Bool(false));
                    } else {
                        map.remove(key);
                    }
                }
            }
            if let Some(Value::Array(langs)) = patch.get("languages") {
                let list: Vec<String> = langs.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
                let normalized = signet_core::languages::normalize_languages(&list, &system_languages_from_env());
                map.insert("languages".into(), serde_json::json!(if normalized.is_empty() { signet_core::languages::default_languages(&system_languages_from_env()) } else { normalized }));
            }
            for key in ["neverSavePasswords", "neverSaveAutofill"] {
                if let Some(Value::Array(kept)) = patch.get(key) {
                    let keep: Vec<&str> = kept.iter().filter_map(Value::as_str).collect();
                    let old: Vec<String> = map.get(key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
                    map.insert(key.into(), serde_json::json!(old.into_iter().filter(|o| keep.contains(&o.as_str())).collect::<Vec<_>>()));
                }
            }
            if let Ok(s) = serde_json::from_value::<Settings>(Value::Object(map)) {
                next = s;
            }
        }
        if next.compact_mode != base.compact_mode || next.tab_layout != base.tab_layout {
            self.hover.sidebar = false;
            self.hover.toolbar = false;
        }
        self.store.data.state.settings = next;
        self.apply_settings_effects(before_lang, &before_langs);
        let look = serde_json::to_string(&self.appearance()).unwrap_or_default();
        if look != before_look {
            self.emit(UiEvent::Appearance(self.appearance()));
        }
        self.changed();
        let info = self.settings_info();
        self.emit(UiEvent::SettingsInfo(Box::new(info)));
    }

    /// Side effects that follow a settings change: search engines, shortcuts, language.
    fn apply_settings_effects(&mut self, before_lang: Option<signet_core::i18n::UiLanguage>, before_langs: &[String]) {
        let s = self.state().settings.clone();
        self.search.set_search_engine(s.search_engine.as_deref());
        self.search.set_search_engines(&signet_core::searchengines::EngineConfig::from_settings(&s));
        self.search.set_language(s.languages.first().map(String::as_str));
        self.search.set_search_locale(s.languages.first().map(String::as_str).unwrap_or("en"));
        self.refresh_key_table();
        if s.ui_language != before_lang {
            let sys = system_languages_from_env();
            let refs: Vec<&str> = sys.iter().map(String::as_str).collect();
            let locale = init_locale(s.ui_language, &refs);
            self.emit(UiEvent::Locale(locale));
            // A language change resets the user agent of the pages whose Develop settings were changed.
            self.reapply_develop();
        }
        if s.languages != before_langs {
            // Pages see the new Accept-Language on the next navigation: reload what is displayed.
            for p in self.panes.clone() {
                if let Some(v) = self.views.view_of(&p.tab_id) {
                    v.reload();
                }
            }
        }
    }

    pub fn locale(&self) -> Locale {
        signet_core::i18n::current_locale()
    }

    pub fn settings_info(&mut self) -> SettingsInfo {
        let st = self.state().clone();
        SettingsInfo {
            settings: st.settings.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            packaged: cfg!(not(debug_assertions)),
            default_browser: crate::phase3::is_default_browser(),
            app_icons: APP_ICONS.iter().map(|id| AppIconInfo { id: (*id).to_string(), name: signet_core::i18n::t(&format!("appIcon.{id}")), preview: String::new() }).collect(),
            import_sources: self.import_sources(),
            boosts: st.boosts.values().cloned().collect(),
            extensions: self.extension_infos(),
            counts: DataCounts { history: self.history.size() as u32, archive: st.archive.len() as u32, downloads: st.downloads.len() as u32, permissions: st.site_permissions.len() as u32 },
            hue: self.hue(),
            download_dir: self.download_dir().to_string_lossy().into_owned(),
            hardware_acceleration_active: st.settings.hardware_acceleration != Some(false),
            proxy_settings: IS_MAC,
        }
    }

    pub fn report_ui_error(&mut self, route: &str, message: &str, stack: &str) {
        use std::io::Write;
        let dir = self.data_dir.join("logs");
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("interface.log")) {
            let _ = writeln!(f, "[{}] {route}: {message}\n{stack}", signet_core::ids::now_ms());
        }
        self.log(format!("ui error in {route}: {message}"));
    }

    pub fn welcome_finish(&mut self) {
        self.store.data.state.settings.onboarded = Some(true);
        self.changed();
        self.send(crate::windows::Cmd::CloseWin(crate::windows::Win::Welcome));
    }

    pub fn set_shortcut_recording(&mut self, on: bool) {
        if self.recording_shortcuts == on {
            return;
        }
        self.recording_shortcuts = on;
        // Suspend the menu accelerators while a shortcut is typed.
        self.dirty.menu_key = String::new();
        self.send(crate::windows::Cmd::RebuildMenu);
        let _ = Map::<String, Value>::new();
    }

    pub fn clear_browsing_data_engine(&mut self, cache: bool, cookies: bool) {
        if !cache && !cookies {
            return;
        }
        let mut profiles = vec![signet_web::ProfileKind::persistent("default")];
        profiles.extend(self.state().spaces.iter().filter(|s| s.is_isolated()).map(|s| signet_web::ProfileKind::persistent(format!("space-{}", s.id))));
        for p in profiles {
            signet_web::clear_profile_cache(&p);
        }
    }
}

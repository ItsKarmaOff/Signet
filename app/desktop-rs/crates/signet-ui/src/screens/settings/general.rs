//! Settings > General (port of the `General` function of `Settings.tsx`).

use gpui::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div, prelude::FluentBuilder};
use serde_json::json;
use signet_core::i18n::{LOCALES, Locale, UiLanguage, current_locale, resolve_locale, system_languages_from_env};
use signet_core::searchengines::{DEFAULT_SEARCH_ENGINE, EngineConfig, resolve_engines};

use super::kit::{self, Btn};
use super::languages::LanguageSettings;
use super::live_folder::LiveFolderSettings;
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{api, tr, tr_args};
use crate::theme::space;

const ARCHIVE_CHOICES: [u32; 6] = [6, 12, 24, 72, 168, 0];
/// Sleep delays offered, in minutes (0 = never).
const SLEEP_CHOICES: [u32; 7] = [5, 15, 30, 60, 120, 360, 0];

pub fn sleep_label(minutes: u32) -> String {
    if minutes == 0 {
        return tr("settings.general.sleep.never");
    }
    if minutes % 60 == 0 {
        tr_args("settings.general.sleep.hours", &[("count", &(minutes / 60).to_string())])
    } else {
        tr_args("settings.general.sleep.minutes", &[("count", &minutes.to_string())])
    }
}

pub fn archive_label(hours: u32) -> String {
    if hours == 0 {
        return tr("settings.general.archive.never");
    }
    if hours % 24 == 0 {
        tr_args("settings.general.archive.days", &[("count", &(hours / 24).to_string())])
    } else {
        tr_args("settings.general.archive.hours", &[("count", &hours.to_string())])
    }
}

fn locale_name(locale: Locale) -> String {
    LOCALES.iter().find(|l| l.id == locale).map(|l| l.name.to_string()).unwrap_or_else(|| locale.id().to_string())
}

pub struct General {
    store: Entity<InfoStore>,
    languages: Entity<LanguageSettings>,
    live: Entity<LiveFolderSettings>,
}

impl General {
    pub fn new(store: Entity<InfoStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        let languages = cx.new(|cx| LanguageSettings::new(store.clone(), cx));
        let live = cx.new(|cx| LiveFolderSettings::new(window, cx));
        Self { store, languages, live }
    }
}

impl Render for General {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = info_of(&self.store, cx);
        let s = &info.settings;
        let pref = s.ui_language.unwrap_or(UiLanguage::System);
        // "System (...)": the language the system would give (the current one if already following the system).
        let system_locale = if pref == UiLanguage::System {
            current_locale()
        } else {
            let langs = system_languages_from_env();
            let refs: Vec<&str> = langs.iter().map(|s| s.as_str()).collect();
            resolve_locale(None, &refs)
        };
        let pref_value = match pref {
            UiLanguage::System => "system",
            UiLanguage::En => "en",
            UiLanguage::Fr => "fr",
        };
        let mut lang_options = vec![("system".to_string(), tr_args("settings.general.uiLanguageSystem", &[("language", &locale_name(system_locale))]))];
        lang_options.extend(LOCALES.iter().map(|l| (l.id.id().to_string(), l.name.to_string())));

        let engine_cfg = EngineConfig::from_settings(s);
        let engines = resolve_engines(&engine_cfg, Some(current_locale().id()));
        let engine_options: Vec<(String, String)> = engines.iter().filter(|e| e.enabled).map(|e| (e.id.clone(), e.name.clone())).collect();
        let engine = s.search_engine.clone().unwrap_or_else(|| DEFAULT_SEARCH_ENGINE.to_string());

        let archive = s.archive_after_hours.max(0.0) as u32;
        let mut archive_options: Vec<(String, String)> = Vec::new();
        if !ARCHIVE_CHOICES.contains(&archive) {
            archive_options.push((archive.to_string(), archive_label(archive)));
        }
        archive_options.extend(ARCHIVE_CHOICES.iter().map(|h| (h.to_string(), archive_label(*h))));

        let sleep = s.tab_sleep_minutes.unwrap_or(30);
        let mut sleep_options: Vec<(String, String)> = Vec::new();
        if !SLEEP_CHOICES.contains(&sleep) {
            sleep_options.push((sleep.to_string(), sleep_label(sleep)));
        }
        sleep_options.extend(SLEEP_CHOICES.iter().map(|m| (m.to_string(), sleep_label(*m))));

        let new_tab = match s.new_tab_position {
            Some(signet_core::types::NewTabPosition::Bottom) => "bottom",
            _ => "top",
        };
        let sw = |id: &'static str, title: &str, hint: Option<String>, on: bool, key: &'static str| {
            kit::row(cx)
                .child(kit::label(cx, title.to_string(), hint.map(Into::into)))
                .child(kit::switch(id, on, move |v, _, cx| kit::update(cx, key, json!(v))))
                .into_any_element()
        };

        let download_dir = kit::row(cx)
            .child(kit::label(cx, tr("settings.general.downloadDir"), None).child(kit::hint_text(cx, info.download_dir.clone())))
            .when(s.download_dir.as_deref().is_some_and(|d| !d.is_empty()), |d| {
                d.child(Btn::new("dl-reset", tr("settings.general.downloadDirReset")).on_click(|_, _, cx| kit::update(cx, "downloadDir", json!(""))))
            })
            .child(Btn::new("dl-change", tr("settings.general.downloadDirChange")).on_click(|_, _, cx| api(cx).choose_download_dir()))
            .into_any_element();

        let default_browser = {
            let hint = if info.packaged {
                if info.default_browser { tr("settings.general.isDefault") } else { tr("settings.general.otherDefault") }
            } else {
                tr("settings.general.installedOnly")
            };
            kit::row(cx)
                .child(kit::label(cx, tr("settings.general.defaultBrowser"), Some(hint.into())))
                .child(if info.default_browser {
                    kit::ok(cx, tr("settings.general.default")).into_any_element()
                } else {
                    Btn::new("make-default", tr("settings.general.makeDefault"))
                        .disabled(!info.packaged)
                        .on_click(|_, _, cx| api(cx).set_default_browser())
                        .into_any_element()
                })
                .into_any_element()
        };

        kit::body()
            .child(kit::group(
                cx,
                vec![
                    kit::row(cx)
                        .child(kit::label(cx, tr("settings.general.uiLanguage"), Some(tr("settings.general.uiLanguageHint").into())))
                        .child(kit::choice("ui-language", pref_value, lang_options, |v, _, cx| kit::update(cx, "uiLanguage", json!(v))))
                        .into_any_element(),
                    kit::row(cx)
                        .child(kit::label(cx, tr("settings.general.searchEngine"), Some(tr("settings.general.searchEngineHint").into())))
                        .child(kit::choice("search-engine", &engine, engine_options, |v, _, cx| kit::update(cx, "searchEngine", json!(v))))
                        .into_any_element(),
                ],
            ))
            .child(kit::group(
                cx,
                vec![
                    kit::row(cx)
                        .child(kit::label(cx, tr("settings.general.archiveAfter"), Some(tr("settings.general.archiveAfterHint").into())))
                        .child(kit::choice("archive-after", &archive.to_string(), archive_options, |v, _, cx| {
                            kit::update(cx, "archiveAfterHours", json!(v.parse::<u32>().unwrap_or(12)))
                        }))
                        .into_any_element(),
                    kit::row(cx)
                        .child(kit::label(cx, tr("settings.general.sleepAfter"), Some(tr("settings.general.sleepAfterHint").into())))
                        .child(kit::choice("sleep-after", &sleep.to_string(), sleep_options, |v, _, cx| {
                            kit::update(cx, "tabSleepMinutes", json!(v.parse::<u32>().unwrap_or(30)))
                        }))
                        .into_any_element(),
                    kit::row(cx)
                        .child(kit::label(cx, tr("settings.general.newTabPosition"), Some(tr("settings.general.newTabPositionHint").into())))
                        .child(kit::choice(
                            "new-tab-position",
                            new_tab,
                            vec![("top".into(), tr("settings.general.newTabTop")), ("bottom".into(), tr("settings.general.newTabBottom"))],
                            |v, _, cx| kit::update(cx, "newTabPosition", json!(v)),
                        ))
                        .into_any_element(),
                    sw("sw-peek", &tr("settings.general.peekLinks"), None, s.peek_external_links, "peekExternalLinks"),
                    sw("sw-swipe", &tr("settings.general.swipe"), Some(tr("settings.general.swipeHint")), s.swipe_navigation != Some(false), "swipeNavigation"),
                    sw("sw-pip", &tr("settings.general.pip"), Some(tr("settings.general.pipHint")), s.auto_pip, "autoPip"),
                    sw(
                        "sw-confirm-quit",
                        &tr("settings.general.confirmQuit"),
                        Some(tr("settings.general.confirmQuitHint")),
                        s.confirm_quit != Some(false),
                        "confirmQuit",
                    ),
                ],
            ))
            .child(kit::group(
                cx,
                vec![
                    download_dir,
                    sw(
                        "sw-ask-path",
                        &tr("settings.general.askDownloadPath"),
                        Some(tr("settings.general.askDownloadPathHint")),
                        s.ask_download_path == Some(true),
                        "askDownloadPath",
                    ),
                ],
            ))
            .child(kit::group(
                cx,
                vec![
                    kit::row_stack(cx)
                        .child(kit::label(cx, tr("settings.general.siteLanguages"), Some(tr("settings.general.siteLanguagesHint").into())))
                        .child(self.languages.clone())
                        .into_any_element(),
                    sw("sw-spell", &tr("settings.general.spellcheck"), Some(tr("settings.general.spellcheckHint")), s.spellcheck != Some(false), "spellcheck"),
                ],
            ))
            .child(kit::group(cx, vec![default_browser]))
            .child(self.live.clone())
            .child(div().h(space::S1))
    }
}

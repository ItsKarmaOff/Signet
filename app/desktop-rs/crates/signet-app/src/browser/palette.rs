//! Port of `browser/palette.ts`: the command bar's results (typed URL, search, keywords, open tabs, history, archive,
//! bookmarks, app actions) and their execution.

use std::collections::HashSet;

use signet_core::core::rank::{history_score, match_quality, ScoreInput};
use signet_core::ids::now_ms;
use signet_core::shortcuts::shortcut_label;
use signet_core::types::{Bookmark, PaletteMode, PaletteResult, Zone};
use signet_core::urls::{fold, to_url};
use signet_features::bookmarks as bm;
use signet_features::internal;

use super::Core;
use crate::app::platform::IS_MAC;

const MAX_RESULTS: usize = 12;
const MAX_PAGES: usize = 5;

struct Scored(f64, PaletteResult);

fn t(key: &str) -> String {
    signet_core::i18n::t(key)
}

impl Core {
    fn shortcut(&self, id: &str) -> Option<String> {
        shortcut_label(id, self.state().settings.shortcuts.as_ref(), IS_MAC)
    }

    /// Fixed app actions: `(id, title key, shortcut command id)`.
    fn action_rows(&self) -> Vec<PaletteResult> {
        let rows: &[(&str, &str, Option<&str>)] = &[
            ("settings", "palette.action.settings", None),
            ("library", "palette.action.library", Some("library")),
            ("new-space", "palette.action.newSpace", None),
            ("split", "palette.action.splitView", Some("openSplit")),
            ("unsplit", "palette.action.unsplit", None),
            ("toggle-sidebar", "palette.action.toggleSidebar", Some("toggleSidebar")),
            ("compact", "palette.action.compact", Some("compactMode")),
            ("tab-layout", if self.horizontal() { "palette.action.tabsSide" } else { "palette.action.tabsTop" }, Some("tabLayout")),
            ("pin", "palette.action.pin", Some("pinTab")),
            ("copy-url", "palette.action.copyUrl", Some("copyUrl")),
            ("little", "palette.action.newLittleSignet", Some("newLittle")),
            ("new-note", "palette.action.newNote", Some("newNote")),
            ("new-easel", "palette.action.newEasel", Some("newEasel")),
            ("private", "palette.action.newPrivateWindow", Some("newPrivate")),
            ("clear-today", "palette.action.archiveToday", None),
            ("close-tab", "palette.action.closeTab", Some("closeTab")),
            ("reopen", "palette.action.reopenTab", Some("reopenTab")),
            ("reload", "palette.action.reload", Some("reload")),
            ("devtools", "palette.action.devTools", Some("devTools")),
            ("new-folder", "palette.action.newFolder", None),
            ("toggle-peek-links", "palette.action.peekLinks", None),
            ("toggle-adblock", "palette.action.adblock", None),
        ];
        let mut out: Vec<PaletteResult> = rows
            .iter()
            .map(|(id, key, sc)| PaletteResult::Action {
                id: format!("action:{id}"),
                title: t(key),
                subtitle: match *id {
                    "toggle-peek-links" => Some(t(if self.state().settings.peek_external_links { "palette.subtitle.on" } else { "palette.subtitle.off" })),
                    "toggle-adblock" => Some(t(if self.state().settings.adblock { "palette.subtitle.on" } else { "palette.subtitle.off" })),
                    _ => None,
                },
                action: (*id).to_string(),
                shortcut: sc.and_then(|c| self.shortcut(c)),
                image: None,
            })
            .collect();
        let action = |id: &str, title: String, sc: Option<&str>| PaletteResult::Action { id: format!("action:{id}"), title, subtitle: None, action: id.to_string(), shortcut: sc.and_then(|c| self.shortcut(c)), image: None };
        if let Some(tab) = self.active_tab_id() {
            let active = self.state().tabs.get(&tab).cloned();
            if self.can_open_on_phone(Some(&tab)) {
                out.push(action("open-on-phone", t("palette.action.openOnPhone"), None));
            }
            if self.state().settings.dev_mode == Some(true) {
                for (id, title) in self.develop_palette_actions(&tab) {
                    out.push(action(&id, title, None));
                }
            }
            if IS_MAC && active.as_ref().is_some_and(|a| a.url.starts_with("http://") || a.url.starts_with("https://")) {
                match self.web_app_for(&active.as_ref().map(|a| a.url.clone()).unwrap_or_default()) {
                    Some((app, name)) => out.push(action(&format!("open-in-app:{app}"), signet_core::i18n::t_args("palette.action.openInApp", &[("name", &name)]), None)),
                    None => out.push(action("install-app", t("palette.action.installApp"), Some("installApp"))),
                }
            }
            if self.can_read_tab(&tab) {
                let reader_on = active.as_ref().is_some_and(|a| a.reader == Some(signet_core::types::ReaderState::On));
                out.push(action("reader", t(if reader_on { "palette.action.exitReader" } else { "palette.action.reader" }), Some("reader")));
            }
            if self.can_translate_tab(&tab) {
                let title = if active.as_ref().is_some_and(|a| a.translation.is_some()) {
                    t("palette.action.showOriginal")
                } else {
                    signet_core::i18n::t_args("palette.action.translate", &[("language", &self.translation_target_name())])
                };
                out.push(action("translate", title, Some("translate")));
            }
        }
        // Installed sites open from the command bar.
        for (app, name) in self.web_app_names() {
            out.push(action(&format!("webapp:{app}"), signet_core::i18n::t_args("palette.action.launchApp", &[("name", &name)]), None));
        }
        out
    }

    pub fn palette_query(&mut self, query: &str, mode: PaletteMode) -> Vec<PaletteResult> {
        let q = query.trim();
        // The suggestions request leaves now, so that its answer is cached when the bar asks for them.
        self.prefetch_suggestions(q);
        let fq = fold(q);
        let mut rows: Vec<Scored> = Vec::new();
        let now = now_ms();
        let aliases = self.state().settings.dev_aliases.clone().unwrap_or_default();
        if q.is_empty() {
            // Nothing typed: the most recent tabs of the space, then (new tab) a few actions.
            if mode == PaletteMode::EditUrl {
                return Vec::new();
            }
            let space = self.active_space().clone();
            let sub = signet_core::i18n::t_args("palette.goToTab", &[("space", space.name.as_str())]);
            let mut ids: Vec<String> = signet_core::model::space_tab_ids(&space).into_iter().filter(|id| mode == PaletteMode::NewTab || space.active_tab_id.as_deref() != Some(id.as_str())).collect();
            ids.sort_by_key(|id| std::cmp::Reverse(self.state().tabs.get(id).map(|t| t.last_active).unwrap_or(0)));
            let mut score = 1000.0;
            for id in ids.into_iter().take(6) {
                if let Some(tab) = self.state().tabs.get(&id) {
                    rows.push(Scored(score, PaletteResult::Tab { id: format!("tab:{id}"), title: display_title(&tab.title, &tab.url), subtitle: Some(sub.clone()), tab_id: id.clone(), url: Some(tab.url.clone()), favicon: tab.favicon.clone(), inline: None }));
                    score -= 1.0;
                }
            }
            if mode == PaletteMode::NewTab {
                for want in ["split", "toggle-sidebar", "little", "reopen"] {
                    if let Some(a) = self.action_rows().into_iter().find(|r| matches!(r, PaletteResult::Action { action, .. } if action == want)) {
                        score -= 1.0;
                        rows.push(Scored(score, a));
                    }
                }
            }
            return finish(rows);
        }
        if let Some(hint) = self.search.keyword_hint_for(q) {
            rows.push(Scored(3000.0, PaletteResult::Keyword { id: format!("kw:{}", hint.id), title: hint.name.clone(), subtitle: None, keyword: signet_core::searchengines::keyword_infos(std::slice::from_ref(&hint)).into_iter().next().unwrap_or(signet_core::searchengines::KeywordInfo { id: hint.id.clone(), keyword: hint.keyword.clone(), name: hint.name.clone(), favicon: None }) }));
        }
        if let Some((engine, query_text, url)) = self.search.keyword_search(q)
            && !query_text.is_empty()
        {
            rows.push(Scored(2500.0, PaletteResult::Search { id: "kwsearch".into(), title: query_text, subtitle: Some(engine.name), url }));
        }
        let tr = |k: &str, a: &[(&str, String)]| {
            let args: Vec<(&str, &str)> = a.iter().map(|(k, v)| (*k, v.as_str())).collect();
            signet_core::i18n::t_args(k, &args)
        };
        for a in internal::internal_suggestions(&tr, q) {
            rows.push(Scored(2000.0, PaletteResult::Url { id: format!("int:{}", a.address), title: a.address.clone(), subtitle: Some(a.description), url: a.address }));
        }
        if let Some(target) = signet_core::devhosts::resolve_alias(q, &aliases) {
            rows.push(Scored(1500.0, PaletteResult::Url { id: format!("alias:{target}"), title: target.clone(), subtitle: None, url: target }));
        }
        if let Some(url) = to_url(q) {
            rows.push(Scored(900.0, PaletteResult::Url { id: format!("url:{url}"), title: url.clone(), subtitle: Some(t("palette.openAddress")), url }));
        }
        // Open tabs.
        let mut seen: HashSet<String> = HashSet::new();
        let ids: Vec<String> = self.state().tabs.keys().cloned().filter(|i| !self.store.data.detached.contains(i)).collect();
        for id in ids {
            let Some(tab) = self.state().tabs.get(&id) else { continue };
            let title = display_title(&tab.title, &tab.url);
            let m = match_quality(&fq, &fold(&title), &fold(&tab.url));
            if m > 0.0 {
                seen.insert(tab.url.clone());
                rows.push(Scored(m + 10.0, PaletteResult::Tab { id: format!("tab:{id}"), title, subtitle: Some(tab.url.clone()), tab_id: id.clone(), url: Some(tab.url.clone()), favicon: tab.favicon.clone(), inline: None }));
            }
        }
        // History.
        let hist = self.history.search(q, &seen, MAX_PAGES);
        for e in hist {
            let s = history_score(&fq, &ScoreInput { title: &e.title, url: &e.url, visits: e.visits, typed: e.typed.unwrap_or(0), last_visit: e.last_visit }, now);
            if s > 0.0 {
                seen.insert(e.url.clone());
                rows.push(Scored(s, PaletteResult::History { id: format!("hist:{}", e.url), title: display_title(&e.title, &e.url), subtitle: Some(e.url.clone()), url: e.url, favicon: e.favicon, inline: None }));
            }
        }
        // Archive.
        let mut archived = 0;
        for a in self.state().archive.clone() {
            if archived >= MAX_PAGES || seen.contains(&a.url) {
                continue;
            }
            let m = match_quality(&fq, &fold(&a.title), &fold(&a.url));
            if m > 0.0 {
                archived += 1;
                rows.push(Scored(0.4 * m, PaletteResult::Archived { id: format!("arch:{}", a.id.clone().unwrap_or_else(|| a.url.clone())), title: display_title(&a.title, &a.url), subtitle: Some(a.url.clone()), url: a.url, favicon: a.favicon, inline: None }));
            }
        }
        // Bookmarks of the space.
        if let Some(list) = self.active_space().bookmarks.clone() {
            collect_bookmarks(&list, &fq, &mut rows);
        }
        // Search row and actions.
        rows.push(Scored(45.0, PaletteResult::Search { id: "search".into(), title: q.to_string(), subtitle: Some(self.search.engine_name()), url: self.search.engine_search_url(q) }));
        for a in self.action_rows() {
            if let PaletteResult::Action { title, .. } = &a {
                let m = match_quality(&fq, &fold(title), "");
                if m > 0.0 {
                    rows.push(Scored(40.0 * m / 100.0 + 1.0, a));
                }
            }
        }
        finish(rows)
    }

    /// Engine suggestions for the text typed: the request of the default engine (or of the keyword's), as
    /// `(engine id, url, text, engine)`.
    fn suggest_request(&self, query: &str) -> Option<(String, String, String, Option<signet_core::searchengines::EngineSpec>)> {
        let q = query.trim();
        if q.is_empty() {
            return None;
        }
        let (engine, text) = match self.search.keyword_search(q) {
            Some((_, text, _)) if text.is_empty() => return None,
            Some((engine, text, _)) => (Some(engine), text),
            None => (None, q.to_string()),
        };
        let (id, url) = self.search.engine_suggestions(&text, engine.as_ref())?;
        Some((id, url, text, engine))
    }

    /// Starts the fetch of the suggestions of `query` in the background (once per address). The answer lands in the
    /// cache, where `palette_suggest` finds it.
    pub fn prefetch_suggestions(&mut self, query: &str) {
        let Some((id, url, text, _)) = self.suggest_request(query) else { return };
        if !self.suggest.begin(&url) {
            return;
        }
        let cache = self.suggest.clone();
        let asked = query.to_string();
        std::thread::spawn(move || {
            use signet_features::http::Http;
            let list = signet_features::http::UreqHttp
                .get(&url, &[("User-Agent", &signet_web::default_user_agent())], std::time::Duration::from_secs(4), 256 * 1024)
                .ok()
                .filter(|r| r.ok())
                .and_then(|r| serde_json::from_slice::<serde_json::Value>(&r.body).ok())
                .map(|j| parse_suggestions(&id, &j, &text, 4))
                .unwrap_or_default();
            cache.finish(url, text, list);
            // The tick tells the command bar (`UiEvent::PaletteSuggestions`).
            cache.announce(asked);
        });
    }

    /// Network suggestions of the engine. Never blocks: the answer of the request started by `palette_query` (or by
    /// the previous call) is in the cache once it arrived; until then the suggestions of a shorter text are filtered.
    /// Called by the tick: sends the answers that arrived since the last call to the command bar.
    pub fn flush_suggestions(&mut self) {
        for q in self.suggest.take_fresh() {
            let results = self.palette_suggest(&q);
            self.emit(signet_core::api::UiEvent::PaletteSuggestions { query: q, results });
        }
    }

    pub fn palette_suggest(&mut self, query: &str) -> Vec<PaletteResult> {
        let Some((_, url, text, engine)) = self.suggest_request(query) else { return Vec::new() };
        let list = match self.suggest.get(&url) {
            Some(list) => list,
            None => {
                self.prefetch_suggestions(query);
                let low = text.to_lowercase();
                self.suggest.last_for_prefix(&low).into_iter().filter(|s| s.to_lowercase().starts_with(&low) && s.to_lowercase() != low).collect()
            }
        };
        list.into_iter()
            .map(|s| {
                let url = match &engine {
                    Some(e) => self.search.search_url_at(e, &s),
                    None => self.search.engine_search_url(&s),
                };
                PaletteResult::Suggestion { id: format!("suggestion:{s}"), title: s, subtitle: None, url }
            })
            .collect()
    }

    pub fn palette_execute(&mut self, result: PaletteResult, mode: PaletteMode) {
        self.close_overlay(false);
        match result {
            PaletteResult::Url { url, .. } => self.open_url(&url, mode, true),
            PaletteResult::Search { url, .. } | PaletteResult::Suggestion { url, .. } => self.open_url(&url, mode, false),
            PaletteResult::History { url, .. } | PaletteResult::Archived { url, .. } => self.open_url(&url, mode, true),
            PaletteResult::Tab { tab_id, .. } => self.activate_tab(&tab_id),
            PaletteResult::Keyword { .. } => {}
            PaletteResult::Action { action, .. } => self.run_palette_action(&action),
        }
    }

    fn run_palette_action(&mut self, action: &str) {
        let id = self.active_tab_id();
        match action {
            "settings" => self.open_settings_window(None),
            "library" => self.open_library_window(None),
            "new-space" => self.edit_space(None),
            "split" => self.open_palette(PaletteMode::Split, None),
            "unsplit" => {
                if let Some(s) = id.as_deref().and_then(|i| self.is_split_item(i)) {
                    self.unsplit(&s);
                }
            }
            "toggle-sidebar" => self.toggle_sidebar(),
            "compact" => self.toggle_compact(),
            "tab-layout" => self.toggle_tab_layout(),
            "pin" => {
                if let Some(i) = id {
                    self.pin_toggle(&i);
                }
            }
            "copy-url" => self.copy_url(None),
            "little" | "new-note" | "new-easel" | "private" | "devtools" => self.run_command(match action {
                "little" => "newLittle",
                "new-note" => "newNote",
                "new-easel" => "newEasel",
                "private" => "newPrivate",
                _ => "devTools",
            }),
            "clear-today" => self.clear_today(),
            "close-tab" => {
                if let Some(i) = id {
                    self.close_tab(&i);
                }
            }
            "reopen" => self.reopen_closed(),
            "reload" => self.reload_tab(None, false),
            "new-folder" => self.create_folder(),
            "toggle-peek-links" => self.toggle_peek_links(),
            "toggle-adblock" => self.toggle_adblock(),
            "open-on-phone" => self.open_on_phone_choose(id.as_deref()),
            "install-app" => self.run_command("installApp"),
            "reader" => self.run_command("reader"),
            "translate" => self.run_command("translate"),
            other if other.starts_with("open-in-app:") => {
                if let Some(i) = id {
                    self.move_tab_to_web_app(&i, &other["open-in-app:".len()..]);
                }
            }
            other if other.starts_with("webapp:") => self.open_web_app(&other["webapp:".len()..]),
            other if other.starts_with("dev:") => {
                if let Some(i) = id {
                    self.run_develop_item(&i, other);
                }
            }
            other => self.log(format!("palette action {other} not handled")),
        }
    }
}

/// Cache of the engines' suggestions, filled by background requests.
#[derive(Clone, Default)]
pub struct SuggestCache(std::sync::Arc<std::sync::Mutex<SuggestInner>>);

#[derive(Default)]
struct SuggestInner {
    done: std::collections::HashMap<String, Vec<String>>,
    pending: HashSet<String>,
    /// Last answer, with the text it was for.
    last: Option<(String, Vec<String>)>,
    /// Queries whose answer just arrived, not yet announced to the interface.
    fresh: Vec<String>,
}

impl SuggestCache {
    /// False if the address is cached or already requested.
    fn begin(&self, url: &str) -> bool {
        let Ok(mut c) = self.0.lock() else { return false };
        if c.done.contains_key(url) || c.pending.contains(url) {
            return false;
        }
        c.pending.insert(url.to_string());
        true
    }

    fn finish(&self, url: String, text: String, list: Vec<String>) {
        let Ok(mut c) = self.0.lock() else { return };
        c.pending.remove(&url);
        if c.done.len() > 200 {
            c.done.clear();
        }
        c.last = Some((text.to_lowercase(), list.clone()));
        c.done.insert(url, list);
    }

    fn announce(&self, query: String) {
        if let Ok(mut c) = self.0.lock() {
            c.fresh.push(query);
        }
    }

    fn take_fresh(&self) -> Vec<String> {
        self.0.lock().map(|mut c| std::mem::take(&mut c.fresh)).unwrap_or_default()
    }

    fn get(&self, url: &str) -> Option<Vec<String>> {
        self.0.lock().ok()?.done.get(url).cloned()
    }

    /// Last answer if it was for a text that `low` extends.
    fn last_for_prefix(&self, low: &str) -> Vec<String> {
        let Ok(c) = self.0.lock() else { return Vec::new() };
        match &c.last {
            Some((t, list)) if low.starts_with(t.as_str()) => list.clone(),
            _ => Vec::new(),
        }
    }
}

const SUGGESTION_MAX_LEN: usize = 200;

/// Text of a suggestion item: a string or an object with `phrase`, `value`, `q` or `text`.
fn suggestion_text(v: &serde_json::Value) -> Option<String> {
    let s = match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Object(o) => ["phrase", "value", "q", "text"].iter().find_map(|k| o.get(*k).and_then(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty()))?,
        _ => return None,
    };
    (!s.is_empty()).then_some(s)
}

fn clean_suggestions(list: &[serde_json::Value], query: &str, limit: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    seen.insert(query.trim().to_lowercase());
    let mut out = Vec::new();
    for item in list {
        let Some(s) = suggestion_text(item) else { continue };
        if s.chars().count() > SUGGESTION_MAX_LEN || !seen.insert(s.to_lowercase()) {
            continue;
        }
        out.push(s);
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Suggestions from the JSON answer of engine `id` (port of `parseSuggestions`): Qwant, DuckDuckGo objects, and
/// OpenSearch (`["query", ["s1", ...]]`) for every other engine, custom ones included.
pub fn parse_suggestions(id: &str, json: &serde_json::Value, query: &str, limit: usize) -> Vec<String> {
    let open_search = || match json.as_array().and_then(|a| a.get(1)).and_then(|l| l.as_array()) {
        Some(l) => clean_suggestions(l, query, limit),
        None => Vec::new(),
    };
    match id {
        "qwant" => json.pointer("/data/items").and_then(|i| i.as_array()).map(|l| clean_suggestions(l, query, limit)).unwrap_or_default(),
        "duckduckgo" => match json.as_array() {
            Some(a) if a.first().is_some_and(|f| f.is_object()) => clean_suggestions(a, query, limit),
            _ => open_search(),
        },
        _ => open_search(),
    }
}

fn display_title(title: &str, url: &str) -> String {
    if title.trim().is_empty() { url.to_string() } else { title.to_string() }
}

fn collect_bookmarks(list: &[Bookmark], fq: &str, rows: &mut Vec<Scored>) {
    for b in list {
        match b {
            Bookmark::Link { id, title, url, favicon } => {
                let m = match_quality(fq, &fold(title), &fold(url));
                if m > 0.0 {
                    rows.push(Scored(m * 0.9, PaletteResult::History { id: format!("bm:{id}"), title: display_title(title, url), subtitle: Some(url.clone()), url: url.clone(), favicon: favicon.clone(), inline: None }));
                }
            }
            Bookmark::Folder { items, .. } => collect_bookmarks(items, fq, rows),
        }
    }
    let _ = bm::BOOKMARK_BAR;
}

fn finish(mut rows: Vec<Scored>) -> Vec<PaletteResult> {
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut seen = HashSet::new();
    rows.into_iter().filter(|r| seen.insert(row_id(&r.1))).take(MAX_RESULTS).map(|r| r.1).collect()
}

fn row_id(r: &PaletteResult) -> String {
    match r {
        PaletteResult::Url { id, .. }
        | PaletteResult::Search { id, .. }
        | PaletteResult::Suggestion { id, .. }
        | PaletteResult::Keyword { id, .. }
        | PaletteResult::History { id, .. }
        | PaletteResult::Archived { id, .. }
        | PaletteResult::Tab { id, .. }
        | PaletteResult::Action { id, .. } => id.clone(),
    }
}

#[allow(dead_code)]
fn _zone(_: Zone) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn open_search_answers_are_cleaned() {
        let j = json!(["re", ["react", "Re", "redux", "react", 3]]);
        assert_eq!(parse_suggestions("google", &j, "re", 4), vec!["react", "redux"]);
        assert!(parse_suggestions("google", &json!({"nope": 1}), "re", 4).is_empty());
    }

    #[test]
    fn qwant_and_duckduckgo_shapes() {
        let q = json!({"status": "success", "data": {"items": [{"value": "cats"}, {"value": "cat"}]}});
        assert_eq!(parse_suggestions("qwant", &q, "cat", 4), vec!["cats"]);
        let d = json!([{"phrase": "dogs"}, {"phrase": "dog food"}]);
        assert_eq!(parse_suggestions("duckduckgo", &d, "dog", 1), vec!["dogs"]);
    }

    #[test]
    fn cache_starts_each_address_once() {
        let c = SuggestCache::default();
        assert!(c.begin("u"));
        assert!(!c.begin("u"));
        c.finish("u".into(), "ab".into(), vec!["abc".into()]);
        assert_eq!(c.get("u"), Some(vec!["abc".into()]));
        assert!(!c.begin("u"));
        assert_eq!(c.last_for_prefix("abc"), vec!["abc".to_string()]);
        assert!(c.last_for_prefix("x").is_empty());
    }
}


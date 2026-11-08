//! PHASE3: reader mode and page translation (`signet_features::reader`, `translate`).
//!
//! Port of `main/features/reader.ts`. Electron ran the scripts in isolated worlds and read their results from the
//! returned promises. `signet-web` only has fire and forget `execute_js_in` (main world), so every script here is
//! wrapped: it posts its results back through `window.__signetPost` on the private channels `sg:reader` and
//! `sg:translate`. `web_event` sees those messages first (the generic page router drops them) and checks:
//! the view is a tab, the frame is the main frame, and the payload carries the per-run secret `k`.
//!
//! The toolbar buttons read `TabInfo.reader` (`available` / `on`), `foreign_lang` and `translation` (`working` / `on`);
//! this module patches them with `Core::patch_tab`.

mod translate;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use serde_json::{json, Value};
use signet_features::reader as feat;
use signet_web::{FrameTarget, WebEvent};

use crate::browser::Core;

/// Private channel of reader messages.
pub(crate) const CHANNEL_READER: &str = "sg:reader";
/// Private channel of translation messages.
pub(crate) const CHANNEL_TRANSLATE: &str = "sg:translate";

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    /// Secret embedded in the scripts we inject: messages without it are ignored.
    token: String,
    /// Token of the examination and of the reading in progress per tab: a navigation invalidates them.
    runs: HashMap<String, u64>,
    seq: u64,
    probes: feat::Probes,
    /// Tabs to re-examine after an in-page navigation, with the time to do it.
    due: HashMap<String, Instant>,
    /// Tabs whose reader view was found "already" present and is being reopened.
    reopening: HashSet<String>,
    translate: translate::Translate,
}

// ---------------------------------------------------------------------------------------------------- scripts

/// Start of every wrapper: defines `P(o)`, which posts `o` (plus the secret and the run) to the app.
fn prelude(channel: &str, token: &str, run: u64) -> String {
    let q = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
    format!("var P=function(o){{o.k={};o.run={run};try{{window.__signetPost({},JSON.stringify(o))}}catch(e){{}}}};", q(token), q(channel))
}

/// Examines the page and posts `{ op: "probe", url, readerable, lang }`. HTML documents only.
pub(crate) fn probe_js(token: &str, run: u64, probe_script: &str) -> String {
    format!(
        "(function(){{{}if(!/^(text\\/html|application\\/xhtml\\+xml)$/i.test(document.contentType||''))return;\
         try{{Promise.resolve({probe_script}).then(function(r){{P({{op:'probe',url:location.href,readerable:!!(r&&r.readerable),lang:String((r&&r.lang)||'')}})}},function(){{}})}}catch(e){{}}}})()",
        prelude(CHANNEL_READER, token, run)
    )
}

/// Opens the reader view, posts `{ op: "opened", result }` (`open`, `none`, `already`, `error`) and, when it opened,
/// forwards every message of the view (`{ op: "msg", msg }`) until it closes.
pub(crate) fn open_js(token: &str, run: u64, open_script: &str) -> String {
    format!(
        "(function(){{{}var R;try{{R={open_script}}}catch(e){{P({{op:'opened',result:'error'}});return}}\
         P({{op:'opened',result:R}});if(R!=='open')return;\
         (async function(){{for(;;){{var r=window.__signetReader;if(!r)return;var m=await r.next();if(!m)return;P({{op:'msg',msg:m}});if(m.type==='close')return}}}})()}})()",
        prelude(CHANNEL_READER, token, run)
    )
}

/// Labels of the reader bar, in the interface language.
fn labels() -> HashMap<String, String> {
    use signet_core::i18n::{t, t_args};
    feat::LABEL_KEYS
        .iter()
        .map(|k| {
            let text = if *k == "minutes" { t_args("reader.minutes", &[("n", "{n}")]) } else { t(&format!("reader.{k}")) };
            (k.to_string(), text)
        })
        .collect()
}

/// What an in-page navigation of a tab being read must do.
#[derive(Debug, PartialEq, Eq)]
enum InPage {
    /// Anchor change under an open reading: nothing to do.
    Ignore,
    /// The content changed under the reading: close it, then re-examine.
    CloseThenProbe,
    Probe,
}

fn in_page_action(reading: bool, base_changed: bool) -> InPage {
    match (reading, base_changed) {
        (true, false) => InPage::Ignore,
        (true, true) => InPage::CloseThenProbe,
        _ => InPage::Probe,
    }
}

// ---------------------------------------------------------------------------------------------------- hooks

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    core.phase3.reader.token = signet_core::ids::new_id();
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    match ev {
        WebEvent::PageMessage { view, is_main_frame: true, channel, payload, .. } if channel == CHANNEL_READER || channel == CHANNEL_TRANSLATE => {
            if let Some(tab) = core.views.tab_of(*view).map(str::to_owned)
                && let Ok(v) = serde_json::from_str::<Value>(payload)
                && v.get("k").and_then(Value::as_str) == Some(core.phase3.reader.token.as_str())
            {
                if channel == CHANNEL_READER {
                    core.reader_message(&tab, &v);
                } else {
                    core.translate_message(&tab, &v);
                }
            }
        }
        WebEvent::LoadStart { view, is_main_frame: true, .. } => {
            if let Some(tab) = core.views.tab_of(*view).map(str::to_owned) {
                core.reader_on_navigate(&tab);
            }
        }
        WebEvent::LoadEnd { view, is_main_frame: true, .. } => {
            if let Some(tab) = core.views.tab_of(*view).map(str::to_owned) {
                core.reader_probe(&tab);
            }
        }
        WebEvent::AddressChanged { view, url, is_main_frame: true } => {
            if let Some(tab) = core.views.tab_of(*view).map(str::to_owned) {
                core.reader_in_page(&tab, url);
            }
        }
        WebEvent::Closed { view } => {
            if let Some(tab) = core.views.tab_of(*view).map(str::to_owned) {
                core.reader_forget(&tab);
            }
        }
        _ => {}
    }
    core.reader_housekeeping();
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    core.reader_housekeeping();
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    core.phase3.reader.due.clear();
    core.phase3.reader.translate.stop();
}

// ---------------------------------------------------------------------------------------------------- Core

impl Core {
    /// Patches the reader and translation fields of a tab (`Value::Null` clears one).
    pub(super) fn reader_patch(&mut self, tab: &str, fields: Value) {
        if let Value::Object(map) = fields {
            self.patch_tab(tab, map, false, false);
        }
    }

    fn reader_run(&self, tab: &str) -> u64 {
        self.phase3.reader.runs.get(tab).copied().unwrap_or(0)
    }

    fn reader_bump(&mut self, tab: &str) -> u64 {
        let st = &mut self.phase3.reader;
        st.seq += 1;
        st.runs.insert(tab.to_owned(), st.seq);
        st.seq
    }

    /// True if the tab shows a page where reading can be attempted (even if the examination did not offer it).
    pub fn can_read_tab(&self, tab: &str) -> bool {
        self.views.view_of(tab).is_some_and(|v| feat::can_read(&v.url()))
    }

    /// Reader mode on or off for a tab (toolbar button, menu, palette).
    pub fn toggle_reader(&mut self, tab_id: &str) {
        let Some(tab) = self.state().tabs.get(tab_id) else { return };
        if tab.reader == Some(signet_core::types::ReaderState::On) {
            self.reader_close(tab_id);
        } else {
            self.reader_open(tab_id);
        }
    }

    fn reader_close(&mut self, tab: &str) {
        if let Some(v) = self.views.view_of(tab) {
            v.execute_js_in(&FrameTarget::Main, feat::CLOSE_SCRIPT);
        }
        self.reader_patch(tab, json!({ "reader": "available" }));
    }

    fn reader_open(&mut self, tab: &str) {
        let Some(v) = self.views.view_of(tab) else { return };
        if !feat::can_read(&v.url()) {
            return;
        }
        let run = self.reader_run(tab);
        let prefs = self.state().settings.reader.clone().unwrap_or_default();
        let script = feat::open_script_default(&prefs, &labels());
        let js = open_js(&self.phase3.reader.token, run, &script);
        self.phase3.reader.reopening.remove(tab);
        v.execute_js_in(&FrameTarget::Main, &js);
    }

    /// Main frame navigation: the page changes, the reading and the translation close by themselves.
    fn reader_on_navigate(&mut self, tab: &str) {
        self.reader_bump(tab);
        self.phase3.reader.due.remove(tab);
        self.phase3.reader.reopening.remove(tab);
        self.translate_on_navigate(tab);
        if self.state().tabs.get(tab).is_some_and(|t| t.reader.is_some() || t.foreign_lang.is_some()) {
            self.reader_patch(tab, json!({ "reader": null, "foreignLang": null }));
        }
    }

    /// The tab no longer has a view: forget what we know about it.
    fn reader_forget(&mut self, tab: &str) {
        let st = &mut self.phase3.reader;
        st.runs.remove(tab);
        st.due.remove(tab);
        st.reopening.remove(tab);
        st.probes.forget(tab);
        self.translate_forget(tab);
        if self.state().tabs.get(tab).is_some_and(|t| t.reader.is_some() || t.foreign_lang.is_some() || t.translation.is_some()) {
            self.reader_patch(tab, json!({ "reader": null, "foreignLang": null, "translation": null }));
        }
    }

    /// Examines the loaded page: readable article? foreign language? (updates the tab).
    fn reader_probe(&mut self, tab: &str) {
        let Some(v) = self.views.view_of(tab) else { return };
        let url = v.url();
        if !feat::can_read(&url) {
            if self.state().tabs.get(tab).is_some_and(|t| t.reader.is_some() || t.foreign_lang.is_some()) {
                self.reader_patch(tab, json!({ "reader": null, "foreignLang": null }));
            }
            return;
        }
        self.phase3.reader.probes.record(tab, &url, signet_core::ids::now_ms());
        let run = self.reader_run(tab);
        v.execute_js_in(&FrameTarget::Main, &probe_js(&self.phase3.reader.token, run, &feat::probe_script_default()));
    }

    /// In-page navigation (single-page site): the content changes under the reading, which is closed, then we re-examine.
    fn reader_in_page(&mut self, tab: &str, url: &str) {
        let Some(info) = self.state().tabs.get(tab) else { return };
        // A navigation in progress ends with a load event, which examines the page.
        if info.loading {
            return;
        }
        let reading = info.reader == Some(signet_core::types::ReaderState::On);
        let base_changed = self.phase3.reader.probes.must_close_reading(tab, url);
        match in_page_action(reading, base_changed) {
            InPage::Ignore => return,
            InPage::CloseThenProbe => self.reader_close(tab),
            InPage::Probe => {}
        }
        let now = signet_core::ids::now_ms();
        if let Some(delay) = self.phase3.reader.probes.reprobe_delay(tab, url, now) {
            self.phase3.reader.due.insert(tab.to_owned(), Instant::now() + delay);
        }
    }

    /// Runs the delayed examinations and the translation results that are ready.
    fn reader_housekeeping(&mut self) {
        if !self.phase3.reader.due.is_empty() {
            let now = Instant::now();
            let ready: Vec<String> = self.phase3.reader.due.iter().filter(|(_, at)| **at <= now).map(|(t, _)| t.clone()).collect();
            for tab in ready {
                self.phase3.reader.due.remove(&tab);
                self.reader_probe(&tab);
            }
        }
        self.translate_pump();
    }

    /// Message of a reader script (`sg:reader`).
    fn reader_message(&mut self, tab: &str, v: &Value) {
        if v.get("run").and_then(Value::as_u64) != Some(self.reader_run(tab)) {
            return;
        }
        match v.get("op").and_then(Value::as_str) {
            Some("probe") => self.reader_probed(tab, v),
            Some("opened") => self.reader_opened(tab, v.get("result").and_then(Value::as_str).unwrap_or("")),
            Some("msg") => {
                match v.get("msg").and_then(feat::parse_message) {
                    Some(feat::ReaderMessage::Prefs(p)) => {
                        self.store.data.state.settings.reader = Some(p);
                        self.changed();
                    }
                    Some(feat::ReaderMessage::Close) => {
                        if self.state().tabs.get(tab).is_some_and(|t| t.reader == Some(signet_core::types::ReaderState::On)) {
                            self.reader_patch(tab, json!({ "reader": "available" }));
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }

    fn reader_probed(&mut self, tab: &str, v: &Value) {
        let Some(view) = self.views.view_of(tab) else { return };
        if v.get("url").and_then(Value::as_str) != Some(view.url().as_str()) {
            return;
        }
        let Some(info) = self.state().tabs.get(tab) else { return };
        let on = info.reader == Some(signet_core::types::ReaderState::On);
        let readerable = v.get("readerable").and_then(Value::as_bool) == Some(true);
        let lang = v.get("lang").and_then(Value::as_str).unwrap_or("");
        let foreign = signet_features::translate::foreign(lang, &self.state().settings.languages);
        let reader = if on { json!("on") } else if readerable { json!("available") } else { Value::Null };
        self.reader_patch(tab, json!({ "reader": reader, "foreignLang": foreign }));
    }

    fn reader_opened(&mut self, tab: &str, result: &str) {
        match result {
            "already" => {
                // View already present without a listener: close it (the old one stops), then open it again.
                if !self.phase3.reader.reopening.insert(tab.to_owned()) {
                    self.phase3.reader.reopening.remove(tab);
                    return;
                }
                self.reader_bump(tab);
                if let Some(v) = self.views.view_of(tab) {
                    v.execute_js_in(&FrameTarget::Main, feat::CLOSE_SCRIPT);
                }
                let run = self.reader_run(tab);
                let prefs = self.state().settings.reader.clone().unwrap_or_default();
                let js = open_js(&self.phase3.reader.token, run, &feat::open_script_default(&prefs, &labels()));
                if let Some(v) = self.views.view_of(tab) {
                    v.execute_js_in(&FrameTarget::Main, &js);
                }
            }
            "none" => {
                self.phase3.reader.reopening.remove(tab);
                self.reader_patch(tab, json!({ "reader": null }));
            }
            "open" => {
                self.phase3.reader.reopening.remove(tab);
                if let Some(v) = self.views.view_of(tab) {
                    self.phase3.reader.probes.set_base(tab, &v.url());
                }
                self.reader_patch(tab, json!({ "reader": "on" }));
            }
            other => {
                self.phase3.reader.reopening.remove(tab);
                self.log(format!("reader: the view did not open ({other})"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prelude_embeds_the_secret_and_the_run() {
        let p = prelude(CHANNEL_READER, "tok\"en", 7);
        assert!(p.contains("o.k=\"tok\\\"en\""));
        assert!(p.contains("o.run=7"));
        assert!(p.contains("window.__signetPost(\"sg:reader\""));
    }

    #[test]
    fn probe_script_is_wrapped() {
        let js = probe_js("t", 3, "(() => ({ readerable: true, lang: 'fr' }))()");
        assert!(js.starts_with("(function(){var P="));
        assert!(js.contains("Promise.resolve((() => ({ readerable: true, lang: 'fr' }))())"));
        assert!(js.contains("op:'probe'"));
        assert!(js.contains("document.contentType"));
        assert!(js.ends_with("})()"));
    }

    #[test]
    fn open_script_posts_its_result_and_listens() {
        let js = open_js("t", 1, "(() => 'open')()");
        assert!(js.contains("R=(() => 'open')()"));
        assert!(js.contains("op:'opened'"));
        assert!(js.contains("r.next()"));
        assert!(js.contains("m.type==='close'"));
    }

    #[test]
    fn real_scripts_are_embedded_without_placeholders() {
        let js = open_js("t", 1, &feat::open_script_default(&Default::default(), &labels()));
        assert!(!js.contains("__READABILITY__") && !js.contains("__PREFS__") && !js.contains("__CSS__"));
        let probe = probe_js("t", 1, &feat::probe_script_default());
        assert!(!probe.contains("__READERABLE__"));
    }

    #[test]
    fn labels_cover_every_key() {
        let l = labels();
        assert_eq!(l.len(), feat::LABEL_KEYS.len());
        assert!(l["minutes"].contains("{n}"));
    }

    #[test]
    fn in_page_rules() {
        assert_eq!(in_page_action(true, false), InPage::Ignore);
        assert_eq!(in_page_action(true, true), InPage::CloseThenProbe);
        assert_eq!(in_page_action(false, true), InPage::Probe);
        assert_eq!(in_page_action(false, false), InPage::Probe);
    }
}

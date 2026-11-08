//! Page translation (port of `Translator` in `main/features/translate.ts`).
//!
//! The page script (`translate.js`) cuts the visible text into batches. Wrapper (see `start_js`): it posts every batch
//! on `sg:translate`; a worker thread sends them to the service one after the other; the results wait in a shared
//! list until the main thread applies them. The main thread wakes on the page messages (`poll` every 250 ms while a
//! batch is outstanding, which also covers a thread finishing while the app is idle) and on the core tick.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use signet_core::types::{ReaderState, TranslationState};
use signet_features::http::UreqHttp;
use signet_features::translate as feat;
use signet_web::FrameTarget;

use super::{prelude, CHANNEL_TRANSLATE};
use crate::browser::Core;

/// Id of the title in a `Work` (page batches use ids >= 0).
const TITLE_ID: i64 = -1;
/// Stops the polling timer of the wrapper.
const STOP_JS: &str = "if(window.__sgTrIv){clearInterval(window.__sgTrIv);window.__sgTrIv=0}";

/// A batch for the worker.
struct Work {
    tab: String,
    run: u64,
    ids: Vec<i64>,
    texts: Vec<String>,
    html: bool,
    target: String,
}

/// A batch the worker finished.
struct Done {
    tab: String,
    run: u64,
    ids: Vec<i64>,
    result: Result<Vec<String>, String>,
}

#[derive(Default)]
pub(super) struct Translate {
    runs: Arc<Mutex<feat::Runs>>,
    done: Arc<Mutex<Vec<Done>>>,
    worker: Option<Sender<Work>>,
    /// Batches applied per run, to tell a total failure from a partial one.
    applied: HashMap<String, u32>,
}

impl Translate {
    fn submit(&mut self, work: Work) {
        if self.worker.is_none() {
            let (tx, rx) = channel::<Work>();
            let runs = self.runs.clone();
            let done = self.done.clone();
            let spawned = std::thread::Builder::new().name("signet-translate".into()).spawn(move || {
                while let Ok(w) = rx.recv() {
                    if !runs.lock().map(|r| r.is_current(&w.tab, w.run)).unwrap_or(false) {
                        continue;
                    }
                    let result = feat::translate_batch(&UreqHttp, &w.texts, &w.target, w.html, &|d| std::thread::sleep(d));
                    if let Ok(mut d) = done.lock() {
                        d.push(Done { tab: w.tab, run: w.run, ids: w.ids, result });
                    }
                }
            });
            if spawned.is_err() {
                return;
            }
            self.worker = Some(tx);
        }
        if let Some(tx) = &self.worker {
            let _ = tx.send(work);
        }
    }

    fn is_current(&self, tab: &str, run: u64) -> bool {
        self.runs.lock().map(|r| r.is_current(tab, run)).unwrap_or(false)
    }

    fn cancel(&mut self, tab: &str) {
        if let Ok(mut r) = self.runs.lock() {
            r.cancel(tab);
        }
        self.applied.remove(tab);
    }

    fn has_done(&self) -> bool {
        self.done.lock().map(|d| !d.is_empty()).unwrap_or(false)
    }

    fn take_done(&self) -> Vec<Done> {
        self.done.lock().map(|mut d| std::mem::take(&mut *d)).unwrap_or_default()
    }

    /// The worker ends when the sender is dropped.
    pub(super) fn stop(&mut self) {
        self.worker = None;
    }
}

/// What to do with a finished batch.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// Insert the translations (or the title) in the page.
    Apply,
    /// Lasting failure after some batches were applied: what is translated stays, we stop there.
    KeepPartial,
    /// Lasting failure before anything was applied: back to the original.
    Restore,
}

fn outcome(ok: bool, applied: u32) -> Outcome {
    match (ok, applied) {
        (true, _) => Outcome::Apply,
        (false, 0) => Outcome::Restore,
        (false, _) => Outcome::KeepPartial,
    }
}

/// Wrapper that installs the page script, then posts the title (`installed`), every batch (`batch`), a `poll` while
/// batches are outstanding and a `settle` after `SETTLE_MS`.
pub(super) fn start_js(token: &str, run: u64, install: &str) -> String {
    format!(
        "(function(){{{}window.__sgPending=0;\
         Promise.resolve({install}).then(function(){{\
         var tr=window.__signetTr;if(!tr)return;\
         P({{op:'installed',title:String(tr.title()||'')}});\
         window.__sgTrIv=setInterval(function(){{if(!window.__signetTr){{clearInterval(window.__sgTrIv);window.__sgTrIv=0;return}}if(window.__sgPending>0)P({{op:'poll'}})}},250);\
         setTimeout(function(){{P({{op:'settle'}})}},{});\
         (async function(){{for(;;){{var t=window.__signetTr;if(!t)return;var b=await t.next();if(!b)return;window.__sgPending++;P({{op:'batch',ids:b.ids,texts:b.texts}})}}}})()\
         }}).catch(function(){{}})}})()",
        prelude(CHANNEL_TRANSLATE, token, run),
        feat::SETTLE_MS
    )
}

/// Script that inserts a translated batch and counts it as no longer outstanding.
fn apply_js(ids: &[i64], htmls: &[String]) -> String {
    format!("window.__sgPending=Math.max(0,(window.__sgPending||0)-1);{}", feat::apply_script(ids, htmls))
}

/// Script that puts the original page back and stops the timer.
fn restore_js() -> String {
    format!("{};{STOP_JS}", feat::RESTORE_SCRIPT)
}

/// Parses the `batch` message: ids and texts of the same length, bounded.
fn parse_batch(v: &Value) -> Option<(Vec<i64>, Vec<String>)> {
    let ids: Vec<i64> = v.get("ids")?.as_array()?.iter().map(|x| x.as_i64()).collect::<Option<_>>()?;
    let texts: Vec<String> = v.get("texts")?.as_array()?.iter().map(|x| x.as_str().map(str::to_owned)).collect::<Option<_>>()?;
    (!ids.is_empty() && ids.len() == texts.len() && ids.iter().all(|i| *i >= 0)).then_some((ids, texts))
}

/// Name of a language in the interface language, for "Translate to {language}". Unknown codes show as the code.
pub(super) fn language_name(code: &str, french: bool) -> String {
    const NAMES: [(&str, &str, &str); 28] = [
        ("en", "English", "anglais"),
        ("fr", "French", "français"),
        ("de", "German", "allemand"),
        ("es", "Spanish", "espagnol"),
        ("it", "Italian", "italien"),
        ("pt", "Portuguese", "portugais"),
        ("nl", "Dutch", "néerlandais"),
        ("sv", "Swedish", "suédois"),
        ("da", "Danish", "danois"),
        ("no", "Norwegian", "norvégien"),
        ("fi", "Finnish", "finnois"),
        ("pl", "Polish", "polonais"),
        ("cs", "Czech", "tchèque"),
        ("hu", "Hungarian", "hongrois"),
        ("ro", "Romanian", "roumain"),
        ("el", "Greek", "grec"),
        ("tr", "Turkish", "turc"),
        ("ru", "Russian", "russe"),
        ("uk", "Ukrainian", "ukrainien"),
        ("ar", "Arabic", "arabe"),
        ("he", "Hebrew", "hébreu"),
        ("hi", "Hindi", "hindi"),
        ("ja", "Japanese", "japonais"),
        ("ko", "Korean", "coréen"),
        ("zh-CN", "Chinese (Simplified)", "chinois (simplifié)"),
        ("zh-TW", "Chinese (Traditional)", "chinois (traditionnel)"),
        ("vi", "Vietnamese", "vietnamien"),
        ("id", "Indonesian", "indonésien"),
    ];
    NAMES.iter().find(|(c, _, _)| c.eq_ignore_ascii_case(code)).map(|(_, en, fr)| if french { *fr } else { *en }).unwrap_or(code).to_owned()
}

impl Core {
    /// Target language code of the service (first preferred site language, else English).
    pub fn translation_target(&self) -> String {
        feat::target(&self.state().settings.languages)
    }

    /// Name of the target language in the interface language ("French").
    pub fn translation_target_name(&self) -> String {
        language_name(&self.translation_target(), self.locale() == signet_core::i18n::Locale::Fr)
    }

    /// True if the tab's page can be translated.
    pub fn can_translate_tab(&self, tab: &str) -> bool {
        self.views.view_of(tab).is_some_and(|v| feat::can_translate(&v.url()))
    }

    /// Translation on or off for a tab (toolbar button, menu, palette).
    pub fn toggle_translation(&mut self, tab_id: &str) {
        let Some(tab) = self.state().tabs.get(tab_id) else { return };
        if tab.translation.is_some() {
            self.translate_restore(tab_id);
        } else {
            self.translate_start(tab_id);
        }
    }

    /// Main frame navigation: the original page comes back by itself, the state is forgotten.
    pub(super) fn translate_on_navigate(&mut self, tab: &str) {
        self.phase3.reader.translate.cancel(tab);
        if self.state().tabs.get(tab).is_some_and(|t| t.translation.is_some()) {
            self.reader_patch(tab, json!({ "translation": null }));
        }
    }

    pub(super) fn translate_forget(&mut self, tab: &str) {
        self.phase3.reader.translate.cancel(tab);
    }

    fn translate_restore(&mut self, tab: &str) {
        self.phase3.reader.translate.cancel(tab);
        if let Some(v) = self.views.view_of(tab) {
            v.execute_js_in(&FrameTarget::Main, &restore_js());
        }
        self.reader_patch(tab, json!({ "translation": null }));
    }

    fn translate_start(&mut self, tab: &str) {
        if !self.can_translate_tab(tab) {
            return;
        }
        // Reader mode leaves the original page under the view: nothing to translate.
        if self.state().tabs.get(tab).is_some_and(|t| t.reader == Some(ReaderState::On)) {
            return;
        }
        let run = {
            let tr = &mut self.phase3.reader.translate;
            tr.applied.insert(tab.to_owned(), 0);
            tr.runs.lock().map(|mut r| r.start(tab)).unwrap_or(0)
        };
        self.reader_patch(tab, json!({ "translation": "working" }));
        if let Some(v) = self.views.view_of(tab) {
            v.execute_js_in(&FrameTarget::Main, &start_js(&self.phase3.reader.token, run, &feat::install_script()));
        }
    }

    /// Message of the translation wrapper (`sg:translate`).
    pub(super) fn translate_message(&mut self, tab: &str, v: &Value) {
        let Some(run) = v.get("run").and_then(Value::as_u64) else { return };
        if !self.phase3.reader.translate.is_current(tab, run) {
            return;
        }
        let target = self.translation_target();
        match v.get("op").and_then(Value::as_str) {
            Some("installed") => {
                // The tab title follows the page.
                if let Some(title) = v.get("title").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    let work = Work { tab: tab.to_owned(), run, ids: vec![TITLE_ID], texts: vec![title.chars().take(500).collect()], html: false, target };
                    self.phase3.reader.translate.submit(work);
                }
            }
            Some("batch") => {
                if let Some((ids, texts)) = parse_batch(v) {
                    self.phase3.reader.translate.submit(Work { tab: tab.to_owned(), run, ids, texts, html: true, target });
                }
            }
            Some("settle") => {
                // With nothing left to translate soon, the page is considered translated.
                if self.state().tabs.get(tab).is_some_and(|t| t.translation == Some(TranslationState::Working)) {
                    self.reader_patch(tab, json!({ "translation": "on" }));
                }
            }
            _ => {}
        }
        self.translate_pump();
    }

    /// Applies the results the worker finished.
    pub(super) fn translate_pump(&mut self) {
        if !self.phase3.reader.translate.has_done() {
            return;
        }
        for d in self.phase3.reader.translate.take_done() {
            if !self.phase3.reader.translate.is_current(&d.tab, d.run) {
                continue;
            }
            let applied = self.phase3.reader.translate.applied.get(&d.tab).copied().unwrap_or(0);
            let view = self.views.view_of(&d.tab);
            match (outcome(d.result.is_ok(), applied), d.result) {
                (Outcome::Apply, Ok(out)) => {
                    let Some(v) = view else { continue };
                    if d.ids == [TITLE_ID] {
                        if let Some(title) = out.first() {
                            v.execute_js_in(&FrameTarget::Main, &feat::set_title_script(title));
                        }
                    } else {
                        v.execute_js_in(&FrameTarget::Main, &apply_js(&d.ids, &out));
                        *self.phase3.reader.translate.applied.entry(d.tab.clone()).or_insert(0) += 1;
                        self.reader_patch(&d.tab, json!({ "translation": "on" }));
                    }
                }
                (Outcome::KeepPartial, Err(e)) => {
                    self.log(format!("translation: {e}"));
                    self.phase3.reader.translate.cancel(&d.tab);
                    if let Some(v) = view {
                        v.execute_js_in(&FrameTarget::Main, STOP_JS);
                    }
                    self.reader_patch(&d.tab, json!({ "translation": "on" }));
                }
                (Outcome::Restore, Err(e)) => {
                    eprintln!("Translation: {e}");
                    let (msg, detail) = (signet_core::i18n::t("translate.failed"), signet_core::i18n::t("translate.failedDetail"));
                    std::thread::spawn(move || crate::phase3::backup::dialog::notify(crate::phase3::backup::dialog::Level::Warning, &msg, &detail, "OK"));
                    self.translate_restore(&d.tab);
                }
                _ => {}
            }
        }
    }
}

#[allow(dead_code)]
const _: Duration = Duration::from_millis(feat::SETTLE_MS);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_policy() {
        assert_eq!(outcome(true, 0), Outcome::Apply);
        assert_eq!(outcome(false, 0), Outcome::Restore);
        assert_eq!(outcome(false, 3), Outcome::KeepPartial);
    }

    #[test]
    fn batch_messages_are_checked() {
        let ok = json!({ "ids": [0, 1], "texts": ["<a i=0>a</a>", "b"] });
        assert_eq!(parse_batch(&ok), Some((vec![0, 1], vec!["<a i=0>a</a>".to_string(), "b".to_string()])));
        assert!(parse_batch(&json!({ "ids": [0], "texts": ["a", "b"] })).is_none());
        assert!(parse_batch(&json!({ "ids": [-1], "texts": ["a"] })).is_none());
        assert!(parse_batch(&json!({ "ids": [], "texts": [] })).is_none());
        assert!(parse_batch(&json!({ "ids": ["x"], "texts": ["a"] })).is_none());
        assert!(parse_batch(&json!({ "ids": [0], "texts": [1] })).is_none());
    }

    #[test]
    fn wrapper_scripts() {
        let js = start_js("tok", 4, "(() => true)()");
        assert!(js.contains("o.run=4") && js.contains("\"sg:translate\""));
        assert!(js.contains("Promise.resolve((() => true)())"));
        assert!(js.contains("op:'batch'") && js.contains("op:'poll'") && js.contains("op:'settle'"));
        assert!(js.contains("1500"));
        let a = apply_js(&[0, 2], &["x".to_string()]);
        assert!(a.starts_with("window.__sgPending=Math.max(0,"));
        assert!(a.contains("apply([0,2], [\"x\"])"));
        assert!(restore_js().contains("restore()") && restore_js().contains("clearInterval"));
    }

    #[test]
    fn embedded_install_script_is_complete() {
        assert!(!start_js("t", 1, &feat::install_script()).contains("__BATCH_CHARS__"));
    }

    #[test]
    fn language_names() {
        assert_eq!(language_name("fr", false), "French");
        assert_eq!(language_name("fr", true), "français");
        assert_eq!(language_name("ZH-cn", false), "Chinese (Simplified)");
        assert_eq!(language_name("xx", false), "xx");
    }

    #[test]
    fn worker_skips_stale_runs_and_shares_results() {
        let mut tr = Translate::default();
        let run = tr.runs.lock().unwrap().start("t1");
        assert!(tr.is_current("t1", run));
        tr.cancel("t1");
        assert!(!tr.is_current("t1", run));
        // A stale batch is dropped without any network call.
        tr.submit(Work { tab: "t1".into(), run, ids: vec![0], texts: vec!["a".into()], html: true, target: "fr".into() });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!tr.has_done());
        tr.stop();
    }
}

//! Settings > Search engines (port of `SearchSettings.tsx`): keywords for the command bar, site searches and
//! custom engines.

use gpui::AppContext as _;
use std::collections::HashMap;

use gpui::{Context, Entity, Focusable, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window};
use gpui_component::input::{InputEvent, InputState};
use serde_json::json;
use signet_core::searchengines::{
    CustomEngine, DEFAULT_SEARCH_ENGINE, EngineConfig, EngineDraft, EngineDraftError, EngineSource, EngineSpec, KeywordOverride, KeywordOverrides, engine_favicon,
    normalize_keyword, resolve_engines, validate_engine_draft,
};

use super::kit::{self, Btn};
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{tr, tr_args};
use crate::icon::IconName;
use crate::theme::space;
use crate::widgets::{Favicon, FaviconSource};
use signet_core::i18n::current_locale;

fn error_key(e: EngineDraftError) -> &'static str {
    match e {
        EngineDraftError::Name => "settings.search.errorName",
        EngineDraftError::Keyword => "settings.search.errorKeyword",
        EngineDraftError::KeywordTaken => "settings.search.errorKeywordTaken",
        EngineDraftError::Template => "settings.search.errorTemplate",
        EngineDraftError::Suggest => "settings.search.errorSuggest",
    }
}

/// Keyword overrides after changing the keyword and / or the disabled flag of a built-in or preset engine.
pub fn with_override(overrides: &KeywordOverrides, engine: &EngineSpec, keyword: Option<Option<&str>>, disabled: Option<bool>) -> KeywordOverrides {
    let mut next = overrides.clone();
    let mut cur: KeywordOverride = next.get(&engine.id).cloned().unwrap_or_default();
    if let Some(k) = keyword {
        match k {
            Some(k) if !k.is_empty() && Some(k) != engine.default_keyword.as_deref() => cur.keyword = Some(k.to_string()),
            _ => cur.keyword = None,
        }
    }
    if let Some(d) = disabled {
        cur.disabled = if d { Some(true) } else { None };
    }
    if cur.keyword.is_some() || cur.disabled.is_some() {
        next.insert(engine.id.clone(), cur);
    } else {
        next.shift_remove(&engine.id);
    }
    next
}

struct Draft {
    id: Option<String>,
    name: Entity<InputState>,
    keyword: Entity<InputState>,
    template: Entity<InputState>,
    suggest: Entity<InputState>,
}

pub struct SearchSettings {
    store: Entity<InfoStore>,
    draft: Option<Draft>,
    error: Option<String>,
    keywords: HashMap<String, Entity<InputState>>,
    keyword_errors: HashMap<String, String>,
}

impl SearchSettings {
    pub fn new(store: Entity<InfoStore>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        Self { store, draft: None, error: None, keywords: HashMap::new(), keyword_errors: HashMap::new() }
    }

    fn engines(&self, cx: &gpui::App) -> Vec<EngineSpec> {
        let info = info_of(&self.store, cx);
        resolve_engines(&EngineConfig::from_settings(&info.settings), Some(current_locale().id()))
    }

    fn save_custom(cx: &gpui::App, list: &[CustomEngine]) {
        kit::update(cx, "searchEngines", serde_json::to_value(list).unwrap_or(json!([])));
    }

    fn save_keywords(cx: &gpui::App, overrides: &KeywordOverrides) {
        kit::update(cx, "searchKeywords", serde_json::to_value(overrides).unwrap_or(json!({})));
    }

    fn open_draft(&mut self, prefill: Option<&EngineSpec>, window: &mut Window, cx: &mut Context<Self>) {
        let mut mk = |placeholder: String, value: String, cx: &mut Context<Self>| {
            let state = kit::input(window, cx, placeholder, value, false, Some(Self::submit));
            cx.subscribe(&state, |this: &mut Self, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) && this.error.take().is_some() {
                    cx.notify();
                }
            })
            .detach();
            state
        };
        let name = mk(tr("settings.search.namePlaceholder"), prefill.map(|e| e.name.clone()).unwrap_or_default(), cx);
        let keyword = mk(tr("settings.search.keyword"), prefill.map(|e| e.keyword.clone()).unwrap_or_default(), cx);
        let template = mk(tr("settings.search.urlPlaceholder"), prefill.map(|e| e.template.clone()).unwrap_or_default(), cx);
        let suggest = mk(tr("settings.search.suggestPlaceholder"), prefill.and_then(|e| e.suggest.clone()).unwrap_or_default(), cx);
        self.draft = Some(Draft { id: prefill.map(|e| e.id.clone()), name, keyword, template, suggest });
        self.error = None;
        cx.notify();
    }

    fn close_draft(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.error = None;
        cx.notify();
    }

    fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &self.draft else { return };
        let engines = self.engines(cx);
        let (name, keyword, template, suggest) = (
            kit::text_of(&draft.name, cx),
            kit::text_of(&draft.keyword, cx),
            kit::text_of(&draft.template, cx),
            kit::text_of(&draft.suggest, cx),
        );
        let check = EngineDraft { name: &name, keyword: &keyword, template: &template, suggest: &suggest };
        if let Some(e) = validate_engine_draft(&check, &engines, draft.id.as_deref()) {
            self.error = Some(tr(error_key(e)));
            cx.notify();
            return;
        }
        let suggest = suggest.trim().to_string();
        let entry = CustomEngine {
            id: draft.id.clone().unwrap_or_else(|| format!("custom-{}", signet_core::ids::new_id().replace('-', "").chars().take(12).collect::<String>())),
            name: name.trim().to_string(),
            keyword: normalize_keyword(&keyword).unwrap_or_default(),
            template: template.trim().to_string(),
            suggest: if suggest.is_empty() { None } else { Some(suggest) },
        };
        let mut custom = info_of(&self.store, cx).settings.search_engines.clone().unwrap_or_default();
        match &draft.id {
            Some(id) => custom = custom.into_iter().map(|c| if &c.id == id { entry.clone() } else { c }).collect(),
            None => custom.push(entry),
        }
        Self::save_custom(cx, &custom);
        self.close_draft(cx);
    }

    /// Saves the keyword typed for an engine. Empty goes back to the default keyword (built-ins and presets only).
    fn commit_keyword(&mut self, engine: &EngineSpec, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let engines = self.engines(cx);
        let k = if value.trim().is_empty() && engine.default_keyword.is_some() { engine.default_keyword.clone() } else { normalize_keyword(value) };
        let error = match &k {
            None => Some(tr(error_key(EngineDraftError::Keyword))),
            Some(k) if engines.iter().any(|o| o.id != engine.id && &o.keyword == k) => Some(tr(error_key(EngineDraftError::KeywordTaken))),
            _ => None,
        };
        if let Some(e) = error {
            self.keyword_errors.insert(engine.id.clone(), e);
            cx.notify();
            return;
        }
        let k = k.unwrap_or_default();
        self.keyword_errors.remove(&engine.id);
        let info = info_of(&self.store, cx);
        if engine.source == EngineSource::Custom {
            let list: Vec<CustomEngine> = info
                .settings
                .search_engines
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|c| if c.id == engine.id { CustomEngine { keyword: k.clone(), ..c } } else { c })
                .collect();
            Self::save_custom(cx, &list);
        } else {
            let overrides = info.settings.search_keywords.clone().unwrap_or_default();
            Self::save_keywords(cx, &with_override(&overrides, engine, Some(Some(&k)), None));
        }
        if let Some(state) = self.keywords.get(&engine.id) {
            kit::set_text(state, &k, window, cx);
        }
        cx.notify();
    }

    fn keyword_input(&mut self, engine: &EngineSpec, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(s) = self.keywords.get(&engine.id) {
            // Follows the saved keyword unless it is being edited.
            let focused = s.read(cx).focus_handle(cx).is_focused(window);
            if !focused && kit::text_of(s, cx) != engine.keyword {
                kit::set_text(s, &engine.keyword, window, cx);
            }
            return s.clone();
        }
        let state = cx.new(|cx| InputState::new(window, cx).default_value(engine.keyword.clone()));
        let spec = engine.clone();
        cx.subscribe_in(&state, window, move |this, state, ev: &InputEvent, window, cx| match ev {
            InputEvent::Blur | InputEvent::PressEnter { .. } => {
                let value = kit::text_of(state, cx);
                if value != spec.keyword {
                    this.commit_keyword(&spec, &value, window, cx);
                }
            }
            InputEvent::Change => {
                if this.keyword_errors.remove(&spec.id).is_some() {
                    cx.notify();
                }
            }
            _ => {}
        })
        .detach();
        self.keywords.insert(engine.id.clone(), state.clone());
        state
    }
}

impl Render for SearchSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = info_of(&self.store, cx);
        let settings = &info.settings;
        let engines = self.engines(cx);
        let default_engine = settings.search_engine.clone().unwrap_or_else(|| DEFAULT_SEARCH_ENGINE.to_string());

        let rows: Vec<gpui::AnyElement> = engines
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let input = self.keyword_input(e, window, cx);
                let source = match e.source {
                    EngineSource::Builtin => "builtin",
                    EngineSource::Preset => "preset",
                    EngineSource::Custom => "custom",
                };
                let mut hint = tr(&format!("settings.search.{source}"));
                if e.id == default_engine {
                    hint.push_str(&format!(" \u{b7} {}", tr("settings.search.default")));
                }
                let favicon = engine_favicon(&e.template).map(|u| FaviconSource::Url(u.into()));
                let mut row = kit::row(cx)
                    .child(Favicon::new(favicon).size(16.0))
                    .child(kit::label(cx, e.name.clone(), Some(hint.into())))
                    .child(kit::field_w(&input, 96.0));
                if let Some(err) = self.keyword_errors.get(&e.id) {
                    row = row.child(kit::err(cx, err.clone()));
                }
                if e.source == EngineSource::Preset {
                    let (spec, store) = (e.clone(), self.store.clone());
                    row = row.child(kit::switch(("engine-on", i), e.enabled, move |_, _, cx| {
                        let overrides = info_of(&store, cx).settings.search_keywords.clone().unwrap_or_default();
                        Self::save_keywords(cx, &with_override(&overrides, &spec, None, Some(spec.enabled)));
                    }));
                }
                if e.source == EngineSource::Custom {
                    let spec = e.clone();
                    let id = e.id.clone();
                    let store = self.store.clone();
                    row = row
                        .child(
                            Btn::new(("engine-edit", i), tr("settings.search.edit"))
                                .icon(IconName::Pencil)
                                .on_click(cx.listener(move |this, _, window, cx| this.open_draft(Some(&spec), window, cx))),
                        )
                        .child(Btn::new(("engine-rm", i), tr("settings.search.remove")).icon(IconName::Trash2).on_click(move |_, _, cx| {
                            let list: Vec<CustomEngine> =
                                info_of(&store, cx).settings.search_engines.clone().unwrap_or_default().into_iter().filter(|c| c.id != id).collect();
                            Self::save_custom(cx, &list);
                        }));
                }
                row.into_any_element()
            })
            .collect();

        let lower = if let Some(draft) = &self.draft {
            let mut form_rows = vec![
                kit::row(cx).child(kit::field(&draft.name)).into_any_element(),
                kit::row(cx).child(kit::field(&draft.keyword)).into_any_element(),
                kit::row(cx).child(kit::field(&draft.template)).into_any_element(),
                kit::row(cx).child(kit::field(&draft.suggest)).into_any_element(),
                kit::note(cx, tr("settings.search.formHint")).px(space::S4).py(space::S2).into_any_element(),
            ];
            if let Some(e) = self.error.clone() {
                form_rows.push(kit::note_err(cx, e).px(space::S4).py(space::S2).into_any_element());
            }
            form_rows.push(
                kit::row(cx)
                    .child(Btn::new("draft-cancel", tr("settings.search.cancel")).on_click(cx.listener(|this, _, _, cx| this.close_draft(cx))))
                    .child(Btn::new("draft-save", tr("settings.search.save")).solid().on_click(cx.listener(|this, _, w, cx| this.submit(w, cx))))
                    .into_any_element(),
            );
            kit::group(cx, form_rows).id("search-draft").on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    this.close_draft(cx);
                }
            })).into_any_element()
        } else {
            kit::group(
                cx,
                vec![kit::row(cx)
                    .child(Btn::new("engine-add", tr("settings.search.add")).on_click(cx.listener(|this, _, w, cx| this.open_draft(None, w, cx))))
                    .into_any_element()],
            )
            .into_any_element()
        };

        let _ = tr_args;
        kit::body()
            .child(kit::note(cx, tr("settings.search.intro")))
            .child(kit::sub(cx, tr("settings.search.engines")))
            .child(kit::group(cx, rows))
            .child(lower)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(id: &str, default_keyword: Option<&str>) -> EngineSpec {
        EngineSpec {
            id: id.into(),
            name: id.into(),
            source: EngineSource::Preset,
            template: "https://x.example/?q=%s".into(),
            suggest: None,
            keyword: default_keyword.unwrap_or("").into(),
            default_keyword: default_keyword.map(String::from),
            enabled: true,
        }
    }

    #[test]
    fn keyword_equal_to_default_is_dropped() {
        let e = spec("wiki", Some("w"));
        let o = with_override(&KeywordOverrides::new(), &e, Some(Some("w")), None);
        assert!(o.is_empty());
        let o = with_override(&KeywordOverrides::new(), &e, Some(Some("wp")), None);
        assert_eq!(o.get("wiki").and_then(|k| k.keyword.as_deref()), Some("wp"));
    }

    #[test]
    fn disabling_and_enabling() {
        let e = spec("wiki", Some("w"));
        let o = with_override(&KeywordOverrides::new(), &e, None, Some(true));
        assert_eq!(o.get("wiki").and_then(|k| k.disabled), Some(true));
        let back = with_override(&o, &e, None, Some(false));
        assert!(back.is_empty());
    }
}

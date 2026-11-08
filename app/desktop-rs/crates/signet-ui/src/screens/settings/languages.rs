//! Ordered list of the languages announced to sites (port of `settings/LanguageSettings.tsx`).

use gpui::{
    App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, prelude::FluentBuilder, px,
};
use serde_json::json;
use signet_core::i18n::{Locale, current_locale};

use super::kit::{self, Btn, DragGhost};
use super::window::{InfoStore, info_of, watch};
use crate::app_state::tr;
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space};

/// Languages offered when adding (BCP 47 code).
pub const CHOICES: [&str; 24] = [
    "fr-FR", "fr-CA", "fr-BE", "fr-CH", "en-US", "en-GB", "en-CA", "en-AU", "es-ES", "es-MX", "de-DE", "it-IT", "pt-PT", "pt-BR", "nl-NL", "ja-JP", "ko-KR",
    "zh-CN", "zh-TW", "ar", "ru-RU", "pl-PL", "sv-SE", "tr-TR",
];


/// Name of a language code in the interface language ("Fran\u{e7}ais (France)"), otherwise the code itself.
pub fn name_of(code: &str) -> String {
    let fr = current_locale() == Locale::Fr;
    match crate::lang_names::dialect_name(code, fr).or_else(|| crate::lang_names::base_language_name(code, fr)) {
        Some(name) => {
            let mut chars = name.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_else(|| code.to_string())
        }
        None => code.to_string(),
    }
}

/// New order after moving the item at `from` to `to` (`splice` semantics); None if nothing changes.
pub fn moved(languages: &[String], from: usize, to: usize) -> Option<Vec<String>> {
    if from == to || to >= languages.len() || from >= languages.len() {
        return None;
    }
    let mut next = languages.to_vec();
    let item = next.remove(from);
    next.insert(to, item);
    Some(next)
}

struct LangDrag(usize);

/// The list, its "add" menu and the reset button.
pub struct LanguageSettings {
    store: Entity<InfoStore>,
}

impl LanguageSettings {
    pub fn new(store: Entity<InfoStore>, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        Self { store }
    }
}

fn save(cx: &App, languages: Vec<String>) {
    kit::update(cx, "languages", json!(languages));
}

impl Render for LanguageSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let languages = info_of(&self.store, cx).settings.languages.clone();
        let n = languages.len();
        let available: Vec<&str> = CHOICES.iter().copied().filter(|c| !languages.iter().any(|l| l == c)).collect();

        let items = languages.iter().enumerate().map(|(i, code)| {
            let (all_up, all_down, all_rm) = (languages.clone(), languages.clone(), languages.clone());
            let drop_langs = languages.clone();
            let name = name_of(code);
            let btn = |id: &'static str, icon: IconName, disabled: bool, tip: String, on: Box<dyn Fn(&mut App)>| {
                kit::icon_btn((id, i), icon, tip, move |_, _, cx| on(cx)).small().disabled(disabled)
            };
            div()
                .id(("lang", i))
                .flex()
                .items_center()
                .gap(space::S2)
                .h(px(30.0))
                .px(space::S2)
                .rounded(radius::CONTROL)
                .bg(t.ctl)
                .cursor_grab()
                .on_drag(LangDrag(i), {
                    let name = name.clone();
                    move |_, _, _, cx| cx.new(|_| DragGhost::new(name.clone()))
                })
                .drag_over::<LangDrag>(move |s, _, _, _| s.border_t_2().border_color(t.accent))
                .on_drop(move |d: &LangDrag, _, cx| {
                    if let Some(next) = moved(&drop_langs, d.0, i) {
                        save(cx, next);
                    }
                })
                .child(div().w(px(16.0)).text_center().text_size(t.type_scale.sub).text_color(t.text_2).child((i + 1).to_string()))
                .child(div().flex_1().child(name.clone()))
                .child(div().text_size(t.type_scale.sub).text_color(t.text_2).child(code.clone()))
                .child(btn(
                    "up",
                    IconName::ArrowUp,
                    i == 0,
                    tr("settings.languages.up"),
                    Box::new(move |cx| {
                        if let Some(next) = moved(&all_up, i, i.saturating_sub(1)) {
                            save(cx, next)
                        }
                    }),
                ))
                .child(btn(
                    "down",
                    IconName::ArrowDown,
                    i + 1 == n,
                    tr("settings.languages.down"),
                    Box::new(move |cx| {
                        if let Some(next) = moved(&all_down, i, i + 1) {
                            save(cx, next)
                        }
                    }),
                ))
                .child(btn(
                    "rm",
                    IconName::X,
                    n < 2,
                    tr("settings.languages.remove"),
                    Box::new(move |cx| save(cx, all_rm.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, l)| l.clone()).collect())),
                ))
        });

        let add_options: Vec<(SharedString, bool, std::rc::Rc<dyn Fn(&mut Window, &mut App)>)> = available
            .iter()
            .map(|c| {
                let code = c.to_string();
                let current = languages.clone();
                let action: std::rc::Rc<dyn Fn(&mut Window, &mut App)> = std::rc::Rc::new(move |_, cx| {
                    let mut next = current.clone();
                    next.push(code.clone());
                    save(cx, next);
                });
                (SharedString::from(name_of(c)), false, action)
            })
            .collect();
        // The closed select shows the "Add a language" prompt, never a selection.
        let add_label = tr("settings.languages.add");
        let add_menu = {
            let entries: Vec<crate::widgets::MenuEntry> = add_options
                .into_iter()
                .map(|(label, _, action)| crate::widgets::MenuEntry::item(label, move |_, w, cx| action(w, cx)))
                .collect();
            let button = gpui_component::button::Button::new("lang-add");
            use gpui_component::Sizable;
            crate::widgets::dropdown_menu("lang-add", button.outline().small().label(add_label).dropdown_caret(true), entries)
        };

        div()
            .flex()
            .flex_col()
            .gap(space::S2)
            .w_full()
            .mt(space::S2)
            .child(div().flex().flex_col().gap(px(2.0)).children(items))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(space::S2)
                    .child(add_menu)
                    .child(Btn::new("lang-reset", tr("settings.languages.reset")).on_click(|_, _, cx| save(cx, Vec::new()))),
            )
            .child(kit::note(cx, tr("settings.languages.note")))
            .when(false, |d| d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn move_follows_splice() {
        assert_eq!(moved(&v(&["a", "b", "c"]), 0, 2), Some(v(&["b", "c", "a"])));
        assert_eq!(moved(&v(&["a", "b", "c"]), 2, 0), Some(v(&["c", "a", "b"])));
        assert_eq!(moved(&v(&["a", "b"]), 1, 1), None);
        assert_eq!(moved(&v(&["a", "b"]), 0, 2), None);
    }

    #[test]
    fn every_choice_has_a_name() {
        for c in CHOICES {
            assert!(crate::lang_names::dialect_name(c, false).is_some(), "{c}");
        }
    }
}

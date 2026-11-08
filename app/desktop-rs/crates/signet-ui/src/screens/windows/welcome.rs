//! Port of `windows/Welcome.tsx` + `styles/welcome.css`: route `#welcome`, the first-launch introduction.
//!
//! Window: frameless with traffic lights (44 px drag zone on top), centered over the main window, about 560 x 640
//! (`main/windows/welcome.ts`). Choices apply right away through `update_settings`, as in Settings.
//! Enter continues, Escape skips (`welcome_finish`).

use super::common::{accent_btn, drag_area, st_btn};
use crate::app_state::{api, tr, tr_args};
use crate::assets::own_bytes;
use crate::icon::IconName;
use crate::theme::{SignetTheme, ThemeExt, radius, space, weight};
use crate::widgets::{MenuEntry, Segmented, Toggle, dropdown_menu};
use gpui::{
    AnyElement, AppContext, Context, ElementId, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, ObjectFit, ParentElement, Render, StatefulInteractiveElement, Styled,
    StyledImage, Window, div, img, prelude::FluentBuilder, px,
};
use gpui_component::{Icon, button::Button};
use serde_json::{Map, Value, json};
use signet_core::i18n::{LOCALES, Locale, UiLanguage, resolve_locale, system_languages_from_env};
use signet_core::searchengines::{DEFAULT_SEARCH_ENGINE, SEARCH_ENGINES};
use signet_core::types::{ImportKind, SettingsInfo, TabLayout, Theme};
use std::collections::HashMap;

const STEPS: [&str; 6] = ["language", "engine", "look", "import", "privacy", "done"];

/// Data imported in one click: passwords and extensions (keychain, downloads) stay in Settings.
pub const QUICK_KINDS: [ImportKind; 3] = [ImportKind::Spaces, ImportKind::Bookmarks, ImportKind::History];

#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportState {
    Busy,
    Done,
}

pub struct Welcome {
    info: Option<SettingsInfo>,
    index: usize,
    imports: HashMap<String, ImportState>,
    focus: FocusHandle,
}

/// Sources offered by the import step: available, with at least one of the quick kinds.
pub fn quick_sources(info: &SettingsInfo) -> Vec<&signet_core::types::ImportSourceInfo> {
    info.import_sources.iter().filter(|s| s.available && s.kinds.iter().any(|k| QUICK_KINDS.contains(k))).collect()
}

fn ui_language_key(l: UiLanguage) -> &'static str {
    match l {
        UiLanguage::System => "system",
        UiLanguage::En => "en",
        UiLanguage::Fr => "fr",
    }
}

impl Welcome {
    pub fn new(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let view = cx.new(|cx: &mut Context<Self>| {
            // Capture harness only: SIGNET_WELCOME_STEP=<0..5> opens on that step.
            let index = std::env::var("SIGNET_CAPTURE").ok().and_then(|_| std::env::var("SIGNET_WELCOME_STEP").ok()).and_then(|v| v.parse::<usize>().ok()).map_or(0, |i| i.min(STEPS.len() - 1));
            let mut me = Self { info: None, index, imports: HashMap::new(), focus: cx.focus_handle() };
            me.refresh(cx);
            // Return from the system "default browser" window.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.refresh(cx);
                }
            })
            .detach();
            me
        });
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
        view
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.info = Some(api(cx).get_settings_info());
        cx.notify();
    }

    fn update(&mut self, patch: Map<String, Value>, cx: &mut Context<Self>) {
        // Shown right away, then confirmed by the app's answer.
        if let Some(info) = self.info.as_mut() {
            let s = &mut info.settings;
            if let Some(v) = patch.get("uiLanguage").and_then(Value::as_str) {
                s.ui_language = Some(match v {
                    "fr" => UiLanguage::Fr,
                    "en" => UiLanguage::En,
                    _ => UiLanguage::System,
                });
            }
            if let Some(v) = patch.get("searchEngine").and_then(Value::as_str) {
                s.search_engine = Some(v.to_string());
            }
            if let Some(v) = patch.get("theme").and_then(Value::as_str) {
                s.theme = Some(match v {
                    "light" => Theme::Light,
                    "dark" => Theme::Dark,
                    _ => Theme::System,
                });
            }
            if let Some(v) = patch.get("tabLayout").and_then(Value::as_str) {
                s.tab_layout = Some(if v == "horizontal" { TabLayout::Horizontal } else { TabLayout::Vertical });
            }
            if let Some(v) = patch.get("adblock").and_then(Value::as_bool) {
                s.adblock = v;
            }
        }
        api(cx).update_settings(patch);
        self.refresh(cx);
    }

    fn last(&self) -> bool {
        self.index == STEPS.len() - 1
    }

    fn next(&mut self, cx: &mut Context<Self>) {
        if self.last() {
            api(cx).welcome_finish();
        } else {
            self.index += 1;
            cx.notify();
        }
    }

    fn run_import(&mut self, id: &str, cx: &mut Context<Self>) {
        self.imports.insert(id.to_string(), ImportState::Busy);
        api(cx).run_import(id, &QUICK_KINDS);
        self.imports.insert(id.to_string(), ImportState::Done);
        cx.notify();
    }
}

impl Focusable for Welcome {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

/// `.st-group` card holding settings rows.
pub fn group(t: &SignetTheme) -> gpui::Div {
    div().flex_none().flex().flex_col().rounded(radius::GROUP).bg(t.win_card).border_1().border_color(t.border).overflow_hidden()
}

/// `.st-row`: label with optional hint on the left, control on the right.
pub fn row(t: &SignetTheme, ix: usize, label: String, hint: Option<String>, control: impl IntoElement) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(space::S3)
        .min_h(space::TOOLBAR_H)
        .px(space::S4)
        .py(space::S2)
        .when(ix > 0, |d| d.border_t(px(0.5)).border_color(t.border))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(space::HALF)
                .child(div().child(label))
                .children(hint.map(|h| div().text_size(t.type_scale.sub).text_color(t.text_2).line_height(gpui::relative(1.35)).child(h))),
        )
        .child(control)
        .into_any_element()
}

fn engine_logo(t: &SignetTheme, id: &str, name: &str) -> AnyElement {
    let has = own_bytes(&format!("engines/{id}.png")).is_some();
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.0))
        .rounded_full()
        .bg(t.surface_2)
        .text_size(t.type_scale.sub)
        .font_weight(weight::STRONG)
        .text_color(t.accent)
        .child(if has {
            img(format!("engines/{id}.png")).size(px(18.0)).rounded(px(4.0)).object_fit(ObjectFit::Contain).into_any_element()
        } else {
            name.chars().next().map(|c| c.to_string()).unwrap_or_default().into_any_element()
        })
        .into_any_element()
}

impl Render for Welcome {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let me = cx.entity();
        let step = STEPS[self.index];
        let last = self.last();
        let info = self.info.clone();

        let body: Option<AnyElement> = info.as_ref().map(|info| {
            let settings = &info.settings;
            match step {
                "language" => {
                    let pref = settings.ui_language.unwrap_or(UiLanguage::System);
                    let system_locale = if pref == UiLanguage::System { signet_core::i18n::current_locale() } else { resolve_locale(None, &system_languages_from_env().iter().map(String::as_str).collect::<Vec<_>>()) };
                    let name_of = |l: Locale| LOCALES.iter().find(|x| x.id == l).map(|x| x.name).unwrap_or("");
                    let system_label = tr_args("settings.general.uiLanguageSystem", &[("language", name_of(system_locale))]);
                    let current = match pref {
                        UiLanguage::System => system_label.clone(),
                        UiLanguage::En => name_of(Locale::En).to_string(),
                        UiLanguage::Fr => name_of(Locale::Fr).to_string(),
                    };
                    let mut entries = vec![{
                        let me = me.clone();
                        MenuEntry::item(system_label, move |_, _, cx| me.update(cx, |this, cx| this.update(patch("uiLanguage", json!("system")), cx))).checked(pref == UiLanguage::System)
                    }];
                    for l in LOCALES.iter() {
                        let me = me.clone();
                        let id = l.id.id();
                        let on = ui_language_key(pref) == id;
                        entries.push(MenuEntry::item(l.name, move |_, _, cx| me.update(cx, |this, cx| this.update(patch("uiLanguage", json!(id)), cx))).checked(on));
                    }
                    let control = dropdown_menu("wl-lang", Button::new("wl-lang-btn").label(current).icon(Icon::from(IconName::ChevronDown)), entries);
                    group(&t).child(row(&t, 0, tr("welcome.language.label"), None, control)).into_any_element()
                }
                "engine" => {
                    let engine = settings.search_engine.clone().unwrap_or_else(|| DEFAULT_SEARCH_ENGINE.to_string());
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(space::S2)
                        .children(SEARCH_ENGINES.iter().enumerate().map(|(ix, e)| {
                            let on = e.id == engine;
                            let me = me.clone();
                            let id = e.id;
                            let hover = t.surface_2;
                            div()
                                .id(ElementId::from(("engine", ix)))
                                .flex_1()
                                .min_w(px(200.0))
                                .flex()
                                .items_center()
                                .gap(space::S3)
                                .h(px(48.0))
                                .px(space::S4)
                                .rounded(radius::GROUP)
                                .bg(if on { t.active } else { t.win_card })
                                .border_1()
                                .border_color(if on { t.accent } else { t.border })
                                .when(on, |d| d.border_2().font_weight(weight::LABEL))
                                .cursor_pointer()
                                .when(!on, |d| d.hover(move |s| s.bg(hover)))
                                .on_click(move |_, _, cx| me.update(cx, |this, cx| this.update(patch("searchEngine", json!(id)), cx)))
                                .child(engine_logo(&t, e.id, e.name))
                                .child(div().flex_1().child(e.name))
                                .children(on.then(|| Icon::from(IconName::Check).size(px(15.0)).text_color(t.accent)))
                        }))
                        .into_any_element()
                }
                "look" => {
                    let theme_ix = match settings.theme.unwrap_or(Theme::System) {
                        Theme::System => 0,
                        Theme::Light => 1,
                        Theme::Dark => 2,
                    };
                    let layout_ix = if settings.tab_layout.unwrap_or(TabLayout::Vertical) == TabLayout::Vertical { 0 } else { 1 };
                    let (me_a, me_b) = (me.clone(), me.clone());
                    let theme = Segmented::new("wl-theme")
                        .item(tr("welcome.look.themeSystem"))
                        .item(tr("welcome.look.themeLight"))
                        .item(tr("welcome.look.themeDark"))
                        .selected(theme_ix)
                        .on_change(move |ix, _, cx| {
                            let v = ["system", "light", "dark"][ix];
                            me_a.update(cx, |this, cx| this.update(patch("theme", json!(v)), cx));
                        });
                    let layout = Segmented::new("wl-layout").item(tr("welcome.look.layoutSide")).item(tr("welcome.look.layoutTop")).selected(layout_ix).on_change(move |ix, _, cx| {
                        let v = ["vertical", "horizontal"][ix];
                        me_b.update(cx, |this, cx| this.update(patch("tabLayout", json!(v)), cx));
                    });
                    group(&t).child(row(&t, 0, tr("welcome.look.theme"), None, theme)).child(row(&t, 1, tr("welcome.look.layout"), None, layout)).into_any_element()
                }
                "import" => {
                    let sources = quick_sources(info);
                    if sources.is_empty() {
                        div().text_center().text_size(t.type_scale.sub).text_color(t.text_2).child(tr("welcome.import.none")).into_any_element()
                    } else {
                        let mut g = group(&t);
                        for (ix, src) in sources.iter().enumerate() {
                            let state = self.imports.get(&src.id).copied();
                            let id = src.id.clone();
                            let me = me.clone();
                            let control: AnyElement = match state {
                                Some(ImportState::Done) => div()
                                    .flex()
                                    .items_center()
                                    .gap(space::S1)
                                    .font_weight(weight::MEDIUM)
                                    .text_color(t.accent)
                                    .child(Icon::from(IconName::Check).size(px(14.0)))
                                    .child(tr("welcome.import.done"))
                                    .into_any_element(),
                                busy => st_btn(ElementId::from(("import", ix)), &t)
                                    .when(busy.is_some(), |b| b.opacity(0.4))
                                    .when(busy.is_none(), |b| b.on_click(move |_, _, cx| me.update(cx, |this, cx| this.run_import(&id, cx))))
                                    .child(if busy.is_some() { tr("welcome.import.importing") } else { tr("welcome.import.run") })
                                    .into_any_element(),
                            };
                            g = g.child(row(&t, ix, src.title.clone(), None, control));
                        }
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S3)
                            .child(g)
                            .child(div().text_center().text_size(t.type_scale.sub).text_color(t.text_2).line_height(gpui::relative(1.45)).child(tr("welcome.import.hint")))
                            .into_any_element()
                    }
                }
                "privacy" => {
                    let me_a = me.clone();
                    let adblock = Toggle::new("wl-adblock", settings.adblock).on_change(move |v, _, cx| me_a.update(cx, |this, cx| this.update(patch("adblock", json!(v)), cx)));
                    let default_hint = if info.packaged {
                        info.default_browser.then(|| tr("welcome.privacy.isDefault"))
                    } else {
                        Some(tr("welcome.privacy.installedOnly"))
                    };
                    let default_control: AnyElement = if info.default_browser {
                        Icon::from(IconName::Check).size(px(14.0)).text_color(t.accent).into_any_element()
                    } else {
                        st_btn("wl-default", &t)
                            .when(!info.packaged, |b| b.opacity(0.4))
                            .when(info.packaged, |b| b.on_click(|_, _, cx| api(cx).set_default_browser()))
                            .child(tr("welcome.privacy.makeDefault"))
                            .into_any_element()
                    };
                    group(&t)
                        .child(row(&t, 0, tr("welcome.privacy.adblock"), Some(tr("welcome.privacy.adblockHint")), adblock))
                        .child(row(&t, 1, tr("welcome.privacy.defaultBrowser"), default_hint, default_control))
                        .into_any_element()
                }
                _ => div().text_center().text_size(t.type_scale.sub).text_color(t.text_2).child(tr("welcome.done.hint")).into_any_element(),
            }
        });

        let dots = div().flex().gap(space::S2).children((0..STEPS.len()).map(|i| {
            let on = i == self.index;
            div().size(px(if on { 9.0 } else { 7.0 })).rounded_full().bg(if on { t.accent } else if i < self.index { t.text_2 } else { t.border }).mt(px(if on { -1.0 } else { 0.0 }))
        }));

        let (me_back, me_next) = (me.clone(), me.clone());
        let skip = div()
            .id("wl-skip")
            .px(space::S2)
            .py(space::S1)
            .rounded(radius::CONTROL)
            .text_color(t.text_2)
            .cursor_pointer()
            .when(last, |d| d.invisible())
            .hover(|s| s.opacity(0.8))
            .on_click(|_, _, cx| api(cx).welcome_finish())
            .child(tr("welcome.skip"));
        let actions = div()
            .flex()
            .gap(space::S2)
            .children((self.index > 0 && !last).then(|| {
                st_btn("wl-back", &t).on_click(move |_, _, cx| me_back.update(cx, |this, cx| {
                    this.index = this.index.saturating_sub(1);
                    cx.notify();
                })).child(tr("welcome.back"))
            }))
            .child(accent_btn("wl-next", &t).h(px(28.0)).on_click(move |_, _, cx| me_next.update(cx, |this, cx| this.next(cx))).child(if last { tr("welcome.start") } else { tr("welcome.next") }));

        let me_key = me.clone();
        div()
            .id("welcome")
            .track_focus(&self.focus)
            .on_key_down(move |ev, _, cx| match ev.keystroke.key.as_str() {
                "escape" => {
                    cx.stop_propagation();
                    api(cx).welcome_finish();
                }
                "enter" => {
                    cx.stop_propagation();
                    me_key.update(cx, |this, cx| this.next(cx));
                }
                _ => {}
            })
            .flex()
            .flex_col()
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .child(drag_area(div().flex_none().h(px(44.0))))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(space::S4)
                    .px(space::S8)
                    .text_center()
                    .child(div().text_size(t.type_scale.hero).font_weight(weight::HERO).line_height(gpui::relative(1.1)).child(tr(&format!("welcome.{step}.title"))))
                    .child(div().max_w(px(420.0)).mt(space::S3).mb(space::S6).text_color(t.text_2).line_height(gpui::relative(1.45)).child(tr(&format!("welcome.{step}.text"))))
                    .children(body.map(|b| div().w_full().max_w(px(460.0)).text_left().child(b))),
            )
            .child(
                div()
                    .flex_none()
                    .grid()
                    .grid_cols(3)
                    .items_center()
                    .pt(space::S4)
                    .px(space::S6)
                    .pb(space::S5)
                    .child(div().flex().justify_start().child(skip))
                    .child(div().flex().justify_center().child(dots))
                    .child(div().flex().justify_end().child(actions)),
            )
    }
}

fn patch(key: &str, value: Value) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert(key.to_string(), value);
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::types::ImportSourceInfo;

    #[test]
    fn only_available_sources_with_quick_data_are_offered() {
        let src = |id: &str, available: bool, kinds: Vec<ImportKind>| ImportSourceInfo { id: id.into(), title: id.into(), available, kinds };
        let info = SettingsInfo {
            import_sources: vec![
                src("a", true, vec![ImportKind::History]),
                src("b", false, vec![ImportKind::History]),
                src("c", true, vec![ImportKind::Passwords]),
                src("d", true, vec![ImportKind::Passwords, ImportKind::Bookmarks]),
            ],
            ..Default::default()
        };
        let ids: Vec<_> = quick_sources(&info).iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["a", "d"]);
    }

    #[test]
    fn patches_are_single_key_objects() {
        let p = patch("theme", json!("dark"));
        assert_eq!(p.len(), 1);
        assert_eq!(p["theme"], "dark");
    }
}

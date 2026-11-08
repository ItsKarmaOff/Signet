//! Building blocks shared by the settings sections (port of the `.st-*` classes of `styles/settings.css`).

use gpui::AppContext as _;
use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, FontWeight, HighlightStyle, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, StyledText, Window, div, img, prelude::FluentBuilder, px, relative,
};
use gpui_component::{
    Icon, Sizable,
    button::Button,
    input::{Input, InputEvent, InputState},
};
use serde_json::{Map, Value};

use crate::app_state::{api, tr, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::{MenuEntry, dropdown_menu};

pub type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

// --------------------------------------------------------------------------- settings calls

/// `updateSettings({ key: value })`.
pub fn update(cx: &App, key: &str, value: Value) {
    let mut patch = Map::new();
    patch.insert(key.to_string(), value);
    api(cx).update_settings(patch);
}

/// `updateSettings({ a, b })`.
pub fn update_many(cx: &App, entries: Vec<(&str, Value)>) {
    let mut patch = Map::new();
    for (k, v) in entries {
        patch.insert(k.to_string(), v);
    }
    api(cx).update_settings(patch);
}

/// Re-render `cx`'s view when the interface language changes (labels are translated at render time).
pub fn watch_locale<T: 'static>(cx: &mut Context<T>) {
    cx.observe(&ui_state(cx), |_, _, cx| cx.notify()).detach();
}

// --------------------------------------------------------------------------- layout

/// `.st-group`: rounded card with hairline separators between its rows.
pub fn group(cx: &App, rows: Vec<AnyElement>) -> Div {
    let t = cx.signet();
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w_full()
        .rounded(radius::GROUP)
        .bg(t.win_card)
        .border(px(0.5))
        .border_color(t.border)
        .overflow_hidden()
        .children(rows.into_iter().enumerate().map(|(i, r)| {
            div().w_full().when(i > 0, |d| d.border_t(px(0.5)).border_color(t.border)).child(r)
        }))
}

pub use gpui::Div;

/// `.st-row`: label on the left, controls on the right.
pub fn row(cx: &App) -> Div {
    let t = cx.signet();
    div().flex().items_center().gap(space::S3).min_h(space::TOOLBAR_H).px(space::S4).py(space::S2).text_size(t.type_scale.base).text_color(t.text)
}

/// Row whose content stacks (`.st-row-stack`).
pub fn row_stack(cx: &App) -> Div {
    row(cx).flex_col().items_stretch()
}

/// `.st-label`: title and optional hint, filling the row.
pub fn label(cx: &App, title: impl Into<SharedString>, hint: Option<SharedString>) -> Div {
    let t = cx.signet();
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(space::HALF)
        .child(div().child(title.into()))
        .when_some(hint, |d, h| d.child(hint_text(cx, h)))
        .text_size(t.type_scale.base)
}

/// `.st-hint`.
pub fn hint_text(cx: &App, text: impl Into<SharedString>) -> Div {
    let t = cx.signet();
    div().text_size(t.type_scale.sub).text_color(t.text_2).line_height(relative(1.35)).child(text.into())
}

/// `.st-note`.
pub fn note(cx: &App, text: impl Into<SharedString>) -> Div {
    let t = cx.signet();
    div().w_full().text_size(t.type_scale.sub).text_color(t.text_2).line_height(relative(1.45)).child(text.into())
}

/// `.st-note.st-err`.
pub fn note_err(cx: &App, text: impl Into<SharedString>) -> Div {
    note(cx, text).text_color(cx.signet().danger).font_weight(weight::MEDIUM)
}

/// `.st-sub` heading.
pub fn sub(cx: &App, text: impl Into<SharedString>) -> Div {
    let t = cx.signet();
    let text: SharedString = text.into();
    div().mt(space::S2).text_size(t.type_scale.cap).font_weight(weight::STRONG).text_color(t.text_2).child(SharedString::from(text.to_uppercase()))
}

/// `.st-ok`: accent check and text.
pub fn ok(cx: &App, text: impl Into<SharedString>) -> Div {
    let t = cx.signet();
    div()
        .flex()
        .items_center()
        .gap(space::S1)
        .font_weight(weight::MEDIUM)
        .text_color(t.accent)
        .child(Icon::from(IconName::Check).size(px(14.0)))
        .child(text.into())
}

/// `.st-err`.
pub fn err(cx: &App, text: impl Into<SharedString>) -> Div {
    div().font_weight(weight::MEDIUM).text_color(cx.signet().danger).child(text.into())
}

/// `.st-empty`: centered icon and paragraphs.
pub fn empty(cx: &App, icon: IconName, lines: Vec<AnyElement>) -> Div {
    let t = cx.signet();
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(space::S1)
        .max_w(px(380.0))
        .mx_auto()
        .mt(space::S8)
        .text_center()
        .text_size(t.type_scale.base)
        .text_color(t.text_2)
        .line_height(relative(1.5))
        .child(div().mb(space::S2).child(Icon::from(icon).size(px(28.0)).text_color(t.text_3)))
        .children(lines)
}

/// Plain paragraph for [`empty`].
pub fn para(text: impl Into<SharedString>) -> AnyElement {
    div().child(text.into()).into_any_element()
}

/// `.st-body` column.
pub fn body() -> Div {
    div().flex().flex_col().gap(space::S4).w_full()
}

/// Section column (`.st-body section`).
pub fn section_col() -> Div {
    div().flex().flex_col().gap(space::S2).w_full()
}

// --------------------------------------------------------------------------- buttons

/// `.st-btn`.
#[derive(IntoElement)]
pub struct Btn {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<IconName>,
    danger: bool,
    solid: bool,
    accent: bool,
    disabled: bool,
    on_click: Option<Handler>,
}

impl Btn {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self { id: id.into(), label: Some(label.into()), icon: None, danger: false, solid: false, accent: false, disabled: false, on_click: None }
    }
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Red text (`.danger`).
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    /// Red fill (`.danger.solid`).
    pub fn solid(mut self) -> Self {
        self.solid = true;
        self
    }
    /// Accent fill (`.st-btn-accent`).
    pub fn accent(mut self) -> Self {
        self.accent = true;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Btn {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let (bg, fg, ring) = if self.solid {
            (t.danger, t.on_accent, false)
        } else if self.accent {
            (t.accent, t.on_accent, false)
        } else if self.danger {
            (t.ctl, t.danger, true)
        } else {
            (t.ctl, t.text, true)
        };
        let disabled = self.disabled;
        div()
            .id(self.id)
            .flex_none()
            .flex()
            .items_center()
            .gap(space::S2)
            .h(px(28.0))
            .px(space::S3)
            .rounded(radius::CONTROL)
            .whitespace_nowrap()
            .text_size(t.type_scale.base)
            .bg(bg)
            .text_color(fg)
            .when(ring, |d| d.border(px(0.5)).border_color(t.ctl_ring))
            .when(disabled, |d| d.opacity(0.4))
            .when(!disabled, |d| d.cursor_pointer().hover(|s| s.opacity(0.88)).active(|s| s.opacity(0.78)))
            .when_some(self.on_click.filter(|_| !disabled), |d, f| d.on_click(move |e, w, cx| f(e, w, cx)))
            .children(self.icon.map(|i| Icon::from(i).size(px(13.0))))
            .children(self.label)
    }
}

/// `button.icon` with a tooltip (the title attribute of the React code).
pub fn icon_btn(id: impl Into<ElementId>, icon: IconName, tip: impl Into<SharedString>, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> crate::widgets::IconButton {
    crate::widgets::IconButton::new(id, icon).tooltip(tip).on_click(on_click)
}

/// Link-looking text button (`.st-link`, `.link-btn`).
pub fn link(cx: &App, id: impl Into<ElementId>, text: impl Into<SharedString>, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> impl IntoElement {
    let t = cx.signet();
    div().id(id).cursor_pointer().text_color(t.accent).hover(|s| s.opacity(0.8)).on_click(move |e, w, cx| on_click(e, w, cx)).child(text.into())
}

// --------------------------------------------------------------------------- controls

/// `.switch`.
pub fn switch(id: impl Into<ElementId>, on: bool, on_change: impl Fn(bool, &mut Window, &mut App) + 'static) -> crate::widgets::Toggle {
    crate::widgets::Toggle::new(id, on).on_change(on_change)
}

/// `.st-select`: a button that opens a list of choices. `options`: (label, selected, action).
/// Shows `placeholder` while no option is selected.
pub fn select_ph(id: impl Into<ElementId>, placeholder: SharedString, options: Vec<(SharedString, bool, Rc<dyn Fn(&mut Window, &mut App)>)>) -> AnyElement {
    let id: ElementId = id.into();
    let current = options.iter().find(|o| o.1).map(|o| o.0.clone()).unwrap_or(placeholder);
    let entries: Vec<MenuEntry> = options
        .into_iter()
        .map(|(label, selected, action)| MenuEntry::item(label, move |_, w, cx| action(w, cx)).checked(selected))
        .collect();
    SelectButton { id, current, entries }.into_any_element()
}

/// The `.st-select` trigger: 28 px, filled with the control color, 7 px radius.
#[derive(IntoElement)]
struct SelectButton {
    id: ElementId,
    current: SharedString,
    entries: Vec<MenuEntry>,
}

impl RenderOnce for SelectButton {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet();
        // A native select is as wide as its longest option: approximate it from the character count.
        let widest = self.entries.iter().filter_map(|e| if let MenuEntry::Item { label, .. } = e { Some(label.chars().count()) } else { None }).max().unwrap_or(0).max(self.current.chars().count());
        let min_w = (widest as f32 * 6.4 + 12.0 + 28.0).max(60.0);
        let button = Button::new(self.id.clone())
            .outline()
            .min_w(px(min_w))
            .small()
            .label(self.current)
            .dropdown_caret(true)
            .h(px(28.0))
            .px(space::S3)
            .rounded(radius::CONTROL)
            .border_0()
            .bg(t.ctl)
            .text_color(t.text)
            .font_weight(weight::BODY);
        div().flex_none().child(dropdown_menu(self.id, button, self.entries))
    }
}

/// [`select_ph`] over `(value, label)` pairs; `on_pick` receives the value.
pub fn choice(id: impl Into<ElementId>, current: &str, options: Vec<(String, String)>, on_pick: impl Fn(&str, &mut Window, &mut App) + 'static) -> AnyElement {
    choice_ph(id, SharedString::default(), current, options, on_pick)
}

/// [`choice`] showing `placeholder` while `current` matches no option.
pub fn choice_ph(
    id: impl Into<ElementId>,
    placeholder: SharedString,
    current: &str,
    options: Vec<(String, String)>,
    on_pick: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_pick = Rc::new(on_pick);
    let opts = options
        .into_iter()
        .map(|(value, label)| {
            let selected = value == current;
            let f = on_pick.clone();
            let action: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |w, cx| f(&value, w, cx));
            (SharedString::from(label), selected, action)
        })
        .collect();
    select_ph(id, placeholder, opts)
}

/// Checkbox (`.st-check input`), 16 px. `partial` draws the indeterminate dash.
pub fn checkbox(cx: &App, id: impl Into<ElementId>, checked: bool, partial: bool) -> gpui::Stateful<Div> {
    let t = cx.signet();
    let filled = checked || partial;
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(16.0))
        .rounded(px(4.0))
        .border(px(1.0))
        .border_color(if filled { t.accent } else { t.text_3 })
        .bg(if filled { t.accent } else { t.ctl })
        .text_color(t.on_accent)
        .cursor_pointer()
        .when(checked && !partial, |d| d.child(Icon::from(IconName::Check).size(px(12.0))))
        .when(partial, |d| d.child(Icon::from(IconName::Minus).size(px(12.0))))
}

/// A `.st-row.st-check` line: checkbox, text, optional hint; the whole row toggles.
pub fn check_row(
    cx: &App,
    id: impl Into<ElementId>,
    checked: bool,
    partial: bool,
    text: impl Into<SharedString>,
    hint: Option<SharedString>,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    check_row_w(cx, id, checked, partial, text, hint, false, on_toggle)
}

/// [`check_row`] with an optional bold label (`.st-check-all`).
#[allow(clippy::too_many_arguments)]
pub fn check_row_w(
    cx: &App,
    id: impl Into<ElementId>,
    checked: bool,
    partial: bool,
    text: impl Into<SharedString>,
    hint: Option<SharedString>,
    bold: bool,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let id: ElementId = id.into();
    row(cx)
        .id(id.clone())
        .cursor_pointer()
        .on_click(move |_, w, cx| on_toggle(w, cx))
        .child(checkbox(cx, ElementId::from((id, "box")), checked, partial))
        .child(div().flex_1().when(bold, |d| d.font_weight(weight::STRONG)).child(text.into()))
        .when_some(hint, |d, h| d.child(hint_text(cx, h)))
        .into_any_element()
}

/// Radio dot (backup mode, live folder query).
pub fn radio_dot(cx: &App, on: bool) -> Div {
    let t = cx.signet();
    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(px(16.0))
        .rounded_full()
        .border(px(1.0))
        .border_color(if on { t.accent } else { t.text_3 })
        .bg(t.ctl)
        .when(on, |d| d.child(div().size(px(8.0)).rounded_full().bg(t.accent)))
}

// --------------------------------------------------------------------------- inputs

/// A single-line input whose changes re-render its owner. `on_enter` runs on Enter.
pub fn input<T: 'static>(
    window: &mut Window,
    cx: &mut Context<T>,
    placeholder: impl Into<SharedString>,
    value: impl Into<SharedString>,
    masked: bool,
    on_enter: Option<fn(&mut T, &mut Window, &mut Context<T>)>,
) -> Entity<InputState> {
    let placeholder = placeholder.into();
    let value = value.into();
    let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value).masked(masked));
    cx.subscribe_in(&state, window, move |this, _, ev: &InputEvent, window, cx| match ev {
        InputEvent::Change => cx.notify(),
        InputEvent::PressEnter { .. } => {
            if let Some(f) = on_enter {
                f(this, window, cx)
            }
        }
        _ => {}
    })
    .detach();
    state
}

/// Current text of an input.
pub fn text_of(state: &Entity<InputState>, cx: &App) -> String {
    state.read(cx).value().to_string()
}

/// Replaces the text of an input.
pub fn set_text(state: &Entity<InputState>, value: &str, window: &mut Window, cx: &mut App) {
    let value = value.to_string();
    state.update(cx, |s, cx| s.set_value(value, window, cx));
}

/// `.st-input` filling the remaining width of a row.
pub fn field(state: &Entity<InputState>) -> Div {
    div().flex_1().min_w_0().child(TextBox { state: state.clone() })
}

/// Fixed width `.st-input`.
pub fn field_w(state: &Entity<InputState>, width: f32) -> Div {
    div().flex_none().w(px(width)).child(TextBox { state: state.clone() })
}

/// `.st-input`: 28 px, control radius, `--ctl` fill and hairline ring drawn by this box around a bare input.
#[derive(IntoElement)]
pub struct TextBox {
    pub state: Entity<InputState>,
}

impl RenderOnce for TextBox {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet();
        div()
            .w_full()
            .min_w_0()
            .h(px(28.0))
            .flex()
            .items_center()
            .rounded(radius::CONTROL)
            .bg(t.ctl)
            .border(px(0.5))
            .border_color(t.ctl_ring)
            .px(space::S3)
            .text_color(t.text)
            .child(Input::new(&self.state).appearance(false).bordered(false).focus_bordered(false).small().w_full().px(px(0.0)).text_size(t.type_scale.base))
    }
}

// --------------------------------------------------------------------------- rich text

/// Text with the `<kbd>`, `<code>` and `<b>` tags of the translations (react-i18next `Trans`) as styled runs.
pub fn rich(cx: &App, text: &str) -> StyledText {
    let t = cx.signet();
    let mut plain = String::new();
    let mut runs: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let Some(open) = rest.find('<') else {
            plain.push_str(rest);
            break;
        };
        plain.push_str(&rest[..open]);
        let after = &rest[open..];
        let tag = ["kbd", "code", "b"].into_iter().find(|name| after.starts_with(&format!("<{name}>")));
        match tag {
            Some(name) => {
                let inner_start = name.len() + 2;
                let close = format!("</{name}>");
                match after.find(&close) {
                    Some(end) => {
                        let start = plain.len();
                        plain.push_str(&after[inner_start..end]);
                        let style = match name {
                            "b" => HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() },
                            "kbd" => HighlightStyle { color: Some(t.text), background_color: Some(t.win_card), ..Default::default() },
                            _ => HighlightStyle { color: Some(t.text), ..Default::default() },
                        };
                        runs.push((start..plain.len(), style));
                        rest = &after[end + close.len()..];
                    }
                    None => {
                        plain.push_str(after);
                        break;
                    }
                }
            }
            None => {
                plain.push('<');
                rest = &after[1..];
            }
        }
    }
    StyledText::new(plain).with_highlights(runs)
}

// --------------------------------------------------------------------------- images

pub use crate::data_url::image_from_data_url;

/// A packaged app icon (`ocean`, `nuit`...), square, rounded.
pub fn app_icon_image(id: &str, size: f32, radius_px: f32) -> AnyElement {
    img(crate::assets::app_icon_path(id)).size(px(size)).rounded(px(radius_px)).into_any_element()
}

/// A square data-URL image with rounded corners, or nothing if it cannot be decoded.
pub fn data_image(url: &str, size: f32, radius_px: f32) -> Option<AnyElement> {
    let image = image_from_data_url(url)?;
    Some(img(image).size(px(size)).flex_none().rounded(px(radius_px)).into_any_element())
}

// --------------------------------------------------------------------------- small helpers

/// `https://example.com/` to `example.com/` (`hostOf`).
pub fn host_of(origin: &str) -> String {
    match origin.find("://") {
        Some(i) if origin[..i].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => origin[i + 3..].to_string(),
        _ => origin.to_string(),
    }
}

/// Translation key lookup with a fallback (i18next `defaultValue`).
pub fn tr_or(key: &str, default: &str) -> String {
    let out = tr(key);
    if out == key { default.to_string() } else { out }
}

/// Months, abbreviated, per locale.
const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_FR: [&str; 12] = ["janv.", "f\u{e9}vr.", "mars", "avr.", "mai", "juin", "juil.", "ao\u{fb}t", "sept.", "oct.", "nov.", "d\u{e9}c."];
const LONG_EN: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const LONG_FR: [&str; 12] =
    ["janvier", "f\u{e9}vrier", "mars", "avril", "mai", "juin", "juillet", "ao\u{fb}t", "septembre", "octobre", "novembre", "d\u{e9}cembre"];

fn is_fr() -> bool {
    signet_core::i18n::current_locale() == signet_core::i18n::Locale::Fr
}

/// "6 Nov 2026" / "6 nov. 2026" (`toLocaleDateString` with a short month).
pub fn short_date(ms: i64) -> String {
    use chrono::{Datelike, TimeZone};
    let Some(d) = chrono::Local.timestamp_millis_opt(ms).single() else { return "-".into() };
    let m = d.month0() as usize;
    let name = if is_fr() { MONTHS_FR[m] } else { MONTHS_EN[m] };
    format!("{} {} {}", d.day(), name, d.year())
}

/// "6 November 2026 14:05" (`toLocaleString` with a long month and the time). "-" for 0.
pub fn long_datetime(ms: i64) -> String {
    use chrono::{Datelike, TimeZone, Timelike};
    if ms == 0 {
        return "-".into();
    }
    let Some(d) = chrono::Local.timestamp_millis_opt(ms).single() else { return "-".into() };
    let m = d.month0() as usize;
    let name = if is_fr() { LONG_FR[m] } else { LONG_EN[m] };
    if is_fr() {
        format!("{} {} {} \u{e0} {:02}:{:02}", d.day(), name, d.year(), d.hour(), d.minute())
    } else {
        format!("{} {}, {} at {:02}:{:02}", name, d.day(), d.year(), d.hour(), d.minute())
    }
}

/// "14:05:09" (`toLocaleTimeString` with 2-digit fields).
pub fn clock_time(ms: i64) -> String {
    use chrono::{TimeZone, Timelike};
    match chrono::Local.timestamp_millis_opt(ms).single() {
        Some(d) => format!("{:02}:{:02}:{:02}", d.hour(), d.minute(), d.second()),
        None => "-".into(),
    }
}

/// Month name in the UI language (autofill expiry list).
pub fn month_name(m: u32) -> &'static str {
    let i = (m.clamp(1, 12) - 1) as usize;
    if is_fr() { LONG_FR[i] } else { LONG_EN[i] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_strips_scheme() {
        assert_eq!(host_of("https://example.com"), "example.com");
        assert_eq!(host_of("example.com"), "example.com");
        assert_eq!(host_of("chrome-extension://abc"), "chrome-extension://abc");
    }
}

// --------------------------------------------------------------------------- drag and drop

/// Small floating label shown under the pointer while a row is dragged.
pub struct DragGhost {
    text: SharedString,
}

impl DragGhost {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self { text: text.into() }
    }
}

impl gpui::Render for DragGhost {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet();
        div()
            .px(space::S3)
            .py(space::S1)
            .rounded(radius::CONTROL)
            .bg(t.glass_view)
            .border(px(0.5))
            .border_color(t.border)
            .shadow(t.shadow_pop.clone())
            .text_size(t.type_scale.base)
            .text_color(t.text)
            .child(self.text.clone())
    }
}

/// `.space-form`: the small floating panel of a form (the host places it).
pub fn form_surface(cx: &App) -> Div {
    let t = cx.signet();
    div()
        .flex()
        .flex_col()
        .gap(space::S3)
        .p(space::S3)
        .rounded(radius::POP)
        .bg(t.glass)
        .border(px(0.5))
        .border_color(t.border)
        .shadow(t.shadow_pop.clone())
        .text_size(t.type_scale.base)
        .text_color(t.text)
}

/// `.st-note` with `<kbd>`, `<code>` and `<b>` runs.
pub fn note_rich(cx: &App, text: &str) -> Div {
    let t = cx.signet();
    div().w_full().text_size(t.type_scale.sub).text_color(t.text_2).line_height(relative(1.45)).child(rich(cx, text))
}

/// Adapts a view update to a `Fn(&mut Window, &mut App)` callback (`InlineConfirm`, `Toggle`...).
pub fn entity_cb<T: 'static>(cx: &Context<T>, f: impl Fn(&mut T, &mut Context<T>) + 'static) -> impl Fn(&mut Window, &mut App) + 'static {
    let this = cx.entity();
    move |_, app| this.update(app, |t, cx| f(t, cx))
}

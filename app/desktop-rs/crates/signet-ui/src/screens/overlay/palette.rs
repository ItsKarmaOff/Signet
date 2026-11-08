//! Command bar (port of `overlay/Palette.tsx`): search as you type, keyword chips, inline completion, edit-URL mode.

use super::common::{accent_for, favicon, image_source, lucide, viewport};
use super::inline::{can_complete, highlight_range, inline_completion};
use super::keyword_mode::{keyword_only, keyword_prefix, keyword_query};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::ShortcutLabel;
use gpui::{
    AnyElement, AppContext, Context, Entity, HighlightStyle, InteractiveElement, IntoElement, Keystroke, MouseButton, ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, StyledText,
    Subscription, Task, Window, div, img, prelude::FluentBuilder, px,
};
use gpui_component::{
    Icon,
    input::{Input, InputEvent, InputState},
};
use signet_core::api::UiEvent;
use signet_core::searchengines::KeywordInfo;
use signet_core::types::{PaletteMode, PaletteResult, Rect};
use std::time::{Duration, Instant};

const PALETTE_W: f32 = 640.0;
/// Minimum delay between two search requests while typing.
const QUERY_GAP: Duration = Duration::from_millis(70);
const SUGGEST_DELAY: Duration = Duration::from_millis(150);

pub fn result_id(r: &PaletteResult) -> &str {
    match r {
        PaletteResult::Url { id, .. }
        | PaletteResult::Search { id, .. }
        | PaletteResult::Suggestion { id, .. }
        | PaletteResult::Keyword { id, .. }
        | PaletteResult::History { id, .. }
        | PaletteResult::Archived { id, .. }
        | PaletteResult::Tab { id, .. }
        | PaletteResult::Action { id, .. } => id,
    }
}

pub fn result_title(r: &PaletteResult) -> &str {
    match r {
        PaletteResult::Url { title, .. }
        | PaletteResult::Search { title, .. }
        | PaletteResult::Suggestion { title, .. }
        | PaletteResult::Keyword { title, .. }
        | PaletteResult::History { title, .. }
        | PaletteResult::Archived { title, .. }
        | PaletteResult::Tab { title, .. }
        | PaletteResult::Action { title, .. } => title,
    }
}

fn result_subtitle(r: &PaletteResult) -> Option<&str> {
    match r {
        PaletteResult::Url { subtitle, .. }
        | PaletteResult::Search { subtitle, .. }
        | PaletteResult::Suggestion { subtitle, .. }
        | PaletteResult::Keyword { subtitle, .. }
        | PaletteResult::History { subtitle, .. }
        | PaletteResult::Archived { subtitle, .. }
        | PaletteResult::Tab { subtitle, .. }
        | PaletteResult::Action { subtitle, .. } => subtitle.as_deref(),
    }
}

fn action_icon(action: &str) -> &'static str {
    if action.starts_with("space:") {
        return "layers";
    }
    if action.starts_with("develop:") {
        return "flask-conical";
    }
    match action {
        "new-space" => "folder-plus",
        "split" | "unsplit" => "columns-2",
        "toggle-sidebar" => "panel-left",
        "pin" => "pin",
        "favorite" => "star",
        "copy-url" => "copy",
        "little" | "devtools-window" => "app-window",
        "clear-today" => "archive",
        "close-tab" => "x",
        "reopen" => "undo-2",
        "reload" => "rotate-cw",
        "devtools" => "code",
        "devtools-right" => "panel-right",
        "devtools-bottom" => "panel-bottom",
        "js-console" => "square-terminal",
        "toggle-peek-links" => "eye",
        "default-browser" => "compass",
        _ => "command",
    }
}

pub struct Palette {
    mode: PaletteMode,
    hue: f64,
    anchor: Option<Rect>,
    keywords: Vec<KeywordInfo>,
    input: Entity<InputState>,
    query: String,
    /// Keyword mode: the engine chip, `query` is then only the text after the keyword.
    kw: Option<KeywordInfo>,
    base: Vec<PaletteResult>,
    base_for: Option<String>,
    suggestions: Vec<PaletteResult>,
    suggest_for: String,
    sel_id: Option<String>,
    /// Inline completion: what is appended (and selected) after the typed text.
    completion: String,
    allow_completion: bool,
    seq: u64,
    last_sent: Option<Instant>,
    query_task: Option<Task<()>>,
    suggest_task: Option<Task<()>>,
    scroll: ScrollHandle,
    scroll_to_sel: bool,
    _input_sub: Subscription,
    _ui_sub: Subscription,
}

impl Palette {
    pub fn new(mode: PaletteMode, initial: &str, hue: f64, keywords: Vec<KeywordInfo>, anchor: Option<Rect>, window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let placeholder = Self::placeholder_for(mode, false);
        let initial = initial.to_string();
        cx.new(|cx: &mut Context<Self>| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.clone()).default_value(initial.clone()));
            let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Change) {
                    this.input_changed(window, cx);
                }
            });
            input.update(cx, |i, cx| {
                i.focus(window, cx);
                if mode == PaletteMode::EditUrl {
                    i.select_all(window, cx);
                }
            });
            let mut this = Self {
                mode,
                hue,
                anchor,
                keywords,
                input,
                query: initial,
                kw: None,
                base: vec![],
                base_for: None,
                suggestions: vec![],
                suggest_for: String::new(),
                sel_id: None,
                completion: String::new(),
                allow_completion: false,
                seq: 0,
                last_sent: None,
                query_task: None,
                suggest_task: None,
                scroll: ScrollHandle::new(),
                scroll_to_sel: false,
                _input_sub: sub,
                _ui_sub: cx.subscribe_in(&ui_state(cx), window, |this: &mut Self, _, ev: &UiEvent, window, cx| {
                    if let UiEvent::PaletteSuggestions { query, results } = ev
                        && *query == this.text()
                    {
                        this.suggestions = results.clone();
                        this.suggest_for = query.clone();
                        this.sync_input(window, cx);
                        cx.notify();
                    }
                }),
            };
            this.query_changed(window, cx);
            this
        })
    }

    fn placeholder_for(mode: PaletteMode, keyword: bool) -> SharedString {
        if keyword {
            return tr("palette.keywordPlaceholder").into();
        }
        match mode {
            PaletteMode::NewTab => tr("toolbar.addressPlaceholder"),
            PaletteMode::EditUrl => tr("palette.editUrl"),
            PaletteMode::Split => tr("palette.splitPlaceholder"),
        }
        .into()
    }

    /// What the app receives.
    fn text(&self) -> String {
        match &self.kw {
            Some(k) => keyword_query(k, &self.query),
            None => self.query.clone(),
        }
    }

    /// Engine suggestions go right after the search row (or the first result if it is far down): they never take the top spot.
    fn results(&self) -> Vec<PaletteResult> {
        let q = self.query.trim().to_lowercase();
        let text = self.text();
        let current: Vec<&PaletteResult> = if self.suggest_for == text {
            self.suggestions.iter().collect()
        } else {
            self.suggestions.iter().filter(|r| result_title(r).to_lowercase().starts_with(&q)).collect()
        };
        if current.is_empty() {
            return self.base.clone();
        }
        let seen: Vec<String> = self.base.iter().map(|r| result_title(r).to_lowercase()).collect();
        let extra: Vec<PaletteResult> = current.into_iter().filter(|r| !seen.contains(&result_title(r).to_lowercase())).cloned().collect();
        if self.base.is_empty() {
            return extra;
        }
        let search = self.base.iter().position(|r| matches!(r, PaletteResult::Search { .. }));
        let at = match search {
            Some(i) if i < 3 => i + 1,
            _ => 1,
        };
        let mut out: Vec<PaletteResult> = self.base[..at].to_vec();
        out.extend(extra);
        out.extend_from_slice(&self.base[at..]);
        out
    }

    fn sel_index(&self, results: &[PaletteResult]) -> usize {
        self.sel_id.as_deref().and_then(|id| results.iter().position(|r| result_id(r) == id)).unwrap_or(0)
    }

    /// The completion belongs to the top row: moving down the list drops it.
    fn shown(&self, results: &[PaletteResult]) -> String {
        if self.sel_index(results) == 0 { self.completion.clone() } else { String::new() }
    }

    fn field(&self, results: &[PaletteResult]) -> String {
        format!("{}{}", self.query, self.shown(results))
    }

    /// Pushes the typed text and the completion into the input, completion selected.
    fn sync_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let results = self.results();
        let field = self.field(&results);
        let shown = self.shown(&results);
        let start = self.query.len();
        let placeholder = Self::placeholder_for(self.mode, self.kw.is_some());
        self.input.update(cx, |i, cx| {
            if i.value().as_ref() != field {
                i.set_value(field.clone(), window, cx);
            }
            if !shown.is_empty() {
                i.set_selected_range(start..field.len(), cx);
            }
            i.set_placeholder(placeholder, window, cx);
        });
    }

    /// Request while typing (spaced by 70 ms), suggestions after 150 ms of calm; `seq` discards stale answers.
    fn query_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.seq += 1;
        let mine = self.seq;
        if self.query.trim().is_empty() {
            self.suggestions.clear();
        }
        self.sel_id = None;
        let wait = self.last_sent.map(|t| QUERY_GAP.saturating_sub(t.elapsed())).unwrap_or(Duration::ZERO);
        if wait.is_zero() {
            self.query_task = None;
            self.send_query(mine, window, cx);
        } else {
            let ex = cx.background_executor().clone();
            self.query_task = Some(cx.spawn_in(window, async move |this, cx| {
                ex.timer(wait).await;
                this.update_in(cx, |this, window, cx| this.send_query(mine, window, cx)).ok();
            }));
        }
        if self.query.trim().is_empty() {
            self.suggest_task = None;
        } else {
            let text = self.text();
            let ex = cx.background_executor().clone();
            self.suggest_task = Some(cx.spawn_in(window, async move |this, cx| {
                ex.timer(SUGGEST_DELAY).await;
                this.update_in(cx, |this, window, cx| {
                    if this.seq != mine {
                        return;
                    }
                    this.suggestions = api(cx).palette_suggest(&text);
                    this.suggest_for = text;
                    this.sync_input(window, cx);
                    cx.notify();
                })
                .ok();
            }));
        }
        self.sync_input(window, cx);
        cx.notify();
    }

    fn send_query(&mut self, mine: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.last_sent = Some(Instant::now());
        let text = self.text();
        let found = api(cx).palette_query(&text, self.mode);
        if mine != self.seq {
            return;
        }
        self.base = found;
        self.base_for = Some(text.clone());
        // Complete only from results for the text currently typed, and only after typing (never after a deletion).
        self.completion = if self.allow_completion && self.kw.is_none() { inline_completion(&self.query, self.base.first()) } else { String::new() };
        self.sync_input(window, cx);
        cx.notify();
    }

    fn input_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value().to_string();
        let results = self.results();
        if value == self.field(&results) {
            return;
        }
        // A keyword followed by a space (or pasted with its query) becomes the chip.
        if self.kw.is_none()
            && let Some((info, rest)) = keyword_prefix(&value, &self.keywords)
        {
            let info = info.clone();
            self.allow_completion = false;
            self.completion.clear();
            self.kw = Some(info);
            self.query = rest;
            self.input.update(cx, |i, cx| i.set_value(self.query.clone(), window, cx));
            self.query_changed(window, cx);
            return;
        }
        let ok = can_complete(&self.query, &value, false);
        // Typing what the completion already shows keeps the rest of it, so it does not flicker.
        let typed = value.get(self.query.len()..).unwrap_or("").to_lowercase();
        let comp_lower = self.completion.to_lowercase();
        self.completion = if ok && comp_lower.starts_with(&typed) { self.completion.get(typed.len()..).unwrap_or("").to_string() } else { String::new() };
        self.allow_completion = ok;
        self.query = value;
        self.query_changed(window, cx);
    }

    fn enter_keyword(&mut self, info: KeywordInfo, window: &mut Window, cx: &mut Context<Self>) {
        self.allow_completion = false;
        self.completion.clear();
        self.kw = Some(info);
        self.query.clear();
        self.query_changed(window, cx);
    }

    fn run(&mut self, r: PaletteResult, window: &mut Window, cx: &mut Context<Self>) {
        match r {
            PaletteResult::Keyword { keyword, .. } => self.enter_keyword(keyword, window, cx),
            other => api(cx).palette_execute(other, self.mode),
        }
    }

    /// Keys of the command bar, called by the overlay before the input sees them. True when consumed.
    pub fn handle_key(&mut self, ks: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let m = ks.modifiers;
        if m.control || m.alt || m.platform || m.function {
            return false;
        }
        let results = self.results();
        let idx = self.sel_index(&results);
        let shown = self.shown(&results);
        match ks.key.as_str() {
            "tab" if !m.shift && self.kw.is_none() => {
                // Tab on a lone keyword enters keyword mode; anything else keeps the default.
                match keyword_only(&self.query, &self.keywords).cloned() {
                    Some(info) => {
                        self.enter_keyword(info, window, cx);
                        true
                    }
                    None => false,
                }
            }
            "backspace" if self.kw.is_some() && self.query.is_empty() => {
                // Empty query: leave the mode and give the keyword text back.
                let kw = self.kw.take();
                self.allow_completion = false;
                self.query = kw.map(|k| k.keyword).unwrap_or_default();
                self.input.update(cx, |i, cx| i.set_value(self.query.clone(), window, cx));
                self.query_changed(window, cx);
                true
            }
            "down" | "up" => {
                if !results.is_empty() {
                    let step: isize = if ks.key == "down" { 1 } else { -1 };
                    let n = results.len() as isize;
                    let next = ((idx as isize + step + n) % n) as usize;
                    self.sel_id = Some(result_id(&results[next]).to_string());
                    self.scroll_to_sel = true;
                    self.sync_input(window, cx);
                    cx.notify();
                }
                true
            }
            "escape" if !shown.is_empty() => {
                // First Escape only removes the completion.
                self.completion.clear();
                self.allow_completion = false;
                self.sync_input(window, cx);
                cx.notify();
                true
            }
            "right" | "end" if !shown.is_empty() && !m.shift && self.input.read(cx).selected_range().start == self.query.len() => {
                self.query = self.field(&results);
                self.allow_completion = false;
                self.completion.clear();
                self.input.update(cx, |i, cx| i.set_value(self.query.clone(), window, cx));
                self.query_changed(window, cx);
                true
            }
            "enter" | "return" => {
                let text = self.text();
                if self.base_for.as_deref() == Some(text.as_str()) {
                    if let Some(r) = results.get(idx).cloned() {
                        self.run(r, window, cx);
                    }
                } else {
                    // Results not up to date yet: query directly for the current input.
                    let fresh = api(cx).palette_query(&text, self.mode);
                    if let Some(r) = fresh.into_iter().next() {
                        self.run(r, window, cx);
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn tag(&self, cx: &gpui::App, icon: Option<AnyElement>, label: String, strong: bool) -> AnyElement {
        let t = cx.signet();
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(space::S1)
            .h(px(24.0))
            .px(space::S2)
            .rounded(radius::CONTROL)
            .text_size(t.type_scale.cap)
            .text_color(if strong { t.text } else { t.text_2 })
            .bg(t.surface_2)
            .when_some(icon, |d, i| d.child(i))
            .child(label)
            .into_any_element()
    }

    fn result_icon(&self, r: &PaletteResult, cx: &gpui::App) -> AnyElement {
        let t = cx.signet();
        let tint = t.text_2;
        match r {
            PaletteResult::Tab { favicon: f, .. } | PaletteResult::History { favicon: f, .. } | PaletteResult::Archived { favicon: f, .. } => favicon(f.as_deref(), 18.0).into_any_element(),
            PaletteResult::Keyword { keyword, .. } => favicon(keyword.favicon.as_deref(), 18.0).into_any_element(),
            PaletteResult::Search { .. } | PaletteResult::Suggestion { .. } => Icon::from(IconName::Search).size(px(17.0)).text_color(tint).into_any_element(),
            PaletteResult::Url { .. } => Icon::from(IconName::Globe).size(px(17.0)).text_color(tint).into_any_element(),
            PaletteResult::Action { action, image, .. } => match image.as_deref().and_then(image_source) {
                Some(src) => img(gpui::ImageSource::from(src)).size(px(22.0)).rounded(px(6.0)).flex_none().into_any_element(),
                None => lucide(action_icon(action)).size(px(17.0)).text_color(tint).into_any_element(),
            },
        }
    }

    fn title(&self, r: &PaletteResult) -> AnyElement {
        let title = result_title(r);
        let marked = matches!(r, PaletteResult::Tab { .. } | PaletteResult::History { .. } | PaletteResult::Archived { .. } | PaletteResult::Suggestion { .. });
        match marked.then(|| highlight_range(title, &self.query)).flatten() {
            Some((a, b)) => StyledText::new(SharedString::from(title.to_string()))
                .with_highlights(vec![(a..b, HighlightStyle { font_weight: Some(weight::TITLE), ..Default::default() })])
                .into_any_element(),
            None => SharedString::from(title.to_string()).into_any_element(),
        }
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let accent = accent_for(cx, self.hue);
        let (w, h) = viewport(window);
        let width = PALETTE_W.min(w - 32.0);
        let (left, top) = match (self.mode, self.anchor) {
            // Grows from the address field: placed over it, centered on it, kept inside the window.
            (PaletteMode::EditUrl, Some(a)) => {
                let center = (a.x + a.width / 2.0) as f32;
                let mid = center.clamp(16.0 + width / 2.0, (w - 16.0 - width / 2.0).max(16.0 + width / 2.0));
                (mid - width / 2.0, ((a.y as f32) - 6.0).clamp(8.0, (h - 220.0).max(8.0)))
            }
            _ => ((w - width) / 2.0, h * 0.18),
        };
        let results = self.results();
        let idx = self.sel_index(&results);
        let shown = self.shown(&results);
        if std::mem::take(&mut self.scroll_to_sel) {
            self.scroll.scroll_to_item(idx);
        }

        let mut input_row = div().flex().items_center().gap(space::S3).h(px(52.0)).px(space::S5);
        if self.mode == PaletteMode::Split {
            let icon = lucide("columns-2").size(px(12.0)).into_any_element();
            input_row = input_row.child(self.tag(cx, Some(icon), tr("palette.splitTag"), false));
        }
        if let Some(kw) = &self.kw {
            let icon = favicon(kw.favicon.as_deref(), 14.0).into_any_element();
            input_row = input_row.child(self.tag(cx, Some(icon), tr_args("palette.searchSite", &[("engine", kw.name.as_str())]), true));
        } else {
            let ico = if shown.is_empty() {
                Icon::from(IconName::Search).size(px(17.0)).text_color(t.text_2).into_any_element()
            } else {
                let f = match results.first() {
                    Some(PaletteResult::Tab { favicon, .. }) | Some(PaletteResult::History { favicon, .. }) | Some(PaletteResult::Archived { favicon, .. }) => favicon.clone(),
                    _ => None,
                };
                favicon(f.as_deref(), 18.0).into_any_element()
            };
            input_row = input_row.child(div().flex().flex_none().w(px(18.0)).justify_center().child(ico));
        }
        input_row = input_row.child(div().flex_1().min_w_0().h_full().flex().items_center().ml(px(-6.0)).child(Input::new(&self.input).appearance(false).bordered(false).focus_bordered(false).w_full().text_size(t.type_scale.lg).text_color(t.text)));

        let rows: Vec<AnyElement> = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let selected = i == idx;
                let id = result_id(r).to_string();
                let (hover_id, click_r) = (id.clone(), r.clone());
                div()
                    .id(("palette-opt", i))
                    .relative()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(space::S3)
                    .h(space::TOOLBAR_H)
                    .px(space::S3)
                    .rounded(radius::ROW)
                    .cursor_pointer()
                    .when(selected, |d| d.bg(t.surface_2).child(div().absolute().left_0().top(space::S3).bottom(space::S3).w(px(2.0)).rounded(px(1.0)).bg(accent)))
                    .on_mouse_move(cx.listener(move |this, _, window, cx| {
                        if this.sel_id.as_deref() != Some(hover_id.as_str()) {
                            this.sel_id = Some(hover_id.clone());
                            this.sync_input(window, cx);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.run(click_r.clone(), window, cx)))
                    .child(div().flex().flex_none().items_center().justify_center().w(px(22.0)).child(self.result_icon(r, cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .line_height(gpui::relative(1.4))
                            .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(t.text).child(self.title(r)))
                            .when_some(result_subtitle(r).map(|s| s.to_string()), |d, s| d.child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().text_size(t.type_scale.sub).text_color(t.text_2).child(s))),
                    )
                    .when_some(
                        match r {
                            PaletteResult::Action { shortcut: Some(s), .. } => Some(signet_core::i18n::platform_keys(s, cfg!(target_os = "macos"))),
                            _ => None,
                        },
                        |d, s| d.child(ShortcutLabel::new(s)),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .absolute()
            .size_full()
            .bg(t.scrim)
            .on_mouse_down(MouseButton::Left, |_, _, cx| api(cx).close_overlay(false))
            .child(
                div()
                    .id("palette")
                    .occlude()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .overflow_hidden()
                    .rounded(radius::POP)
                    .bg(t.glass_view)
                    .border(px(0.5))
                    .border_color(t.border)
                    .shadow(t.shadow_modal.clone())
                    .text_color(t.text)
                    .text_size(t.type_scale.base)
                    .child(input_row)
                    .when(!rows.is_empty(), |d| d.child(div().id("palette-results").flex().flex_col().p(px(6.0)).max_h(px(440.0)).overflow_y_scroll().track_scroll(&self.scroll).children(rows))),
            )
    }
}


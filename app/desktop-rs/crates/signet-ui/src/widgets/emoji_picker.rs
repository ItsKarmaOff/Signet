//! Port of `ui/EmojiPicker.tsx`. Data: emojibase `compact.json` for `en` and `fr`, filtered like
//! `scripts/emoji-plugin.ts` (no skin tones, no too-recent emoji) and embedded in `assets/emoji/<locale>.json`.

use crate::assets::own_bytes;
use crate::icon::IconName;
use crate::theme::{ThemeExt, emoji_font, radius, space, weight};
use gpui::{
    Context, ElementId, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, ScrollStrategy, SharedString, StatefulInteractiveElement, Styled, UniformListScrollHandle, Window, div,
    prelude::FluentBuilder, px, uniform_list,
};
use gpui_component::{
    Icon,
    input::{InputEvent, InputState},
};
use std::sync::Arc;

use super::text_field::{TextField, text_field_state};

const RECENTS_MAX: usize = 16;
const CELL: f32 = 32.0;

/// Emitted when an emoji is chosen. The app persists recents from it.
pub struct EmojiPicked(pub String);

/// Translated strings.
#[derive(Clone)]
pub struct EmojiPickerLabels {
    pub search: SharedString,
    pub recent: SharedString,
    pub no_results: SharedString,
}

#[derive(Clone)]
struct Emoji {
    ch: SharedString,
    label: SharedString,
    haystack: String,
}

struct Section {
    label: SharedString,
    icon: SharedString,
    items: Vec<Emoji>,
}

enum Row {
    Header(SharedString),
    Cells(Vec<Emoji>),
}

fn fold(s: &str) -> String {
    // Lowercase and strip the common Latin accents (the data is already folded the same way).
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ÿ' | 'ý' => 'y',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

fn load_sections(locale: &str) -> Vec<Section> {
    let name = if locale.starts_with("fr") { "fr" } else { "en" };
    let Some(bytes) = own_bytes(&format!("emoji/{name}.json")) else { return vec![] };
    let Ok(serde_json::Value::Array(groups)) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return vec![] };
    groups
        .iter()
        .filter_map(|g| {
            let g = g.as_array()?;
            let items = g.get(3)?.as_array()?.iter().filter_map(|e| {
                let e = e.as_array()?;
                let label = e.get(1)?.as_str()?;
                Some(Emoji { ch: e.first()?.as_str()?.to_string().into(), label: label.to_string().into(), haystack: e.get(2).and_then(|h| h.as_str()).unwrap_or(label).to_string() })
            });
            Some(Section { label: g.get(1)?.as_str()?.to_string().into(), icon: g.get(2)?.as_str()?.to_string().into(), items: items.collect() })
        })
        .collect()
}

/// Emoji picker view: search field, category tabs, a virtualized grid and a caption with the hovered name.
///
/// ```ignore
/// let picker = cx.new(|cx| EmojiPicker::new(window, cx, "en", labels, recents));
/// cx.subscribe(&picker, |this, _, ev: &EmojiPicked, cx| { /* ev.0 */ }).detach();
/// ```
pub struct EmojiPicker {
    sections: Arc<Vec<Section>>,
    rows: Vec<Row>,
    /// First row of each section, aligned with `sections`.
    section_rows: Vec<usize>,
    recents: Vec<Emoji>,
    recent_row: Option<usize>,
    input: Entity<InputState>,
    query: String,
    first_result: Option<SharedString>,
    scroll: UniformListScrollHandle,
    hover: SharedString,
    labels: EmojiPickerLabels,
    cols: usize,
}

impl EventEmitter<EmojiPicked> for EmojiPicker {}

impl EmojiPicker {
    /// `locale`: `en` or `fr` (anything else falls back to `en`). `recents`: previously picked emoji, newest first.
    pub fn new(window: &mut Window, cx: &mut Context<Self>, locale: &str, labels: EmojiPickerLabels, recents: Vec<String>) -> Self {
        let sections = Arc::new(load_sections(locale));
        let input = text_field_state(window, cx, labels.search.clone(), "");
        cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, _window, cx| match ev {
            InputEvent::Change => {
                this.query = fold(input.read(cx).value().trim());
                this.rebuild();
                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                if let Some(ch) = this.first_result.clone() {
                    this.pick(ch.to_string(), cx);
                }
            }
            _ => {}
        })
        .detach();
        let mut me = Self {
            sections,
            rows: vec![],
            section_rows: vec![],
            recents: vec![],
            recent_row: None,
            input,
            query: String::new(),
            first_result: None,
            scroll: UniformListScrollHandle::new(),
            hover: SharedString::default(),
            labels,
            cols: 8,
        };
        me.recents = recents
            .iter()
            .filter_map(|c| me.sections.iter().flat_map(|s| s.items.iter()).find(|e| e.ch.as_ref() == c.as_str()).cloned())
            .take(RECENTS_MAX)
            .collect();
        me.rebuild();
        me
    }

    /// Number of emoji per row (default 8, 32 px each).
    pub fn columns(mut self, cols: usize) -> Self {
        self.cols = cols.max(1);
        self.rebuild();
        self
    }

    fn chunk(items: &[Emoji], cols: usize) -> impl Iterator<Item = Row> + '_ {
        items.chunks(cols).map(|c| Row::Cells(c.to_vec()))
    }

    fn rebuild(&mut self) {
        self.rows.clear();
        self.section_rows.clear();
        self.recent_row = None;
        self.first_result = None;
        if !self.query.is_empty() {
            let words: Vec<&str> = self.query.split_whitespace().collect();
            let results: Vec<Emoji> = self.sections.iter().flat_map(|s| s.items.iter()).filter(|e| words.iter().all(|w| e.haystack.contains(w))).cloned().collect();
            self.first_result = results.first().map(|e| e.ch.clone());
            self.rows.extend(Self::chunk(&results, self.cols));
            return;
        }
        if !self.recents.is_empty() {
            self.recent_row = Some(0);
            self.rows.push(Row::Header(self.labels.recent.clone()));
            self.rows.extend(Self::chunk(&self.recents, self.cols));
        }
        for s in self.sections.iter() {
            self.section_rows.push(self.rows.len());
            self.rows.push(Row::Header(s.label.clone()));
            self.rows.extend(Self::chunk(&s.items, self.cols));
        }
    }

    fn pick(&mut self, ch: String, cx: &mut Context<Self>) {
        self.recents.retain(|e| e.ch.as_ref() != ch.as_str());
        if let Some(e) = self.sections.iter().flat_map(|s| s.items.iter()).find(|e| e.ch.as_ref() == ch.as_str()).cloned() {
            self.recents.insert(0, e);
            self.recents.truncate(RECENTS_MAX);
        }
        cx.emit(EmojiPicked(ch));
    }

    /// Index of the row showing the section currently at the top.
    fn active_tab(&self) -> Option<usize> {
        if !self.query.is_empty() {
            return None;
        }
        let top = self.scroll.0.borrow().base_handle.logical_scroll_top().0;
        if self.recent_row.is_some() && self.section_rows.first().is_none_or(|first| top < *first) {
            return Some(usize::MAX);
        }
        Some(self.section_rows.iter().rposition(|r| *r <= top).unwrap_or(0))
    }
}

impl Render for EmojiPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let searching = !self.query.is_empty();
        let active = self.active_tab();
        let row_count = self.rows.len();
        let width = px(self.cols as f32 * CELL + 8.0);

        let mut tabs: Vec<gpui::AnyElement> = vec![];
        if let Some(r) = self.recent_row {
            let on = active == Some(usize::MAX);
            tabs.push(self.tab(ElementId::from("ep-tab-recent"), on, searching, Icon::from(IconName::Clock).size(px(14.0)).into_any_element(), r, &t, cx));
        }
        for (i, s) in self.sections.iter().enumerate() {
            let on = active == Some(i);
            let glyph = div().font_family(emoji_font()).text_size(px(14.0)).child(s.icon.clone()).into_any_element();
            tabs.push(self.tab(ElementId::from(("ep-tab", i)), on, searching, glyph, self.section_rows[i], &t, cx));
        }

        div()
            .flex()
            .flex_col()
            .gap(space::S2)
            .w(width)
            .min_w_0()
            .child(TextField::new(&self.input))
            .when(!searching, |d| d.child(div().flex().gap(space::HALF).overflow_hidden().children(tabs)))
            .child(
                div()
                    .h(px(220.0))
                    .w_full()
                    .rounded(radius::ROW)
                    .bg(t.surface_2)
                    .px(space::S1)
                    .pb(space::S1)
                    .overflow_hidden()
                    .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                    .child(if row_count == 0 {
                        div().p(space::S6).text_center().text_color(t.text_2).child(self.labels.no_results.clone()).into_any_element()
                    } else {
                        uniform_list(
                            "ep-grid",
                            row_count,
                            cx.processor(move |this, range: std::ops::Range<usize>, _w, cx| {
                                let t = cx.signet().clone();
                                range
                                    .map(|ix| match &this.rows[ix] {
                                        Row::Header(label) => div()
                                            .h(px(CELL))
                                            .flex()
                                            .items_end()
                                            .pb(space::S1)
                                            .px(space::S2)
                                            .text_size(t.type_scale.cap)
                                            .font_weight(weight::STRONG)
                                            .text_color(t.text_2)
                                            .child(label.clone())
                                            .into_any_element(),
                                        Row::Cells(cells) => div()
                                            .flex()
                                            .h(px(CELL))
                                            .children(cells.iter().enumerate().map(|(ci, e)| {
                                                let (ch, label) = (e.ch.clone(), e.label.clone());
                                                let ch2 = ch.clone();
                                                div()
                                                    .id(ElementId::from(("ep-cell", ix * 64 + ci)))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .size(px(CELL))
                                                    .rounded(radius::CONTROL)
                                                    .font_family(emoji_font())
                                                    .text_size(px(20.0))
                                                    .cursor_pointer()
                                                    .hover(move |s| s.bg(t.sel))
                                                    .active(move |s| s.opacity(0.7))
                                                    .on_hover(cx.listener(move |this, hovered: &bool, _w, cx| {
                                                        this.hover = if *hovered { label.clone() } else { SharedString::default() };
                                                        cx.notify();
                                                    }))
                                                    .on_click(cx.listener(move |this, _, _w, cx| this.pick(ch2.to_string(), cx)))
                                                    .child(ch)
                                            }))
                                            .into_any_element(),
                                    })
                                    .collect()
                            }),
                        )
                        .track_scroll(&self.scroll)
                        .size_full()
                        .into_any_element()
                    }),
            )
            .child(
                div()
                    .h(px(16.0))
                    .px(space::HALF)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(t.type_scale.cap)
                    .text_color(t.text_2)
                    .child(if self.hover.is_empty() { SharedString::from(" ") } else { self.hover.clone() }),
            )
    }
}

impl EmojiPicker {
    #[allow(clippy::too_many_arguments)]
    fn tab(&self, id: ElementId, on: bool, disabled: bool, glyph: gpui::AnyElement, row: usize, t: &crate::theme::SignetTheme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = t.clone();
        div()
            .id(id)
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .min_w(px(24.0))
            .h(px(28.0))
            .rounded(radius::CONTROL)
            .text_color(t.text_2)
            .opacity(if disabled { 0.3 } else if on { 1.0 } else { 0.6 })
            .when(on, |d| d.bg(t.sel))
            .when(!disabled, |d| {
                d.cursor_pointer().hover(move |s| s.bg(t.surface_2).opacity(1.0)).on_click(cx.listener(move |this, _, _w, cx| {
                    this.scroll.scroll_to_item(row, ScrollStrategy::Top);
                    cx.notify();
                }))
            })
            .child(glyph)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_loads_in_both_languages() {
        for l in ["en", "fr"] {
            let s = load_sections(l);
            assert!(s.len() >= 8, "{l}");
            assert!(s.iter().all(|s| !s.items.is_empty()));
        }
    }

    #[test]
    fn search_folds_accents() {
        let fr = load_sections("fr");
        let hit = fr.iter().flat_map(|s| s.items.iter()).any(|e| e.haystack.contains(&fold("éléphant")) || e.haystack.contains("elephant"));
        assert!(hit);
        assert_eq!(fold("Été"), "ete");
    }
}

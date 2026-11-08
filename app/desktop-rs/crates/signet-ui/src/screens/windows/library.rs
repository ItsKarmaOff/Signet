//! Port of `windows/Library.tsx` + `styles/library.css`: route `#library`, history, archive, downloads and spaces.
//!
//! Window: regular window (frameless with traffic lights, nav column 200 px wide with 52 px of room on top),
//! about 900 x 660 (`main/windows/library.ts`). The section is chosen by `UiEvent::LibrarySection` (`open_library`).
//! Not ported: the title separator that fades in with the scroll (the hairline is always hidden).

use super::common::{drag_area, favicon, st_btn};
use super::dates::{by_day, time_of};
use super::downloads::{DownloadRow, live};
use super::common::domain_of;
use crate::app_state::{api, snapshot, tr, tr_args, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, emoji_font, radius, space, weight};
use crate::widgets::InlineConfirm;
use gpui::{
    AnyElement, AppContext, Context, ElementId, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, ListAlignment, ListState, Window,
    div, hsla, linear_color_stop, linear_gradient, list, prelude::FluentBuilder, px,
};
use gpui_component::{
    Icon, Sizable, Size,
    input::{Input, InputEvent, InputState},
};
use signet_core::api::UiEvent;
use signet_core::i18n::current_locale;
use signet_core::types::{ArchivedEntry, BrowsingDataKind, HISTORY_QUERY_MAX, HistoryEntry, Item, LibrarySection, Space};
use std::time::Duration;

const PAGE: usize = 100;
const ROW_H: f32 = 36.0;

const SECTIONS: [(LibrarySection, &str, IconName); 4] = [
    (LibrarySection::History, "library.section.history", IconName::History),
    (LibrarySection::Archive, "library.section.archive", IconName::Archive),
    (LibrarySection::Downloads, "library.section.downloads", IconName::Download),
    (LibrarySection::Spaces, "library.section.spaces", IconName::Layers),
];

/// A line of the history or archive list.
#[derive(Clone)]
enum Line {
    Day(String),
    History(HistoryEntry),
    Archive(ArchivedEntry),
}

/// Number of tabs of an item (`tabIdsOf(i).length`).
pub fn tab_count(item: &Item) -> usize {
    match item {
        Item::Tab(_) => 1,
        Item::Split(s) => s.tab_ids.len(),
        Item::Folder(f) => f.items.iter().map(tab_count).sum(),
    }
}

pub struct Library {
    section: LibrarySection,
    input: Entity<InputState>,
    /// Debounced query (the field is read after 160 ms of silence).
    q: String,
    q_gen: u64,
    confirm: bool,
    history: Vec<HistoryEntry>,
    history_done: bool,
    archive: Vec<ArchivedEntry>,
    ready: bool,
    lines: Vec<Line>,
    list: ListState,
    loading_more: bool,
}

impl Library {
    pub fn new(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("library.history.search")));
            cx.subscribe_in(&ui_state(cx), window, |this, _, ev: &UiEvent, window, cx| {
                if let UiEvent::LibrarySection(s) = ev {
                    this.set_section(*s, window, cx);
                }
            })
            .detach();
            // Snapshot changes (downloads, spaces) redraw the window.
            cx.observe(&ui_state(cx), |_, _, cx| cx.notify()).detach();
            cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, _window, cx| {
                if matches!(ev, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.debounce(text, cx);
                }
            })
            .detach();
            // Back to the foreground: re-read the lists.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.reload(true, cx);
                }
            })
            .detach();
            let mut me = Self {
                section: LibrarySection::History,
                input,
                q: String::new(),
                q_gen: 0,
                confirm: false,
                history: vec![],
                history_done: false,
                archive: vec![],
                ready: false,
                lines: vec![],
                list: ListState::new(0, ListAlignment::Top, px(600.0)),
                loading_more: false,
            };
            me.reload(false, cx);
            me
        })
    }

    fn searchable(&self) -> bool {
        matches!(self.section, LibrarySection::History | LibrarySection::Archive)
    }

    fn set_section(&mut self, section: LibrarySection, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        self.confirm = false;
        self.q.clear();
        self.q_gen += 1;
        let placeholder = if section == LibrarySection::Archive { tr("library.archive.search") } else { tr("library.history.search") };
        self.input.update(cx, |i, cx| {
            i.set_value("", window, cx);
            i.set_placeholder(placeholder, window, cx);
        });
        self.ready = false;
        self.reload(false, cx);
    }

    fn debounce(&mut self, text: String, cx: &mut Context<Self>) {
        self.q_gen += 1;
        let gen_ = self.q_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(160)).await;
            this.update(cx, |this, cx| {
                if this.q_gen == gen_ && this.q != text {
                    this.q = text;
                    this.reload(false, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Reads the first page again (`keep`: as many entries as displayed, for a return to the foreground).
    fn reload(&mut self, keep: bool, cx: &mut Context<Self>) {
        match self.section {
            LibrarySection::History => {
                let target = if keep { PAGE.max(self.history.len()) } else { PAGE };
                let mut page: Vec<HistoryEntry> = Vec::new();
                let mut full = true;
                while page.len() < target {
                    let limit = HISTORY_QUERY_MAX.min(target - page.len());
                    let part = api(cx).query_history(&self.q, page.len(), limit);
                    let got = part.len();
                    page.extend(part);
                    if got < limit {
                        full = false;
                        break;
                    }
                }
                self.history = page;
                self.history_done = !full;
                self.ready = true;
            }
            LibrarySection::Archive => {
                self.archive = api(cx).list_archive(&self.q);
                self.ready = true;
            }
            _ => self.ready = true,
        }
        self.rebuild();
        if !keep {
            self.list.scroll_to(gpui::ListOffset::default());
        }
        cx.notify();
    }

    fn load_more(&mut self, cx: &mut Context<Self>) {
        if self.section != LibrarySection::History || self.history_done {
            return;
        }
        let part = api(cx).query_history(&self.q, self.history.len(), PAGE);
        self.history_done = part.len() < PAGE;
        let seen: std::collections::HashSet<String> = self.history.iter().map(|e| e.url.clone()).collect();
        self.history.extend(part.into_iter().filter(|e| !seen.contains(&e.url)));
        self.rebuild();
    }

    fn rebuild(&mut self) {
        let now = chrono::Local::now().timestamp_millis();
        let locale = current_locale();
        let tz = chrono::Local;
        let old_len = self.lines.len();
        self.lines = match self.section {
            LibrarySection::History => by_day(self.history.clone(), |e| e.last_visit, now, locale, &tz)
                .into_iter()
                .flat_map(|g| std::iter::once(Line::Day(g.label)).chain(g.items.into_iter().map(Line::History)))
                .collect(),
            LibrarySection::Archive => by_day(self.archive.clone(), |e| e.archived_at, now, locale, &tz)
                .into_iter()
                .flat_map(|g| std::iter::once(Line::Day(g.label)).chain(g.items.into_iter().map(Line::Archive)))
                .collect(),
            _ => vec![],
        };
        // Same lines of text with a new count: keep the scroll position.
        let top = self.list.logical_scroll_top();
        self.list.reset(self.lines.len());
        if old_len > 0 {
            self.list.scroll_to(top);
        }
    }

    fn remove_history(&mut self, url: &str, cx: &mut Context<Self>) {
        self.history.retain(|e| e.url != url);
        api(cx).delete_history(url);
        self.rebuild();
        cx.notify();
    }

    fn drop_archived(&mut self, id: &str, cx: &mut Context<Self>) {
        self.archive.retain(|e| e.id != id);
        self.rebuild();
        cx.notify();
    }

    fn render_line(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.signet().clone();
        let Some(line) = self.lines.get(ix).cloned() else { return div().into_any_element() };
        let locale = current_locale();
        let tz = chrono::Local;
        match line {
            Line::Day(label) => div()
                .w_full()
                .h(px(if ix == 0 { 37.0 } else { 42.0 }))
                .flex()
                .items_end()
                .pb(space::S1)
                .px(space::S2)
                .text_size(t.type_scale.sub)
                .font_weight(weight::STRONG)
                .text_color(t.text_2)
                .child(capitalize(&label))
                .into_any_element(),
            Line::History(e) => {
                let url = e.url.clone();
                let url_del = e.url.clone();
                let me = cx.entity();
                let a = api(cx);
                lib_row(
                    ("hrow", ix),
                    &t,
                    e.favicon.as_deref(),
                    if e.title.is_empty() { e.url.clone() } else { e.title.clone() },
                    domain_of(&e.url),
                    time_of(e.last_visit, locale, &tz),
                    None,
                    Some(Box::new(move |_, _| a.open_library_url(&url))),
                    Box::new(move |_, cx| me.update(cx, |this, cx| this.remove_history(&url_del, cx))),
                )
            }
            Line::Archive(e) => {
                let id = e.id.clone();
                let id_del = e.id.clone();
                let id_restore = e.id.clone();
                let me = cx.entity();
                let me_restore = cx.entity();
                let a = api(cx);
                let a_restore = a.clone();
                let restore = div()
                    .id(("restore", ix))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S1)
                    .h(px(24.0))
                    .px(space::S2)
                    .rounded(px(6.0))
                    .text_size(t.type_scale.sub)
                    .text_color(t.accent)
                    .bg(t.sel)
                    .cursor_pointer()
                    .hover(|s| s.opacity(0.9))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| {
                        me_restore.update(cx, |this, cx| this.drop_archived(&id_restore, cx));
                        a_restore.restore_archived(&id_restore);
                    })
                    .child(Icon::from(IconName::RotateCcw).size(px(12.0)))
                    .child(tr("library.archive.restore"))
                    .into_any_element();
                lib_row(
                    ("arow", ix),
                    &t,
                    e.favicon.as_deref(),
                    if e.title.is_empty() { e.url.clone() } else { e.title.clone() },
                    domain_of(&e.url),
                    time_of(e.archived_at, locale, &tz),
                    Some(restore),
                    None,
                    Box::new(move |_, cx| {
                        me.update(cx, |this, cx| this.drop_archived(&id_del, cx));
                        a.delete_archived(&id);
                    }),
                )
            }
        }
    }
}

type RowAction = Box<dyn Fn(&mut Window, &mut gpui::App)>;

/// `LibRow`: favicon, title, domain, time, optional action, delete button revealed on hover.
#[allow(clippy::too_many_arguments)]
fn lib_row(id: (&'static str, usize), t: &crate::theme::SignetTheme, fav: Option<&str>, title: String, sub: String, time: String, action: Option<AnyElement>, open: Option<RowAction>, remove: RowAction) -> AnyElement {
    let group: SharedString = format!("{}-{}", id.0, id.1).into();
    let hover = t.surface_2;
    let (border, text, text_2) = (t.border, t.text, t.text_2);
    div()
        .id(id)
        .group(group.clone())
        .w_full()
        .h(px(ROW_H))
        .flex()
        .items_center()
        .gap(space::S3)
        .pl(space::S3)
        .pr(px(6.0))
        .rounded(radius::ROW)
        .text_size(t.type_scale.base)
        .hover(move |s| s.bg(hover))
        .when_some(open, |d, f| d.cursor_pointer().on_click(move |_, w, cx| f(w, cx)))
        .child(favicon(fav, 16.0, false))
        .child(div().flex_1().min_w_0().truncate().child(title))
        .child(div().flex_none().w(px(190.0)).min_w_0().truncate().text_size(t.type_scale.sub).text_color(t.text_2).child(sub))
        .child(div().flex_none().min_w(px(40.0)).whitespace_nowrap().text_right().text_size(t.type_scale.sub).text_color(t.text_3).child(time))
        .children(action)
        .child(
            div()
                .id((id.0, id.1 + 1_000_000))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.0))
                .rounded(px(6.0))
                .text_color(text_2)
                .invisible()
                .group_hover(group, |s| s.visible())
                .hover(move |s| s.bg(border).text_color(text))
                .cursor_pointer()
                .tooltip(move |w, cx| gpui_component::tooltip::Tooltip::new(tr("library.delete")).build(w, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, w, cx| remove(w, cx))
                .child(Icon::from(IconName::X).size(px(13.0))),
        )
        .into_any_element()
}

/// `st-empty lib-empty`.
fn empty(t: &crate::theme::SignetTheme, icon: IconName, text: String) -> AnyElement {
    div()
        .mt(px(80.0))
        .mx_auto()
        .max_w(px(380.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(space::S3)
        .text_center()
        .text_color(t.text_2)
        .line_height(gpui::relative(1.5))
        .child(Icon::from(icon).size(px(28.0)).text_color(t.text_3))
        .child(text)
        .into_any_element()
}

fn space_card(t: &crate::theme::SignetTheme, space: &Space, active: bool, ix: usize) -> AnyElement {
    let a_switch = space.id.clone();
    let a_menu = space.id.clone();
    let pinned = space.pinned.iter().map(tab_count).sum::<usize>();
    let today = space.today.iter().map(tab_count).sum::<usize>();
    let hues = space.theme.as_ref().map(|th| th.hues.clone()).unwrap_or_default();
    let dot = div().flex_none().size(px(8.0)).rounded_full();
    let dot = if hues.len() >= 2 {
        dot.bg(linear_gradient(
            135.0,
            linear_color_stop(hsla((hues[0] / 360.0) as f32, 0.7, 0.62, 1.0), 0.0),
            linear_color_stop(hsla((hues[hues.len() - 1] / 360.0) as f32, 0.7, 0.62, 1.0), 1.0),
        ))
    } else {
        match space.theme.as_ref() {
            None => dot.bg(t.accent),
            Some(_) => dot.bg(hsla(((hues.first().copied().unwrap_or(space.hue)) / 360.0) as f32, 0.7, 0.62, 1.0)),
        }
    };
    let hover = t.surface_2;
    div()
        .id(("space", ix))
        .flex_1()
        .min_w(px(320.0))
        .flex()
        .items_center()
        .gap(space::S4)
        .p(space::S4)
        .rounded(radius::GROUP)
        .bg(t.win_card)
        .border_1()
        .border_color(if active { t.accent } else { t.border })
        .when(active, |d| d.border_2())
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .on_click(move |_, _, cx| api(cx).switch_space(&a_switch))
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .size(space::TOOLBAR_H)
                .rounded(radius::TILE)
                .bg(t.sel)
                .font_family(emoji_font())
                .text_size(px(24.0))
                .line_height(gpui::relative(1.0))
                .child(space.icon.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(space::HALF)
                .text_size(t.type_scale.sub)
                .line_height(gpui::relative(1.4))
                .text_color(t.text_2)
                .child(div().flex().items_center().gap(space::S2).text_size(t.type_scale.base).font_weight(weight::STRONG).text_color(t.text).child(dot).child(div().min_w_0().truncate().child(space.name.clone())))
                .child(div().truncate().child(tr_args("library.spaces.pinned", &[("count", &pinned.to_string())])))
                .child(div().truncate().child(tr_args("library.spaces.today", &[("count", &today.to_string())])))
                .when(space.is_isolated(), |c| {
                    c.child(
                        div()
                            .mt(space::S1)
                            .self_start()
                            .flex()
                            .items_center()
                            .gap(space::S1)
                            .px(space::S2)
                            .py(px(1.0))
                            .rounded(px(6.0))
                            .bg(t.surface_2)
                            .child(Icon::from(IconName::UserRound).size(px(11.0)))
                            .child(tr("library.spaces.isolated")),
                    )
                }),
        )
        .child(
            st_btn(("space-edit", ix), t)
                .self_start()
                .h(px(24.0))
                .px(space::S2)
                .gap(space::S1)
                .text_size(t.type_scale.sub)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| api(cx).show_space_menu(&a_menu))
                .child(Icon::from(IconName::Pencil).size(px(12.0)))
                .child(tr("library.spaces.edit")),
        )
        .into_any_element()
}

impl Render for Library {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let snap = snapshot(cx);
        let me = cx.entity();
        let section = self.section;

        let nav = drag_area(div().flex_none().w(px(200.0)).flex().flex_col().gap(space::HALF).px(space::S3).pb(space::S3))
            .child(div().flex_none().h(px(52.0)))
            .children(SECTIONS.iter().map(|(id, label, icon)| {
                let on = *id == section;
                let me = me.clone();
                let id = *id;
                let hover = t.surface_2;
                div()
                    .id(("nav", id as usize))
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .h(space::S7)
                    .px(space::S3)
                    .rounded(radius::CONTROL)
                    .text_color(t.text)
                    .text_size(t.type_scale.base)
                    .cursor_pointer()
                    .when(on, |d| d.bg(t.active).shadow(t.active_shadow.clone()))
                    .when(!on, |d| d.hover(move |s| s.bg(hover)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, window, cx| me.update(cx, |this, cx| this.set_section(id, window, cx)))
                    .child(Icon::from(*icon).size(px(15.0)).text_color(if on { t.accent } else { t.text_2 }))
                    .child(tr(label))
            }));

        let current = SECTIONS.iter().find(|s| s.0 == section).map(|s| tr(s.1)).unwrap_or_default();
        let has_query = !self.input.read(cx).value().is_empty();
        let search = self.searchable().then(|| {
            let me = me.clone();
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S2)
                .w(px(240.0))
                .h(space::S7)
                .px(space::S2)
                .rounded(radius::ROW)
                .bg(t.surface_2)
                .text_color(t.text_2)
                .text_size(t.type_scale.base)
                .font_weight(weight::BODY)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(Icon::from(IconName::Search).size(px(13.0)))
                .child(div().flex_1().min_w_0().text_size(t.type_scale.base).child(Input::new(&self.input).appearance(false).bordered(false).focus_bordered(false).with_size(Size::Size(t.type_scale.base / 0.875)).px(px(0.0)).text_color(t.text)))
                .when(has_query, |d| {
                    d.child(
                        div()
                            .id("lib-x")
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(space::S4)
                            .rounded_full()
                            .bg(t.border)
                            .text_color(t.text_2)
                            .cursor_pointer()
                            .tooltip(|w, cx| gpui_component::tooltip::Tooltip::new(tr("library.clearSearch")).build(w, cx))
                            .on_click(move |_, window, cx| {
                                me.update(cx, |this, cx| {
                                    this.input.update(cx, |i, cx| i.set_value("", window, cx));
                                })
                            })
                            .child(Icon::from(IconName::X).size(px(12.0))),
                    )
                })
        });

        let history_clear = (section == LibrarySection::History).then(|| -> AnyElement {
            let me = me.clone();
            if !self.confirm {
                st_btn("lib-clear", &t).on_click(move |_, _, cx| me.update(cx, |this, cx| {
                    this.confirm = true;
                    cx.notify();
                })).child(tr("library.history.clear")).into_any_element()
            } else {
                let me2 = me.clone();
                InlineConfirm::new("lib-confirm", tr("library.history.confirm"), tr("library.downloads.clear"), tr("common.cancel"))
                    .on_cancel(move |_, cx| me.update(cx, |this, cx| {
                        this.confirm = false;
                        cx.notify();
                    }))
                    .on_confirm(move |_, cx| {
                        api(cx).clear_browsing_data(&[BrowsingDataKind::History]);
                        me2.update(cx, |this, cx| {
                            this.confirm = false;
                            this.reload(false, cx);
                        });
                    })
                    .into_any_element()
            }
        });
        let downloads_clear = (section == LibrarySection::Downloads).then(|| {
            let any_dead = snap.downloads.iter().any(|d| !live(d));
            st_btn("dl-clear", &t)
                .when(!any_dead, |b| b.opacity(0.4))
                .when(any_dead, |b| b.on_click(|_, _, cx| api(cx).clear_downloads()))
                .child(tr("library.downloads.clear"))
        });

        let title = drag_area(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S3)
                .h(px(52.0))
                .px(space::S7)
                .text_size(t.type_scale.base)
                .font_weight(weight::BODY)
                .child(div().flex_1().min_w_0().text_size(t.type_scale.title).font_weight(weight::TITLE).child(current)),
        )
        .children(search)
        .children(history_clear)
        .children(downloads_clear);

        let body: AnyElement = match section {
            LibrarySection::History | LibrarySection::Archive => {
                if self.ready && self.lines.is_empty() {
                    let text = if !self.q.is_empty() {
                        tr("library.noResults")
                    } else if section == LibrarySection::History {
                        tr("library.history.empty")
                    } else {
                        tr("library.archive.empty")
                    };
                    empty(&t, if section == LibrarySection::History { IconName::History } else { IconName::Archive }, text)
                } else {
                    list(
                        self.list.clone(),
                        cx.processor(|this, ix: usize, _window, cx| {
                            if !this.history_done && !this.loading_more && this.section == LibrarySection::History && ix + 30 >= this.lines.len() {
                                // Never rebuild the list while it is being painted: load on the next turn.
                                this.loading_more = true;
                                cx.spawn(async move |this, cx| {
                                    this.update(cx, |this, cx| {
                                        this.loading_more = false;
                                        this.load_more(cx);
                                        cx.notify();
                                    })
                                    .ok();
                                })
                                .detach();
                            }
                            this.render_line(ix, cx)
                        }),
                    )
                    .size_full()
                    .into_any_element()
                }
            }
            LibrarySection::Downloads => {
                if snap.downloads.is_empty() {
                    empty(&t, IconName::Download, tr("library.downloads.empty"))
                } else {
                    div()
                        .id("lib-dl")
                        .size_full()
                        .flex()
                        .flex_col()
                        .gap(space::HALF)
                        .pt(space::S3)
                        .overflow_y_scroll()
                        .children(snap.downloads.iter().cloned().map(DownloadRow::new))
                        .into_any_element()
                }
            }
            LibrarySection::Spaces => div()
                .id("lib-spaces")
                .size_full()
                .overflow_y_scroll()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(space::S3)
                        .pt(space::S4)
                        .children(snap.spaces.iter().enumerate().map(|(ix, sp)| space_card(&t, sp, sp.id == snap.active_space_id, ix))),
                )
                .child(div().mt(space::S4).text_size(t.type_scale.sub).line_height(gpui::relative(1.45)).text_color(t.text_2).child(tr("library.spaces.note")))
                .into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .bg(t.bg)
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .line_height(gpui::relative(1.4))
            .child(nav)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .bg(t.win_bg)
                    .border_l(px(0.5))
                    .border_color(t.border)
                    .child(title)
                    .child(div().flex_1().min_h_0().pt(space::S1).pb(space::S7).px(space::S5).child(body)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::types::{FolderItem, SplitItem, TabItem};

    #[test]
    fn counts_tabs_of_items() {
        let tab = Item::Tab(TabItem { tab_id: "a".into(), group: None });
        let split = Item::Split(SplitItem { id: "s".into(), tab_ids: vec!["b".into(), "c".into()], ..Default::default() });
        let folder = Item::Folder(FolderItem { items: vec![tab.clone(), split.clone()], ..Default::default() });
        assert_eq!(tab_count(&tab), 1);
        assert_eq!(tab_count(&split), 2);
        assert_eq!(tab_count(&folder), 3);
    }
}

/// `::first-letter { text-transform: uppercase }` of the day headings.
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

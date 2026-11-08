//! Port of `renderer/src/toolbar/BookmarksBar.tsx`: the space's bookmarks bar, under a pane's address bar.
//! Bookmarks that do not fit go into the "»" menu; a folder opens as a popover above the pages. Click: in the
//! pane's tab; Command-click (Ctrl-click) or middle click: new tab. Drag: reorder, file into a folder, or drop a tab
//! to add it.

use super::anchor::Anchor;
use super::icons::favicon_source;
use super::util::{BarColors, rect_of, wants_new_tab};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::screens::tabs::TabDrag;
use crate::theme::{ThemeExt, radius, space};
use crate::widgets::Favicon;
use gpui::{
    App, AppContext, Context, DragMoveEvent, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    TextRun, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{Icon, tooltip::Tooltip};
use signet_core::api::UiEvent;
use signet_core::types::{Bookmark, BookmarkTarget, DragSource, PopoverRequest, RenameTarget};

/// Room reserved for the "»" button when bookmarks overflow.
const MORE: f32 = 30.0;
/// Gap between two bookmarks.
const ITEM_GAP: f32 = 2.0;
/// Horizontal padding of the bar (`0 var(--s2)`).
const BAR_PAD: f32 = 8.0;
const MAX_ITEM: f32 = 180.0;

/// A bookmark being dragged (reorder in the bar or file into a folder).
#[derive(Clone)]
pub struct BookmarkDrag {
    pub id: String,
    label: SharedString,
}

struct Ghost(SharedString, BarColors);

impl Render for Ghost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().h(px(24.0)).px(space::S2).flex().items_center().rounded(radius::CONTROL).bg(self.1.hover).text_color(self.1.fg).child(self.0.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
    Into,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mark {
    index: usize,
    side: Side,
}

/// How many bookmarks fit in `width`, given the width of each one: all of them if they fit, otherwise those that
/// leave room for the "»" button.
pub fn fit_count(widths: &[f32], width: f32) -> usize {
    let mut rights = Vec::with_capacity(widths.len());
    let mut x = 0.0;
    for (i, w) in widths.iter().enumerate() {
        x += w + if i > 0 { ITEM_GAP } else { 0.0 };
        rights.push(x);
    }
    match rights.last() {
        Some(last) if *last > width => rights.iter().position(|r| *r > width - MORE).unwrap_or(widths.len()),
        _ => widths.len(),
    }
}

/// Where a bookmark dragged over item `x` (0..1 across the item) lands. A folder's middle files inside.
pub fn side_at(x: f32, folder: bool) -> Side {
    if folder && x > 0.25 && x < 0.75 {
        Side::Into
    } else if x < 0.5 {
        Side::Before
    } else {
        Side::After
    }
}

fn has_bookmark(items: &[Bookmark], id: &str) -> bool {
    items.iter().any(|b| match b {
        Bookmark::Folder { id: fid, items, .. } => fid == id || has_bookmark(items, id),
        Bookmark::Link { id: lid, .. } => lid == id,
    })
}

pub struct BookmarksBar {
    items: Vec<Bookmark>,
    tab_id: Option<String>,
    focused: bool,
    keys: Option<String>,
    bar: BarColors,
    mark: Option<Mark>,
    bar_anchor: Anchor,
    more: Anchor,
    /// Width the last render laid out for, to re-render when the real one differs.
    used_width: f32,
    item_anchors: std::rc::Rc<std::cell::RefCell<Vec<(String, Anchor)>>>,
    fit: usize,
}

impl BookmarksBar {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.subscribe(&ui_state(cx), |this: &mut Self, _, e: &UiEvent, cx| {
                if let UiEvent::RenameBookmark { id } = e {
                    this.rename(id, cx);
                }
            })
            .detach();
            Self {
                items: vec![],
                tab_id: None,
                focused: false,
                keys: None,
                bar: BarColors { bg: gpui::black(), fg: gpui::black(), fg2: gpui::black(), hover: gpui::black() },
                mark: None,
                bar_anchor: Anchor::default(),
                more: Anchor::default(),
                used_width: 0.0,
                item_anchors: Default::default(),
                fit: 0,
            }
        })
    }

    /// Props from the main window. `keys`: current "Add to bookmarks" shortcut (None: no shortcut).
    pub fn set(&mut self, items: &[Bookmark], tab_id: Option<&str>, focused: bool, keys: Option<String>, bar: BarColors, cx: &mut Context<Self>) {
        if self.items != items || self.tab_id.as_deref() != tab_id || self.focused != focused || self.keys != keys || self.bar != bar {
            self.items = items.to_vec();
            self.tab_id = tab_id.map(String::from);
            self.focused = focused;
            self.keys = keys;
            self.bar = bar;
            cx.notify();
        }
    }

    /// Rename requested by the bookmark's menu: field under the bookmark, or under "»" if it is hidden.
    fn rename(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.focused {
            return;
        }
        let shown = |anchors: &[(String, Anchor)], id: &str, fit: usize| anchors.iter().take(fit).find(|(i, _)| i == id).map(|(_, a)| a.bounds());
        let anchors = self.item_anchors.borrow();
        let holder = self.items.iter().find(|b| b.id() == id || matches!(b, Bookmark::Folder { items, .. } if has_bookmark(items, id))).map(|b| b.id().to_string());
        let bounds = shown(&anchors, id, self.fit)
            .or_else(|| holder.and_then(|h| shown(&anchors, &h, self.fit)))
            .or_else(|| (self.fit < self.items.len()).then(|| self.more.bounds()))
            .unwrap_or_else(|| self.bar_anchor.bounds());
        api(cx).open_popover(PopoverRequest::Rename { target: RenameTarget::Bookmark { id: id.to_string() } }, rect_of(bounds));
    }

    fn target(&self, m: Mark) -> BookmarkTarget {
        match (self.items.get(m.index), m.side) {
            (Some(Bookmark::Folder { id, items, .. }), Side::Into) => BookmarkTarget { folder_id: Some(id.clone()), index: items.len() },
            (_, Side::After) => BookmarkTarget { folder_id: None, index: m.index + 1 },
            _ => BookmarkTarget { folder_id: None, index: m.index },
        }
    }

    /// Drop of a bookmark (reorder) at the marked place, or at the end.
    fn drop_bookmark(&mut self, id: &str, cx: &mut Context<Self>) {
        let target = self.mark.take().map(|m| self.target(m)).unwrap_or(BookmarkTarget { folder_id: None, index: self.items.len() });
        api(cx).move_bookmark(id, target);
    }

    /// A tab dropped on the bar (from the sidebar or the tab strip): adds it at the marked place, or at the end.
    pub fn drop_tab(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        let target = self.mark.take().map(|m| self.target(m)).unwrap_or(BookmarkTarget { folder_id: None, index: self.items.len() });
        api(cx).add_bookmark(tab_id, target);
        cx.notify();
    }
}

fn text_width(window: &Window, text: &str, size: gpui::Pixels) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let style = window.text_style();
    let run = TextRun { len: text.len(), font: style.font(), color: style.color, background_color: None, underline: None, strikethrough: None };
    f32::from(window.text_system().shape_line(text.to_string().into(), size, &[run], None).width)
}

impl Render for BookmarksBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.mark = None;
        }
        let t = cx.signet().clone();
        let bar = self.bar;
        let size = t.type_scale.sub;
        // Width available to the bookmarks: measured by the previous frame, re-measured after paint.
        let measured = f32::from(self.bar_anchor.bounds().size.width);
        let width = (measured - 2.0 * BAR_PAD).max(0.0);
        let widths: Vec<f32> = self
            .items
            .iter()
            .map(|b| {
                let (title, folder) = match b {
                    Bookmark::Link { title, .. } => (title.as_str(), false),
                    Bookmark::Folder { name, .. } => (name.as_str(), true),
                };
                let w = 2.0 * 8.0 + 14.0 + 6.0 + text_width(window, title, size) + if folder { 6.0 + 12.0 } else { 0.0 };
                w.min(MAX_ITEM)
            })
            .collect();
        let fit = if measured > 0.0 { fit_count(&widths, width) } else { self.items.len() };
        self.fit = fit;
        {
            let mut anchors = self.item_anchors.borrow_mut();
            anchors.resize_with(self.items.len(), || (String::new(), Anchor::default()));
            for (slot, b) in anchors.iter_mut().zip(&self.items) {
                if slot.0 != b.id() {
                    *slot = (b.id().to_string(), Anchor::default());
                }
            }
        }
        let anchors: Vec<Anchor> = self.item_anchors.borrow().iter().map(|(_, a)| a.clone()).collect();
        // Re-render once if the real width is not the one this frame used.
        if (measured - self.used_width).abs() > 0.5 {
            self.used_width = measured;
            cx.notify();
        }

        let tab_id = self.tab_id.clone();
        let mut row = div().id("bm-items").flex_1().min_w_0().h_full().flex().items_center().gap(px(ITEM_GAP)).overflow_hidden();
        for (i, item) in self.items.iter().enumerate().take(fit) {
            let (id, label, folder) = match item {
                Bookmark::Link { id, title, .. } => (id.clone(), title.clone(), false),
                Bookmark::Folder { id, name, .. } => (id.clone(), name.clone(), true),
            };
            let tip = match item {
                Bookmark::Link { title, url, .. } => format!("{title}\n{url}"),
                Bookmark::Folder { name, .. } => name.clone(),
            };
            let mark = self.mark.filter(|m| m.index == i);
            let anchor = anchors[i].clone();
            let a_click = anchor.clone();
            let (tid_click, tid_menu, tid_mid) = (tab_id.clone(), tab_id.clone(), tab_id.clone());
            let (id_click, id_menu, id_mid, id_drag) = (id.clone(), id.clone(), id.clone(), id.clone());
            let drag = BookmarkDrag { id: id_drag, label: label.clone().into() };
            let accent = t.accent;
            let icon: gpui::AnyElement = match item {
                Bookmark::Link { favicon, .. } => Favicon::new(favicon_source(favicon.as_deref())).size(14.0).into_any_element(),
                Bookmark::Folder { .. } => Icon::from(crate::IconName::Folder).size(px(14.0)).into_any_element(),
            };
            row = row.child(
                div()
                    .id(SharedString::from(format!("bm-{id}")))
                    .relative()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .max_w(px(MAX_ITEM))
                    .h(px(24.0))
                    .px(space::S2)
                    .rounded(radius::CONTROL)
                    .text_size(size)
                    .text_color(bar.fg2)
                    .occlude()
                    .cursor_pointer()
                    .hover(move |s| s.bg(bar.hover).text_color(bar.fg))
                    .when(mark.is_some_and(|m| m.side == Side::Into), |d| d.bg(bar.hover).border_2().border_color(accent))
                    .when(mark.is_some_and(|m| m.side == Side::Before), |d| d.child(div().absolute().left(px(-2.0)).top(px(3.0)).bottom(px(3.0)).w(px(2.0)).rounded(px(1.0)).bg(accent)))
                    .when(mark.is_some_and(|m| m.side == Side::After), |d| d.child(div().absolute().right(px(-2.0)).top(px(3.0)).bottom(px(3.0)).w(px(2.0)).rounded(px(1.0)).bg(accent)))
                    .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
                    .on_drag(drag, move |d: &BookmarkDrag, _, _, cx| cx.new(|_| Ghost(d.label.clone(), bar)))
                    .on_drag_move::<BookmarkDrag>(cx.listener(move |this, e: &DragMoveEvent<BookmarkDrag>, _, cx| {
                        let b = e.bounds;
                        let p = e.event.position;
                        if !b.contains(&p) {
                            return;
                        }
                        let x = f32::from(p.x - b.origin.x) / f32::from(b.size.width).max(1.0);
                        let side = side_at(x, folder);
                        if this.mark != Some(Mark { index: i, side }) {
                            this.mark = Some(Mark { index: i, side });
                            cx.notify();
                        }
                    }))
                    .on_drag_move::<TabDrag>(cx.listener(move |this, e: &DragMoveEvent<TabDrag>, _, cx| {
                        let b = e.bounds;
                        let p = e.event.position;
                        if !b.contains(&p) || !matches!(e.drag(cx).src, DragSource::Tab { .. }) {
                            return;
                        }
                        let x = f32::from(p.x - b.origin.x) / f32::from(b.size.width).max(1.0);
                        let side = side_at(x, folder);
                        if this.mark != Some(Mark { index: i, side }) {
                            this.mark = Some(Mark { index: i, side });
                            cx.notify();
                        }
                    }))
                    .on_click(move |e, _, cx| {
                        if e.is_right_click() {
                            return;
                        }
                        if folder {
                            api(cx).open_popover(PopoverRequest::Bookmarks { folder_id: Some(id_click.clone()), from: None, tab_id: tid_click.clone() }, rect_of(a_click.bounds()));
                        } else {
                            let new_tab = wants_new_tab(e.modifiers(), e.is_middle_click());
                            api(cx).open_bookmark(&id_click, tid_click.as_deref(), new_tab);
                        }
                    })
                    .on_mouse_down(MouseButton::Middle, move |_, _, cx| {
                        if !folder {
                            cx.stop_propagation();
                            api(cx).open_bookmark(&id_mid, tid_mid.as_deref(), true);
                        }
                    })
                    .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                        cx.stop_propagation();
                        api(cx).show_bookmark_menu(&id_menu, tid_menu.as_deref());
                    })
                    .child(icon)
                    .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(label))
                    .when(folder, |d| d.child(Icon::from(crate::IconName::ChevronDown).size(px(12.0)).opacity(0.7)))
                    .child(anchor.track()),
            );
        }
        if self.items.is_empty() {
            let hint = match &self.keys {
                Some(k) if !k.is_empty() => tr_args("bookmarks.hint", &[("keys", k)]),
                _ => tr("bookmarks.hintNoKeys"),
            };
            row = row.child(
                div().px(space::S2).text_size(t.type_scale.cap).text_color(bar.fg2).opacity(0.8).whitespace_nowrap().overflow_hidden().text_ellipsis().child(hint),
            );
        }

        let overflow = fit < self.items.len();
        let more_anchor = self.more.clone();
        let more_track = self.more.clone();
        let tid = tab_id.clone();
        let items_len = self.items.len();
        let _ = items_len;
        div()
            .id("bm-bar")
            .relative()
            .h_full()
            .w_full()
            .flex()
            .items_center()
            .gap(space::HALF)
            .px(space::S2)
            .pb(space::S1)
            .text_color(bar.fg2)
            .on_mouse_down(MouseButton::Right, |_, _, cx| api(cx).show_bookmarks_bar_menu())
            .on_drop(cx.listener(|this, d: &BookmarkDrag, _, cx| {
                this.drop_bookmark(&d.id, cx);
                cx.notify();
            }))
            .on_drop(cx.listener(|this, d: &TabDrag, _, cx| {
                // A tab from the sidebar or the tab strip: added as a bookmark (the other drags do not apply).
                if let DragSource::Tab { tab_id } = &d.src {
                    this.drop_tab(tab_id, cx);
                } else {
                    this.mark = None;
                    cx.notify();
                }
            }))
            .child(row)
            .when(overflow, move |d| {
                d.child(
                    div()
                        .id("bm-more")
                        .relative()
                        .flex_none()
                        .w(px(26.0))
                        .h(px(24.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius::CONTROL)
                        .occlude()
                        .cursor_pointer()
                        .hover(move |s| s.bg(bar.hover))
                        .tooltip(|w, cx| Tooltip::new(tr("bookmarks.more")).build(w, cx))
                        .on_click(move |_, _, cx| {
                            api(cx).open_popover(PopoverRequest::Bookmarks { folder_id: None, from: Some(fit), tab_id: tid.clone() }, rect_of(more_anchor.bounds()));
                        })
                        .child(Icon::from(crate::IconName::ChevronsRight).size(px(15.0)))
                        .child(more_track.track()),
                )
            })
            .child(self.bar_anchor.track())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_fits() {
        assert_eq!(fit_count(&[50.0, 50.0], 200.0), 2);
        assert_eq!(fit_count(&[], 10.0), 0);
        // Exactly the width: still fits.
        assert_eq!(fit_count(&[99.0, 99.0], 200.0), 2);
    }

    #[test]
    fn overflow_leaves_room_for_more() {
        // Rights: 80, 162, 244. Width 200, limit 170: two fit.
        assert_eq!(fit_count(&[80.0, 80.0, 80.0], 200.0), 2);
        // Width 180, limit 150: only one.
        assert_eq!(fit_count(&[80.0, 80.0, 80.0], 180.0), 1);
        assert_eq!(fit_count(&[200.0, 10.0], 100.0), 0);
    }

    #[test]
    fn drop_sides() {
        assert_eq!(side_at(0.1, false), Side::Before);
        assert_eq!(side_at(0.9, false), Side::After);
        assert_eq!(side_at(0.5, false), Side::After);
        assert_eq!(side_at(0.5, true), Side::Into);
        assert_eq!(side_at(0.1, true), Side::Before);
        assert_eq!(side_at(0.8, true), Side::After);
    }

    #[test]
    fn finds_nested_bookmarks() {
        let items = vec![Bookmark::Folder { id: "f".into(), name: "F".into(), items: vec![Bookmark::Link { id: "l".into(), title: "L".into(), url: "u".into(), favicon: None }] }];
        assert!(has_bookmark(&items, "l"));
        assert!(has_bookmark(&items, "f"));
        assert!(!has_bookmark(&items, "x"));
    }
}

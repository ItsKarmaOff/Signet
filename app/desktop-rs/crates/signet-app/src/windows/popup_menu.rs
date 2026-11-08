//! Context menu drawn by GPUI in its own small window: the menu of Linux (no GTK in the process), also usable on
//! macOS with `SIGNET_GPUI_MENU=1` (to try it there). Windows and macOS normally use the native menus
//! (`win.rs`, `mac.rs`), which block until a choice is made; this one is asynchronous: the callback runs when an
//! item is clicked (or entered with the keyboard), and never if the menu is dismissed.
//!
//! The pure parts (flattening of the entries, sizes, keyboard moves, clamping to the screen) are at the top and
//! unit tested. Submenus open inline under their row (the window grows), which is simple and robust.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, prelude::FluentBuilder, px, size,
};
use gpui_component::Icon;
use signet_ui::icon::IconName;
use signet_ui::theme::{ThemeExt, radius, space};

use super::Entry;
use crate::browser::{Browser, Core};

pub type Callback = Box<dyn FnOnce(&mut Core, &str)>;

pub const ROW_H: f32 = 28.0;
pub const SEP_H: f32 = 9.0;
pub const PAD: f32 = 5.0;
pub const MIN_W: f32 = 160.0;
pub const MAX_W: f32 = 380.0;
const CHAR_W: f32 = 7.2;
const SIDE: f32 = 56.0;

static AUTOCLOSE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Self-test hook: every menu dismisses itself after `ms` milliseconds (0 = never), native ones included.
pub fn set_autoclose(ms: u64) {
    AUTOCLOSE_MS.store(ms, std::sync::atomic::Ordering::Relaxed);
}

pub fn autoclose() -> Option<u64> {
    Some(AUTOCLOSE_MS.load(std::sync::atomic::Ordering::Relaxed)).filter(|m| *m > 0)
}

/// Whether the GPUI menu replaces the native one (always on Linux, opt-in elsewhere).
pub fn wanted() -> bool {
    cfg!(target_os = "linux") || std::env::var_os("SIGNET_GPUI_MENU").is_some_and(|v| v != "0")
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowKind {
    Sep,
    Item { label: String, id: String, enabled: bool, checked: bool },
    Sub { label: String, open: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Index path in the entry tree (the key of an open submenu).
    pub path: Vec<usize>,
    pub depth: usize,
    pub kind: RowKind,
}

impl Row {
    /// Rows the keyboard and the pointer can land on.
    pub fn selectable(&self) -> bool {
        match &self.kind {
            RowKind::Sep => false,
            RowKind::Item { enabled, .. } => *enabled,
            RowKind::Sub { .. } => true,
        }
    }
}

/// The visible rows: submenus listed under their row only when open.
pub fn flatten(entries: &[Entry], open: &HashSet<Vec<usize>>) -> Vec<Row> {
    fn walk(entries: &[Entry], prefix: &[usize], depth: usize, open: &HashSet<Vec<usize>>, out: &mut Vec<Row>) {
        for (i, e) in entries.iter().enumerate() {
            let mut path = prefix.to_vec();
            path.push(i);
            match e {
                Entry::Sep => out.push(Row { path, depth, kind: RowKind::Sep }),
                Entry::Item { label, id, enabled, checked } => out.push(Row { path, depth, kind: RowKind::Item { label: label.clone(), id: id.clone(), enabled: *enabled, checked: *checked } }),
                Entry::Sub { label, items } => {
                    let is_open = open.contains(&path);
                    out.push(Row { path: path.clone(), depth, kind: RowKind::Sub { label: label.clone(), open: is_open } });
                    if is_open {
                        walk(items, &path, depth + 1, open, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(entries, &[], 0, open, &mut out);
    // A separator at an edge or doubled (after a hidden entry) is noise.
    let mut cleaned: Vec<Row> = Vec::with_capacity(out.len());
    for r in out {
        if r.kind == RowKind::Sep && cleaned.last().is_none_or(|p| p.kind == RowKind::Sep) {
            continue;
        }
        cleaned.push(r);
    }
    while cleaned.last().is_some_and(|r| r.kind == RowKind::Sep) {
        cleaned.pop();
    }
    cleaned
}

/// Ids of every leaf, depth-first (the mapping a native menu uses for its tags).
pub fn leaf_ids(entries: &[Entry]) -> Vec<String> {
    let mut v = Vec::new();
    for e in entries {
        match e {
            Entry::Item { id, .. } => v.push(id.clone()),
            Entry::Sub { items, .. } => v.extend(leaf_ids(items)),
            Entry::Sep => {}
        }
    }
    v
}

/// Size of the window for these rows.
pub fn menu_size(rows: &[Row]) -> (f32, f32) {
    let h: f32 = rows.iter().map(|r| if r.kind == RowKind::Sep { SEP_H } else { ROW_H }).sum::<f32>() + 2.0 * PAD;
    let w = rows
        .iter()
        .map(|r| match &r.kind {
            RowKind::Sep => 0.0,
            RowKind::Item { label, .. } | RowKind::Sub { label, .. } => label.chars().count() as f32 * CHAR_W + SIDE + r.depth as f32 * 14.0,
        })
        .fold(MIN_W, f32::max);
    (w.min(MAX_W), h)
}

/// Next selectable row after `cur` going `dir` (1 or -1), wrapping around; from nothing, the first or the last.
pub fn step(rows: &[Row], cur: Option<usize>, dir: i32) -> Option<usize> {
    let n = rows.len() as i32;
    if n == 0 || !rows.iter().any(Row::selectable) {
        return None;
    }
    let mut i = match cur {
        Some(c) => c as i32,
        None if dir > 0 => -1,
        None => n,
    };
    for _ in 0..n {
        i = (i + dir).rem_euclid(n);
        if rows[i as usize].selectable() {
            return Some(i as usize);
        }
    }
    None
}

/// Top-left of a menu of `menu` size wanted at `at`, kept inside `screen` (x, y, w, h): it flips to the left of the
/// point or above it when it would overflow, then is clamped.
pub fn clamp_origin(at: (f32, f32), menu: (f32, f32), screen: (f32, f32, f32, f32)) -> (f32, f32) {
    let (sx, sy, sw, sh) = screen;
    let mut x = at.0;
    let mut y = at.1;
    if x + menu.0 > sx + sw {
        x -= menu.0;
    }
    if y + menu.1 > sy + sh {
        y -= menu.1;
    }
    let x = x.min(sx + sw - menu.0).max(sx);
    let y = y.min(sy + sh - menu.1).max(sy);
    (x, y)
}

// ---------------------------------------------------------------------------------------------------- view

struct MenuView {
    entries: Vec<Entry>,
    open: HashSet<Vec<usize>>,
    rows: Vec<Row>,
    selected: Option<usize>,
    cb: Rc<RefCell<Option<Callback>>>,
    browser: Rc<Browser>,
    focus: FocusHandle,
    was_active: bool,
    _sub: Option<Subscription>,
}

impl Focusable for MenuView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MenuView {
    fn relayout(&mut self, window: &mut Window) {
        self.rows = flatten(&self.entries, &self.open);
        let (w, h) = menu_size(&self.rows);
        window.resize(size(px(w), px(h)));
    }

    /// Closes the window without running the callback.
    fn dismiss(&mut self, window: &mut Window) {
        if self.cb.borrow_mut().take().is_some()
            && let Ok(c) = self.browser.core.try_borrow()
        {
            c.note("popup-closed picked=none");
        }
        window.remove_window();
    }

    fn activate(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(idx).cloned() else { return };
        match row.kind {
            RowKind::Sep => {}
            RowKind::Sub { open, .. } => {
                if open {
                    self.open.retain(|p| !p.starts_with(&row.path));
                } else {
                    self.open.insert(row.path.clone());
                }
                self.relayout(window);
                cx.notify();
            }
            RowKind::Item { id, enabled, .. } => {
                if !enabled {
                    return;
                }
                let cb = self.cb.borrow_mut().take();
                let browser = self.browser.clone();
                if let Ok(c) = browser.core.try_borrow() {
                    c.note(format!("popup-closed picked={id}"));
                }
                window.remove_window();
                // After the window is gone and outside of its event handler: the callback may change any window.
                cx.defer(move |_| {
                    if let Some(cb) = cb {
                        cb(&mut browser.core.borrow_mut(), &id);
                    }
                });
            }
        }
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match e.keystroke.key.as_str() {
            "escape" => self.dismiss(window),
            "down" => self.selected = step(&self.rows, self.selected, 1),
            "up" => self.selected = step(&self.rows, self.selected, -1),
            "home" => self.selected = step(&self.rows, None, 1),
            "end" => self.selected = step(&self.rows, None, -1),
            "enter" | "space" => {
                if let Some(i) = self.selected {
                    self.activate(i, window, cx);
                }
            }
            "right" => {
                if let Some(i) = self.selected
                    && matches!(self.rows[i].kind, RowKind::Sub { open: false, .. })
                {
                    self.activate(i, window, cx);
                }
            }
            "left" => {
                if let Some(i) = self.selected
                    && let Some(parent) = self.rows[i].path.split_last().map(|(_, p)| p.to_vec()).filter(|p| !p.is_empty())
                {
                    self.open.retain(|p| !p.starts_with(&parent));
                    self.relayout(window);
                    self.selected = self.rows.iter().position(|r| r.path == parent);
                }
            }
            _ => return,
        }
        cx.notify();
    }
}

impl Render for MenuView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let mut list = div().flex().flex_col().p(px(PAD)).size_full();
        for (i, row) in self.rows.iter().enumerate() {
            let el = match &row.kind {
                RowKind::Sep => div().h(px(SEP_H)).flex().items_center().child(div().h(px(1.0)).w_full().bg(t.border)).into_any_element(),
                RowKind::Item { label, enabled, checked, .. } => {
                    let color = if *enabled { t.text } else { t.text_3 };
                    div()
                        .id(("row", i))
                        .h(px(ROW_H))
                        .flex()
                        .items_center()
                        .gap(space::S2)
                        .px(space::S2)
                        .pl(px(8.0 + row.depth as f32 * 14.0))
                        .rounded(radius::ROW_IN)
                        .text_color(color)
                        .text_size(t.type_scale.base)
                        .when(self.selected == Some(i) && *enabled, |d| d.bg(t.hover))
                        .child(div().w(px(14.0)).flex().items_center().when(*checked, |d| d.child(Icon::from(IconName::Check).size(px(14.0)))))
                        .child(div().flex_1().overflow_hidden().whitespace_nowrap().child(label.clone()))
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if this.selected != Some(i) {
                                this.selected = Some(i);
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| this.activate(i, window, cx)))
                        .into_any_element()
                }
                RowKind::Sub { label, open } => div()
                    .id(("row", i))
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .px(space::S2)
                    .pl(px(8.0 + row.depth as f32 * 14.0))
                    .rounded(radius::ROW_IN)
                    .text_color(t.text)
                    .text_size(t.type_scale.base)
                    .when(self.selected == Some(i), |d| d.bg(t.hover))
                    .child(div().w(px(14.0)))
                    .child(div().flex_1().overflow_hidden().whitespace_nowrap().child(label.clone()))
                    .child(Icon::from(if *open { IconName::ChevronDown } else { IconName::ChevronRight }).size(px(14.0)))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.selected != Some(i) {
                            this.selected = Some(i);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| this.activate(i, window, cx)))
                    .into_any_element(),
            };
            list = list.child(el);
        }
        div()
            .id("popup-menu")
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e, window, cx| this.on_key(e, window, cx)))
            .child(div().size_full().bg(t.glass_view).border_1().border_color(t.border).rounded(radius::CARD).shadow(t.shadow_pop.clone()).child(list))
    }
}

/// Opens the menu at `at` (global screen coordinates, logical pixels). Returns the window so the manager can close
/// it when another menu replaces it.
pub fn open(browser: &Rc<Browser>, entries: Vec<Entry>, at: (f64, f64), cb: Callback, cx: &mut App) -> Option<AnyWindowHandle> {
    let rows = flatten(&entries, &HashSet::new());
    if rows.is_empty() {
        return None;
    }
    let (w, h) = menu_size(&rows);
    let screen = cx
        .displays()
        .iter()
        .map(|d| d.visible_bounds())
        .find(|b| {
            let (x, y) = (f32::from(b.origin.x), f32::from(b.origin.y));
            at.0 as f32 >= x && (at.0 as f32) < x + f32::from(b.size.width) && at.1 as f32 >= y && (at.1 as f32) < y + f32::from(b.size.height)
        })
        .or_else(|| cx.primary_display().map(|d| d.visible_bounds()))
        .map(|b| (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height)))
        .unwrap_or((0.0, 0.0, 1920.0, 1080.0));
    let (x, y) = clamp_origin((at.0 as f32, at.1 as f32), (w, h), screen);
    let cb = Rc::new(RefCell::new(Some(cb)));
    let b = browser.clone();
    let cb2 = cb.clone();
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(x), px(y)), size(px(w), px(h))))),
            titlebar: None,
            window_background: WindowBackgroundAppearance::Transparent,
            kind: WindowKind::PopUp,
            focus: true,
            show: true,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        },
        move |window, cx| {
            let view = cx.new(|cx| {
                let focus = cx.focus_handle();
                // Focus lost (a click elsewhere, another window): the menu goes away, like a native one.
                let sub = cx.observe_window_activation(window, |this: &mut MenuView, window, _cx| {
                    if window.is_window_active() {
                        this.was_active = true;
                    } else if this.was_active {
                        this.dismiss(window);
                    }
                });
                MenuView { entries, open: HashSet::new(), rows, selected: None, cb: cb2, browser: b, focus, was_active: false, _sub: Some(sub) }
            });
            window.focus(&view.focus_handle(cx), cx);
            view
        },
    );
    match opened {
        Ok(h) => {
            let _ = h.update(cx, |_, w, _| w.activate_window());
            browser.core.borrow().note("popup-shown gpui");
            if let Some(ms) = autoclose() {
                let h2: AnyWindowHandle = h.into();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(ms)).await;
                    let _ = cx.update(|app| {
                        let _ = h2.update(app, |view, w, cx| {
                            if let Ok(v) = view.downcast::<MenuView>() {
                                v.update(cx, |v, _| v.dismiss(w));
                            }
                        });
                    });
                })
                .detach();
            }
            Some(h.into())
        }
        Err(e) => {
            eprintln!("cannot open the context menu: {e}");
            let _ = cb;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(l: &str, id: &str, enabled: bool) -> Entry {
        Entry::Item { label: l.into(), id: id.into(), enabled, checked: false }
    }

    fn sample() -> Vec<Entry> {
        vec![
            Entry::Sep,
            item("A", "a", true),
            Entry::Sep,
            Entry::Sep,
            item("B", "b", false),
            Entry::Sub { label: "More".into(), items: vec![item("C", "c", true), Entry::Sep, item("D", "d", true)] },
            Entry::Sep,
        ]
    }

    #[test]
    fn flatten_drops_edge_and_double_separators() {
        let rows = flatten(&sample(), &HashSet::new());
        let kinds: Vec<&str> = rows.iter().map(|r| match r.kind { RowKind::Sep => "-", RowKind::Item { .. } => "i", RowKind::Sub { .. } => "s" }).collect();
        assert_eq!(kinds, ["i", "-", "i", "s"]);
    }

    #[test]
    fn open_submenu_adds_indented_rows() {
        let mut open = HashSet::new();
        open.insert(vec![5]);
        let rows = flatten(&sample(), &open);
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[4].depth, 1);
        assert_eq!(rows[4].path, vec![5, 0]);
    }

    #[test]
    fn leaf_ids_are_depth_first() {
        assert_eq!(leaf_ids(&sample()), ["a", "b", "c", "d"]);
    }

    #[test]
    fn step_skips_disabled_and_separators_and_wraps() {
        let rows = flatten(&sample(), &HashSet::new());
        assert_eq!(step(&rows, None, 1), Some(0));
        assert_eq!(step(&rows, Some(0), 1), Some(3));
        assert_eq!(step(&rows, Some(3), 1), Some(0));
        assert_eq!(step(&rows, None, -1), Some(3));
        assert_eq!(step(&rows, Some(0), -1), Some(3));
        assert_eq!(step(&[], None, 1), None);
    }

    #[test]
    fn size_grows_with_rows_and_stays_bounded() {
        let rows = flatten(&sample(), &HashSet::new());
        let (w, h) = menu_size(&rows);
        assert_eq!(h, 3.0 * ROW_H + SEP_H + 2.0 * PAD);
        assert_eq!(w, MIN_W);
        let long = flatten(&[item(&"x".repeat(200), "x", true)], &HashSet::new());
        assert_eq!(menu_size(&long).0, MAX_W);
    }

    #[test]
    fn origin_flips_then_clamps_to_the_screen() {
        let screen = (0.0, 0.0, 1000.0, 800.0);
        assert_eq!(clamp_origin((100.0, 100.0), (200.0, 300.0), screen), (100.0, 100.0));
        assert_eq!(clamp_origin((900.0, 100.0), (200.0, 300.0), screen), (700.0, 100.0));
        assert_eq!(clamp_origin((100.0, 700.0), (200.0, 300.0), screen), (100.0, 400.0));
        assert_eq!(clamp_origin((10.0, 10.0), (2000.0, 2000.0), screen), (0.0, 0.0));
        assert_eq!(clamp_origin((-50.0, -50.0), (100.0, 100.0), (0.0, 0.0, 500.0, 500.0)), (0.0, 0.0));
    }
}

//! Port of `tabs/TabStrip.tsx` (SPEC v12, tabs on top): the same Snapshot as the sidebar, shown as a row of pills with
//! the same calls to the app (activation, move, menus, rename). Dropdown menus and forms go through `open_popover`:
//! they live in the overlay, above the web views.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, AppContext, Bounds, Context, DragMoveEvent, Entity, EventEmitter, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, Subscription, WeakEntity, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{Icon, tooltip::Tooltip};
use signet_core::api::UiEvent;
use signet_core::i18n::IS_MAC;
use signet_core::shortcuts::shortcut_label;
use signet_core::types::{DragSource, FolderItem, IconTarget, Item, PaletteMode, PopoverRequest, Rect, RenameTarget, Snapshot, SplitItem, SplitLayout, TabGroup, TabInfo, Zone};

use super::custom::{RenameEvent, RenameField, display_title, emoji_icon, folder_icon, tab_icon, tint};
use super::groups::{Drop, Indicator, Run, runs_of, today_mark};
use super::optimistic::{Optimistic, activate_tab, optimistic};
use super::preview_hover::{PreviewDriver, preview_driver};
use crate::title_area::no_drag;
use super::row::{BoundsMap, DragGhost, NavRing, RowCallbacks, RowEnv, TabDrag, probe, tab_line, with_gestures};
use super::{find_folder, folder_ids, item_key, item_tab_ids, media_labels, media_view};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::icon::IconName;
use crate::motion::reduced_motion;
use crate::screens::sidebar::dnd::{Axis, Cell2, DragCtx, Hit, Region, rank, target_for};
use crate::theme::{SignetTheme, ThemeExt, radius, space, weight};
use crate::widgets::{IconButton, MediaChip};

/// A dragged tab hovering for a while over the space switcher: the menu opens.
const DWELL_MS: u64 = 450;

fn rect_of(b: Bounds<Pixels>) -> Rect {
    Rect::new(f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64, f32::from(b.size.width) as f64, f32::from(b.size.height) as f64)
}

/// Requests the strip cannot serve itself.
#[derive(Clone, Debug, PartialEq)]
pub enum TabStripEvent {}
impl EventEmitter<TabStripEvent> for TabStrip {}

pub struct TabStrip {
    floating: bool,
    opt: Entity<Optimistic>,
    preview: Entity<PreviewDriver>,
    days: ScrollHandle,
    bounds: BoundsMap,
    nav: NavRing,
    regions: RefCell<Vec<Region>>,
    root: Cell2,
    dragging: bool,
    src: Option<DragSource>,
    src_id: Option<String>,
    forbidden: HashSet<String>,
    forbidden_hit: bool,
    drop: Option<Drop>,
    over_space: bool,
    dwell: u64,
    editing: Option<(String, Entity<RenameField>)>,
    edit_sub: Option<Subscription>,
    seen_tabs: Option<(String, HashSet<String>)>,
    fresh: HashMap<String, Instant>,
    reveal: Option<(String, u8)>,
    last_reveal: Option<String>,
    _subs: Vec<Subscription>,
}

impl TabStrip {
    /// Tab row of the main window (`inset: 0 0 auto 0`, 40 px tall: the main window places it).
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(false, window, cx)
    }

    /// Tab row of the floating toolbar (`#toolbar` window in compact mode).
    pub fn new_floating(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(true, window, cx)
    }

    fn build(floating: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let opt = optimistic(cx);
        let preview = preview_driver(cx);
        let ui = ui_state(cx);
        let subs = vec![
            cx.observe(&ui, |_, _, cx| cx.notify()),
            cx.observe(&opt, |_, _, cx| cx.notify()),
            cx.subscribe_in(&ui, window, |this, _ui, ev: &UiEvent, window, cx| this.on_event(ev, window, cx)),
        ];
        TabStrip {
            floating,
            opt,
            preview,
            days: ScrollHandle::new(),
            bounds: BoundsMap::default(),
            nav: NavRing::default(),
            regions: RefCell::new(Vec::new()),
            root: Default::default(),
            dragging: false,
            src: None,
            src_id: None,
            forbidden: HashSet::new(),
            forbidden_hit: false,
            drop: None,
            over_space: false,
            dwell: 0,
            editing: None,
            edit_sub: None,
            seen_tabs: None,
            fresh: HashMap::new(),
            reveal: None,
            last_reveal: None,
            _subs: subs,
        }
    }

    fn anchor_of(&self, key: &str) -> Rect {
        self.bounds.get(key).map(rect_of).unwrap_or_else(|| self.bounds.get("space-switcher").map(rect_of).unwrap_or(Rect::new(78.0, 6.0, 140.0, 28.0)))
    }

    fn on_event(&mut self, ev: &UiEvent, window: &mut Window, cx: &mut Context<Self>) {
        let snap = ui_state(cx).read(cx).snapshot.clone();
        match ev {
            UiEvent::RenameFolder { folder_id } => {
                // Same channel for tab groups: they are recognized by their label in the row.
                let is_group = snap.spaces.iter().any(|s| s.groups.as_ref().is_some_and(|g| g.contains_key(folder_id)));
                let (target, key) = if is_group {
                    (RenameTarget::Group { group_id: folder_id.clone() }, format!("hdr:{folder_id}"))
                } else {
                    (RenameTarget::Folder { folder_id: folder_id.clone() }, folder_id.clone())
                };
                let anchor = self.anchor_of(&key);
                api(cx).open_popover(PopoverRequest::Rename { target }, anchor);
            }
            UiEvent::RenameTab { tab_id } => {
                // A today tab is renamed in place; pills without a title go through a field below them.
                let in_days = snap.spaces.iter().any(|s| s.today.iter().any(|i| item_tab_ids(i).contains(tab_id)));
                if in_days && !snap.favorites.contains(tab_id) {
                    self.begin_edit(tab_id, window, cx);
                } else {
                    let anchor = self.anchor_of(tab_id);
                    api(cx).open_popover(PopoverRequest::Rename { target: RenameTarget::Tab { tab_id: tab_id.clone() } }, anchor);
                }
            }
            UiEvent::PickIcon(target) => {
                let key = match target {
                    IconTarget::Tab { tab_id } => tab_id.clone(),
                    IconTarget::Folder { folder_id } => folder_id.clone(),
                    IconTarget::Group { group_id } => format!("hdr:{group_id}"),
                };
                let anchor = self.anchor_of(&key);
                api(cx).open_popover(PopoverRequest::Icon { target: target.clone() }, anchor);
            }
            _ => {}
        }
    }

    fn begin_edit(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let snap = ui_state(cx).read(cx).snapshot.clone();
        let Some(t) = snap.tabs.get(id) else { return };
        let initial = t.custom_title.clone().unwrap_or_else(|| display_title(t, &tr("sidebar.newTab")));
        let field = cx.new(|cx| RenameField::new(window, cx, &initial, &t.title, 200));
        let id_owned = id.to_string();
        self.edit_sub = Some(cx.subscribe(&field, move |this, _f, ev: &RenameEvent, cx| {
            this.editing = None;
            this.edit_sub = None;
            if let RenameEvent::Commit(v) = ev {
                let snap = ui_state(cx).read(cx).snapshot.clone();
                if let Some(t) = snap.tabs.get(&id_owned) {
                    let next = v.trim().to_string();
                    let custom = t.custom_title.clone().unwrap_or_default();
                    if next != custom && !(next == t.title && t.custom_title.is_none()) {
                        api(cx).rename_tab(&id_owned, if next.is_empty() { None } else { Some(&next) });
                    }
                }
            }
            cx.notify();
        }));
        self.editing = Some((id.to_string(), field));
        self.preview.update(cx, |p, cx| p.hide(cx));
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------------------
    // Drag and drop (same calls to the app as the sidebar)

    fn start_drag(&mut self, src: DragSource, cx: &mut Context<Self>) {
        let snap = ui_state(cx).read(cx).snapshot.clone();
        let space = active_space(&snap, self.opt.read(cx).pending().space_id.as_deref());
        self.forbidden = match (&src, space) {
            (DragSource::Folder { id }, Some(sp)) => find_folder(&sp.pinned, id).map(|f| folder_ids(&Item::Folder(f.clone())).into_iter().collect()).unwrap_or_default(),
            _ => HashSet::new(),
        };
        self.src_id = Some(match &src {
            DragSource::Tab { tab_id } => tab_id.clone(),
            DragSource::Group { id } => format!("g:{id}"),
            DragSource::Split { id } | DragSource::Folder { id } => id.clone(),
        });
        self.dragging = true;
        self.drop = None;
        self.preview.update(cx, |p, cx| p.pointer_down(cx));
        if let DragSource::Tab { tab_id } = &src {
            api(cx).begin_tab_drag(tab_id);
        }
        self.src = Some(src);
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if matches!(self.src, Some(DragSource::Tab { .. })) {
            api(cx).end_tab_drag();
        }
        self.dragging = false;
        self.src = None;
        self.src_id = None;
        self.drop = None;
        self.forbidden.clear();
        self.over_space = false;
        self.dwell += 1;
    }

    fn put_drop(&mut self, t: Option<Drop>, cx: &mut Context<Self>) {
        let mut t = t;
        self.forbidden_hit = t.as_ref().and_then(|d| d.target.folder_id.as_ref()).is_some_and(|f| self.forbidden.contains(f));
        if self.forbidden_hit {
            t = None;
        }
        if self.drop != t {
            self.drop = t;
            cx.notify();
        }
    }

    fn drag_over(&mut self, e: &DragMoveEvent<TabDrag>, cx: &mut Context<Self>) {
        let pos = e.event.position;
        if !self.dragging || !self.root.get().contains(&pos) {
            if self.dragging {
                self.put_drop(None, cx);
            }
            return;
        }
        let drag = e.drag(cx).clone();
        let snap = ui_state(cx).read(cx).snapshot.clone();
        let space_id = active_space(&snap, self.opt.read(cx).pending().space_id.as_deref()).map(|s| s.id.clone()).unwrap_or_default();
        let best = {
            let regions = self.regions.borrow();
            regions
                .iter()
                .filter(|r| r.bounds().contains(&pos))
                .max_by(|a, b| {
                    let area = |x: Bounds<Pixels>| f32::from(x.size.width) * f32::from(x.size.height);
                    rank(&a.hit).cmp(&rank(&b.hit)).then(area(b.bounds()).partial_cmp(&area(a.bounds())).unwrap_or(std::cmp::Ordering::Equal))
                })
                .cloned()
        };
        let Some(region) = best else {
            self.put_drop(None, cx);
            return;
        };
        let folder_len = |id: &str| snap.spaces.iter().find_map(|s| find_folder(&s.pinned, id)).map(|f| f.items.len());
        let ctx = DragCtx { src: &drag.src, space_id: &space_id, current: self.drop.as_ref(), forbidden: &self.forbidden, folder_len: &folder_len };
        let target = target_for(&region, pos, &ctx, None);
        self.put_drop(target, cx);
    }

    fn dropped(&mut self, drag: &TabDrag, cx: &mut Context<Self>) {
        if self.over_space {
            return;
        }
        let target = self.drop.clone();
        if let (Some(d), false) = (target, self.forbidden_hit) {
            api(cx).move_item(drag.src.clone(), d.target);
        }
        self.end_drag(cx);
        cx.notify();
    }
}

fn active_space<'a>(snap: &'a Snapshot, optimistic_id: Option<&str>) -> Option<&'a signet_core::types::Space> {
    optimistic_id.and_then(|id| snap.spaces.iter().find(|s| s.id == id)).or_else(|| snap.spaces.iter().find(|s| s.id == snap.active_space_id)).or_else(|| snap.spaces.first())
}

fn vmarker(t: &SignetTheme, ind: Indicator) -> AnyElement {
    div().absolute().top(px(3.0)).bottom(px(3.0)).w(px(2.0)).rounded(px(1.0)).bg(t.accent).when(ind == Indicator::Before, |d| d.left(px(-2.0))).when(ind == Indicator::After, |d| d.right(px(-2.0))).into_any_element()
}

fn drag_wrap(el: gpui::Stateful<gpui::Div>, drag: TabDrag, env: &RowEnv) -> AnyElement {
    let (cbs, src) = (env.callbacks.clone(), drag.src.clone());
    // A chip never starts a window move or a title double-click: the press stays here.
    no_drag(el)
        .on_drag(drag, move |d, _o, w, cx| {
            (cbs.drag_start)(&src, w, cx);
            DragGhost::view(d.label.clone(), cx)
        })
        .into_any_element()
}

/// Per-render context of the chips.
struct Ctx<'a> {
    snap: &'a Snapshot,
    env: &'a RowEnv,
    t: &'a SignetTheme,
    drop: Option<&'a Drop>,
    regions: &'a RefCell<Vec<Region>>,
}

impl Ctx<'_> {
    fn reg(&self, hit: Hit, key: &str) -> Cell2 {
        let cell = self.env.bounds.cell(key);
        self.regions.borrow_mut().push(Region { hit, cell: cell.clone() });
        cell
    }
    fn active(&self, id: &str) -> bool {
        self.env.active_id.as_deref() == Some(id)
    }
}

fn indicator(d: Option<&Drop>, zone: Zone, i: usize, len: usize) -> Option<Indicator> {
    let d = d?;
    if d.target.zone != zone || d.target.folder_id.is_some() || d.into.is_some() {
        return None;
    }
    if d.target.index == i {
        Some(Indicator::Before)
    } else if i + 1 == len && d.target.index == len {
        Some(Indicator::After)
    } else {
        None
    }
}

/// Square pill for a favorite or pinned item: favicon (or emoji) only, title in a tooltip.
fn icon_chip(c: &Ctx, tab: &TabInfo, zone: Zone, i: usize, len: usize, cx: &mut App) -> AnyElement {
    if c.env.src_id.as_deref() == Some(tab.id.as_str()) {
        return div().into_any_element();
    }
    let t = c.t;
    c.reg(Hit::Item { zone, index: i, folder: None, run: None, card: None, horizontal: true }, &tab.id);
    let active = c.active(&tab.id);
    let id = tab.id.clone();
    let activate: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |_, cx| activate_tab(&id, cx));
    let title = SharedString::from(display_title(tab, &tr("sidebar.newTab")));
    let hover_bg = t.surface_2;
    let mut el = div()
        .id(SharedString::from(format!("chip-{}", tab.id)))
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.0))
        .rounded(radius::ROW)
        .text_color(t.text_tab)
        .when(active, |d| d.bg(t.active).shadow(t.active_shadow.clone()).text_color(t.text))
        .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
        .tooltip(move |w, cx| Tooltip::new(title.clone()).build(w, cx))
        .child(div().when(!tab.loaded, |d| d.opacity(0.7)).child(tab_icon(tab, 16.0)))
        .child(probe(c.env.bounds.cell(&tab.id)));
    if tab.audible || tab.muted {
        el = el.child(
            div()
                .absolute()
                .right(px(1.0))
                .bottom(px(1.0))
                .flex()
                .items_center()
                .justify_center()
                .size(px(12.0))
                .rounded_full()
                .bg(t.surface)
                .border_1()
                .border_color(t.border)
                .text_color(t.text_2)
                .child(Icon::from(if tab.muted { IconName::VolumeX } else { IconName::Volume2 }).size(px(9.0))),
        );
    }
    if let Some(ind) = indicator(c.drop, zone, i, len) {
        el = el.child(vmarker(t, ind));
    }
    let el = with_gestures(el, tab, activate, c.env, &tab.id, cx);
    let drag = TabDrag { src: DragSource::Tab { tab_id: tab.id.clone() }, favorite: zone == Zone::Favorites, label: display_title(tab, &tr("sidebar.newTab")).into() };
    drag_wrap(el, drag, c.env)
}

/// Today tab: favicon + title, close button on hover.
fn day_chip(c: &Ctx, tab: &TabInfo, i: usize, mark: Option<Indicator>, run: Option<&Run>, card: Option<Cell2>, cx: &mut App) -> AnyElement {
    if c.env.src_id.as_deref() == Some(tab.id.as_str()) {
        return div().into_any_element();
    }
    let t = c.t;
    c.reg(Hit::Item { zone: Zone::Today, index: i, folder: None, run: run.cloned(), card, horizontal: true }, &tab.id);
    let active = c.active(&tab.id);
    let id = tab.id.clone();
    let activate: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |_, cx| activate_tab(&id, cx));
    let group = format!("chip-{}", tab.id);
    let in_group = run.is_some_and(|r| r.group.is_some());
    let hover_bg = t.surface_2;
    let title = SharedString::from(display_title(tab, &tr("sidebar.newTab")));
    let mut el = div()
        .id(SharedString::from(group.clone()))
        .group(SharedString::from(group.clone()))
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap(space::S2)
        .h(px(if in_group { 28.0 } else { 30.0 }))
        .flex_basis(px(220.0))
        .flex_shrink(1.0)
        .min_w(px(120.0))
        .max_w(px(220.0))
        .pl(px(10.0))
        .pr(space::S1)
        .rounded(radius::ROW)
        .text_color(t.text_tab)
        .when(active, |d| d.bg(t.active).shadow(t.active_shadow.clone()).text_color(t.text))
        .when(!active, |d| d.hover(move |s| s.bg(hover_bg).text_color(t.text)))
        .tooltip(move |w, cx| Tooltip::new(title.clone()).build(w, cx))
        .children(tab_line(tab, Zone::Today, c.env, &group, t, "chip"))
        .child(probe(c.env.bounds.cell(&tab.id)));
    if let Some(ind) = mark {
        el = el.child(vmarker(t, ind));
    }
    let el = no_drag(with_gestures(el, tab, activate, c.env, &tab.id, cx));
    if c.env.editing.as_ref().is_some_and(|(e, _)| e == &tab.id) {
        return el.into_any_element();
    }
    let drag = TabDrag { src: DragSource::Tab { tab_id: tab.id.clone() }, favorite: false, label: display_title(tab, &tr("sidebar.newTab")).into() };
    drag_wrap(el, drag, c.env)
}

/// Split view: a pill that groups its tabs' favicons and the layout icon.
fn split_chip(c: &Ctx, item: &SplitItem, zone: Zone, i: usize, mark: Option<Indicator>, run: Option<&Run>, card: Option<Cell2>, cx: &mut App) -> AnyElement {
    if c.env.src_id.as_deref() == Some(item.id.as_str()) {
        return div().into_any_element();
    }
    let t = c.t;
    c.reg(Hit::Item { zone, index: i, folder: None, run: run.cloned(), card, horizontal: true }, &item.id);
    let layout = match item.layout {
        Some(SplitLayout::Rows) => IconName::Rows2,
        Some(SplitLayout::Grid) => IconName::Grid2x2,
        _ => IconName::Columns2,
    };
    let mut el = div()
        .id(SharedString::from(format!("tsplit-{}", item.id)))
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .h(px(if run.is_some_and(|r| r.group.is_some()) { 28.0 } else { 30.0 }))
        .px(space::S1)
        .rounded(radius::ROW)
        .bg(t.surface_2)
        .border_1()
        .border_color(t.border)
        .child(probe(c.env.bounds.cell(&item.id)));
    let members: Vec<&TabInfo> = item.tab_ids.iter().filter_map(|id| c.snap.tabs.get(id)).collect();
    let first_label = members.first().map(|m| display_title(m, &tr("sidebar.newTab"))).unwrap_or_default();
    for tab in members {
        let active = c.active(&tab.id);
        let id = tab.id.clone();
        let activate: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |_, cx| api(cx).focus_pane(&id));
        let hover_bg = t.surface_2;
        let title = SharedString::from(display_title(tab, &tr("sidebar.newTab")));
        let seg = div()
            .id(SharedString::from(format!("tseg-{}", tab.id)))
            .flex()
            .items_center()
            .justify_center()
            .w(px(26.0))
            .h(px(24.0))
            .rounded(px(6.0))
            .when(active, |d| d.bg(t.active).shadow(t.active_shadow.clone()))
            .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
            .tooltip(move |w, cx| Tooltip::new(title.clone()).build(w, cx))
            .child(div().when(!tab.loaded, |d| d.opacity(0.7)).child(tab_icon(tab, 16.0)))
            .child(probe(c.env.bounds.cell(&tab.id)));
        el = el.child(with_gestures(seg, tab, activate, c.env, &tab.id, cx));
    }
    el = el.child(div().flex().items_center().justify_center().w(px(16.0)).opacity(0.5).text_color(t.text_2).child(Icon::from(layout).size(px(12.0))));
    if let Some(ind) = mark {
        el = el.child(vmarker(t, ind));
    }
    drag_wrap(el, TabDrag { src: DragSource::Split { id: item.id.clone() }, favorite: false, label: first_label.into() }, c.env)
}

/// Pinned folder: tinted icon; a click opens the list of its tabs above the pages.
fn folder_chip(c: &Ctx, f: &FolderItem, i: usize, len: usize) -> AnyElement {
    if c.env.src_id.as_deref() == Some(f.id.as_str()) {
        return div().into_any_element();
    }
    let t = c.t;
    c.reg(Hit::FolderHead { id: f.id.clone(), index: i, parent: None, horizontal: true }, &f.id);
    let inside = c.env.active_id.as_deref().is_some_and(|a| f.items.iter().any(|it| item_tab_ids(it).iter().any(|x| x == a)));
    let tn = f.hue.map(|h| tint(h as f32, t.is_dark()));
    let into = c.drop.is_some_and(|d| d.into.as_deref() == Some(f.id.as_str()));
    let fg = tn.map(|x| x.fg).unwrap_or(t.text_2);
    let (id, id2) = (f.id.clone(), f.id.clone());
    let bounds = c.env.bounds.clone();
    let hover_bg = t.surface_2;
    let title = SharedString::from(f.name.clone());
    let mut el = div()
        .id(SharedString::from(format!("fchip-{}", f.id)))
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.0))
        .rounded(radius::ROW)
        .text_color(fg)
        .when_some(tn, |d, x| d.bg(x.bg))
        .when(inside, |d| d.bg(t.active).shadow(t.active_shadow.clone()))
        .when(into, |d| d.bg(t.active).border_2().border_color(t.accent))
        .when(tn.is_none() && !inside && !into, |d| d.hover(move |s| s.bg(hover_bg)))
        .tooltip(move |w, cx| Tooltip::new(title.clone()).build(w, cx))
        .on_click(move |_, _, cx| {
            let r = bounds.get(&id).map(rect_of).unwrap_or_default();
            api(cx).open_popover(PopoverRequest::Folder { folder_id: id.clone() }, r);
        })
        .on_mouse_down(MouseButton::Right, move |_, _, cx| api(cx).show_folder_menu(&id2))
        .child(folder_icon(f, 16.0, fg, t.danger))
        .child(probe(c.env.bounds.cell(&f.id)));
    if let Some(ind) = indicator(c.drop, Zone::Pinned, i, len) {
        el = el.child(vmarker(t, ind));
    }
    drag_wrap(el, TabDrag { src: DragSource::Folder { id: f.id.clone() }, favorite: false, label: f.name.clone().into() }, c.env)
}

/// Group label: its title in color (click: collapse / expand, right click: group menu).
fn group_label(c: &Ctx, run: &Run, header: &TabGroup, name: String, before: bool, card: Cell2) -> AnyElement {
    let t = c.t;
    let id = run.group.clone().unwrap_or_default();
    c.reg(Hit::GroupHead { run: run.clone(), card, horizontal: true }, &format!("hdr:{id}"));
    let tn = tint(header.hue as f32, t.is_dark());
    let into = c.drop.is_some_and(|d| d.into.as_deref() == Some(id.as_str()));
    let (id1, id2) = (id.clone(), id.clone());
    let hover = tn.bg_hover;
    let title = SharedString::from(name.clone());
    let mut el = div()
        .id(SharedString::from(format!("tglabel-{id}")))
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(space::S1)
        .max_w(px(140.0))
        .h(px(28.0))
        .px(space::S2)
        .rounded(radius::ROW)
        .text_color(tn.fg)
        .font_weight(weight::STRONG)
        .when(into, |d| d.border_2().border_color(t.accent))
        .hover(move |s| s.bg(hover))
        .tooltip(move |w, cx| Tooltip::new(title.clone()).build(w, cx))
        .on_click(move |_, _, cx| api(cx).toggle_group(&id1, None))
        .on_mouse_down(MouseButton::Right, move |_, _, cx| api(cx).show_group_menu(&id2))
        .children(header.icon.as_deref().filter(|e| !e.is_empty()).map(|e| emoji_icon(e, 14.0)))
        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(SharedString::from(name.clone())))
        .child(probe(c.env.bounds.cell(&format!("hdr:{id}"))));
    if before {
        el = el.child(div().absolute().top(px(3.0)).bottom(px(3.0)).left(px(-4.0)).w(px(2.0)).rounded(px(1.0)).bg(t.accent));
    }
    let src = DragSource::Group { id: run.group.clone().unwrap_or_default() };
    drag_wrap(el, TabDrag { src, favorite: false, label: name.into() }, c.env)
}

fn group_box(t: &SignetTheme, hue: f32) -> gpui::Div {
    div().relative().flex().flex_none().items_center().gap(px(2.0)).p(px(2.0)).rounded(radius::ROW + px(2.0)).bg(tint(hue, t.is_dark()).bg)
}

impl Render for TabStrip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let snap = ui_state(cx).read(cx).snapshot.clone();
        let pending = self.opt.read(cx).pending().clone();
        let Some(space_now) = active_space(&snap, pending.space_id.as_deref()).cloned() else { return div().into_any_element() };
        let active_id = pending.tab_id.clone().or(space_now.active_tab_id.clone());
        let reduced = reduced_motion(cx);

        if self.dragging && !cx.has_active_drag() {
            self.end_drag(cx);
        }

        // Pills that arrived since the last render open in width.
        let tab_ids: HashSet<String> = space_now.pinned.iter().chain(space_now.today.iter()).flat_map(item_tab_ids).collect();
        match &self.seen_tabs {
            Some((sid, seen)) if *sid == space_now.id => {
                for id in tab_ids.difference(seen) {
                    self.fresh.insert(id.clone(), Instant::now());
                }
            }
            _ => self.fresh.clear(),
        }
        self.fresh.retain(|_, at| at.elapsed() < Duration::from_millis(400));
        self.seen_tabs = Some((space_now.id.clone(), tab_ids));
        let fresh: Rc<HashSet<String>> = Rc::new(self.fresh.keys().cloned().collect());

        // The active tab stays visible.
        if self.last_reveal != active_id {
            self.last_reveal = active_id.clone();
            self.reveal = active_id.clone().map(|id| (id, 0));
        }
        if let Some((id, tries)) = self.reveal.clone() {
            let vp = self.days.bounds();
            match self.bounds.get(&id) {
                Some(b) if vp.size.width > px(0.0) => {
                    let mut off = self.days.offset();
                    if b.origin.x < vp.origin.x {
                        off.x += vp.origin.x - b.origin.x;
                        self.days.set_offset(off);
                    } else if b.origin.x + b.size.width > vp.origin.x + vp.size.width {
                        off.x -= b.origin.x + b.size.width - (vp.origin.x + vp.size.width);
                        self.days.set_offset(off);
                    }
                    self.reveal = None;
                }
                _ if tries < 6 => {
                    self.reveal = Some((id, tries + 1));
                    window.request_animation_frame();
                }
                _ => self.reveal = None,
            }
        }

        let this: WeakEntity<TabStrip> = cx.weak_entity();
        let callbacks = {
            let (t1, t2) = (this.clone(), this.clone());
            Rc::new(RowCallbacks {
                drag_start: Box::new(move |src, _w, cx| {
                    let src = src.clone();
                    t1.update(cx, |s, cx| s.start_drag(src, cx)).ok();
                }),
                edit: Box::new(move |id, w, cx| {
                    let id = id.to_string();
                    t2.update(cx, |s, cx| s.begin_edit(&id, w, cx)).ok();
                }),
                close: Box::new(move |tab, _w, cx| api(cx).close_tab(&tab.id)),
            })
        };
        self.nav.begin_pass();
        let env = RowEnv {
            active_id: active_id.clone(),
            src_id: self.src_id.clone(),
            fresh: fresh.clone(),
            editing: self.editing.clone(),
            nav: self.nav.clone(),
            bounds: self.bounds.clone(),
            callbacks,
            preview: self.preview.clone(),
            preview_below: true,
            motion: None,
        };
        self.regions.borrow_mut().clear();
        let shown_drop = if self.dragging { self.drop.clone() } else { None };
        let c = Ctx { snap: &snap, env: &env, t: &t, drop: shown_drop.as_ref(), regions: &self.regions };

        // Lists
        let favorites: Vec<&TabInfo> = snap.favorites.iter().filter_map(|id| snap.tabs.get(id)).collect();
        let show_favorites = !favorites.is_empty() || self.dragging;
        let show_pinned = !space_now.pinned.is_empty() || self.dragging;

        let list_group = |key: &str, zone: Zone, len: usize, children: Vec<Cell2>| {
            let cell = c.env.bounds.cell(key);
            c.regions.borrow_mut().push(Region { hit: Hit::List { zone, len, children, axis: Axis::Horizontal }, cell: cell.clone() });
            cell
        };

        let fav_node = show_favorites.then(|| {
            let kids: Vec<AnyElement> = favorites.iter().enumerate().map(|(i, tab)| icon_chip(&c, tab, Zone::Favorites, i, favorites.len(), cx)).collect();
            let cells = favorites.iter().map(|tb| c.env.bounds.cell(&tb.id)).collect();
            let cell = list_group("list:strip-fav", Zone::Favorites, favorites.len(), cells);
            div()
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .gap(space::HALF)
                .min_h(px(32.0))
                .when(favorites.is_empty(), |d| d.px(space::S2).border_2().border_dashed().border_color(t.border).rounded(radius::ROW))
                .child(probe(cell))
                .children(kids)
                .when(favorites.is_empty(), |d| d.child(div().text_size(t.type_scale.cap).text_color(t.text_2).whitespace_nowrap().child(tr("sidebar.dropFavorites"))))
        });

        let pinned_node = show_pinned.then(|| {
            let len = space_now.pinned.len();
            let kids: Vec<AnyElement> = space_now
                .pinned
                .iter()
                .enumerate()
                .filter_map(|(i, item)| match item {
                    Item::Folder(f) => Some(folder_chip(&c, f, i, len)),
                    Item::Split(s) => Some(split_chip(&c, s, Zone::Pinned, i, indicator(c.drop, Zone::Pinned, i, len), None, None, cx)),
                    Item::Tab(ti) => snap.tabs.get(&ti.tab_id).map(|tab| icon_chip(&c, tab, Zone::Pinned, i, len, cx)),
                })
                .collect();
            let cells = space_now.pinned.iter().map(|i| c.env.bounds.cell(&item_key(i))).collect();
            let cell = list_group("list:strip-pinned", Zone::Pinned, len, cells);
            div()
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .gap(space::HALF)
                .min_h(px(32.0))
                .when(len == 0, |d| d.px(space::S2).border_2().border_dashed().border_color(t.border).rounded(radius::ROW))
                .child(probe(cell))
                .children(kids)
                .when(len == 0, |d| d.child(div().text_size(t.type_scale.cap).text_color(t.text_2).whitespace_nowrap().child(tr("sidebar.dropPin"))))
        });

        // Today tabs: groups are tinted boxes with their label.
        let items = &space_now.today;
        let mark = today_mark(items, c.drop);
        let mut day_kids: Vec<AnyElement> = Vec::new();
        let mut day_cells: Vec<Cell2> = Vec::new();
        let default_group = TabGroup { name: String::new(), icon: None, hue: 215.0, open: true };
        for run in runs_of(items) {
            let chip = |i: usize, run: Option<&Run>, before_card: bool, card: Option<Cell2>, cx: &mut App| -> AnyElement {
                let m = match &mark {
                    Some(m) if m.index == i as i64 && !before_card && !m.card => Some(m.side),
                    _ => None,
                };
                match &items[i] {
                    Item::Split(s) => split_chip(&c, s, Zone::Today, i, m, run, card, cx),
                    Item::Tab(ti) => snap.tabs.get(&ti.tab_id).map(|tab| day_chip(&c, tab, i, m, run, card, cx)).unwrap_or_else(|| div().into_any_element()),
                    Item::Folder(_) => div().into_any_element(),
                }
            };
            let Some(group) = run.group.clone() else {
                day_cells.push(c.env.bounds.cell(&item_key(&items[run.start])));
                day_kids.push(chip(run.start, None, false, None, cx));
                continue;
            };
            let header = space_now.groups.as_ref().and_then(|g| g.get(&group)).unwrap_or(&default_group);
            let card_cell = c.env.bounds.cell(&format!("card:{group}"));
            day_cells.push(card_cell.clone());
            c.regions.borrow_mut().push(Region { hit: Hit::GroupCard { run: run.clone(), horizontal: true }, cell: card_cell.clone() });
            if c.env.src_id.as_deref() == Some(format!("g:{group}").as_str()) {
                continue;
            }
            // Insertion right before the group: the marker goes on the label, not on the first chip.
            let before_card = mark.as_ref().is_some_and(|m| m.side == Indicator::Before && m.index == run.start as i64) && c.drop.and_then(|d| d.target.group_id.as_deref()) != Some(group.as_str());
            let first = item_tab_ids(&items[run.start]).first().and_then(|id| snap.tabs.get(id));
            let name = if header.name.is_empty() { first.map(|f| display_title(f, &tr("sidebar.newTab"))).unwrap_or_default() } else { header.name.clone() };
            let mut boxed = group_box(&t, header.hue as f32).child(probe(card_cell.clone())).child(group_label(&c, &run, header, name, before_card, card_cell.clone()));
            for i in run.start..run.end {
                // Collapsed: only the active tab's pill remains, if it is in the group.
                let keep = header.open || active_id.as_deref().is_some_and(|a| item_tab_ids(&items[i]).iter().any(|x| x == a));
                if keep {
                    boxed = boxed.child(chip(i, Some(&run), before_card, Some(card_cell.clone()), cx));
                }
            }
            day_kids.push(boxed.into_any_element());
            if mark.as_ref().is_some_and(|m| m.card && m.index == run.end as i64 - 1) {
                day_kids.push(div().relative().flex_none().w(px(0.0)).self_stretch().child(div().absolute().top(px(3.0)).bottom(px(3.0)).left(px(1.0)).w(px(2.0)).rounded(px(1.0)).bg(t.accent)).into_any_element());
            }
        }
        let days_cell = list_group("list:strip-days", Zone::Today, items.len(), day_cells);
        let days = div()
            .id("strip-days")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(space::HALF)
            .min_w_0()
                .min_h(px(32.0))
            .overflow_x_scroll()
            .track_scroll(&self.days)
            .on_scroll(cx.listener(|this, e: &gpui::ScrollEvent, _w, cx| {
                // Two-finger swipe on the row: horizontal scroll of today tabs (a vertical wheel scrolls it too).
                let (dx, dy) = match e.delta {
                    gpui::ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
                    gpui::ScrollDelta::Lines(l) => (l.x * 16.0, l.y * 16.0),
                };
                if dy.abs() > dx.abs() {
                    let mut off = this.days.offset();
                    off.x += px(dy);
                    this.days.set_offset(off);
                    cx.notify();
                }
                this.preview.update(cx, |p, cx| p.hide(cx));
            }))
            .child(probe(days_cell))
            .children(day_kids);
        self.nav.end_pass();

        // Space picker
        let over = self.over_space;
        let switcher = {
            let hover_bg = t.surface_2;
            let sp_id = space_now.id.clone();
            let bounds = self.bounds.clone();
            let cell = self.bounds.cell("space-switcher");
            div()
                .id("ts-space")
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.0))
                .h(px(30.0))
                .pl(px(10.0))
                .pr(space::S2)
                .rounded(radius::ROW)
                .font_weight(weight::STRONG)
                .text_color(t.text)
                .when(over, |d| d.bg(t.surface_2).border_2().border_color(t.accent))
                .when(!over, |d| d.hover(move |s| s.bg(hover_bg)))
                .tooltip({
                    let tip = tr("tabstrip.spaces");
                    move |w, cx| Tooltip::new(tip.clone()).build(w, cx)
                })
                .on_click(move |_, _, cx| {
                    let r = bounds.get("space-switcher").map(rect_of).unwrap_or_default();
                    api(cx).open_popover(PopoverRequest::Spaces, r)
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, move |_, _, cx| api(cx).show_space_menu(&sp_id))
                .on_drag_move::<TabDrag>(cx.listener(|this, e: &DragMoveEvent<TabDrag>, _w, cx| {
                    let inside = e.bounds.contains(&e.event.position);
                    if inside && !this.over_space {
                        this.over_space = true;
                        this.dwell += 1;
                        let dwell = this.dwell;
                        cx.notify();
                        // Lingering on the switcher opens the spaces menu (the tab is dropped there).
                        cx.spawn(async move |this, cx| {
                            cx.background_executor().timer(Duration::from_millis(DWELL_MS)).await;
                            this.update(cx, |s, cx| {
                                if s.over_space && s.dwell == dwell {
                                    let r = s.anchor_of("space-switcher");
                                    api(cx).open_popover(PopoverRequest::Spaces, r);
                                }
                            })
                            .ok();
                        })
                        .detach();
                    } else if !inside && this.over_space {
                        this.over_space = false;
                        this.dwell += 1;
                        cx.notify();
                    }
                }))
                .on_drop::<TabDrag>(cx.listener(|this, _d: &TabDrag, _w, cx| {
                    // Dropping on the switcher itself sends nowhere: the send happens on a menu row.
                    this.over_space = false;
                    this.dwell += 1;
                    cx.notify();
                }))
                .child(probe(cell))
                .child(div().text_size(t.type_scale.lg).line_height(px(16.0)).font_family(crate::theme::emoji_font()).child(SharedString::from(space_now.icon.clone())))
                .child(div().max_w(px(120.0)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(SharedString::from(space_now.name.clone())))
                .when(space_now.is_isolated(), |d| d.child(div().flex_none().text_color(t.text_3).child(Icon::from(IconName::UserRound).size(px(12.0)))))
                .child(div().flex_none().text_color(t.text_3).child(Icon::from(IconName::ChevronDown).size(px(13.0))))
        };

        let media = media_view(&snap).map(|(info, tab_id)| {
            let (a, b) = (tab_id.clone(), tab_id.clone());
            MediaChip::new("strip-media", info, media_labels()).on_activate(move |_, _, cx| api(cx).activate_tab(&a)).on_toggle(move |_, _, cx| api(cx).media_toggle(&b))
        });
        let dl_bounds = self.bounds.clone();
        let dl = (!snap.downloads.is_empty()).then(|| {
            let cell = self.bounds.cell("strip-downloads");
            div().relative().child(probe(cell)).child(IconButton::new("ts-downloads", IconName::Download).tooltip(tr("sidebar.downloads")).on_click(move |_, _, cx| {
                let r = dl_bounds.get("strip-downloads").map(rect_of).unwrap_or_default();
                api(cx).open_popover(PopoverRequest::Downloads, r)
            }))
        });
        let keys = shortcut_label("library", snap.settings.shortcuts.as_ref(), IS_MAC);
        let library_tip = match keys {
            Some(k) => tr_args("sidebar.library", &[("keys", &k)]),
            None => tr("sidebar.libraryNoKeys"),
        };
        let _ = (reduced, window);
        let pad_left = if self.floating { px(72.0) } else { px(if cfg!(target_os = "macos") { 78.0 } else { 8.0 }) };

        div()
            .id("tabstrip")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(space::S1)
            .w_full()
            .h(px(40.0))
            .pl(pad_left)
            .pr(space::S2)
            .text_size(t.type_scale.base)
            .text_color(t.text)
            .when(self.floating, |d| d.rounded(radius::POP))
            .when(!self.floating, crate::title_area::title_area)
            .child(probe(self.root.clone()))
            .on_drag_move::<TabDrag>(cx.listener(|this, e: &DragMoveEvent<TabDrag>, _w, cx| this.drag_over(e, cx)))
            .on_drop::<TabDrag>(cx.listener(|this, d: &TabDrag, _w, cx| this.dropped(d, cx)))
            .child(switcher)
            .children(fav_node)
            .children(pinned_node)
            .when(show_favorites || show_pinned, |d| d.child(div().flex_none().w(px(1.0)).h(px(16.0)).mx(space::S1).bg(t.border)))
            .child({
                // Edge fades where today's tabs continue (`--fl` / `--fr`, 24 px).
                let (off, max) = (self.days.offset().x, self.days.max_offset().x);
                let color = if self.floating { t.glass_view } else { t.bg };
                let left = if off < px(-1.0) { crate::widgets::edge_fade(crate::widgets::FadeEdge::Left, 24.0, color) } else { None };
                let right = if max + off > px(1.0) { crate::widgets::edge_fade(crate::widgets::FadeEdge::Right, 24.0, color) } else { None };
                div().relative().flex().min_w_0().child(days).children(left).children(right)
            })
            .child(IconButton::new("ts-plus", IconName::Plus).tooltip(tr("sidebar.newTab")).on_click(|_, _, cx| api(cx).open_palette(PaletteMode::NewTab, None)))
            .child(div().flex_1().self_stretch().min_w(space::S2))
            .children(media)
            .children(dl)
            .child(IconButton::new("ts-library", IconName::Library).tooltip(library_tip).on_click(|_, _, cx| api(cx).open_library(None)))
            .child(IconButton::new("ts-settings", IconName::Settings).tooltip(tr("sidebar.settings")).on_click(|_, _, cx| api(cx).open_settings(None)))
            .into_any_element()
    }
}


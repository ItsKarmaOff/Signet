//! Port of `tabs/TabRow.tsx`: tab rows, split view rows, folder and group headers, favorite tiles. Plain functions
//! that build elements from a [`RowEnv`] (the host view owns the state: drag, rename, focus).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    Animation, AnimationExt, AnyElement, App, Bounds, Context, DragMoveEvent, ElementId, Entity, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Pixels, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, canvas, div, prelude::FluentBuilder, px,
};
use gpui_component::{Icon, tooltip::Tooltip};
use signet_core::types::{DragSource, FolderItem, Item, Snapshot, SplitItem, SplitLayout, TabGroup, TabInfo, Zone};

use super::custom::{RenameField, Tint, display_title, folder_icon, group_icon, live_title, tab_icon, tint};
use super::groups::Indicator;
use super::motion::{GLIDE, PillKind, RowMotion};
use super::optimistic;
use super::preview_hover::{HoverTarget, PreviewDriver};
use crate::app_state::{api, tr};
use crate::icon::IconName;
use crate::motion::{ease_out, spring_curve};
use crate::theme::{SignetTheme, ThemeExt, radius, space, weight};

/// Drag payload of every draggable item of the sidebar and of the tab row.
#[derive(Clone)]
pub struct TabDrag {
    pub src: DragSource,
    /// A favorite can only be displayed (central zone): the drop zones use it.
    pub favorite: bool,
    pub label: SharedString,
}

/// What follows the pointer while dragging.
pub struct DragGhost {
    label: SharedString,
}

impl DragGhost {
    pub fn view(label: SharedString, cx: &mut App) -> gpui::Entity<DragGhost> {
        use gpui::AppContext;
        cx.new(|_| DragGhost { label })
    }
}

impl Render for DragGhost {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        div()
            .max_w(px(220.0))
            .h(px(28.0))
            .px(space::S3)
            .flex()
            .items_center()
            .rounded(radius::ROW)
            .bg(t.glass_view)
            .shadow(t.shadow_pop.clone())
            .text_size(t.type_scale.base)
            .text_color(t.text)
            .opacity(0.92)
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(self.label.clone())
    }
}

/// Drag-over callback of a row: the drag event, already filtered on the row's bounds.
pub type DragOverFn = Rc<dyn Fn(&DragMoveEvent<TabDrag>, &mut Window, &mut App)>;

/// Shared bounds of rows (window coordinates), filled while painting. Used by hover previews and anchored popovers.
#[derive(Clone, Default)]
pub struct BoundsMap(Rc<RefCell<HashMap<String, Rc<Cell<Bounds<Pixels>>>>>>);

impl BoundsMap {
    pub fn cell(&self, id: &str) -> Rc<Cell<Bounds<Pixels>>> {
        self.0.borrow_mut().entry(id.to_string()).or_default().clone()
    }
    pub fn get(&self, id: &str) -> Option<Bounds<Pixels>> {
        self.0.borrow().get(id).map(|c| c.get()).filter(|b| b.size.width > px(0.0))
    }
    /// Drops the cells of rows that are gone.
    pub fn retain(&self, keep: &HashSet<String>) {
        self.0.borrow_mut().retain(|k, _| keep.contains(k));
    }
}

/// An invisible child that records the parent's bounds.
pub fn probe(cell: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |b, _, _| cell.set(b), |_, _, _, _| {}).absolute().size_full()
}

/// Roving focus of the rows: arrows move between the rows in display order, only the active row is a Tab stop.
#[derive(Clone, Default)]
pub struct NavRing {
    handles: Rc<RefCell<HashMap<String, FocusHandle>>>,
    order: Rc<RefCell<Vec<String>>>,
}

impl NavRing {
    pub fn begin_pass(&self) {
        self.order.borrow_mut().clear();
    }
    /// Forgets the handles of rows that were not drawn during the pass.
    pub fn end_pass(&self) {
        let seen: HashSet<String> = self.order.borrow().iter().cloned().collect();
        self.handles.borrow_mut().retain(|k, _| seen.contains(k));
    }
    fn handle(&self, id: &str, cx: &mut App) -> FocusHandle {
        self.order.borrow_mut().push(id.to_string());
        self.handles.borrow_mut().entry(id.to_string()).or_insert_with(|| cx.focus_handle()).clone()
    }
    fn step(&self, id: &str, dir: i32, window: &mut Window, cx: &mut App) {
        let order = self.order.borrow();
        let Some(i) = order.iter().position(|x| x == id) else { return };
        let j = i as i32 + dir;
        if j < 0 {
            return;
        }
        if let Some(h) = order.get(j as usize).and_then(|n| self.handles.borrow().get(n).cloned()) {
            window.focus(&h, cx);
        }
    }
    /// Gives keyboard focus back to a row once the list is redrawn.
    pub fn focus(&self, id: &str, window: &mut Window, cx: &mut App) {
        if let Some(h) = self.handles.borrow().get(id).cloned() {
            window.focus(&h, cx);
        }
    }
}

/// Callbacks into the host view.
pub struct RowCallbacks {
    /// A drag starts on this item (the host hides it and opens its drop gap).
    pub drag_start: Box<dyn Fn(&DragSource, &mut Window, &mut App)>,
    /// Put the title of a tab, folder or group into edit mode.
    pub edit: Box<dyn Fn(&str, &mut Window, &mut App)>,
    /// Close a tab (the host may animate a ghost of the row).
    pub close: Box<dyn Fn(&TabInfo, &mut Window, &mut App)>,
}

/// What the rows of one render pass share.
#[derive(Clone)]
pub struct RowEnv {
    pub active_id: Option<String>,
    /// Id of the item held by a drag (its row is hidden).
    pub src_id: Option<String>,
    pub fresh: Rc<HashSet<String>>,
    /// Item being renamed (tab id, folder id or group id) and its field.
    pub editing: Option<(String, Entity<RenameField>)>,
    pub nav: NavRing,
    pub bounds: BoundsMap,
    pub callbacks: Rc<RowCallbacks>,
    pub preview: Entity<PreviewDriver>,
    /// Hover previews in the top row show below the chip.
    pub preview_below: bool,
    /// Animations of the sidebar rows (`None` in the tab row).
    pub motion: Option<RowMotion>,
}

impl RowEnv {
    fn is_src(&self, id: &str) -> bool {
        self.src_id.as_deref() == Some(id)
    }
    fn field_for(&self, id: &str) -> Option<Entity<RenameField>> {
        self.editing.as_ref().filter(|(e, _)| e == id).map(|(_, f)| f.clone())
    }
}

/// `drag_over` wrapper: only reacts when the pointer is inside the element.
pub(crate) fn over_listener(f: DragOverFn) -> impl Fn(&DragMoveEvent<TabDrag>, &mut Window, &mut App) + 'static {
    move |e, w, cx| {
        if e.bounds.contains(&e.event.position) {
            f(e, w, cx)
        }
    }
}

pub(crate) fn mini(id: String, icon: IconName, tip: String, t: &SignetTheme) -> gpui::Stateful<gpui::Div> {
    let hover = t.surface_2;
    let (fg, hover_fg) = (t.text_2, t.text);
    div()
        .id(SharedString::from(id))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(space::S5)
        .rounded(px(6.0))
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.bg(hover).text_color(hover_fg))
        .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(Icon::from(icon).size(px(14.0)))
}

pub(crate) fn indicator_line(t: &SignetTheme, ind: Indicator) -> impl IntoElement {
    div().absolute().left(space::S2).right(space::S2).h(px(2.0)).rounded(px(1.0)).bg(t.accent).when(ind == Indicator::Before, |d| d.top(px(-2.0))).when(ind == Indicator::After, |d| d.bottom(px(-2.0)))
}

fn norm(u: &str) -> &str {
    u.strip_suffix('/').unwrap_or(u)
}

/// Content of a tab row: favicon, sound, title, "new" dot, back to the pinned URL, close.
pub(crate) fn tab_line(tab: &TabInfo, zone: Zone, env: &RowEnv, group: &str, t: &SignetTheme, tip_prefix: &str) -> Vec<AnyElement> {
    let mut out: Vec<AnyElement> = vec![div().flex_none().when(!tab.loaded, |d| d.opacity(0.7)).child(tab_icon(tab, 16.0)).into_any_element()];
    // Sound to the left of the title: the cross that appears on the right on hover does not move it.
    if tab.audible || tab.muted {
        let id = tab.id.clone();
        let (icon, tip) = if tab.muted { (IconName::VolumeX, tr("media.unmute")) } else { (IconName::Volume2, tr("media.mute")) };
        out.push(
            mini(format!("{tip_prefix}-sound-{}", tab.id), icon, tip, t)
                .mx(px(-3.0))
                .on_click(move |_, _, cx| api(cx).toggle_mute(&id))
                .into_any_element(),
        );
    }
    // Title: double-click = rename in place.
    let editing = env.field_for(&tab.id);
    match editing {
        Some(field) => out.push(field.into_any_element()),
        None => {
            let (id, edit) = (tab.id.clone(), env.callbacks.clone());
            let shown = display_title(tab, &tr("sidebar.newTab"));
            out.push(
                div()
                    .id(SharedString::from(format!("{tip_prefix}-title-{}", tab.id)))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(t.text_tab)
                    .when(!tab.loaded, |d| d.opacity(0.7))
                    .group_hover(SharedString::from(group.to_string()), {
                        let c = t.text;
                        move |s| s.text_color(c)
                    })
                    .on_mouse_down(MouseButton::Left, move |e, w, cx| {
                        if e.click_count >= 2 {
                            cx.stop_propagation();
                            (edit.edit)(&id, w, cx);
                        }
                    })
                    .child(shown)
                    .into_any_element(),
            );
        }
    }
    // Item of a live folder not yet opened.
    if tab.live_unread == Some(true) {
        out.push(div().flex_none().size(px(6.0)).rounded_full().bg(t.accent).into_any_element());
    }
    let drifted = zone == Zone::Pinned && tab.pinned_url.as_deref().is_some_and(|p| norm(&tab.url) != norm(p));
    if drifted {
        let id = tab.id.clone();
        out.push(
            mini(format!("{tip_prefix}-reset-{}", tab.id), IconName::ChevronRight, tr("tabs.resetPinned"), t)
                .font_weight(weight::STRONG)
                .on_click(move |_, _, cx| api(cx).reset_pinned(&id))
                .into_any_element(),
        );
    }
    // Close: collapsed while the row is neither hovered nor focused.
    {
        let (tab2, cbs) = (tab.clone(), env.callbacks.clone());
        let tip = if tab.pinned_url.is_some() { tr("tabs.unload") } else { tr("common.close") };
        out.push(
            mini(format!("{tip_prefix}-close-{}", tab.id), IconName::X, tip, t)
                .w(px(0.0))
                .opacity(0.0)
                .group_hover(SharedString::from(group.to_string()), |s| s.w(space::S5).opacity(1.0))
                .on_click(move |_, w, cx| (cbs.close)(&tab2, w, cx))
                .into_any_element(),
        );
    }
    out
}

/// Gestures shared by rows: activation on press, middle click, right click, hover preview, keyboard.
pub(crate) fn with_gestures<E>(el: E, tab: &TabInfo, activate: Rc<dyn Fn(&mut Window, &mut App)>, env: &RowEnv, key: &str, cx: &mut App) -> E
where
    E: InteractiveElement + StatefulInteractiveElement,
{
    let focus = env.nav.handle(key, cx);
    let (id, cbs, nav) = (tab.id.clone(), env.callbacks.clone(), env.nav.clone());
    let (tab_c, key_c) = (tab.clone(), key.to_string());
    let (act2, act3) = (activate.clone(), activate);
    let driver = env.preview.clone();
    let (driver_hover, driver_down) = (driver.clone(), driver);
    let cell = env.bounds.cell(&tab.id);
    let (below, editing) = (env.preview_below, env.field_for(&tab.id).is_some());
    let id_hover = tab.id.clone();
    let id_menu = tab.id.clone();
    let tab_close = tab.clone();
    let cbs_close = cbs.clone();
    el.track_focus(&focus)
        .tab_stop(true)
        .on_key_down(move |e: &KeyDownEvent, w, cx| {
            let k = &e.keystroke;
            let m = &k.modifiers;
            match k.key.as_str() {
                "delete" | "backspace" if !m.alt && !m.control && !m.platform => (cbs.close)(&tab_c, w, cx),
                "up" if m.alt && m.shift => {
                    api(cx).move_tab_step(&id, -1);
                    nav.focus(&key_c, w, cx);
                }
                "down" if m.alt && m.shift => {
                    api(cx).move_tab_step(&id, 1);
                    nav.focus(&key_c, w, cx);
                }
                "down" | "right" if !m.alt && !m.control && !m.platform => nav.step(&key_c, 1, w, cx),
                "up" | "left" if !m.alt && !m.control && !m.platform => nav.step(&key_c, -1, w, cx),
                "enter" | "space" if !m.alt && !m.control && !m.platform => act2(w, cx),
                _ => return,
            }
            cx.stop_propagation();
        })
        .on_hover(move |hovered, _w, cx| {
            let target = HoverTarget { tab_id: id_hover.clone(), bounds: cell.clone(), below, editing };
            let pressed = cx.has_active_drag();
            driver_hover.update(cx, |d, cx| if *hovered { d.hover_enter(target, pressed, cx) } else { d.hover_leave(cx) });
        })
        .on_mouse_down(MouseButton::Left, move |e, w, cx| {
            driver_down.update(cx, |d, cx| d.pointer_down(cx));
            // No drag or selection is prevented: the press activates.
            if !e.modifiers.control {
                act3(w, cx);
            }
        })
        .on_mouse_down(MouseButton::Middle, move |_, w, cx| (cbs_close.close)(&tab_close, w, cx))
        .on_mouse_down(MouseButton::Right, move |_, _, cx| api(cx).show_tab_menu(&id_menu))
}

fn fresh_anim(el: AnyElement, id: String) -> AnyElement {
    div()
        .child(el)
        .with_animation(
        ElementId::from(SharedString::from(format!("fresh-{id}"))),
        Animation::new(crate::theme::duration::NORMAL).with_easing(ease_out),
        |el, d| el.opacity(d),
    )
    .into_any_element()
}

/// The active background of a row while the pill slides (`None` when this row does not host it).
fn pill_element(env: &RowEnv, tab_id: &str, t: &SignetTheme, radius_px: gpui::Pixels) -> Option<AnyElement> {
    let p = env.motion.as_ref()?.pill.as_ref().filter(|p| p.running())?;
    if p.host_id != tab_id {
        return None;
    }
    let id = SharedString::from(format!("pill-{}", p.generation));
    let base = || div().absolute().rounded(radius_px).bg(t.active).shadow(t.active_shadow.clone());
    match &p.kind {
        PillKind::Fade => Some(base().top(px(0.0)).left(px(0.0)).size_full().with_animation(id, Animation::new(crate::theme::duration::FAST).with_easing(ease_out), |d, k| d.opacity(k)).into_any_element()),
        PillKind::Slide { from, to } => {
            let host = env.bounds.get(tab_id)?;
            let rel = |b: &gpui::Bounds<Pixels>| (f32::from(b.origin.x - host.origin.x), f32::from(b.origin.y - host.origin.y), f32::from(b.size.width), f32::from(b.size.height));
            let (a, z) = (rel(from), rel(to));
            Some(
                base()
                    .with_animation(id, Animation::new(GLIDE).with_easing(spring_curve), move |d, k| {
                        let l = |x: f32, y: f32| x + (y - x) * k;
                        d.left(px(l(a.0, z.0))).top(px(l(a.1, z.1))).w(px(l(a.2, z.2))).h(px(l(a.3, z.3)))
                    })
                    .into_any_element(),
            )
        }
    }
}

/// True while the pill (not the row itself) draws the background of this row.
fn pill_owns(env: &RowEnv, tab_id: &str) -> bool {
    env.motion.as_ref().and_then(|m| m.pill.as_ref()).is_some_and(|p| p.running() && p.to_id == tab_id)
}

/// A sidebar row for a tab. `nested`: inside a folder (smaller radius, tighter padding).
pub fn tab_row(tab: &TabInfo, zone: Zone, indicator: Option<Indicator>, nested: bool, over: Option<DragOverFn>, env: &RowEnv, cx: &mut App) -> AnyElement {
    if env.is_src(&tab.id) {
        return div().into_any_element();
    }
    let t = cx.signet().clone();
    let active = env.active_id.as_deref() == Some(tab.id.as_str());
    let group = format!("row-{}", tab.id);
    let editing = env.field_for(&tab.id).is_some();
    let drag = TabDrag { src: DragSource::Tab { tab_id: tab.id.clone() }, favorite: false, label: display_title(tab, &tr("sidebar.newTab")).into() };
    let id = tab.id.clone();
    let activate: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |_, cx| optimistic::activate_tab(&id, cx));
    let hover_bg = t.surface_2;
    let mut row = div()
        .id(SharedString::from(format!("row-{}", tab.id)))
        .group(SharedString::from(group.clone()))
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(space::S2)
        .h(t.row_h)
        .pl(if nested { space::S2 } else { space::S3 })
        .pr(space::S2)
        .rounded(if nested { radius::ROW_IN } else { radius::ROW })
        .text_size(t.type_scale.base)
        .when(active && !pill_owns(env, &tab.id), |d| d.bg(t.active).shadow(t.active_shadow.clone()))
        .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
        .children(pill_element(env, &tab.id, &t, if nested { radius::ROW_IN } else { radius::ROW }))
        .children(tab_line(tab, zone, env, &group, &t, "row"))
        .child(probe(env.bounds.cell(&tab.id)));
    if let Some(ind) = indicator {
        row = row.child(indicator_line(&t, ind));
    }
    let row = with_gestures(row, tab, activate, env, &tab.id, cx);
    let cbs = env.callbacks.clone();
    let row = if editing {
        row.into_any_element()
    } else {
        let src = drag.src.clone();
        row.on_drag(drag, move |d, _off, w, cx| {
            (cbs.drag_start)(&src, w, cx);
            DragGhost::view(d.label.clone(), cx)
        })
        .into_any_element()
    };
    let row = match over {
        Some(f) => div().child(row).on_drag_move::<TabDrag>(over_listener(f)).into_any_element(),
        None => row,
    };
    if env.fresh.contains(&tab.id) { fresh_anim(row, tab.id.clone()) } else { row }
}

/// A split view row: its tabs stacked in one card, with the layout icon.
pub fn split_row(item: &SplitItem, snap: &Snapshot, indicator: Option<Indicator>, over: Option<DragOverFn>, env: &RowEnv, cx: &mut App) -> AnyElement {
    if env.is_src(&item.id) {
        return div().into_any_element();
    }
    let t = cx.signet().clone();
    let members: Vec<&TabInfo> = item.tab_ids.iter().filter_map(|id| snap.tabs.get(id)).collect();
    let layout_icon = match item.layout {
        Some(SplitLayout::Rows) => IconName::Rows2,
        Some(SplitLayout::Grid) => IconName::Grid2x2,
        _ => IconName::Columns2,
    };
    let drag = TabDrag { src: DragSource::Split { id: item.id.clone() }, favorite: false, label: members.first().map(|m| display_title(m, &tr("sidebar.newTab"))).unwrap_or_default().into() };
    let mut card = div()
        .id(SharedString::from(format!("split-{}", item.id)))
        .relative()
        .flex_none()
        .flex()
        .flex_col()
        .rounded(radius::TILE)
        .bg(t.surface_2);
    for (i, tab) in members.iter().enumerate() {
        let active = env.active_id.as_deref() == Some(tab.id.as_str());
        let group = format!("seg-{}", tab.id);
        let id = tab.id.clone();
        let activate: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |_, cx| api(cx).focus_pane(&id));
        let hover_bg = t.surface_2;
        let seg = div()
            .id(SharedString::from(format!("seg-{}", tab.id)))
            .group(SharedString::from(group.clone()))
            .relative()
            .flex()
            .items_center()
            .gap(space::S2)
            .h(t.row_h - px(2.0))
            .pl(space::S3)
            .pr(space::S2)
            .rounded(radius::ROW)
            .text_size(t.type_scale.base)
            .when(active, |d| d.bg(t.active).shadow(t.active_shadow.clone()))
            .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
            .when(i > 0 && !active, |d| d.child(div().absolute().top(px(-1.0)).left(space::S3).right(space::S3).h(px(1.0)).bg(t.border)))
            .children(tab_line(tab, Zone::Today, env, &group, &t, "seg"))
            .child(probe(env.bounds.cell(&tab.id)));
        card = card.child(with_gestures(seg, tab, activate, env, &tab.id, cx));
    }
    card = card.child(probe(env.bounds.cell(&item.id)));
    card = card.child(div().absolute().top(space::S2).right(space::S2).opacity(0.45).text_color(t.text_2).child(Icon::from(layout_icon).size(px(12.0))));
    if let Some(ind) = indicator {
        card = card.child(indicator_line(&t, ind));
    }
    let cbs = env.callbacks.clone();
    let src = drag.src.clone();
    let card = card
        .on_drag(drag, move |d, _o, w, cx| {
            (cbs.drag_start)(&src, w, cx);
            DragGhost::view(d.label.clone(), cx)
        })
        .into_any_element();
    match over {
        Some(f) => div().child(card).on_drag_move::<TabDrag>(over_listener(f)).into_any_element(),
        None => card,
    }
}

/// Header row of a folder (pinned) or of a tab group (today): icon, name, chevron. `kind` tells which API to call.
pub enum Header<'a> {
    Folder(&'a FolderItem),
    Group { id: &'a str, group: &'a TabGroup, first: Option<&'a TabInfo>, name: String },
}

/// Folder or group header. Click: collapse / expand; double-click or F2: rename; drag: the whole card; right click: menu.
pub fn header_row(header: Header, indicator: Option<Indicator>, into: bool, tint_hue: Option<f32>, over: Option<DragOverFn>, env: &RowEnv, cx: &mut App) -> AnyElement {
    let t = cx.signet().clone();
    let (id, open, name, src) = match &header {
        Header::Folder(f) => (f.id.clone(), f.open, f.name.clone(), DragSource::Folder { id: f.id.clone() }),
        Header::Group { id, group, name, .. } => (id.to_string(), group.open, name.clone(), DragSource::Group { id: id.to_string() }),
    };
    let is_group = matches!(header, Header::Group { .. });
    let tinted: Option<Tint> = tint_hue.map(|h| tint(h, t.is_dark()));
    let fg = tinted.map(|x| x.fg).unwrap_or(t.text_2);
    let title_color = tinted.map(|x| x.fg).unwrap_or(t.text);
    let editing = env.field_for(&id);
    let icon: AnyElement = match &header {
        Header::Folder(f) => folder_icon(f, 16.0, fg, t.danger),
        Header::Group { group, first, .. } => group_icon(Some(group), *first, 16.0, fg),
    };
    let live_tip = match &header {
        Header::Folder(f) => f.live.as_ref().map(live_title),
        _ => None,
    };
    let hover_bg = t.surface_2;
    let toggle = {
        let id = id.clone();
        move |cx: &mut App, open: Option<bool>| {
            if is_group { api(cx).toggle_group(&id, open) } else { api(cx).toggle_folder(&id, open) }
        }
    };
    let (click_id, cbs) = (id.clone(), env.callbacks.clone());
    let toggle2 = toggle.clone();
    let key = format!("{}-{}", if is_group { "g" } else { "f" }, id);
    let focus = env.nav.handle(&key, cx);
    let (nav, key2, id_key, cbs_key) = (env.nav.clone(), key.clone(), id.clone(), env.callbacks.clone());
    let menu_id = id.clone();
    let mut row = div()
        .id(SharedString::from(format!("hdr-{key}")))
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(space::S2)
        .h(t.row_h)
        .pl(space::S2)
        .pr(space::S2)
        .rounded(radius::ROW_IN)
        .text_size(t.type_scale.base)
        .when(into, |d| d.bg(t.active).border_2().border_color(t.accent))
        .when(!into, |d| d.hover(move |s| s.bg(hover_bg)))
        .track_focus(&focus)
        .tab_stop(false)
        .on_key_down(move |e: &KeyDownEvent, w, cx| {
            if e.keystroke.modifiers.alt || e.keystroke.modifiers.control || e.keystroke.modifiers.platform {
                return;
            }
            match e.keystroke.key.as_str() {
                "right" if !open => toggle2(cx, None),
                "left" if open => toggle2(cx, None),
                "f2" => (cbs_key.edit)(&id_key, w, cx),
                "down" => nav.step(&key2, 1, w, cx),
                "up" => nav.step(&key2, -1, w, cx),
                "right" => nav.step(&key2, 1, w, cx),
                "left" => nav.step(&key2, -1, w, cx),
                "enter" | "space" => toggle2(cx, None),
                _ => return,
            }
            cx.stop_propagation();
        })
        .on_click(move |e, w, cx| {
            if e.click_count() >= 2 {
                // The first click toggled the card: put it back as it was, then rename.
                toggle(cx, Some(!open));
                (cbs.edit)(&click_id, w, cx);
            } else {
                toggle(cx, None);
            }
        })
        .on_mouse_down(MouseButton::Right, move |_, _, cx| {
            if is_group { api(cx).show_group_menu(&menu_id) } else { api(cx).show_folder_menu(&menu_id) }
        })
        .child(div().flex_none().child(icon));
    row = match editing {
        Some(field) => row.child(field),
        None => row.child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_weight(weight::STRONG)
                .text_color(title_color)
                .when_some(live_tip, |d, _tip| d)
                .child(SharedString::from(name.clone())),
        ),
    };
    row = row.child(probe(env.bounds.cell(&format!("hdr:{id}"))));
    // One icon turned by 90 degrees when open; while the body unfolds, the turn is animated.
    let quarter = std::f32::consts::FRAC_PI_2;
    let chevron = Icon::from(IconName::ChevronRight).size(px(14.0));
    let chevron = match env.motion.as_ref().and_then(|m| m.unfold_of(&id)) {
        Some(u) => chevron
            .with_animation(
                SharedString::from(format!("chevron-{id}-{}", u.opening)),
                Animation::new(crate::theme::duration::NORMAL).with_easing(ease_out),
                move |c, k| c.rotate(gpui::radians(quarter * if u.opening { k } else { 1.0 - k })),
            )
            .into_any_element(),
        None => chevron.rotate(gpui::radians(if open { quarter } else { 0.0 })).into_any_element(),
    };
    row = row.child(div().flex_none().text_color(t.text_3).child(chevron));
    if let Some(ind) = indicator {
        row = row.child(indicator_line(&t, ind));
    }
    let drag = TabDrag { src: src.clone(), favorite: false, label: name.into() };
    let cbs2 = env.callbacks.clone();
    let row = if env.field_for(&id).is_some() {
        row.into_any_element()
    } else {
        row.on_drag(drag, move |d, _o, w, cx| {
            (cbs2.drag_start)(&src, w, cx);
            DragGhost::view(d.label.clone(), cx)
        })
        .into_any_element()
    };
    match over {
        Some(f) => div().child(row).on_drag_move::<TabDrag>(over_listener(f)).into_any_element(),
        None => row,
    }
}

/// Favorite tile (grid of 3 above the lists).
pub fn favorite_tile(tab: &TabInfo, indicator: Option<Indicator>, over: Option<DragOverFn>, env: &RowEnv, cx: &mut App) -> AnyElement {
    if env.is_src(&tab.id) {
        return div().into_any_element();
    }
    let t = cx.signet().clone();
    let active = env.active_id.as_deref() == Some(tab.id.as_str());
    let id = tab.id.clone();
    let activate: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |_, cx| optimistic::activate_tab(&id, cx));
    let drag = TabDrag { src: DragSource::Tab { tab_id: tab.id.clone() }, favorite: true, label: display_title(tab, &tr("sidebar.newTab")).into() };
    let hover_bg = t.surface_2;
    let mut tile = div()
        .id(SharedString::from(format!("tile-{}", tab.id)))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .h(space::TOOLBAR_H)
        .rounded(radius::TILE)
        .bg(t.surface_2)
        .border_1()
        .border_color(t.border)
        .when(active, |d| d.bg(t.active).shadow(t.active_shadow.clone()))
        .when(!active, |d| d.hover(move |s| s.bg(hover_bg)))
        .child(div().when(!tab.loaded, |d| d.opacity(0.6)).child(tab_icon(tab, 18.0)))
        .child(probe(env.bounds.cell(&tab.id)));
    if let Some(ind) = indicator {
        tile = tile.child(
            div()
                .absolute()
                .top(px(2.0))
                .bottom(px(2.0))
                .w(px(2.0))
                .rounded(px(1.0))
                .bg(t.accent)
                .when(ind == Indicator::Before, |d| d.left(px(-5.0)))
                .when(ind == Indicator::After, |d| d.right(px(-5.0))),
        );
    }
    let tile = with_gestures(tile, tab, activate, env, &tab.id, cx);
    let (cbs, src) = (env.callbacks.clone(), drag.src.clone());
    let tile = tile
        .on_drag(drag, move |d, _o, w, cx| {
            (cbs.drag_start)(&src, w, cx);
            DragGhost::view(d.label.clone(), cx)
        })
        .into_any_element();
    match over {
        Some(f) => div().child(tile).on_drag_move::<TabDrag>(over_listener(f)).into_any_element(),
        None => tile,
    }
}

/// Row of any non-folder item in a list.
pub fn item_row(
    item: &Item,
    snap: &Snapshot,
    zone: Zone,
    indicator: Option<Indicator>,
    nested: bool,
    over: Option<DragOverFn>,
    env: &RowEnv,
    cx: &mut App,
) -> AnyElement {
    match item {
        Item::Tab(ti) => match snap.tabs.get(&ti.tab_id) {
            Some(tab) => tab_row(tab, zone, indicator, nested, over, env, cx),
            None => div().into_any_element(),
        },
        Item::Split(s) => split_row(s, snap, indicator, over, env, cx),
        Item::Folder(_) => div().into_any_element(),
    }
}


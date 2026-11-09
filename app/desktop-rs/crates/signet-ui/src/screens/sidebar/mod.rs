//! The sidebar: favorites grid, space header, pinned zone with folders, separator, today's tabs, "new tab" row,
//! space strip at the bottom, swipe between spaces, drag and drop, resizing. Port of `sidebar/Sidebar.tsx`,
//! `SpaceStrip.tsx` and `SpacePreview.tsx`.
//!
//! # Embedding
//!
//! - Main window: `let sidebar = cx.new(|cx| Sidebar::new(window, cx))`. The view is `width` px wide
//!   (`Snapshot.sidebar_width`, or the local value while the user resizes it) and as tall as its parent
//!   (`h_full()`); the main window places it (left or right, see `Snapshot.settings.sidebar_side`) and animates the
//!   slide (`UiState.sidebar_slide`). Read the width to lay out with [`Sidebar::current_width`].
//! - Floating sidebar window (`#sidebar`): `Sidebar::new_floating(window, cx)` (no traffic light padding, hairline
//!   border and glass fill; pointer enter/leave call `set_sidebar_hover`, held while a drag, a rename, a panel or the
//!   icon form is open).
//! - Panels hosted by other modules: the sidebar emits [`SidebarEvent`]; the host builds the space form and the
//!   downloads panel and hands them back with [`Sidebar::set_space_form`] and [`Sidebar::set_downloads_panel`]
//!   (drawn over the bottom bar, like the `.ex` layers of the React version).

pub(crate) mod dnd;
mod lists;
mod space;
mod swipe;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, AnyView, App, AppContext, Bounds, Context, DragMoveEvent, Entity, EventEmitter, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, div, prelude::FluentBuilder, px,
};
use signet_core::api::UiEvent;
use signet_core::i18n::IS_MAC;
use signet_core::shortcuts::shortcut_label;
use signet_core::types::{TabInfo, DownloadStatus, DragSource, IconTarget, Item, PaletteMode, PopoverRequest, Rect, RenameTarget, SidebarSide, Snapshot, Space, SpaceProfile, Zone};

use self::dnd::{Cell2, DragCtx, Region, rank, target_for};
use self::lists::{ListCtx, favorites_list, pinned_list, today_list};
use self::space::{per_page_for, space_preview};
use self::swipe::{Swipe, SwipeCfg};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::icon::IconName;
use crate::motion::{Presence, RUBBER_C, Spring, reduced_motion, rubberband, surface_enter};
use crate::screens::tabs::custom::{IconForm, IconFormClose, RenameEvent, RenameField};
use crate::screens::tabs::groups::Drop;
use crate::screens::tabs::motion::{Flip, PillKind, PillSlide, RowMotion, Unfold};
use crate::screens::tabs::optimistic::{Optimistic, optimistic, switch_space};
use crate::screens::tabs::preview_hover::{PreviewDriver, preview_driver};
use crate::screens::tabs::row::{BoundsMap, NavRing, RowCallbacks, RowEnv, TabDrag, probe};
use crate::screens::tabs::{display_title, find_folder, folder_ids, item_tab_ids, media_labels, media_view};
use crate::theme::{ThemeExt, radius, space as sp, weight};
use crate::widgets::{IconButton, MediaCard};
use gpui_component::{Icon, tooltip::Tooltip};

// Same bounds as the app's store: 180-420 px, the rubber band covers the rest during the gesture.
pub const SIDEBAR_MIN: f32 = 180.0;
pub const SIDEBAR_MAX: f32 = 420.0;
const GAP: f32 = 6.0;

/// Requests the sidebar cannot serve itself (the panels live in other modules).
#[derive(Clone, Debug, PartialEq)]
pub enum SidebarEvent {
    /// Open the space form (`None`: new space). The host shows its form and hands it back with `set_space_form`.
    EditSpace(Option<String>),
    /// The downloads button was clicked (the host toggles the panel with `set_downloads_panel`).
    ToggleDownloads,
}

impl EventEmitter<SidebarEvent> for Sidebar {}

/// Payload of the resize handle drag.
#[derive(Clone)]
struct ResizeDrag;

struct ResizeGhost;
impl Render for ResizeGhost {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.0))
    }
}

struct ResizeState {
    /// Width at grab time, restored by Escape.
    start: f32,
    grab: f32,
    last: (f32, f64, f32),
    shown: f32,
}

/// Row of a tab that was just closed, faded out in its place (port of `closeAnimated`).
struct Ghost {
    tab: TabInfo,
    bounds: Bounds<Pixels>,
    active: bool,
    start: Instant,
}

/// Duration of the ghost fade (`TabRow.tsx`: 130 ms for a sidebar row).
const GHOST: Duration = Duration::from_millis(130);

struct Release {
    spring: Spring,
    start: f64,
    to: f32,
    sent: i32,
}

pub struct Sidebar {
    floating: bool,
    started: Instant,
    opt: Entity<Optimistic>,
    preview: Entity<PreviewDriver>,
    scroll: ScrollHandle,
    bounds: BoundsMap,
    nav: NavRing,
    regions: RefCell<Vec<Region>>,
    root: Cell2,
    strip: Cell2,
    // drag and drop
    dragging: bool,
    src: Option<DragSource>,
    src_id: Option<String>,
    forbidden: HashSet<String>,
    forbidden_hit: bool,
    drop: Option<Drop>,
    gap_h: f32,
    settling: Option<(String, Instant)>,
    dropped_on_dot: bool,
    over_space: Option<String>,
    // rename / panels
    editing: Option<(String, Entity<RenameField>)>,
    edit_sub: Option<Subscription>,
    icon_form: Option<Entity<IconForm>>,
    icon_sub: Option<Subscription>,
    space_form: Option<AnyView>,
    downloads: Option<AnyView>,
    seen_downloads: HashSet<String>,
    // width
    resize: Option<ResizeState>,
    resize_cancelled: bool,
    resize_esc: Option<Subscription>,
    release: Option<Release>,
    width: Option<f32>,
    width_until: Option<Instant>,
    // swipe
    swipe: Swipe,
    strip_swipe: Swipe,
    /// `SIGNET_SWIPE` (capture harness) already applied.
    dev_swipe_done: bool,
    strip_pick: Option<(usize, usize, usize)>,
    last_space: Option<String>,
    last_page: Option<(usize, usize)>,
    // fresh rows
    seen_tabs: Option<(String, HashSet<String>)>,
    fresh: HashMap<String, Instant>,
    // keep the active tab visible
    reveal: Option<(String, u8)>,
    last_reveal_key: (Option<String>, usize),
    inside: bool,
    /// "... Undo" announcement shown over the bottom bar (main sidebar only, see `overlay::toast`).
    toast: Option<Entity<crate::screens::overlay::ToastView>>,
    // panels kept mounted while they fade out (`useExit`)
    // motion
    flip: Flip,
    flip_sig: String,
    flip_space: Option<String>,
    unfold: HashMap<String, Unfold>,
    open_seen: Option<(String, HashMap<String, (bool, f32)>)>,
    pill: Option<PillSlide>,
    pill_last: Option<(String, Bounds<Pixels>)>,
    pill_space: Option<String>,
    pill_gen: u64,
    ghosts: Vec<Ghost>,
    ex_icon: Presence<AnyView>,
    ex_form: Presence<AnyView>,
    ex_downloads: Presence<AnyView>,
    _subs: Vec<Subscription>,
}

impl Sidebar {
    /// Sidebar of the main window.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(false, window, cx)
    }

    /// Floating sidebar (`#sidebar` window), shown while the real one is hidden.
    pub fn new_floating(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(true, window, cx)
    }

    /// Displayed width: the local value during a resize, otherwise the saved one.
    pub fn current_width(&self, cx: &App) -> f32 {
        self.width.unwrap_or(ui_state(cx).read(cx).snapshot.sidebar_width as f32).clamp(SIDEBAR_MIN - 160.0, SIDEBAR_MAX + 160.0)
    }

    /// Hands the space form (built by the host) to the sidebar, or removes it (`None`).
    pub fn set_space_form(&mut self, view: Option<AnyView>, cx: &mut Context<Self>) {
        if view.is_some() {
            self.downloads = None;
            self.icon_form = None;
        }
        self.space_form = view;
        cx.notify();
    }

    /// Hands the downloads panel (built by the host) to the sidebar, or removes it (`None`).
    pub fn set_downloads_panel(&mut self, view: Option<AnyView>, cx: &mut Context<Self>) {
        if view.is_some() {
            self.space_form = None;
            self.seen_downloads = ui_state(cx).read(cx).snapshot.downloads.iter().map(|d| d.id.clone()).collect();
        }
        self.downloads = view;
        cx.notify();
    }

    /// Opens the "choose an icon" panel for a tab, folder or group.
    pub fn pick_icon(&mut self, target: IconTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.downloads = None;
        self.space_form = None;
        let form = cx.new(|cx| IconForm::new(target, window, cx));
        self.icon_sub = Some(cx.subscribe(&form, |this, _f, _ev: &IconFormClose, cx| {
            this.icon_form = None;
            cx.notify();
        }));
        self.icon_form = Some(form);
        cx.notify();
    }

    fn build(floating: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let opt = optimistic(cx);
        let preview = preview_driver(cx);
        let ui = ui_state(cx);
        let mut subs = vec![
            cx.observe(&ui, |_, _, cx| cx.notify()),
            cx.observe(&opt, |_, _, cx| cx.notify()),
            cx.subscribe_in(&ui, window, |this, _ui, ev: &UiEvent, window, cx| this.on_event(ev, window, cx)),
        ];
        subs.shrink_to_fit();
        let seen = ui.read(cx).snapshot.downloads.iter().map(|d| d.id.clone()).collect();
        let scroll = ScrollHandle::new();
        Sidebar {
            floating,
            started: Instant::now(),
            opt,
            preview,
            scroll: scroll.clone(),
            bounds: BoundsMap::default(),
            nav: NavRing::default(),
            regions: RefCell::new(Vec::new()),
            root: Default::default(),
            strip: Default::default(),
            dragging: false,
            src: None,
            src_id: None,
            forbidden: HashSet::new(),
            forbidden_hit: false,
            drop: None,
            gap_h: 0.0,
            settling: None,
            dropped_on_dot: false,
            over_space: None,
            editing: None,
            edit_sub: None,
            icon_form: None,
            icon_sub: None,
            space_form: None,
            downloads: None,
            seen_downloads: seen,
            resize: None,
            resize_cancelled: false,
            resize_esc: None,
            release: None,
            width: None,
            width_until: None,
            swipe: Swipe::default(),
            strip_swipe: Swipe::default(),
            dev_swipe_done: false,
            strip_pick: None,
            last_space: None,
            last_page: None,
            seen_tabs: None,
            fresh: HashMap::new(),
            reveal: None,
            last_reveal_key: (None, 0),
            inside: true,
            toast: (!floating).then(|| crate::screens::overlay::ToastView::embedded(window, cx)),
            flip: Flip::new(scroll.clone()),
            flip_sig: String::new(),
            flip_space: None,
            unfold: HashMap::new(),
            open_seen: None,
            pill: None,
            pill_last: None,
            pill_space: None,
            pill_gen: 0,
            ghosts: Vec::new(),
            ex_icon: Presence::default(),
            ex_form: Presence::default(),
            ex_downloads: Presence::default(),
            _subs: subs,
        }
    }

    fn now_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }

    fn on_event(&mut self, ev: &UiEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            UiEvent::EditSpace { space_id } => {
                self.downloads = None;
                self.icon_form = None;
                cx.emit(SidebarEvent::EditSpace(space_id.clone()));
            }
            UiEvent::RenameFolder { folder_id } => self.begin_edit(folder_id, window, cx),
            UiEvent::RenameTab { tab_id } => {
                self.downloads = None;
                // A favorite (tile) has no in-place field: renamed through the popover.
                let snap = ui_state(cx).read(cx).snapshot.clone();
                if snap.favorites.contains(tab_id) || self.bounds.get(tab_id).is_none() {
                    self.editing = None;
                    let r = self.bounds.get(tab_id).map(rect_of).unwrap_or_default();
                    api(cx).open_popover(PopoverRequest::Rename { target: RenameTarget::Tab { tab_id: tab_id.clone() } }, r);
                } else {
                    self.begin_edit(tab_id, window, cx);
                }
            }
            UiEvent::PickIcon(target) => {
                self.space_form_close();
                self.pick_icon(target.clone(), window, cx);
            }
            _ => {}
        }
    }

    fn space_form_close(&mut self) {
        self.space_form = None;
        self.downloads = None;
    }

    // ---------------------------------------------------------------------------------------------------------
    // Rename in place

    /// Puts the title of a tab, folder or tab group into edit mode.
    fn begin_edit(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let snap = ui_state(cx).read(cx).snapshot.clone();
        let Some(space) = snap.spaces.iter().find(|s| s.id == snap.active_space_id).or(snap.spaces.first()) else { return };
        let (initial, placeholder) = if let Some(f) = snap.spaces.iter().find_map(|s| find_folder(&s.pinned, id)) {
            (f.name.clone(), String::new())
        } else if let Some(g) = space.groups.as_ref().and_then(|m| m.get(id)) {
            let first = space.today.iter().find(|i| i.group() == Some(id)).and_then(|i| item_tab_ids(i).into_iter().next()).and_then(|t| snap.tabs.get(&t));
            (if g.name.is_empty() { first.map(|t| display_title(t, &tr("sidebar.newTab"))).unwrap_or_default() } else { g.name.clone() }, String::new())
        } else if let Some(t) = snap.tabs.get(id) {
            (t.custom_title.clone().unwrap_or_else(|| display_title(t, &tr("sidebar.newTab"))), t.title.clone())
        } else {
            return;
        };
        let field = cx.new(|cx| RenameField::new(window, cx, &initial, &placeholder, 200));
        let id_owned = id.to_string();
        self.edit_sub = Some(cx.subscribe(&field, move |this, _f, ev: &RenameEvent, cx| {
            this.finish_edit(&id_owned, ev, cx);
        }));
        self.editing = Some((id.to_string(), field));
        self.preview.update(cx, |p, cx| p.hide(cx));
        cx.notify();
    }

    fn finish_edit(&mut self, id: &str, ev: &RenameEvent, cx: &mut Context<Self>) {
        self.editing = None;
        self.edit_sub = None;
        if let RenameEvent::Commit(value) = ev {
            let snap = ui_state(cx).read(cx).snapshot.clone();
            let next = value.trim().to_string();
            let a = api(cx);
            if let Some(f) = snap.spaces.iter().find_map(|s| find_folder(&s.pinned, id)) {
                if !next.is_empty() && next != f.name {
                    a.rename_folder(id, &next);
                }
            } else if let Some((space, g)) = snap.spaces.iter().find_map(|s| s.groups.as_ref().and_then(|m| m.get(id)).map(|g| (s, g))) {
                let first = space.today.iter().find(|i| i.group() == Some(id)).and_then(|i| item_tab_ids(i).into_iter().next()).and_then(|t| snap.tabs.get(&t));
                let shown = if g.name.is_empty() { first.map(|t| display_title(t, &tr("sidebar.newTab"))).unwrap_or_default() } else { g.name.clone() };
                if !next.is_empty() && next != shown {
                    a.rename_group(id, &next);
                }
            } else if let Some(t) = snap.tabs.get(id) {
                let custom = t.custom_title.clone().unwrap_or_default();
                if next != custom && !(next == t.title && t.custom_title.is_none()) {
                    a.rename_tab(id, if next.is_empty() { None } else { Some(&next) });
                }
            }
        }
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------------------
    // Drag and drop

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
        let key = match &src {
            DragSource::Tab { tab_id } => Some(tab_id.clone()),
            DragSource::Split { id } | DragSource::Folder { id } => Some(id.clone()),
            DragSource::Group { .. } => None,
        };
        self.gap_h = key.and_then(|k| self.bounds.get(&k)).map(|b| f32::from(b.size.height)).unwrap_or(0.0);
        self.settling = None;
        self.dragging = true;
        self.drop = None;
        self.preview.update(cx, |p, cx| p.pointer_down(cx));
        if let DragSource::Tab { tab_id } = &src {
            api(cx).begin_tab_drag(tab_id);
        }
        self.src = Some(src);
        cx.notify();
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

    /// Central hit test while a tab is dragged over the window.
    fn drag_over(&mut self, e: &DragMoveEvent<TabDrag>, cx: &mut Context<Self>) {
        let pos = e.event.position;
        if self.floating {
            self.inside = self.root.get().contains(&pos);
        }
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
                    let (ba, bb) = (a.bounds(), b.bounds());
                    let area = |x: Bounds<Pixels>| f32::from(x.size.width) * f32::from(x.size.height);
                    rank(&a.hit).cmp(&rank(&b.hit)).then(area(bb).partial_cmp(&area(ba)).unwrap_or(std::cmp::Ordering::Equal))
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

    fn end_drag_state(&mut self) {
        self.dragging = false;
        self.src = None;
        self.src_id = None;
        self.drop = None;
        self.forbidden.clear();
        self.forbidden_hit = false;
        self.over_space = None;
        self.settling = None;
    }

    fn finish_tab_drag(&mut self, cx: &mut Context<Self>) {
        if matches!(self.src, Some(DragSource::Tab { .. })) {
            api(cx).end_tab_drag();
        }
    }

    /// Drop on a list: the target computed while hovering.
    fn dropped(&mut self, drag: &TabDrag, cx: &mut Context<Self>) {
        if self.dropped_on_dot {
            self.dropped_on_dot = false;
            return;
        }
        if self.over_space.is_some() {
            return;
        }
        let target = self.drop.clone();
        self.finish_tab_drag(cx);
        self.dragging = false;
        match target {
            Some(d) if !self.forbidden_hit => {
                api(cx).move_item(drag.src.clone(), d.target);
                // The gap and the collapsed place stay until the state arrives (or a short delay), so the item lands
                // in the gap instead of reappearing at its old place.
                self.settling = Some((String::new(), Instant::now()));
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_millis(460)).await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
                .detach();
            }
            _ => self.end_drag_state(),
        }
        cx.notify();
    }

    fn drop_on_space(&mut self, drag: &TabDrag, space: &Space, cx: &mut Context<Self>) {
        self.dropped_on_dot = true;
        self.finish_tab_drag(cx);
        // A folder only goes into the pinned tabs; the rest lands in today's tabs.
        let target = match drag.src {
            DragSource::Folder { .. } => signet_core::types::DropTarget { zone: Zone::Pinned, space_id: space.id.clone(), folder_id: None, group_id: None, index: space.pinned.len() },
            _ => signet_core::types::DropTarget { zone: Zone::Today, space_id: space.id.clone(), folder_id: None, group_id: None, index: 0 },
        };
        api(cx).move_item(drag.src.clone(), target);
        self.settling = Some((String::new(), Instant::now()));
        self.over_space = None;
        self.dragging = false;
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------------------
    // Resizing

    fn resize_move(&mut self, e: &DragMoveEvent<ResizeDrag>, right: bool, cx: &mut Context<Self>) {
        if self.resize_cancelled {
            return;
        }
        if self.resize.is_none() {
            self.resize_esc = Some(cx.observe_keystrokes(|this, ev, _w, cx| {
                if ev.keystroke.key == "escape" {
                    this.resize_cancel(cx);
                }
            }));
        }
        let root = self.root.get();
        let pos_x = f32::from(e.event.position.x);
        let (origin, shown) = (f32::from(root.origin.x), f32::from(root.size.width));
        let reduce = reduced_motion(cx);
        let now = self.now_ms();
        let st = self.resize.get_or_insert_with(|| {
            // Edge pulled: the right one of the sidebar on the left, the left one of the sidebar on the right.
            let edge = if right { origin } else { origin + shown };
            ResizeState { start: shown, grab: pos_x - edge, last: (shown, now, 0.0), shown }
        });
        self.release = None;
        let raw = if right { origin + shown - (pos_x - st.grab) } else { pos_x - st.grab - origin };
        let w = if reduce {
            raw.clamp(SIDEBAR_MIN, SIDEBAR_MAX)
        } else if raw < SIDEBAR_MIN {
            SIDEBAR_MIN + rubberband(raw - SIDEBAR_MIN, 160.0, RUBBER_C)
        } else if raw > SIDEBAR_MAX {
            SIDEBAR_MAX + rubberband(raw - SIDEBAR_MAX, 160.0, RUBBER_C)
        } else {
            raw
        };
        let dt = now - st.last.1;
        if dt > 0.0 {
            st.last = (w, now, st.last.2 * 0.5 + ((w - st.last.0) / dt as f32) * 1000.0 * 0.5);
        }
        st.shown = w;
        let before = self.width;
        self.width = Some(w.round());
        if before != self.width {
            api(cx).set_sidebar_width(w.round() as f64, true);
            cx.notify();
        }
    }

    /// Escape during the gesture: the width of the grab is restored (saved with `live = false`) and the rest of the
    /// drag is ignored.
    fn resize_cancel(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.resize.take() else { return };
        self.resize_cancelled = true;
        self.resize_esc = None;
        self.release = None;
        let to = st.start.round();
        self.width = Some(to);
        self.width_until = Some(Instant::now() + Duration::from_millis(500));
        api(cx).set_sidebar_width(to as f64, false);
        cx.notify();
    }

    /// End of the gesture: the width springs back into bounds from the edge velocity, then is saved.
    fn resize_release(&mut self, cx: &mut Context<Self>) {
        let Some(st) = self.resize.take() else { return };
        let now = self.now_ms();
        let to = st.shown.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        let idle = now - st.last.1 > 100.0;
        if (st.shown - to).abs() < 0.5 || reduced_motion(cx) {
            self.width = Some(to);
            api(cx).set_sidebar_width(to as f64, false);
            self.width_until = Some(Instant::now() + Duration::from_millis(500));
        } else {
            let spring = Spring::new(st.shown, to).velocity(if idle { 0.0 } else { st.last.2 }).response(0.3);
            self.release = Some(Release { spring, start: now, to, sent: st.shown.round() as i32 });
        }
        cx.notify();
    }

    /// The closed tab's row fades out where it was (pinned tabs stay in the list: no ghost).
    fn ghost_of(&mut self, tab: &TabInfo, cx: &mut Context<Self>) {
        if reduced_motion(cx) || tab.pinned_url.is_some() {
            return;
        }
        let Some(bounds) = self.bounds.get(&tab.id) else { return };
        let active = ui_state(cx).read(cx).snapshot.spaces.iter().any(|s| s.active_tab_id.as_deref() == Some(tab.id.as_str()));
        self.ghosts.push(Ghost { tab: tab.clone(), bounds, active, start: Instant::now() });
        cx.notify();
    }

    /// True if both rows (window coordinates) are in the same list (favorites, pinned, today).
    fn same_list(&self, a: &Bounds<Pixels>, b: &Bounds<Pixels>) -> bool {
        let of = |r: &Bounds<Pixels>| ["list:favorites", "list:pinned", "list:today"].into_iter().find(|k| self.bounds.get(k).is_some_and(|l| l.contains(&r.center())));
        let (la, lb) = (of(a), of(b));
        la.is_some() && la == lb
    }

    // ---------------------------------------------------------------------------------------------------------
    // Spaces

    fn space_cfg(&self, spaces: &[Space], active: &str, width: f32, reduced: bool) -> SwipeCfg {
        SwipeCfg { width: (width - 2.0 * GAP).max(1.0), index: spaces.iter().position(|s| s.id == active).unwrap_or(0), count: spaces.len(), reduced, animate_jumps: false }
    }
}

/// Open state and estimated open height of every folder (any depth).
fn collect_opens(items: &[Item], row_h: f32, out: &mut HashMap<String, (bool, f32)>) {
    for i in items {
        if let Item::Folder(f) = i {
            out.insert(f.id.clone(), (f.open, f.items.iter().map(|x| item_tab_ids(x).len().max(1)).sum::<usize>() as f32 * (row_h + 2.0) + 2.0));
            collect_opens(&f.items, row_h, out);
        }
    }
}

/// True if the tab is drawn as a plain row (not a segment of a shared view, not a favorite tile).
fn is_plain_tab(space: &Space, id: &str) -> bool {
    [&space.pinned, &space.today].into_iter().any(|items| crate::screens::tabs::leaf_of(items, id).is_some_and(|i| matches!(i, Item::Tab(_))))
}

fn rect_of(b: Bounds<Pixels>) -> Rect {
    Rect::new(f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64, f32::from(b.size.width) as f64, f32::from(b.size.height) as f64)
}

/// Space displayed: the optimistic one, otherwise the active one.
fn active_space<'a>(snap: &'a Snapshot, optimistic_id: Option<&str>) -> Option<&'a Space> {
    optimistic_id.and_then(|id| snap.spaces.iter().find(|s| s.id == id)).or_else(|| snap.spaces.iter().find(|s| s.id == snap.active_space_id)).or_else(|| snap.spaces.first())
}

/// Signature of the list structure: what, when it changes, moves rows (order, open folders and groups, favorites).
fn data_sig(space: &Space, favorites: &[String], with_open: bool) -> String {
    fn sig(items: &[Item], out: &mut String, with_open: bool) {
        for i in items {
            match i {
                Item::Folder(f) => {
                    out.push_str(&format!("{}{}(", f.id, if !with_open { '=' } else if f.open { '+' } else { '-' }));
                    sig(&f.items, out, with_open);
                    out.push_str("),");
                }
                other => {
                    out.push_str(&crate::screens::tabs::item_key(other));
                    if let Some(g) = other.group() {
                        out.push('@');
                        out.push_str(g);
                    }
                    out.push(',');
                }
            }
        }
    }
    let mut s = String::new();
    sig(&space.pinned, &mut s, with_open);
    s.push('|');
    sig(&space.today, &mut s, with_open);
    s.push('|');
    if let Some(groups) = &space.groups {
        for (id, g) in groups {
            s.push_str(&format!("{id}{},", if !with_open { '=' } else if g.open { '+' } else { '-' }));
        }
    }
    s.push('|');
    s.push_str(&favorites.join(","));
    s
}

/// Progress of the active downloads (0..1, None when the size is unknown).
fn downloads_active(s: &Snapshot) -> bool {
    s.downloads.iter().any(|d| d.status == DownloadStatus::Progressing)
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let snap = ui_state(cx).read(cx).snapshot.clone();
        let pending = self.opt.read(cx).pending().clone();
        let Some(space) = active_space(&snap, pending.space_id.as_deref()).cloned() else { return div().into_any_element() };
        let active_id = pending.tab_id.clone().or(space.active_tab_id.clone());
        let right = snap.settings.sidebar_side == Some(SidebarSide::Right);
        let reduced = reduced_motion(cx);
        let now = self.now_ms();

        // Width: local during a gesture, until the saved width has caught up.
        if self.release.is_none() && self.resize.is_none() && self.width.is_some() {
            let caught_up = self.width.is_some_and(|w| (w - snap.sidebar_width as f32).abs() < 1.0);
            if caught_up || self.width_until.is_some_and(|u| u <= Instant::now()) {
                self.width = None;
                self.width_until = None;
            }
        }
        if let Some(r) = &mut self.release {
            let t_s = ((now - r.start) / 1000.0) as f32;
            let (x, _) = r.spring.sample(t_s);
            let done = r.spring.is_done(t_s);
            let rounded = x.round() as i32;
            if rounded != r.sent {
                r.sent = rounded;
                api(cx).set_sidebar_width(rounded as f64, true);
            }
            self.width = Some(x);
            if done {
                let to = r.to;
                self.release = None;
                self.width = Some(to);
                api(cx).set_sidebar_width(to as f64, false);
                self.width_until = Some(Instant::now() + Duration::from_millis(500));
            } else {
                window.request_animation_frame();
            }
        }
        // A resize handle drag that ended: release.
        if self.resize.is_some() && !cx.has_active_drag() {
            self.resize_esc = None;
            self.resize_release(cx);
            window.request_animation_frame();
        }
        if !cx.has_active_drag() {
            self.resize_cancelled = false;
            self.resize_esc = None;
        }
        let width = self.width.unwrap_or(snap.sidebar_width as f32);

        // Drag state: a drag that ended without a drop here is over; a settled drop frees the gap.
        let sig = data_sig(&space, &snap.favorites, true);
        if self.dragging && !cx.has_active_drag() {
            self.finish_tab_drag(cx);
            self.end_drag_state();
        }
        if let Some((s0, at)) = &mut self.settling {
            if s0.is_empty() {
                *s0 = sig.clone();
            }
            // The signature taken at drop time is the one of the first render after it.
            let changed = *s0 != sig;
            if changed || at.elapsed() > Duration::from_millis(450) {
                self.end_drag_state();
            }
        }

        // Rows that arrived since the last render (excluding space changes) unfold.
        let tab_ids: HashSet<String> = space.pinned.iter().chain(space.today.iter()).flat_map(item_tab_ids).collect();
        match &self.seen_tabs {
            Some((sid, seen)) if *sid == space.id => {
                for id in tab_ids.difference(seen) {
                    self.fresh.insert(id.clone(), Instant::now());
                }
            }
            _ => self.fresh.clear(),
        }
        self.fresh.retain(|_, at| at.elapsed() < Duration::from_millis(400));
        self.seen_tabs = Some((space.id.clone(), tab_ids));
        let fresh: Rc<HashSet<String>> = Rc::new(self.fresh.keys().cloned().collect());
        if !self.fresh.is_empty() {
            window.request_animation_frame();
        }

        // Swipe between spaces.
        let cfg = self.space_cfg(&snap.spaces, &space.id, width, reduced);
        if self.last_space.as_deref() != Some(space.id.as_str()) {
            let from = self.last_space.as_ref().and_then(|l| snap.spaces.iter().position(|s| &s.id == l));
            self.swipe.page_changed(&cfg, from, now);
            self.last_space = Some(space.id.clone());
        }
        if !self.dev_swipe_done {
            self.dev_swipe_done = true;
            if let Some(f) = std::env::var("SIGNET_SWIPE").ok().and_then(|v| v.parse::<f32>().ok()) {
                self.swipe.freeze(f.clamp(-1.0, 1.0) * cfg.width);
            }
        }
        if let Some(dir) = self.swipe.frame(&cfg, now) {
            let target = (cfg.index as i32 + dir) as usize;
            if let Some(next) = snap.spaces.get(target).map(|s| s.id.clone()) {
                cx.defer(move |cx| switch_space(&next, cx));
            }
        }
        if self.swipe.needs_frames() || self.swipe.swiping() {
            window.request_animation_frame();
        }

        // Keep the active tab visible (a new tab added at the bottom of a long list).
        let reveal_key = (active_id.clone(), space.today.len());
        if reveal_key != self.last_reveal_key {
            self.last_reveal_key = reveal_key;
            self.reveal = active_id.clone().map(|id| (id, 0));
        }
        if let Some((id, tries)) = self.reveal.clone() {
            let vp = self.scroll.bounds();
            match self.bounds.get(&id) {
                Some(b) if vp.size.height > px(0.0) => {
                    let (top, bottom) = (b.origin.y, b.origin.y + b.size.height);
                    let mut off = self.scroll.offset();
                    if top < vp.origin.y {
                        off.y += vp.origin.y - top;
                        self.scroll.set_offset(off);
                    } else if bottom > vp.origin.y + vp.size.height {
                        off.y -= bottom - (vp.origin.y + vp.size.height);
                        self.scroll.set_offset(off);
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

        // Motion: list glide (FLIP), folder unfolding, sliding active pill.
        self.flip.begin_frame(reduced);
        if self.flip_space.as_deref() != Some(space.id.as_str()) {
            self.flip.reset();
            self.flip_space = Some(space.id.clone());
        }
        let drop_sig = match (&self.drop, self.dragging || self.settling.is_some()) {
            (Some(d), true) if !reduced && d.into.is_none() => format!("{:?}/{:?}/{:?}/{}", d.target.zone, d.target.folder_id, d.target.group_id, d.target.index),
            _ => String::new(),
        };
        let flip_sig = format!("{}#{}#{drop_sig}", data_sig(&space, &snap.favorites, false), self.src_id.clone().unwrap_or_default());
        if flip_sig != self.flip_sig {
            self.flip_sig = flip_sig;
            self.flip.bump();
        }
        let row_h = f32::from(t.row_h);
        let mut opens: HashMap<String, (bool, f32)> = HashMap::new();
        collect_opens(&space.pinned, row_h, &mut opens);
        for (id, g) in space.groups.iter().flatten() {
            let n = space.today.iter().filter(|i| i.group() == Some(id.as_str())).count();
            opens.insert(id.clone(), (g.open, n as f32 * (row_h + 2.0)));
        }
        match &self.open_seen {
            Some((sid, prev)) if *sid == space.id && !reduced => {
                for (id, (now, est)) in &opens {
                    if prev.get(id).is_some_and(|(was, _)| was != now) {
                        let measured = self.bounds.get(&format!("body:{id}")).map(|b| f32::from(b.size.height)).filter(|h| *h > 1.0);
                        self.unfold.insert(id.clone(), Unfold { opening: *now, start: Instant::now(), height: measured.unwrap_or(*est) });
                    }
                }
            }
            _ => self.unfold.clear(),
        }
        self.open_seen = Some((space.id.clone(), opens));
        self.unfold.retain(|_, u| u.running());
        if !self.unfold.is_empty() {
            window.request_animation_frame();
        }

        let active_bounds = active_id.as_deref().and_then(|id| self.bounds.get(id));
        if let (Some((last_id, from)), Some(id)) = (self.pill_last.clone(), active_id.clone())
            && last_id != id
            && !reduced
            && self.pill_space.as_deref() == Some(space.id.as_str())
            && is_plain_tab(&space, &id)
            && is_plain_tab(&space, &last_id)
        {
            self.pill_gen += 1;
            let kind = match active_bounds {
                Some(to) if self.same_list(&from, &to) => PillKind::Slide { from, to },
                _ => PillKind::Fade,
            };
            // The upper row of the two hosts the pill, so that the text of the rows it crosses stays above it.
            let up = matches!(kind, PillKind::Slide { from, to } if from.origin.y < to.origin.y) && snap.tabs.contains_key(&last_id);
            self.pill = Some(PillSlide { to_id: id.clone(), host_id: if up { last_id } else { id }, kind, generation: self.pill_gen, start: Instant::now() });
        }
        self.pill_last = match (active_id.clone(), active_bounds) {
            (Some(id), Some(b)) => Some((id, b)),
            _ => None,
        };
        self.pill_space = Some(space.id.clone());
        if self.pill.as_ref().is_some_and(|p| !p.running()) {
            self.pill = None;
        }
        if self.pill.is_some() {
            window.request_animation_frame();
        }
        self.ghosts.retain(|g| g.start.elapsed() < GHOST);
        if !self.ghosts.is_empty() {
            window.request_animation_frame();
        }
        let motion = (!reduced).then(|| RowMotion { flip: self.flip.clone(), pill: self.pill.clone().map(Rc::new), unfold: Rc::new(self.unfold.clone()) });

        // Shared environment of the rows.
        let this: WeakEntity<Sidebar> = cx.weak_entity();
        let callbacks = {
            let (t1, t2, t3) = (this.clone(), this.clone(), this.clone());
            Rc::new(RowCallbacks {
                drag_start: Box::new(move |src, _w, cx| {
                    let src = src.clone();
                    t1.update(cx, |s, cx| s.start_drag(src, cx)).ok();
                }),
                edit: Box::new(move |id, w, cx| {
                    let id = id.to_string();
                    t2.update(cx, |s, cx| s.begin_edit(&id, w, cx)).ok();
                }),
                close: Box::new(move |tab, _w, cx| {
                    let tab = tab.clone();
                    t3.update(cx, |s, cx| s.ghost_of(&tab, cx)).ok();
                    api(cx).close_tab(&tab.id)
                }),
            })
        };
        self.nav.begin_pass();
        let env = RowEnv {
            active_id: active_id.clone(),
            src_id: self.src_id.clone(),
            fresh,
            editing: self.editing.clone(),
            nav: self.nav.clone(),
            bounds: self.bounds.clone(),
            callbacks,
            preview: self.preview.clone(),
            preview_below: false,
            motion,
        };
        self.regions.borrow_mut().clear();
        let gapped = !reduced;
        let shown_drop = if self.dragging || self.settling.is_some() { self.drop.clone() } else { None };
        let ctx = ListCtx { snap: &snap, space: &space, env: &env, drop: shown_drop.as_ref(), gapped, gap_h: self.gap_h, regions: &self.regions, t: &t };

        let show_favorites = !snap.favorites.is_empty() || self.dragging;
        let favorites = show_favorites.then(|| favorites_list(&ListCtx { drop: shown_drop.as_ref().filter(|d| d.target.zone == Zone::Favorites), ..ctx_clone(&ctx) }, cx, &snap.favorites, true));
        let pinned = pinned_list(&ListCtx { drop: shown_drop.as_ref().filter(|d| d.target.zone == Zone::Pinned), ..ctx_clone(&ctx) }, cx, &space.pinned, self.dragging);
        let today = today_list(&ListCtx { drop: shown_drop.as_ref().filter(|d| d.target.zone == Zone::Today), ..ctx_clone(&ctx) }, cx, &space.today);
        self.nav.end_pass();
        let live: HashSet<String> = snap.tabs.keys().cloned().collect();
        self.bounds.retain(&live_keys(&live, &snap));

        let page_w = (width - 2.0 * GAP).max(1.0);
        let swiping = self.swipe.swiping();
        let x = self.swipe.offset();
        let idx = cfg.index;
        let prev = idx.checked_sub(1).and_then(|i| snap.spaces.get(i));
        let next = snap.spaces.get(idx + 1);
        let preview_for = |s: Option<&Space>, left: f32| -> Option<AnyElement> {
            let s = s?;
            Some(div().absolute().top(px(0.0)).bottom(px(0.0)).left(px(left)).w(px(page_w)).overflow_hidden().child(space_preview(s, &snap, cx)).into_any_element())
        };
        let preview_prev = if swiping { preview_for(prev, -page_w + x) } else { None };
        let preview_next = if swiping { preview_for(next, page_w + x) } else { None };

        let cur_page = div()
            .id("space-page")
            .relative()
            .flex()
            .flex_col()
            .gap(sp::HALF)
            .min_w_0()
            .left(px(x))
            .child(pinned)
            .child(self.divider(&space, &t, cx))
            .child(self.new_tab_row(&t))
            .child(today);

        let clip = div()
            .id("space-clip")
            .relative()
            .flex_none()
            .min_w_0()
            .overflow_hidden()
            .on_scroll(cx.listener(move |this, e: &gpui::ScrollEvent, _w, cx| {
                let (dx, dy) = scroll_delta(e);
                let snap = ui_state(cx).read(cx).snapshot.clone();
                let pending = this.opt.read(cx).pending().clone();
                let Some(sp_now) = active_space(&snap, pending.space_id.as_deref()) else { return };
                let width = this.width.unwrap_or(snap.sidebar_width as f32);
                let cfg = this.space_cfg(&snap.spaces, &sp_now.id.clone(), width, reduced_motion(cx));
                let now = this.now_ms();
                let out = this.swipe.wheel(&cfg, dx, dy, now);
                this.preview.update(cx, |p, cx| p.hide(cx));
                if out.consumed {
                    cx.stop_propagation();
                    cx.notify();
                }
                if let Some(dir) = out.switch {
                    let target = (cfg.index as i32 + dir) as usize;
                    if let Some(id) = snap.spaces.get(target).map(|s| s.id.clone()) {
                        switch_space(&id, cx);
                    }
                }
            }))
            .children(preview_prev)
            .child(cur_page)
            .children(preview_next);

        let scroll = div()
            .id("sidebar-scroll")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(sp::HALF)
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .children(favorites)
            .child(clip);

        let media = media_view(&snap).map(|(info, tab_id)| {
            let (a, b, c, d, e) = (tab_id.clone(), tab_id.clone(), tab_id.clone(), tab_id.clone(), tab_id.clone());
            div().px(px(GAP)).child(
                MediaCard::new("sidebar-media", info, media_labels())
                    .on_activate(move |_, _, cx| api(cx).activate_tab(&a))
                    .on_toggle(move |_, _, cx| api(cx).media_toggle(&b))
                    .on_previous(move |_, _, cx| api(cx).media_skip(&c, signet_core::api::SkipDirection::Previous))
                    .on_next(move |_, _, cx| api(cx).media_skip(&d, signet_core::api::SkipDirection::Next))
                        .on_mute(move |_, _, cx| api(cx).toggle_mute(&e)),
            )
        });

        let unseen = self.downloads.is_none() && snap.downloads.iter().any(|d| !self.seen_downloads.contains(&d.id));
        let head = self.head(&space, &snap, right, unseen, &t, cx);
        let bottom = self.bottom_bar(&snap, &space, width, &t, window, cx);

        // Floating bar: moving the mouse out closes it, unless a drag is in progress or a panel is open.
        let blocked = self.dragging || self.editing.is_some() || self.icon_form.is_some() || self.space_form.is_some() || self.downloads.is_some();
        if self.floating && !blocked && !self.inside {
            api(cx).set_sidebar_hover(false);
        }

        let handle = div()
            .id("sidebar-resize")
            .absolute()
            .top(px(0.0))
            .bottom(px(0.0))
            .w(px(6.0))
            .when(!right, |d| d.right(px(0.0)))
            .when(right, |d| d.left(px(0.0)))
            .cursor_col_resize()
            .on_drag(ResizeDrag, |_, _, _, cx| cx.new(|_| ResizeGhost));

        // Panels: the wanted view is mirrored in a `Presence` so that it stays mounted for its exit animation.
        let wanted: [Option<AnyView>; 3] = [self.icon_form.clone().map(Into::into), self.space_form.clone(), self.downloads.clone()];
        let getters: [fn(&mut Sidebar) -> &mut Presence<AnyView>; 3] = [|s| &mut s.ex_icon, |s| &mut s.ex_form, |s| &mut s.ex_downloads];
        for (want, get) in wanted.into_iter().zip(getters) {
            let live = get(self).value().is_some() && !get(self).closing();
            match want {
                Some(v) => {
                    get(self).update(Some(v));
                }
                None if live => Presence::set(self, get, None, cx),
                None => {}
            }
        }
        let panel = |p: &Presence<AnyView>, id: &'static str| {
            p.value().cloned().map(|view| {
                let closing = p.closing();
                div().id(id).absolute().left(px(GAP)).right(px(GAP)).bottom(sp::TOOLBAR_H).child(surface_enter(div().w_full().text_size(t.type_scale.base).child(view), SharedString::from(format!("{id}-{closing}")), closing, reduced))
            })
        };
        let origin = self.root.get().origin;
        let ghosts: Vec<AnyElement> = self.ghosts.iter().enumerate().map(|(i, g)| ghost_row(g, i, origin, &t)).collect();
        let root_cell = self.root.clone();
        let hover_blocked = blocked;
        div()
            .id("sidebar")
            .relative()
            .flex()
            .flex_col()
            .h_full()
            .w(px(width))
            .flex_none()
            .text_size(t.type_scale.base)
            .text_color(t.text)
            .when(self.floating, |d| d.rounded(radius::POP).bg(t.glass_view).border_1().border_color(t.border))
            .child(probe(root_cell))
            .on_hover(cx.listener(move |this, hovered: &bool, _w, cx| {
                if !this.floating {
                    return;
                }
                this.inside = *hovered;
                if *hovered {
                    api(cx).set_sidebar_hover(true);
                } else if !hover_blocked && !this.dragging {
                    api(cx).set_sidebar_hover(false);
                }
            }))
            .on_drag_move::<TabDrag>(cx.listener(|this, e: &DragMoveEvent<TabDrag>, _w, cx| this.drag_over(e, cx)))
            .on_drag_move::<ResizeDrag>(cx.listener(move |this, e: &DragMoveEvent<ResizeDrag>, _w, cx| this.resize_move(e, right, cx)))
            .on_drop::<TabDrag>(cx.listener(|this, d: &TabDrag, _w, cx| this.dropped(d, cx)))
            .child(head)
            .child({
                // Edge fades where the list continues (CSS `scroll-edges`, 12 px).
                let color = if self.floating { t.glass_view } else { t.bg };
                let (off, max) = (self.scroll.offset().y, self.scroll.max_offset().y);
                let fades = if t.settings.high_contrast { [None, None] } else { crate::widgets::vertical_fades(f32::from(off), f32::from(max), 12.0, color) };
                div().relative().flex_1().min_h_0().flex().flex_col().px(px(GAP)).child(scroll).children(fades.into_iter().flatten())
            })
            .children(media)
            .child(bottom)
            .children(panel(&self.ex_icon, "icon-form"))
            .children(panel(&self.ex_form, "space-form"))
            .children(panel(&self.ex_downloads, "downloads-panel"))
            .children(ghosts)
            .children(self.toast.clone())
            .child(handle)
            .into_any_element()
    }
}

/// Fading copy of a closed row, placed at the row's last place.
fn ghost_row(g: &Ghost, i: usize, origin: gpui::Point<Pixels>, t: &crate::theme::SignetTheme) -> AnyElement {
    use gpui::AnimationExt as _;
    let b = g.bounds;
    div()
        .absolute()
        .left(b.origin.x - origin.x)
        .top(b.origin.y - origin.y)
        .w(b.size.width)
        .h(b.size.height)
        .flex()
        .items_center()
        .gap(sp::S2)
        .pl(sp::S3)
        .pr(sp::S2)
        .rounded(radius::ROW)
        .text_size(t.type_scale.base)
        .when(g.active, |d| d.bg(t.active).shadow(t.active_shadow.clone()))
        .child(div().flex_none().child(crate::screens::tabs::custom::tab_icon(&g.tab, 16.0)))
        .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(t.text_tab).child(display_title(&g.tab, &tr("sidebar.newTab"))))
        .with_animation(
            gpui::ElementId::from(SharedString::from(format!("ghost-{}-{i}", g.tab.id))),
            gpui::Animation::new(GHOST).with_easing(crate::motion::ease_out),
            |d, k| d.opacity(1.0 - k),
        )
        .into_any_element()
}

/// `ListCtx` is built once per list with its own drop; the rest is shared.
fn ctx_clone<'a>(c: &ListCtx<'a>) -> ListCtx<'a> {
    ListCtx { snap: c.snap, space: c.space, env: c.env, drop: c.drop, gapped: c.gapped, gap_h: c.gap_h, regions: c.regions, t: c.t }
}

/// Keys to keep in the bounds map: tab ids and the card, header and list probes (rebuilt every render, cheap).
fn live_keys(tabs: &HashSet<String>, snap: &Snapshot) -> HashSet<String> {
    let mut keep = tabs.clone();
    for sp in &snap.spaces {
        collect_keys(&sp.pinned, &mut keep);
        collect_keys(&sp.today, &mut keep);
        if let Some(g) = &sp.groups {
            for id in g.keys() {
                keep.insert(format!("card:{id}"));
                keep.insert(format!("hdr:{id}"));
                keep.insert(format!("body:{id}"));
            }
        }
    }
    for k in ["list:favorites", "list:pinned", "list:today"] {
        keep.insert(k.to_string());
    }
    keep
}

fn collect_keys(items: &[Item], keep: &mut HashSet<String>) {
    for i in items {
        match i {
            Item::Folder(f) => {
                for p in ["card:", "hdr:", "body:"] {
                    keep.insert(format!("{p}{}", f.id));
                }
                collect_keys(&f.items, keep);
            }
            Item::Split(s) => {
                keep.insert(s.id.clone());
            }
            _ => {}
        }
    }
}

/// Wheel delta in the DOM sign (fingers moving left give a positive `dx`).
fn scroll_delta(e: &gpui::ScrollEvent) -> (f32, f32) {
    let (x, y) = match e.delta {
        gpui::ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y)),
        gpui::ScrollDelta::Lines(l) => (l.x * 16.0, l.y * 16.0),
    };
    (-x, -y)
}

// -------------------------------------------------------------------------------------------------------------
// Pieces of the layout

impl Sidebar {
    fn head(&self, space: &Space, snap: &Snapshot, right: bool, unseen: bool, t: &crate::theme::SignetTheme, cx: &mut Context<Self>) -> AnyElement {
        let id = space.id.clone();
        let left_pad = if self.floating {
            sp::S4
        } else if right {
            sp::S4 + px(GAP)
        } else if cfg!(target_os = "macos") {
            px(78.0 + GAP)
        } else {
            sp::S4
        };
        let dl_open = self.downloads.is_some();
        let active = downloads_active(snap);
        div()
            .id("sidebar-head")
            .flex_none()
            .flex()
            .items_center()
            .gap(sp::S2)
            .h(sp::TOOLBAR_H)
            .pl(left_pad)
            .pr(sp::S2)
            .when(!self.floating, crate::title_area::title_area)
            .on_mouse_down(MouseButton::Right, move |_, _, cx| api(cx).show_space_menu(&id))
            .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(weight::STRONG).text_color(t.text).child(SharedString::from(space.name.clone())))
            .when(space.profile == Some(SpaceProfile::Isolated), |d| {
                d.child(div().id("profile-badge").flex_none().text_color(t.text_2).tooltip({
                    let tip = tr("sidebar.isolatedProfile");
                    move |w, cx| Tooltip::new(tip.clone()).build(w, cx)
                }).child(Icon::from(IconName::UserRound).size(px(13.0))))
            })
            .child(div().flex_1())
            .when(!snap.downloads.is_empty(), |d| {
                d.child(
                    div()
                        .relative()
                        .child(
                            IconButton::new("sb-downloads", IconName::Download)
                                .tooltip(tr("sidebar.downloads"))
                                .active(dl_open || active)
                                .on_click(cx.listener(|_this, _, _w, cx| cx.emit(SidebarEvent::ToggleDownloads))),
                        )
                        .when(unseen, |d| d.child(div().absolute().top(px(4.0)).right(px(4.0)).size(px(7.0)).rounded_full().bg(t.accent))),
                )
            })
            .child(
                div()
                    .on_mouse_down(MouseButton::Right, |_, _, cx| {
                        cx.stop_propagation();
                        api(cx).show_add_folder_menu();
                    })
                    .child(IconButton::new("sb-add-folder", IconName::FolderPlus).icon_size(15.0).tooltip(tr("sidebar.newFolder")).on_click(|_, _, cx| api(cx).create_folder())),
            )
            .into_any_element()
    }

    /// Thin separator; "Archive" appears on hover.
    fn divider(&self, space: &Space, t: &crate::theme::SignetTheme, _cx: &mut Context<Self>) -> AnyElement {
        let has_today = !space.today.is_empty();
        let (bg, hover_bg, text, text_2) = (t.bg, t.surface_2, t.text, t.text_2);
        div()
            .id("sidebar-divider")
            .group("sb-divider")
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .h(sp::S3)
            .my(sp::S1)
            .px(sp::S3)
            .child(div().flex_1().h(px(1.0)).bg(t.border))
            .when(has_today, |d| {
                d.child(
                    div()
                        .id("sb-archive")
                        .absolute()
                        .right(sp::S1)
                        .flex()
                        .items_center()
                        .gap(sp::S1)
                        .h(px(20.0))
                        .px(sp::S2)
                        .rounded(radius::CONTROL)
                        .text_size(t.type_scale.cap)
                        .text_color(text_2)
                        .bg(bg)
                        .opacity(0.0)
                        .group_hover("sb-divider", |s| s.opacity(1.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg).text_color(text))
                        .on_click(|_, _, cx| api(cx).clear_today())
                        .child(Icon::from(IconName::Archive).size(px(12.0)))
                        .child(tr("sidebar.archive")),
                )
            })
            .into_any_element()
    }

    fn new_tab_row(&self, t: &crate::theme::SignetTheme) -> AnyElement {
        let (hover_bg, text, text_2) = (t.surface_2, t.text, t.text_2);
        div()
            .id("sidebar-new-tab")
            .flex()
            .flex_none()
            .items_center()
            .gap(sp::S2)
            .w_full()
            .h(t.row_h)
            .pl(sp::S3)
            .pr(sp::S2)
            .rounded(radius::ROW)
            .text_size(t.type_scale.base)
            .text_color(text_2)
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg).text_color(text))
            .on_click(|_, _, cx| api(cx).open_palette(PaletteMode::NewTab, None))
            .on_mouse_down(MouseButton::Right, |_, _, cx| api(cx).show_new_menu())
            .child(Icon::from(IconName::Plus).size(px(16.0)))
            .child(div().flex_1().child(tr("sidebar.newTab")))
            .into_any_element()
    }

    fn bottom_bar(&mut self, snap: &Snapshot, space: &Space, width: f32, t: &crate::theme::SignetTheme, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let keys = shortcut_label("library", snap.settings.shortcuts.as_ref(), IS_MAC);
        let library_tip = match keys {
            Some(k) => tr_args("sidebar.library", &[("keys", &k)]),
            None => tr("sidebar.libraryNoKeys"),
        };
        let strip = self.space_strip(snap, space, width, t, window, cx);
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(sp::HALF)
            .h(px(36.0))
            .px(sp::S1 + px(GAP))
            .child(IconButton::new("sb-settings", IconName::Settings).tooltip(tr("sidebar.settings")).on_click(|_, _, cx| api(cx).open_settings(None)))
            .child(IconButton::new("sb-library", IconName::Library).tooltip(library_tip).on_click(|_, _, cx| api(cx).open_library(None)))
            .child(strip)
            .child(IconButton::new("sb-new-space", IconName::Plus).tooltip(tr("sidebar.newSpace")).on_click(cx.listener(|this, _, _w, cx| {
                this.downloads = None;
                cx.emit(SidebarEvent::EditSpace(None));
            })))
            .into_any_element()
    }

    /// Space icons at the bottom, arranged in pages when they do not fit; pages are swiped like spaces.
    fn space_strip(&mut self, snap: &Snapshot, space: &Space, width: f32, t: &crate::theme::SignetTheme, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Room left in the bar: sidebar minus margins, two buttons before and one after.
        let measured = f32::from(self.strip.get().size.width);
        let strip_w = if measured > 1.0 { measured } else { (width - 2.0 * GAP - 2.0 * f32::from(sp::S1) - 3.0 * 28.0 - 3.0).max(1.0) };
        let per = per_page_for(strip_w);
        let active = snap.spaces.iter().position(|s| s.id == space.id).unwrap_or(0);
        let pages = snap.spaces.len().div_ceil(per).max(1);
        let own = match self.strip_pick {
            Some((page, a, p)) if a == active && p == per => page,
            _ => active / per,
        };
        let shown = own.min(pages - 1);
        // A change in the number of icons per page recenters without sliding.
        let animate = self.last_page.is_none_or(|(_, p)| p == per);
        let strip_cfg = SwipeCfg { width: strip_w, index: shown, count: pages, reduced: reduced_motion(cx), animate_jumps: animate };
        if self.last_page != Some((shown, per)) {
            let from = self.last_page.map(|(p, _)| p);
            self.strip_swipe.page_changed(&strip_cfg, from, self.now_ms());
            self.last_page = Some((shown, per));
        }
        let now = self.now_ms();
        if let Some(dir) = self.strip_swipe.frame(&strip_cfg, now) {
            let page = (shown as i32 + dir).clamp(0, pages as i32 - 1) as usize;
            self.strip_pick = Some((page, active, per));
        }
        if self.strip_swipe.needs_frames() || self.strip_swipe.swiping() {
            _window.request_animation_frame();
        }
        let x = self.strip_swipe.offset();
        let fade_color = if self.floating { t.glass_view } else { t.bg };
        let mut track = div().relative().flex().flex_row().left(px(-(shown as f32) * strip_w + x));
        for (pi, chunk) in snap.spaces.chunks(per).enumerate() {
            let mut page = div().flex().flex_none().justify_center().w(px(strip_w));
            for (j, sp_i) in chunk.iter().enumerate() {
                let i = pi * per + j;
                page = page.child(self.space_dot(sp_i, sp_i.id == space.id, i, t, cx));
            }
            track = track.child(page);
        }
        div()
            .id("space-strip")
            .relative()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .child(probe(self.strip.clone()))
            .on_scroll(cx.listener(move |this, e: &gpui::ScrollEvent, _w, cx| {
                let (dx, dy) = scroll_delta(e);
                let now = this.now_ms();
                let out = this.strip_swipe.wheel(&strip_cfg, dx, dy, now);
                if out.consumed {
                    cx.stop_propagation();
                    cx.notify();
                }
                if let Some(dir) = out.switch {
                    let page = (strip_cfg.index as i32 + dir).clamp(0, strip_cfg.count as i32 - 1) as usize;
                    this.strip_pick = Some((page, active, per));
                    cx.notify();
                }
            }))
            .child(track)
            .children(if shown > 0 { crate::widgets::edge_fade(crate::widgets::FadeEdge::Left, 10.0, fade_color) } else { None })
            .children(if shown + 1 < pages { crate::widgets::edge_fade(crate::widgets::FadeEdge::Right, 10.0, fade_color) } else { None })
            .into_any_element()
    }

    fn space_dot(&self, sp_i: &Space, on: bool, i: usize, t: &crate::theme::SignetTheme, cx: &mut Context<Self>) -> AnyElement {
        let over = self.over_space.as_deref() == Some(sp_i.id.as_str());
        let (sel, id, id2, id3) = (t.sel, sp_i.id.clone(), sp_i.id.clone(), sp_i.id.clone());
        let name = SharedString::from(sp_i.name.clone());
        let space_c = sp_i.clone();
        let emoji_size = if over { 15.0 * 1.25 } else { 15.0 };
        div()
            .id(SharedString::from(format!("space-dot-{i}")))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(sp::S7)
            .rounded(radius::CONTROL)
            .when(on || over, |d| d.bg(sel))
            .when(!on && !over, |d| d.hover(move |s| s.bg(sel.opacity(0.5))))
            .cursor_pointer()
            .tooltip(move |w, cx| Tooltip::new(name.clone()).build(w, cx))
            .on_click(move |_, _, cx| switch_space(&id, cx))
            .on_mouse_down(MouseButton::Right, move |_, _, cx| api(cx).show_space_menu(&id2))
            .on_drag_move::<TabDrag>(cx.listener(move |this, e: &DragMoveEvent<TabDrag>, _w, cx| {
                let inside = e.bounds.contains(&e.event.position);
                let cur = this.over_space.as_deref() == Some(id3.as_str());
                if inside && !cur {
                    this.over_space = Some(id3.clone());
                    cx.notify();
                } else if !inside && cur {
                    this.over_space = None;
                    cx.notify();
                }
            }))
            .on_drop::<TabDrag>(cx.listener(move |this, d: &TabDrag, _w, cx| this.drop_on_space(d, &space_c, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(emoji_size))
                    .line_height(px(15.0))
                    .font_family(crate::theme::emoji_font())
                    .opacity(if on || over { 1.0 } else { 0.6 })
                    .child(SharedString::from(sp_i.icon.clone())),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::types::{FolderItem, TabItem};

    fn space(open: bool) -> Space {
        let tab = |id: &str| Item::Tab(TabItem { tab_id: id.into(), group: None });
        let folder = Item::Folder(FolderItem { id: "f".into(), name: "f".into(), open, items: vec![tab("a")], ..Default::default() });
        Space { id: "s".into(), pinned: vec![folder], today: vec![tab("b")], ..Default::default() }
    }

    #[test]
    fn folding_changes_the_data_signature_but_not_the_glide_signature() {
        assert_ne!(data_sig(&space(true), &[], true), data_sig(&space(false), &[], true));
        assert_eq!(data_sig(&space(true), &[], false), data_sig(&space(false), &[], false));
    }

    #[test]
    fn open_estimates_cover_nested_folders() {
        let mut out = HashMap::new();
        collect_opens(&space(true).pinned, 30.0, &mut out);
        let (open, height) = out["f"];
        assert!(open && height > 30.0);
    }
}

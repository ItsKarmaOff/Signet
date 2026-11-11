//! Main window root views (port of `renderer/src/App.tsx`).
//!
//! - [`MainWindow`] (route `''`): the sidebar, the cards around each pane with their bar and bookmarks bar, the
//!   dividers, the focus ring and the empty state. The web areas themselves are left empty: the native CEF views sit
//!   on top of them.
//! - [`FloatingSidebar`] (route `#sidebar`) and [`FloatingToolbar`] (route `#toolbar`): the hidden sidebar and the
//!   compact mode's toolbar, shown over the window in their own transparent window.
//!
//! What the app provides, see the report of the work: window options, traffic lights position
//! ([`traffic_lights`]), the pane rectangles in `Snapshot.panes`, the `UiEvent`s through `app_state::install`.

mod background;
mod embed;
mod geometry;

pub use geometry::{card_radius, traffic_lights};

use crate::app_state::{api, snapshot, tr, ui_state};
use crate::icon::IconName;
use crate::motion::{ease_out, reduced_motion, spring_curve};
use crate::screens::toolbar::{BOOKMARK_BAR, BarFade, BookmarksBar, IS_MAC, PANE_BAR, Toolbar, ToolbarProps, bar_colors};
use crate::theme::{ThemeExt, duration, radius, space};
use crate::screens::devtools::{DevToolsCard, ResponsiveFrame};
use crate::screens::sidebar::Sidebar;
use crate::screens::tabs::Dividers;
use background::{apply_space_theme, layers};
use geometry::{BAR_OVERFLOW, GAP, TAB_BAR, stretched, top_left};
use gpui::{
    Animation, AnimationExt, AnyView, App, AppContext, BoxShadow, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Window, div, point, prelude::FluentBuilder, px, relative,
};
use gpui_component::Icon;
use signet_core::api::UiEvent;
use signet_core::floating::float_pose;
use signet_core::shortcuts::shortcut_label;
use signet_core::slide::slide_at;
use signet_core::types::{PaletteMode, Rect, SidebarSide, Snapshot, Space, TabLayout};
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn active_space(s: &Snapshot) -> Option<&Space> {
    s.spaces.iter().find(|x| x.id == s.active_space_id).or_else(|| s.spaces.first())
}

fn pxf(v: f64) -> gpui::Pixels {
    px(v as f32)
}

/// Absolute placement of a rectangle.
fn place<E: Styled>(d: E, r: Rect) -> E {
    d.absolute().left(pxf(r.x)).top(pxf(r.y)).w(pxf(r.width)).h(pxf(r.height))
}

/// Part of the window the user can move it by (`-webkit-app-region: drag`). Not while an overlay covers the window.
fn drag_area<E: InteractiveElement>(d: E, covered: bool) -> E {
    if covered {
        return d;
    }
    crate::title_area::title_area(d)
}

/// Linear interpolation of two rectangles.
pub fn lerp_rect(a: Rect, b: Rect, t: f64) -> Rect {
    let l = |x: f64, y: f64| x + (y - x) * t;
    Rect::new(l(a.x, b.x), l(a.y, b.y), l(a.width, b.width), l(a.height, b.height))
}

/// Focus ring state: glides from one pane to another when the focus changes panel, but follows the geometry without
/// lag otherwise (divider, window, sidebar).
#[derive(Default)]
struct Ring {
    id: Option<String>,
    card: Option<Rect>,
    from: Option<Rect>,
    generation: u64,
}

pub struct MainWindow {
    toolbars: HashMap<String, Entity<Toolbar>>,
    bookmarks: HashMap<String, Entity<BookmarksBar>>,
    sidebar: Entity<Sidebar>,
    dividers: Entity<Dividers>,
    devtools: HashMap<String, Entity<DevToolsCard>>,
    responsive: HashMap<String, Entity<ResponsiveFrame>>,
    /// The pointer is in the compact mode's top strip (it reveals the floating toolbar; leaving it re-arms the leave).
    top_inside: bool,
    strip: AnyView,
    ring: Ring,
    /// Color fade of each pane's bar (site color change, 240 ms).
    fades: HashMap<String, BarFade>,
    _sidebar_sub: gpui::Subscription,
}

impl MainWindow {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let strip = embed::tab_strip(window, cx, false);
        cx.new(|cx| {
            let (sidebar, sidebar_sub) = embed::sidebar(window, false, cx);
            let dividers = embed::dividers(window, cx);
            let state = ui_state(cx);
            // The space tint applies to the whole app (CSS variables `--h`...).
            let snap = state.read(cx).snapshot.clone();
            if let Some(space) = active_space(&snap) {
                apply_space_theme(space, cx);
            }
            cx.observe(&state, |_, state, cx| {
                let snap = state.read(cx).snapshot.clone();
                if let Some(space) = active_space(&snap) {
                    apply_space_theme(space, cx);
                }
                cx.notify();
            })
            .detach();
            // Cmd+L: main asks where the visible address field is (active pane's bar); without it the command bar
            // stays centered.
            cx.subscribe(&state, |this: &mut Self, _, e: &UiEvent, cx| {
                if matches!(e, UiEvent::EditAddressRequest) {
                    let rect = this.toolbars.values().find(|t| !t.read(cx).is_dim_or_floating()).map(|t| t.read(cx).address_rect()).filter(|r| r.width > 0.0);
                    api(cx).open_palette(PaletteMode::EditUrl, rect);
                }
            })
            .detach();
            Self { toolbars: HashMap::new(), bookmarks: HashMap::new(), sidebar, dividers, devtools: HashMap::new(), responsive: HashMap::new(), top_inside: false, strip, ring: Ring::default(), fades: HashMap::new(), _sidebar_sub: sidebar_sub }
        })
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = ui_state(cx);
        let (snap, progress) = {
            let ui = state.read(cx);
            (ui.snapshot.clone(), ui.sidebar_slide)
        };
        let s: &Snapshot = &snap;
        let t = cx.signet().clone();
        let space = active_space(s).cloned();
        let slide = s.sidebar_slide.as_ref().map(|plan| slide_at(plan, progress.unwrap_or(plan.motion.from)));
        let (panes, dividers) = stretched(s, slide.as_ref());
        let covered = s.overlay_open;
        let card_r = card_radius(s);

        let compact = s.settings.compact_mode;
        let horizontal = s.settings.tab_layout == Some(TabLayout::Horizontal);
        let bookmarks_on = s.settings.show_bookmarks_bar != Some(false);
        let side_right = !horizontal && s.settings.sidebar_side == Some(SidebarSide::Right);
        let left = if s.sidebar_visible { s.sidebar_width as f32 } else { 0.0 };
        let shift = slide.as_ref().map_or(0.0, |sl| sl.sidebar as f32);
        let new_tab_keys = shortcut_label("newTab", s.settings.shortcuts.as_ref(), IS_MAC);
        let bookmark_keys = shortcut_label("bookmark", s.settings.shortcuts.as_ref(), IS_MAC);

        // Sidebar on the right: the top-left card carries the traffic lights (Mac only).
        let corner = if IS_MAC && side_right && !compact { top_left(&panes) } else { None };
        // The sidebar toggle lives in the top-left pane's bar, sidebar shown or not (compact mode: the floating bar).
        let first = if !horizontal && !compact { top_left(&panes) } else { None };

        // Prune the bars of panes that are gone.
        self.toolbars.retain(|id, _| panes.iter().any(|p| &p.tab_id == id));
        self.bookmarks.retain(|id, _| panes.iter().any(|p| &p.tab_id == id));
        self.fades.retain(|id, _| panes.iter().any(|p| &p.tab_id == id));
        let reduced = reduced_motion(cx);
        self.devtools.retain(|id, _| panes.iter().any(|p| &p.tab_id == id && p.devtools.is_some()));
        self.responsive.retain(|id, _| panes.iter().any(|p| &p.tab_id == id && p.responsive.is_some()));

        let ring_pane = (panes.len() > 1).then(|| panes.iter().find(|p| p.focused)).flatten();
        match ring_pane {
            Some(p) => {
                if self.ring.id.as_deref() != Some(p.tab_id.as_str()) {
                    self.ring.from = self.ring.card;
                    self.ring.generation += 1;
                    self.ring.id = Some(p.tab_id.clone());
                }
                self.ring.card = Some(p.card);
            }
            None => self.ring = Ring { generation: self.ring.generation, ..Ring::default() },
        }

        let mut root = div().id("main-window").relative().size_full().overflow_hidden().text_color(t.text).text_size(t.type_scale.base).line_height(gpui::relative(crate::theme::LINE_HEIGHT));
        for layer in layers(&t) {
            root = root.child(div().absolute().size_full().bg(layer));
        }
        if compact {
            root = root.on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, cx| {
                // Left the strip into the window without entering the bar (Electron `onMouseLeave`): the bar closes after
                // its delay unless the pointer reaches it (its first move cancels the leave).
                if this.top_inside && e.position.y >= space::GAP {
                    this.top_inside = false;
                    api(cx).set_toolbar_hover(false);
                }
            }));
        }

        // Window drag strip along the top edge.
        if !compact && !horizontal {
            let strip = div().absolute().top(px(0.0)).right(px(0.0)).h(space::GAP).left(if s.sidebar_visible { px(0.0) } else { space::GAP });
            root = root.child(drag_area(strip, covered));
        }
        if horizontal && !compact {
            root = root.child(div().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0)).h(px(TAB_BAR)).child(self.strip.clone()));
        }
        if compact {
            let hover = div()
                .id("top-hover")
                .absolute()
                .top(px(0.0))
                .left(px(0.0))
                .right(px(0.0))
                .h(px(6.0))
                .on_mouse_move(cx.listener(|this, _: &gpui::MouseMoveEvent, _, cx| {
                    if !this.top_inside {
                        this.top_inside = true;
                        api(cx).set_toolbar_hover(true);
                    }
                }))
                .on_mouse_exit(cx.listener(|this, _: &gpui::MouseExitEvent, _, cx| {
                    // Left the window upward from the strip.
                    if std::mem::take(&mut this.top_inside) {
                        api(cx).set_toolbar_hover(false);
                    }
                }));
            root = root.child(drag_area(hover, covered));
        }
        if !s.sidebar_visible && !horizontal {
            let rim = div().id("rim-hover").absolute().top(px(0.0)).bottom(px(0.0)).w(space::GAP).on_mouse_move(|_, _, cx| api(cx).set_sidebar_hover(true));
            root = root.child(if side_right { rim.right(px(0.0)) } else { rim.left(px(0.0)) });
        }
        if s.sidebar_visible {
            let bar = div().absolute().top(px(0.0)).bottom(px(0.0)).w(px(left)).child(self.sidebar.clone());
            root = root.child(if side_right { bar.right(px(-shift)) } else { bar.left(px(shift)) });
        }

        // Cards.
        let border = BoxShadow { color: t.border, offset: point(px(0.0), px(0.0)), blur_radius: px(0.0), spread_radius: px(0.5), inset: false };
        let mut shadows = t.shadow_card.clone();
        shadows.push(border);
        for (index, p) in panes.iter().enumerate() {
            let tab = s.tabs.get(&p.tab_id);
            let bar = self.fades.entry(p.tab_id.clone()).or_default().step(bar_colors(tab.and_then(|t| t.theme_color.as_deref()), &t), reduced, window);
            let dim = panes.len() > 1 && !p.focused;
            let mut card = place(div().id(SharedString::from(format!("pane-{}", p.tab_id))), p.card).rounded(card_r).shadow(shadows.clone()).bg(if slide.is_some() { bar.bg } else { t.surface });
            if !compact {
                let fill = PANE_BAR + if bookmarks_on { BOOKMARK_BAR } else { 0.0 } + if p.responsive.is_some() { 0.0 } else { BAR_OVERFLOW };
                card = card.child(div().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0)).h(px(fill)).rounded_t(card_r).bg(bar.bg));
                let props = ToolbarProps {
                    tab_id: Some(p.tab_id.clone()),
                    leading: corner == Some(index),
                    sidebar_toggle: first == Some(index),
                    narrow: p.card.width < 420.0,
                    dim,
                    floating: false,
                    no_drag: covered,
                };
                let toolbar = self.toolbars.entry(p.tab_id.clone()).or_insert_with(|| Toolbar::new(props.clone(), cx)).clone();
                toolbar.update(cx, |tb, cx| tb.set_props(props, cx));
                card = card.child(div().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0)).h(px(PANE_BAR)).child(toolbar));
                if bookmarks_on {
                    let items = space.as_ref().and_then(|sp| sp.bookmarks.clone()).unwrap_or_default();
                    let bm = self.bookmarks.entry(p.tab_id.clone()).or_insert_with(|| BookmarksBar::new(cx)).clone();
                    let focused = p.focused || panes.len() == 1;
                    bm.update(cx, |b, cx| b.set(&items, Some(&p.tab_id), focused, bookmark_keys.clone(), bar, cx));
                    card = card.child(div().absolute().top(px(PANE_BAR)).left(px(0.0)).right(px(0.0)).h(px(BOOKMARK_BAR)).when(dim, |d| d.opacity(0.7)).child(bm));
                }
            }
            root = root.child(card);
            if let Some(r) = &p.responsive {
                let frame = self.responsive.entry(p.tab_id.clone()).or_insert_with(|| ResponsiveFrame::new(window, cx, p.tab_id.clone())).clone();
                frame.update(cx, |f, cx| f.set(p.rect, Some(r.clone()), cx));
                root = root.child(frame);
            }
            if let Some(d) = &p.devtools {
                let card = self.devtools.entry(p.tab_id.clone()).or_insert_with(|| DevToolsCard::new(cx, p.tab_id.clone())).clone();
                card.update(cx, |c, cx| c.set(Some(d.clone()), dim, cx));
                root = root.child(card);
            }
        }

        if panes.is_empty() {
            let top = if horizontal && !compact { 44.0 } else if compact { 6.0 } else { GAP };
            let l = if s.sidebar_visible && !side_right { left } else { 6.0 };
            let r = if s.sidebar_visible && side_right { left } else { 6.0 };
            let button = crate::title_area::no_drag(div()
                .id("empty-new-tab")
                .flex()
                .items_center()
                .gap(space::S2)
                .h(px(32.0))
                .px(space::S4)
                .rounded(radius::CONTROL)
                .bg(t.accent)
                .text_color(t.on_accent)
                .cursor_pointer()
                .hover(|s| s.opacity(0.9))
                .on_click(|_, _, cx| api(cx).open_palette(PaletteMode::NewTab, None))
                .child(Icon::from(IconName::Plus).size(px(15.0)))
                .child(tr("sidebar.newTab"))
                .when_some(new_tab_keys.filter(|k| !k.is_empty()), |d, k| d.child(div().text_size(t.type_scale.cap).opacity(0.7).child(k))));
            let empty = div()
                .id("empty")
                .absolute()
                .top(px(top))
                .bottom(space::GAP)
                .left(px(l))
                .right(px(r))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(space::S4)
                .rounded(card_r)
                .bg(t.surface)
                .shadow(shadows.clone())
                .text_color(t.text_2)
                .child(Icon::from(IconName::Globe).size(px(28.0)).text_color(t.text_3))
                .child(div().child(tr("app.noTabs")))
                .child(button);
            root = root.child(drag_area(empty, covered));
        }

        let _ = dividers;
        root = root.child(self.dividers.clone());

        // Focus ring.
        if let (Some(p), Some(_)) = (ring_pane, &self.ring.id) {
            let to = p.card;
            let ring = |r: Rect| {
                div()
                    .absolute()
                    .left(pxf(r.x - 3.0))
                    .top(pxf(r.y - 3.0))
                    .w(pxf(r.width + 6.0))
                    .h(pxf(r.height + 6.0))
                    .rounded(card_r + px(3.0))
                    .border_2()
                    .border_color(t.accent)
            };
            match self.ring.from {
                Some(from) if !reduced_motion(cx) => {
                    let id = SharedString::from(format!("focus-ring-{}", self.ring.generation));
                    root = root.child(ring(to).with_animation(id, Animation::new(duration::NORMAL).with_easing(ease_out), move |_, delta| {
                        let r = lerp_rect(from, to, f64::from(delta));
                        div()
                            .absolute()
                            .left(pxf(r.x - 3.0))
                            .top(pxf(r.y - 3.0))
                            .w(pxf(r.width + 6.0))
                            .h(pxf(r.height + 6.0))
                            .rounded(card_r + px(3.0))
                            .border_2()
                            .border_color(t.accent)
                    }));
                }
                _ => root = root.child(ring(to)),
            }
        }
        root
    }
}

/// One slide of the floating card (enter or exit).
#[derive(Clone, Copy)]
struct FloatMotion {
    at: Instant,
    from: f32,
    to: f32,
    dur: Duration,
    enter: bool,
}

impl FloatMotion {
    fn eased(&self, p: f32, reduced: bool) -> f32 {
        if self.enter && !reduced { spring_curve(p) } else { ease_out(p) }
    }

    fn done(&self) -> bool {
        self.at.elapsed() >= self.dur
    }

    /// Offset now (card widths): an interrupted motion hands it to the next one.
    fn offset_now(&self, reduced: bool) -> f32 {
        let p = (self.at.elapsed().as_secs_f32() / self.dur.as_secs_f32()).min(1.0);
        float_pose(self.from, self.to, self.eased(p, reduced)).0
    }
}

/// The parked card takes no pointer event (the hidden view is not in the window in the Electron version): hidden, and
/// its exit slide is over. While the exit plays, a return re-reveals it.
fn card_shielded(shown: bool, motion: Option<FloatMotion>) -> bool {
    !shown && motion.is_none_or(|m| m.done())
}

/// macOS slides the floating cards in the window server (Core Animation, see `mac::card_slide`); the other platforms
/// animate them with GPUI.
const NATIVE_SLIDE: bool = cfg!(target_os = "macos");

/// Reveal state of a floating card window (sidebar, compact toolbar): armed once the window presented its first frames,
/// shown, and the motion in progress (drawn by GPUI on Linux and Windows; on macOS only its clock, for the shield).
#[derive(Default)]
struct CardPresence {
    armed: bool,
    shown: bool,
    generation: u64,
    motion: Option<FloatMotion>,
}

/// What `CardPresence::follow` decided.
#[derive(Debug, PartialEq)]
enum CardChange {
    Enter,
    Exit(Duration),
}

impl CardPresence {
    /// Follows the snapshot. `parked`: parked offset in card units; `exit`: exit duration.
    fn follow(&mut self, want: bool, parked: f32, exit: Duration, reduced: bool) -> Option<CardChange> {
        let want = want && self.armed;
        if want == self.shown {
            return None;
        }
        // Only an unfinished motion is continued (a finished one may be on the other side after a setting change).
        let from = match self.motion {
            Some(m) if !m.done() => m.offset_now(reduced),
            _ => {
                if want {
                    parked
                } else {
                    0.0
                }
            }
        };
        let (to, dur) = if want { (0.0, duration::SLOW) } else { (parked, exit) };
        self.generation += 1;
        self.motion = Some(FloatMotion { at: Instant::now(), from, to, dur, enter: want });
        self.shown = want;
        Some(if want { CardChange::Enter } else { CardChange::Exit(dur) })
    }

    /// The occluding layer that keeps a parked card quiet. On macOS the parked window still gets pointer moves from its
    /// tracking area although it ignores the mouse, and GPUI's hitboxes stay at rest (no reveal, cursor, tooltip).
    fn shield(&self) -> Option<gpui::Stateful<gpui::Div>> {
        card_shielded(self.shown, self.motion).then(|| div().id("float-shield").absolute().top(px(0.0)).left(px(0.0)).size_full().occlude())
    }
}

/// Arms a card after the window's first two frames (a fresh window's first frames are slow: Metal pipelines, empty
/// glyph atlas, the first layout; React waits two animation frames).
fn arm_after_first_frames<T: 'static>(window: &mut Window, weak: gpui::WeakEntity<T>, presence: fn(&mut T) -> &mut CardPresence) {
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |_, cx| {
            let _ = weak.update(cx, |this, cx| {
                presence(this).armed = true;
                cx.notify();
            });
        });
    });
}

/// Re-renders once an exit is over, so the shield appears (the native slide paints nothing meanwhile).
fn notify_after<T: 'static>(dur: Duration, cx: &mut Context<T>) {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(dur).await;
        this.update(cx, |_, cx| cx.notify()).ok();
    })
    .detach();
}

/// The sidebar shown over the window while it is hidden (route `#sidebar`): slides in from its edge.
pub struct FloatingSidebar {
    sidebar: Entity<Sidebar>,
    /// Armed once the window presented its first frames (see [`arm_after_first_frames`]).
    presence: CardPresence,
    last_render: Option<Instant>,
    _sidebar_sub: gpui::Subscription,
}

impl FloatingSidebar {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        // The sidebar's subscriptions are set up: main may replay its pending sends.
        let this = cx.new(|cx| {
            let (sidebar, sub) = embed::sidebar(window, true, cx);
            cx.observe(&ui_state(cx), |_, _, cx| cx.notify()).detach();
            Self { sidebar, presence: CardPresence::default(), last_render: None, _sidebar_sub: sub }
        });
        arm_after_first_frames(window, this.downgrade(), |t| &mut t.presence);
        api(cx).overlay_ready();
        this
    }
}

impl Render for FloatingSidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = snapshot(cx);
        let parked = if s.settings.sidebar_side == Some(SidebarSide::Right) { 1.0 } else { -1.0 };
        let reduced = reduced_motion(cx);
        let exit = Duration::from_millis(signet_core::floating::sidebar_motion(0.0, false, reduced).exit_ms);
        match self.presence.follow(s.sidebar_floating, parked, exit, reduced) {
            Some(CardChange::Enter) => self.sidebar.update(cx, |sb, _| sb.on_reveal()),
            Some(CardChange::Exit(d)) => notify_after(d, cx),
            None => {}
        }
        if crate::trace::on() {
            let now = Instant::now();
            let dt = self.last_render.map(|t| now.duration_since(t).as_secs_f64() * 1000.0).unwrap_or(0.0);
            if let Some(m) = self.presence.motion.filter(|m| m.at.elapsed() < m.dur + Duration::from_millis(100)) {
                crate::trace::mark(|| format!("motion +{:.1} ms dt={dt:.1} ms gen={} enter={}", m.at.elapsed().as_secs_f64() * 1000.0, self.presence.generation, m.enter));
            }
            self.last_render = Some(now);
        }
        // No background and no border here: the Sidebar draws the one rounded card.
        let body = div().absolute().size_full().child(self.sidebar.clone());
        let root = div().relative().size_full().overflow_hidden();
        if NATIVE_SLIDE {
            // The card is always painted at rest: the window manager slides and fades the window's layer natively.
            return root.child(body).children(self.presence.shield()).into_any_element();
        }
        match self.presence.motion {
            Some(m) if self.presence.shown || !m.done() => {
                let id = SharedString::from(format!("float-sidebar-{}", self.presence.generation));
                root.child(body.with_animation(id, Animation::new(m.dur), move |el, p| {
                    let (offset, opacity) = float_pose(m.from, m.to, m.eased(p, reduced));
                    el.left(relative(offset)).opacity(opacity)
                }))
                .children(self.presence.shield())
                .into_any_element()
            }
            // Parked: past its edge and invisible, and shielded from the mouse.
            _ => root.child(body.left(relative(parked)).opacity(0.0)).children(self.presence.shield()).into_any_element(),
        }
    }
}

/// The compact mode's toolbar shown over the window (route `#toolbar`): fade in and a 6 px slide. macOS: a parked
/// window like the floating sidebar (native slide, shield, blur); the other platforms create and close it.
pub struct FloatingToolbar {
    strip: AnyView,
    toolbar: Entity<Toolbar>,
    presence: CardPresence,
    /// The pointer is over the card (set by its sensor, cleared on exit and at each reveal).
    inside: bool,
}

impl FloatingToolbar {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let strip = embed::tab_strip(window, cx, true);
        let props = ToolbarProps { leading: IS_MAC, sidebar_toggle: true, floating: true, ..Default::default() };
        let toolbar = Toolbar::new(props, cx);
        let this = cx.new(|cx| {
            cx.observe(&ui_state(cx), |_, _, cx| cx.notify()).detach();
            Self { strip, toolbar, presence: CardPresence::default(), inside: false }
        });
        arm_after_first_frames(window, this.downgrade(), |t| &mut t.presence);
        this
    }
}

impl Render for FloatingToolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = snapshot(cx);
        let t = cx.signet().clone();
        let reduced = reduced_motion(cx);
        let m = signet_core::floating::toolbar_motion(reduced);
        match self.presence.follow(s.toolbar_floating, -1.0, Duration::from_millis(m.exit_ms), reduced) {
            // The pointer is still in the main window's top strip: the first real move over the card must call
            // `set_toolbar_hover(true)` to cancel the strip's leave.
            Some(CardChange::Enter) => self.inside = false,
            Some(CardChange::Exit(d)) => notify_after(d, cx),
            None => {}
        }
        let horizontal = s.settings.tab_layout == Some(TabLayout::Horizontal);
        let content: gpui::AnyElement = if horizontal {
            self.strip.clone().into_any_element()
        } else {
            let props = ToolbarProps { leading: IS_MAC, sidebar_toggle: true, floating: true, ..Default::default() };
            self.toolbar.update(cx, |tb, cx| tb.set_props(props, cx));
            self.toolbar.clone().into_any_element()
        };
        let blurred = crate::theme::float_blur(cx, crate::theme::FloatCard::Toolbar);
        let body = div()
            .id("float-toolbar")
            .absolute()
            .left(px(0.0))
            .right(px(0.0))
            .h_full()
            .rounded(px(signet_core::floating::card_radius(crate::theme::window_corner(cx).map(f64::from)) as f32))
            .bg(t.float_fill(blurred))
            .border(px(0.5))
            .border_color(t.border)
            .overflow_hidden()
            .child(content);
        // Real events only (GPUI `on_hover` also fires at paint time). On top of the bar: its buttons occlude, a listener
        // on the card would not be hovered over them and would miss the window exit made from a button.
        let sensor = div()
            .id("float-toolbar-sensor")
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .size_full()
            .on_mouse_move(cx.listener(|this, _: &gpui::MouseMoveEvent, _, cx| {
                if this.presence.shown && !this.inside {
                    this.inside = true;
                    api(cx).set_toolbar_hover(true);
                }
            }))
            .on_mouse_exit(cx.listener(|this, _: &gpui::MouseExitEvent, _, cx| {
                this.inside = false;
                if !cx.has_active_drag() {
                    api(cx).set_toolbar_hover(false);
                }
            }));
        let root = div().relative().size_full().overflow_hidden();
        if NATIVE_SLIDE {
            // At rest: the window manager slides and fades the window's layer natively.
            return root.child(body).child(sensor).children(self.presence.shield()).into_any_element();
        }
        if !self.presence.shown {
            return root.child(body.top(px(-6.0)).opacity(0.0)).child(sensor).children(self.presence.shield()).into_any_element();
        }
        let id = SharedString::from(format!("float-toolbar-{}", self.presence.generation));
        root.child(body.with_animation(
            id,
            Animation::new(duration::SLOW).with_easing(if reduced { ease_out } else { spring_curve }),
            move |el, p| el.top(px(-6.0 * (1.0 - p))).opacity((p * 2.0).min(1.0)),
        ))
        .child(sensor)
        .children(self.presence.shield())
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(100.0, 50.0, 200.0, 100.0);
        assert_eq!(lerp_rect(a, b, 0.0), a);
        assert_eq!(lerp_rect(a, b, 1.0), b);
        assert_eq!(lerp_rect(a, b, 0.5), Rect::new(50.0, 25.0, 150.0, 100.0));
    }

    #[test]
    fn presence_follows_the_snapshot() {
        let mut p = CardPresence::default();
        let exit = Duration::from_millis(200);
        // Not armed yet: nothing happens.
        assert_eq!(p.follow(true, -1.0, exit, false), None);
        p.armed = true;
        assert_eq!(p.follow(true, -1.0, exit, false), Some(CardChange::Enter));
        assert_eq!(p.follow(true, -1.0, exit, false), None);
        // Halfway through the enter, an exit starts from where the card is.
        assert_eq!(p.follow(false, -1.0, exit, false), Some(CardChange::Exit(exit)));
        assert_eq!(p.follow(false, -1.0, exit, false), None);
        let m = p.motion.unwrap();
        assert_eq!(m.to, -1.0);
        // A new enter interrupts the unfinished exit and starts from its current offset.
        let now = m.offset_now(false);
        assert_eq!(p.follow(true, -1.0, exit, false), Some(CardChange::Enter));
        let e = p.motion.unwrap();
        assert!((e.from - now).abs() < 0.05, "{} vs {now}", e.from);
        assert_eq!(e.to, 0.0);
    }

    fn motion(ago_ms: u64, dur_ms: u64, enter: bool) -> FloatMotion {
        FloatMotion { at: Instant::now() - Duration::from_millis(ago_ms), from: 0.0, to: -1.0, dur: Duration::from_millis(dur_ms), enter }
    }

    #[test]
    fn parked_card_is_shielded() {
        assert!(card_shielded(false, None));
        assert!(!card_shielded(true, None));
        assert!(!card_shielded(true, Some(motion(1000, 320, true))));
        assert!(!card_shielded(false, Some(motion(0, 200, false))));
        assert!(card_shielded(false, Some(motion(1000, 200, false))));
    }

    #[test]
    fn active_space_falls_back_to_first() {
        let mut s = Snapshot::default();
        assert!(active_space(&s).is_none());
        s.spaces = vec![Space { id: "a".into(), ..Default::default() }, Space { id: "b".into(), ..Default::default() }];
        s.active_space_id = "b".into();
        assert_eq!(active_space(&s).map(|x| x.id.as_str()), Some("b"));
        s.active_space_id = "zz".into();
        assert_eq!(active_space(&s).map(|x| x.id.as_str()), Some("a"));
    }
}

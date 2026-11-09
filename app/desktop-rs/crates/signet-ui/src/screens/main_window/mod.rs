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

pub use geometry::traffic_lights;

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
use signet_core::shortcuts::shortcut_label;
use signet_core::slide::slide_at;
use signet_core::types::{PaletteMode, Rect, SidebarSide, Snapshot, Space, TabLayout};
use std::collections::HashMap;

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
            Self { toolbars: HashMap::new(), bookmarks: HashMap::new(), sidebar, dividers, devtools: HashMap::new(), responsive: HashMap::new(), strip, ring: Ring::default(), fades: HashMap::new(), _sidebar_sub: sidebar_sub }
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
                .on_hover(|hover, _, cx| api(cx).set_toolbar_hover(*hover));
            root = root.child(drag_area(hover, covered));
        }
        if !s.sidebar_visible && !horizontal {
            let rim = div().id("rim-hover").absolute().top(px(0.0)).bottom(px(0.0)).w(space::GAP).on_hover(|hover, _, cx| {
                if *hover {
                    api(cx).set_sidebar_hover(true);
                }
            });
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
            let mut card = place(div().id(SharedString::from(format!("pane-{}", p.tab_id))), p.card).rounded(radius::CARD).shadow(shadows.clone()).bg(if slide.is_some() { bar.bg } else { t.surface });
            if !compact {
                let fill = PANE_BAR + if bookmarks_on { BOOKMARK_BAR } else { 0.0 } + if p.responsive.is_some() { 0.0 } else { BAR_OVERFLOW };
                card = card.child(div().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0)).h(px(fill)).rounded_t(radius::CARD).bg(bar.bg));
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
                .rounded(radius::CARD)
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
                    .rounded(px(12.0 + 3.0))
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
                            .rounded(px(15.0))
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

/// The sidebar shown over the window while it is hidden (route `#sidebar`): slides in from its edge.
pub struct FloatingSidebar {
    sidebar: Entity<Sidebar>,
    shown: bool,
    generation: u64,
    _sidebar_sub: gpui::Subscription,
}

impl FloatingSidebar {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        // The sidebar's subscriptions are set up: main may replay its pending sends.
        let this = cx.new(|cx| {
            let (sidebar, sub) = embed::sidebar(window, true, cx);
            cx.observe(&ui_state(cx), |_, _, cx| cx.notify()).detach();
            Self { sidebar, shown: false, generation: 0, _sidebar_sub: sub }
        });
        api(cx).overlay_ready();
        this
    }
}

impl Render for FloatingSidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = snapshot(cx);
        let t = cx.signet().clone();
        let right = s.settings.sidebar_side == Some(SidebarSide::Right);
        if s.sidebar_floating && !self.shown {
            self.generation += 1;
        }
        self.shown = s.sidebar_floating;
        let from = if right { 1.0 } else { -1.0 };
        let body = div().absolute().size_full().bg(t.glass_view).border_color(t.border).child(self.sidebar.clone());
        let id = SharedString::from(format!("float-sidebar-{}", self.generation));
        let reduced = reduced_motion(cx);
        let root = div().relative().size_full().overflow_hidden();
        if !self.shown {
            // Hidden: parked off its edge, the main process keeps the view in place during the slide.
            return root.child(body.left(relative(from)).opacity(0.0)).into_any_element();
        }
        root.child(body.with_animation(
            id,
            Animation::new(duration::SLOW).with_easing(if reduced { ease_out } else { spring_curve }),
            move |el, p| el.left(relative(from * (1.0 - p))).opacity((p * 2.7).min(1.0)),
        ))
        .into_any_element()
    }
}

/// The compact mode's toolbar shown over the window (route `#toolbar`): fade in and a 6 px slide.
pub struct FloatingToolbar {
    strip: AnyView,
    toolbar: Entity<Toolbar>,
    shown: bool,
    generation: u64,
}

impl FloatingToolbar {
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let strip = embed::tab_strip(window, cx, true);
        let props = ToolbarProps { leading: IS_MAC, sidebar_toggle: true, floating: true, ..Default::default() };
        let toolbar = Toolbar::new(props, cx);
        cx.new(|cx| {
            cx.observe(&ui_state(cx), |_, _, cx| cx.notify()).detach();
            Self { strip, toolbar, shown: false, generation: 0 }
        })
    }
}

impl Render for FloatingToolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = snapshot(cx);
        let t = cx.signet().clone();
        if s.toolbar_floating && !self.shown {
            self.generation += 1;
        }
        self.shown = s.toolbar_floating;
        let horizontal = s.settings.tab_layout == Some(TabLayout::Horizontal);
        let content: gpui::AnyElement = if horizontal {
            self.strip.clone().into_any_element()
        } else {
            let props = ToolbarProps { leading: IS_MAC, sidebar_toggle: true, floating: true, ..Default::default() };
            self.toolbar.update(cx, |tb, cx| tb.set_props(props, cx));
            self.toolbar.clone().into_any_element()
        };
        let reduced = reduced_motion(cx);
        let body = div()
            .id("float-toolbar")
            .absolute()
            .left(px(0.0))
            .right(px(0.0))
            .h_full()
            .rounded(radius::POP)
            .bg(t.glass_view)
            .border(px(0.5))
            .border_color(t.border)
            .overflow_hidden()
            .on_hover(|hover, _, cx| api(cx).set_toolbar_hover(*hover))
            .child(content);
        let root = div().relative().size_full().overflow_hidden();
        if !self.shown {
            return root.child(body.top(px(-6.0)).opacity(0.0)).into_any_element();
        }
        let id = SharedString::from(format!("float-toolbar-{}", self.generation));
        root.child(body.with_animation(
            id,
            Animation::new(duration::SLOW).with_easing(if reduced { ease_out } else { spring_curve }),
            move |el, p| el.top(px(-6.0 * (1.0 - p))).opacity((p * 2.0).min(1.0)),
        ))
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

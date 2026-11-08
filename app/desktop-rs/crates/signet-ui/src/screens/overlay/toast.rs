//! Archive announcement (port of `overlay/Toast.tsx`): "N tabs archived - Undo".
//!
//! Like the Electron app, it is shown by exactly one host: inside the sidebar (`ArchiveToast`, embedded view, when the
//! sidebar is on) or in its own small window (route `#toast`, [`ToastView`]) when no sidebar is visible (tabs on top,
//! compact mode, sidebar hidden). Both listen to `UiEvent::Archived`; each one ignores it when it is not its turn.

use crate::app_state::{api, tr, tr_args, ui_state};
use crate::motion::{Presence, reduced_motion, surface_enter};
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::IconButton;
use crate::IconName;
use gpui::{AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription, Task, Window, div, px};
use gpui_component::input::Escape;
use signet_core::api::{ArchiveKind, ArchivedNotice, UiEvent};
use signet_core::types::{Snapshot, TabLayout};
use std::time::Duration;

/// Display duration before disappearing (pausing on hover extends it).
const DURATION: Duration = Duration::from_millis(5000);
/// Time after which the host window can disappear (the exit animation).
const EXIT: Duration = Duration::from_millis(240);

/// Root view of the `#toast` window (384 x 72 px, at the bottom of the page area; the toast is inset by 12 px).
pub struct ToastView {
    /// Hosted by the sidebar instead of its own window.
    embedded: bool,
    toast: Presence<ArchivedNotice>,
    paused: bool,
    timer: Option<Task<()>>,
    gone: Option<Task<()>>,
    _sub: Subscription,
}

/// True when the sidebar is actually shown (`isSidebarOn`): visible, not compact, not tabs on top.
pub fn sidebar_on(s: &Snapshot) -> bool {
    s.sidebar_visible && !s.settings.compact_mode && s.settings.tab_layout != Some(TabLayout::Horizontal)
}

impl ToastView {
    /// Root view of the `#toast` window.
    pub fn new(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        Self::build(false, window, cx)
    }

    /// The announcement drawn inside the sidebar (above the bottom bar); the sidebar adds it as a child.
    pub fn embedded(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        Self::build(true, window, cx)
    }

    fn build(embedded: bool, window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let state = ui_state(cx);
        cx.new(|cx: &mut Context<Self>| {
            let sub = cx.subscribe_in(&state, window, move |this: &mut Self, state, ev: &UiEvent, window, cx| match ev {
                UiEvent::Archived(_) if sidebar_on(&state.read(cx).snapshot) != embedded => {}
                UiEvent::Archived(n) => {
                    this.gone = None;
                    this.paused = false;
                    Presence::set(this, |t| &mut t.toast, Some(n.clone()), cx);
                    this.arm(window, cx);
                }
                // Undo window elapsed on the app side: no more Undo to offer.
                UiEvent::UndoExpired(id) => {
                    if this.toast.value().is_some_and(|n| n.id == *id) {
                        this.dismiss(window, cx);
                    }
                }
                _ => {}
            });
            // Subscribed: the app replays the pending announcement.
            if !embedded {
                api(cx).overlay_ready();
            }
            Self { embedded, toast: Presence::new(None), paused: false, timer: None, gone: None, _sub: sub }
        })
    }

    fn arm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ex = cx.background_executor().clone();
        self.timer = Some(cx.spawn_in(window, async move |this, cx| {
            ex.timer(DURATION).await;
            this.update_in(cx, |this, window, cx| {
                if this.paused {
                    // The wait restarts when the pointer leaves.
                    this.timer = None;
                } else {
                    this.dismiss(window, cx);
                }
            })
            .ok();
        }));
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.timer = None;
        if self.toast.value().is_none() || self.toast.closing() {
            return;
        }
        Presence::set(self, |t| &mut t.toast, None, cx);
        let ex = cx.background_executor().clone();
        self.gone = Some(cx.spawn_in(window, async move |this, cx| {
            ex.timer(EXIT).await;
            this.update(cx, |this, cx| {
                if this.toast.value().is_none() && !this.embedded {
                    api(cx).toast_done();
                }
            })
            .ok();
        }));
    }
}

impl Render for ToastView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let Some(n) = self.toast.value().cloned() else { return div().absolute().into_any_element() };
        let closing = self.toast.closing();
        let message = match n.kind {
            ArchiveKind::SpaceDeleted => tr_args("sidebar.spaceDeleted", &[("name", n.name.as_str())]),
            ArchiveKind::Archived => tr_args("sidebar.archived", &[("count", &n.count.to_string())]),
            ArchiveKind::TabsClosed => tr_args("sidebar.tabsClosed", &[("count", &n.count.to_string())]),
        };
        let reduced = reduced_motion(cx);
        let id = n.id;
        let hover = t.sel;
        let (inset, bottom) = if self.embedded { (px(6.0), px(36.0) + space::S2) } else { (px(12.0), px(12.0)) };
        let body = div()
            .id(("toast", id))
            .absolute()
            .left(inset)
            .right(inset)
            .bottom(bottom)
            .flex()
            .items_center()
            .gap(space::S2)
            .py(space::S2)
            .pr(space::S2)
            .pl(space::S3)
            .rounded(radius::POP)
            .bg(t.glass_view)
            .border_1()
            .border_color(t.border)
            .shadow(t.shadow_pop.clone())
            .text_size(t.type_scale.sub)
            .text_color(t.text)
            .on_hover(cx.listener(|this, over: &bool, window, cx| {
                this.paused = *over;
                if !*over && !this.toast.closing() && this.toast.value().is_some() {
                    this.arm(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Escape, window, cx| this.dismiss(window, cx)))
            .child(div().flex_1().min_w_0().child(message))
            .child(
                div()
                    .id(("toast-undo", id))
                    .flex_none()
                    .px(space::S2)
                    .py(space::S1)
                    .rounded(radius::CONTROL)
                    .text_color(t.accent)
                    .font_weight(weight::STRONG)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        api(cx).undo_archive(id);
                        this.dismiss(window, cx);
                    }))
                    .child(tr("common.undo")),
            )
            .child(IconButton::new(("toast-close", id), IconName::X).small().icon_size(14.0).tooltip(tr("common.close")).on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx))));
        let body = surface_enter(body, ("toast-anim", id * 2 + u64::from(closing)), closing, reduced);
        // Embedded: only the card exists in the sidebar (no full-size layer that would block the rows).
        if self.embedded { body.into_any_element() } else { div().size_full().child(body).into_any_element() }
    }
}

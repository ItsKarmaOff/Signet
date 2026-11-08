//! `#overlay` route (port of `overlay/Overlay.tsx`): a transparent full-window surface that shows the command bar, the
//! popovers, the Peek frame, the capture tool, the password prompt and the zoom bubble, depending on the payload.

use super::capture::Capture;
use super::palette::Palette;
use super::password::{PasswordList, PasswordPrompt};
use super::peek::peek_frame;
use super::popover::PopoverView;
use super::site::SitePanel;
use super::webapp::WebAppInstall;
use crate::screens::tabs::DropZones;
use super::zoom::ZoomBubble;
use crate::app_state::{api, ui_state};
use crate::motion::{Presence, ease_out};
use crate::theme::duration::FAST;
use gpui::{
    Animation, AnimationExt, AnyElement, AppContext, Context, ElementId, Entity, InteractiveElement, IntoElement, Keystroke, MouseButton, ParentElement, Render, Styled, Subscription, Window, div,
};
use signet_core::api::{PasswordAnswer, UiEvent};
use signet_core::types::OverlayPayload;

#[derive(Clone)]
enum Child {
    Palette(Entity<Palette>),
    Popover(Entity<PopoverView>),
    Site(Entity<SitePanel>),
    Password(Entity<PasswordPrompt>),
    Passwords(Entity<PasswordList>),
    Capture(Entity<Capture>),
    Zoom(Entity<ZoomBubble>),
    WebApp(Entity<WebAppInstall>),
    DropZones(Entity<DropZones>),
    /// Peek frame and drop zones are drawn from the payload alone.
    Plain,
}

#[derive(Clone)]
struct Active {
    payload: OverlayPayload,
    child: Child,
    generation: u64,
}

/// Root view of the `#overlay` window. The app sizes the window (the whole main window, or just a card for the compact
/// password prompt and the zoom bubble) and calls nothing else: it shows and hides through `OverlayShow` / `OverlayHide`.
pub struct OverlayRoot {
    active: Presence<Active>,
    generation: u64,
    _events: Subscription,
    _keys: Subscription,
}

/// Modes whose exit is animated (the others go away right away).
fn animated(p: &OverlayPayload) -> bool {
    matches!(
        p,
        OverlayPayload::Palette { .. }
            | OverlayPayload::Site { .. }
            | OverlayPayload::Popover { .. }
            | OverlayPayload::Password { .. }
            | OverlayPayload::Passwords { .. }
            | OverlayPayload::Webapp { .. }
            | OverlayPayload::Zoom { .. }
            | OverlayPayload::Peek { .. }
    )
}

impl OverlayRoot {
    pub fn new(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let state = ui_state(cx);
        let handle = window.window_handle();
        cx.new(|cx: &mut Context<Self>| {
            let events = cx.subscribe_in(&state, window, |this: &mut Self, _, ev: &UiEvent, window, cx| match ev {
                UiEvent::OverlayShow(p) => this.show((**p).clone(), window, cx),
                UiEvent::OverlayHide { instant } => this.hide(*instant, cx),
                _ => {}
            });
            let weak = cx.weak_entity();
            let keys = cx.intercept_keystrokes(move |ev, window, cx| {
                if window.window_handle() != handle {
                    return;
                }
                let consumed = weak.update(cx, |this, cx| this.on_key(&ev.keystroke, window, cx)).unwrap_or(false);
                if consumed {
                    cx.stop_propagation();
                }
            });
            // Subscribed: the app sends the pending payload.
            api(cx).overlay_ready();
            Self { active: Presence::new(None), generation: 0, _events: events, _keys: keys }
        })
    }

    fn show(&mut self, p: OverlayPayload, window: &mut Window, cx: &mut Context<Self>) {
        // Identical reopening during the exit: cancel the exit and keep the surface (no remount).
        if self.active.closing()
            && let Some(a) = self.active.value().filter(|a| a.payload == p).cloned()
        {
            Presence::set(self, |t| &mut t.active, Some(a), cx);
            return;
        }
        // A new zoom value restarts the bubble's wait.
        if let (OverlayPayload::Zoom { tab_id, percent, default_percent, .. }, Some(a)) = (&p, self.active.value().cloned())
            && !self.active.closing()
            && let Child::Zoom(z) = &a.child
        {
            z.update(cx, |z, cx| z.set(tab_id.clone(), *percent, *default_percent, window, cx));
            Presence::set(self, |t| &mut t.active, Some(Active { payload: p, ..a }), cx);
            return;
        }
        // Drop zones are updated in place while a drag goes on.
        if let (OverlayPayload::Dropzones { area, split_card }, Some(a)) = (&p, self.active.value().cloned())
            && !self.active.closing()
            && let Child::DropZones(z) = &a.child
        {
            z.update(cx, |z, cx| z.set_payload(*area, *split_card, cx));
            Presence::set(self, |t| &mut t.active, Some(Active { payload: p, ..a }), cx);
            return;
        }
        self.generation += 1;
        let child = match &p {
            OverlayPayload::Palette { palette_mode, initial, hue, keywords, anchor } => Child::Palette(Palette::new(*palette_mode, initial, *hue, keywords.clone(), *anchor, window, cx)),
            OverlayPayload::Popover { popover, anchor, .. } => Child::Popover(PopoverView::new(popover.clone(), *anchor, window, cx)),
            OverlayPayload::Site { info, anchor } => Child::Site(SitePanel::new(info.clone(), *anchor, cx)),
            OverlayPayload::Password { token, origin, username, update, kind, anchor, .. } => {
                Child::Password(PasswordPrompt::new(token.clone(), origin.clone(), username.clone(), *update, *kind, *anchor, cx))
            }
            OverlayPayload::Passwords { origin, logins, anchor } => Child::Passwords(PasswordList::new(origin.clone(), logins.clone(), *anchor, cx)),
            OverlayPayload::Capture { tab_id, rect, hue } => Child::Capture(Capture::new(tab_id.clone(), *rect, *hue, cx)),
            OverlayPayload::Zoom { tab_id, percent, default_percent, .. } => Child::Zoom(ZoomBubble::new(tab_id.clone(), *percent, *default_percent, window, cx)),
            OverlayPayload::Webapp { token, name, url, icon, maskable, hue, card } => {
                Child::WebApp(WebAppInstall::new(token.clone(), name.clone(), url.clone(), icon.clone(), *maskable, *hue, *card, window, cx))
            }
            OverlayPayload::Dropzones { area, split_card } => Child::DropZones(cx.new(|_| DropZones::new(*area, *split_card))),
            OverlayPayload::Peek { .. } => Child::Plain,
        };
        let active = Active { payload: p, child, generation: self.generation };
        Presence::set(self, |t| &mut t.active, Some(active), cx);
    }

    fn hide(&mut self, instant: bool, cx: &mut Context<Self>) {
        let keep_exit = !instant && self.active.value().is_some_and(|a| animated(&a.payload));
        if keep_exit {
            Presence::set(self, |t| &mut t.active, None, cx);
        } else {
            self.active = Presence::new(None);
            cx.notify();
        }
    }

    /// Keys of the overlay window, before the focused input sees them. True when consumed.
    fn on_key(&mut self, ks: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.active.closing() {
            return false;
        }
        let Some(active) = self.active.value().cloned() else { return false };
        let plain = !(ks.modifiers.control || ks.modifiers.alt || ks.modifiers.platform);
        let escape = ks.key == "escape" && plain;
        match &active.child {
            Child::Palette(p) => {
                if p.update(cx, |p, cx| p.handle_key(ks, window, cx)) {
                    return true;
                }
            }
            Child::Popover(p) if escape => {
                if p.update(cx, |p, cx| p.handle_escape(cx)) {
                    return true;
                }
            }
            Child::Capture(c) if plain => {
                if c.update(cx, |c, cx| c.handle_key(&ks.key, window, cx)) {
                    return true;
                }
            }
            _ => {}
        }
        if escape {
            api(cx).close_overlay(false);
            return true;
        }
        false
    }
}

impl Render for OverlayRoot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let closing = self.active.closing();
        let Some(active) = self.active.value().cloned() else { return div().size_full().into_any_element() };
        let backdrop = |dismiss: Option<String>| -> AnyElement {
            div()
                .absolute()
                .size_full()
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    if let Some(token) = &dismiss {
                        api(cx).answer_password_prompt(token, PasswordAnswer::Dismiss);
                    }
                    api(cx).close_overlay(false);
                })
                .into_any_element()
        };
        let (back, body): (Option<AnyElement>, AnyElement) = match (&active.payload, &active.child) {
            (OverlayPayload::Palette { .. }, Child::Palette(p)) => (None, p.clone().into_any_element()),
            (OverlayPayload::Popover { .. }, Child::Popover(p)) => (Some(backdrop(None)), p.clone().into_any_element()),
            (OverlayPayload::Site { .. }, Child::Site(p)) => (Some(backdrop(None)), p.clone().into_any_element()),
            (OverlayPayload::Password { token, anchor, .. }, Child::Password(p)) => (anchor.map(|_| backdrop(Some(token.clone()))), p.clone().into_any_element()),
            (OverlayPayload::Passwords { .. }, Child::Passwords(p)) => (Some(backdrop(None)), p.clone().into_any_element()),
            (OverlayPayload::Webapp { .. }, Child::WebApp(p)) => (Some(backdrop(None)), p.clone().into_any_element()),
            (OverlayPayload::Dropzones { .. }, Child::DropZones(p)) => (None, p.clone().into_any_element()),
            (OverlayPayload::Capture { .. }, Child::Capture(p)) => (None, p.clone().into_any_element()),
            (OverlayPayload::Zoom { .. }, Child::Zoom(p)) => (None, p.clone().into_any_element()),
            (OverlayPayload::Peek { card, title, url, .. }, _) => (None, peek_frame(*card, title, url, cx)),
            _ => (None, div().into_any_element()),
        };
        let generation = active.generation;
        let layer = div().absolute().size_full().children(back).child(body).with_animation(
            ElementId::from(("overlay-layer", generation * 2 + u64::from(closing))),
            Animation::new(FAST).with_easing(ease_out),
            move |el, d| el.opacity(if closing { 1.0 - d } else { d }),
        );
        let root = div().size_full().child(layer);
        // A click during the exit targets the page behind: remove the view right away.
        if closing {
            root.capture_any_mouse_down(|_, _, cx| api(cx).close_overlay(true)).into_any_element()
        } else {
            root.into_any_element()
        }
    }
}

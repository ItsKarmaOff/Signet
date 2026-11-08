//! Little Signet and private windows (`main/windows/little.ts`, `private.ts`): GPUI windows that host their own CEF
//! views. The core (`phase3/windows.rs`) decides what they contain; this file only opens, closes and reports them.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyView, AnyWindowHandle, App, AppContext as _, Bounds, Context, IntoElement, ParentElement as _, Render, Styled as _, Subscription, Window, WindowBounds, WindowOptions, div, point, px, size,
};
use gpui_component::Root;
use raw_window_handle::HasWindowHandle as _;

use crate::app::platform::window_chrome;
use crate::browser::Browser;

/// Offset between two windows of the same kind (px).
pub const CASCADE: f32 = 24.0;

#[derive(Clone, Debug, PartialEq)]
pub enum AuxKind {
    Little,
    /// `vertical`: tabs in a sidebar (traffic lights lower).
    Private { vertical: bool },
    /// Installed web app (`windows/webapp` of the renderer): `bounds` remembered frame (x, y, w, h), `cascade`
    /// shifts it from the previous window of the same app.
    WebApp { title: String, bounds: Option<(f32, f32, f32, f32)>, cascade: bool },
}

impl AuxKind {
    /// Initial size, minimum size and traffic light position.
    pub fn frame(&self) -> ((f32, f32), Option<(f32, f32)>, (f32, f32)) {
        match self {
            AuxKind::Little => ((760.0, 820.0), None, (14.0, 14.0)),
            AuxKind::Private { vertical } => ((1100.0, 780.0), Some((520.0, 360.0)), if *vertical { (16.0, 16.0) } else { (16.0, 13.0) }),
            AuxKind::WebApp { bounds, .. } => (bounds.map_or((1060.0, 760.0), |b| (b.2, b.3)), Some((380.0, 300.0)), (14.0, 12.0)),
        }
    }
}

/// Root of an auxiliary window: reports its size and activation to the core.
pub struct AuxHost {
    id: u64,
    child: AnyView,
    browser: Rc<Browser>,
    last: Option<(f64, f64)>,
    last_frame: Option<(i32, i32, i32, i32)>,
    web_app: bool,
    _activation: Subscription,
}

impl AuxHost {
    fn new(id: u64, child: AnyView, browser: Rc<Browser>, web_app: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let activation = cx.observe_window_activation(window, |this, window, _| {
            let active = window.is_window_active();
            if let Ok(mut c) = this.browser.core.try_borrow_mut() {
                c.on_aux_active(this.id, active);
            }
        });
        Self { id, child, browser, last: None, last_frame: None, web_app, _activation: activation }
    }
}

impl Render for AuxHost {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let cur = (f64::from(f32::from(vp.width)), f64::from(f32::from(vp.height)));
        if self.last != Some(cur) {
            self.last = Some(cur);
            if let Ok(mut c) = self.browser.core.try_borrow_mut() {
                c.on_aux_resized(self.id, cur);
            }
        }
        if self.web_app {
            // Frame on screen, remembered for the next launch of the app.
            let b = window.bounds();
            let f = (f32::from(b.origin.x).round() as i32, f32::from(b.origin.y).round() as i32, f32::from(b.size.width).round() as i32, f32::from(b.size.height).round() as i32);
            if self.last_frame != Some(f) {
                self.last_frame = Some(f);
                if let Ok(mut c) = self.browser.core.try_borrow_mut() {
                    c.web_app_window_bounds(self.id, signet_core::types::Rect::new(f64::from(f.0), f64::from(f.1), f64::from(f.2), f64::from(f.3)));
                }
            }
        }
        crate::quitkeys::gpui_handlers(div()).size_full().line_height(gpui::relative(signet_ui::theme::LINE_HEIGHT)).child(self.child.clone())
    }
}

/// The open auxiliary windows (owned by the window manager).
#[derive(Default)]
pub struct AuxWindows {
    wins: HashMap<u64, AnyWindowHandle>,
    last: Option<(f32, f32)>,
}

impl AuxWindows {
    pub fn open(&mut self, browser: &Rc<Browser>, id: u64, kind: AuxKind, cx: &mut App) {
        self.wins.retain(|_, h| h.update(cx, |_, _, _| ()).is_ok());
        if self.wins.is_empty() {
            self.last = None;
        }
        let ((w, h), min, lights) = kind.frame();
        let title = match &kind {
            AuxKind::Little => "Signet".to_string(),
            AuxKind::Private { .. } => signet_core::i18n::t("private.badge"),
            AuxKind::WebApp { title, .. } => title.clone(),
        };
        let (titlebar, background) = window_chrome(&title, Some(lights), true);
        let remembered = match &kind {
            AuxKind::WebApp { bounds: Some(b), cascade, .. } => Some((b.0 + if *cascade { CASCADE } else { 0.0 }, b.1 + if *cascade { CASCADE } else { 0.0 })),
            _ => None,
        };
        let bounds = match remembered.or(self.last) {
            Some((x, y)) if remembered.is_some() => Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
            Some((x, y)) => Bounds::new(point(px(x + CASCADE), px(y + CASCADE)), size(px(w), px(h))),
            None => Bounds::centered(None, size(px(w), px(h)), cx),
        };
        let web_app = matches!(kind, AuxKind::WebApp { .. });
        let origin = std::cell::Cell::new(None::<(f32, f32)>);
        let b = browser.clone();
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar,
                window_background: background,
                focus: true,
                show: true,
                window_min_size: min.map(|(mw, mh)| size(px(mw), px(mh))),
                ..Default::default()
            },
            |window, cx| {
                let raw = window.window_handle().ok().map(|h| h.as_raw());
                let vp = window.viewport_size();
                let o = window.bounds().origin;
                origin.set(Some((f32::from(o.x), f32::from(o.y))));
                if let Ok(mut c) = b.core.try_borrow_mut() {
                    c.on_aux_created(id, raw, (f64::from(f32::from(vp.width)), f64::from(f32::from(vp.height))));
                }
                let child: AnyView = match &kind {
                    AuxKind::Little => signet_ui::screens::windows::little::Little::new(window, cx).into(),
                    AuxKind::Private { .. } => signet_ui::screens::windows::private::Private::new(window, cx).into(),
                    AuxKind::WebApp { .. } => signet_ui::screens::windows::webapp::WebApp::new(window, cx).into(),
                };
                let host = cx.new(|cx| AuxHost::new(id, child, b.clone(), web_app, window, cx));
                cx.new(|cx| Root::new(host, window, cx))
            },
        );
        match opened {
            Ok(handle) => {
                if !web_app {
                    self.last = origin.get().or(self.last);
                }
                let b = browser.clone();
                handle
                    .update(cx, |_, w, cx| {
                        w.on_window_should_close(cx, move |_, _| {
                            if let Ok(mut c) = b.core.try_borrow_mut() {
                                c.on_aux_closed(id);
                            }
                            true
                        });
                    })
                    .ok();
                self.wins.insert(id, handle.into());
            }
            Err(e) => {
                eprintln!("cannot open the auxiliary window {id}: {e}");
                if let Ok(mut c) = browser.core.try_borrow_mut() {
                    c.on_aux_closed(id);
                }
            }
        }
        cx.activate(true);
    }

    pub fn close(&mut self, id: u64, cx: &mut App) {
        if let Some(h) = self.wins.remove(&id) {
            let _ = h.update(cx, |_, w, _| w.remove_window());
        }
    }

    pub fn focus(&mut self, id: u64, cx: &mut App) {
        if let Some(h) = self.wins.get(&id) {
            let _ = h.update(cx, |_, w, _| w.activate_window());
        }
    }
}

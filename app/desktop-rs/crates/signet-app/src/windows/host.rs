//! The main window's root: wraps the main screen and tells the browser about size and position changes (the layout
//! of the CEF views depends on them).

use std::rc::Rc;

use gpui::{AnyView, Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div};

use crate::browser::Browser;

/// Root wrapper that sets the default line height of the interface (`signet_ui::theme::LINE_HEIGHT`).
pub struct LineHeight(pub AnyView);

impl Render for LineHeight {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().line_height(gpui::relative(signet_ui::theme::LINE_HEIGHT)).child(self.0.clone())
    }
}

pub struct MainHost {
    pub child: AnyView,
    pub browser: Rc<Browser>,
    last: Option<(f64, f64, f64, f64)>,
}

impl MainHost {
    pub fn new(child: AnyView, browser: Rc<Browser>) -> Self {
        Self { child, browser, last: None }
    }
}

impl Render for MainHost {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let b = window.bounds();
        let cur = (f64::from(f32::from(vp.width)), f64::from(f32::from(vp.height)), f64::from(f32::from(b.origin.x)), f64::from(f32::from(b.origin.y)));
        if self.last != Some(cur) {
            self.last = Some(cur);
            self.browser.core.borrow_mut().on_main_resized((cur.0, cur.1), (cur.2, cur.3));
        }
        div().size_full().child(self.child.clone())
    }
}

/// Root of the `#devtools` window: tells the core the content size, so the frontend view follows the window.
pub struct ToolsHost {
    child: AnyView,
    browser: Rc<Browser>,
    last: Option<(f64, f64)>,
}

impl ToolsHost {
    pub fn new(child: AnyView, browser: Rc<Browser>) -> Self {
        Self { child, browser, last: None }
    }
}

impl Render for ToolsHost {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let cur = (f64::from(f32::from(vp.width)), f64::from(f32::from(vp.height)));
        if self.last != Some(cur) {
            self.last = Some(cur);
            if let Ok(mut c) = self.browser.core.try_borrow_mut() {
                c.devtools_window_resized(cur);
            }
        }
        div().size_full().child(self.child.clone())
    }
}

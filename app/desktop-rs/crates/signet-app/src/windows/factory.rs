//! Maps a surface to the root view of its screen (`signet_ui::screens::*`). A screen that is not available yet gets a
//! small placeholder so the window still opens (marked `// SCREEN:`).

use gpui::{AnyView, App, AppContext as _, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div, px};

use super::{Surface, Win, host};

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Main,
    Floating(Surface),
    Win(Win),
}

struct Placeholder {
    title: SharedString,
}

impl Render for Placeholder {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p(px(12.0)).bg(gpui::rgba(0x1e1e2ecc)).text_color(gpui::rgb(0xcdd6f4)).child(self.title.clone())
    }
}

fn placeholder(name: &str, window: &mut Window, cx: &mut App) -> AnyView {
    let _ = window;
    cx.new(|_| Placeholder { title: format!("{name} (screen not wired)").into() }).into()
}

/// Builds the root view of a surface.
pub fn build(kind: Kind, window: &mut Window, cx: &mut App) -> AnyView {
    let main = matches!(kind, Kind::Main);
    let view = build_inner(kind, window, cx);
    if main { view } else { with_line_height(view, cx) }
}

/// Wraps a root view so every text inside inherits the interface's default line height.
pub fn with_line_height(view: AnyView, cx: &mut App) -> AnyView {
    cx.new(|_| host::LineHeight(view)).into()
}

fn build_inner(kind: Kind, window: &mut Window, cx: &mut App) -> AnyView {
    use signet_ui::screens::{main_window, overlay, settings, tabs, windows};
    match kind {
        Kind::Main => main_window::MainWindow::new(window, cx).into(),
        Kind::Floating(s) => match s {
            Surface::Overlay => overlay::OverlayRoot::new(window, cx).into(),
            Surface::Sidebar => main_window::FloatingSidebar::new(window, cx).into(),
            Surface::Toolbar => main_window::FloatingToolbar::new(window, cx).into(),
            Surface::Find => overlay::FindBar::new(window, cx).into(),
            Surface::Toast => overlay::ToastView::new(window, cx).into(),
            Surface::Preview => tabs::preview::new_view(window, cx).into(),
            Surface::Quit => overlay::QuitHud::new(window, cx).into(),
        },
        Kind::Win(w) => match w {
            Win::Settings => settings::SettingsWindow::new(window, cx).into(),
            Win::Library => windows::library::Library::new(window, cx).into(),
            Win::Welcome => windows::welcome::Welcome::new(window, cx).into(),
            Win::Passkey => windows::passkey::Passkey::new(window, cx).into(),
            Win::Boost => overlay::BoostWindow::new(window, cx).into(),
            Win::Devtools => signet_ui::screens::devtools::DevToolsWindow::new(window, cx).into(),
        },
    }
}

#[allow(dead_code)]
fn _placeholder(name: &str, window: &mut Window, cx: &mut App) -> AnyView {
    placeholder(name, window, cx)
}

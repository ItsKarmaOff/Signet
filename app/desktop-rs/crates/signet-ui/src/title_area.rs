//! Window title area: the parts of a window the user moves it by, with the system's double-click action.
//!
//! macOS and Linux: a left press arms a move that starts on the first mouse move with the button down (so a click
//! or a double-click never starts a native move that would swallow the second press); a double-click (`click_count`
//! 2) runs the system action (zoom, fill, minimize, window menu, nothing). Windows: no handler, the hit test answers
//! `HTCAPTION` and `DefWindowProc` toggles maximize natively.

use gpui::{InteractiveElement, MouseButton, StatefulInteractiveElement, WindowControlArea};
#[cfg(not(target_os = "windows"))]
use gpui::{App, Pixels, Point, Window};

#[cfg(not(target_os = "windows"))]
use crate::app_state::api;

#[cfg(not(target_os = "windows"))]
static ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Makes the element a title area (`-webkit-app-region: drag`). Children that are buttons must stop the press with
/// [`no_drag`] (or occlude).
pub fn title_area<E: InteractiveElement>(el: E) -> E {
    let el = el.window_control_area(WindowControlArea::Drag);
    #[cfg(not(target_os = "windows"))]
    let el = {
        use std::sync::atomic::Ordering::Relaxed;
        el.on_mouse_down(MouseButton::Left, |e, window, cx| match e.click_count {
            1 => ARMED.store(true, Relaxed),
            2 => {
                ARMED.store(false, Relaxed);
                title_double_click(e.position, window, cx);
            }
            _ => ARMED.store(false, Relaxed),
        })
        .on_mouse_move(|e, window, _| {
            if ARMED.load(Relaxed) && e.pressed_button == Some(MouseButton::Left) {
                ARMED.store(false, Relaxed);
                window.start_window_move();
            }
        })
        .on_mouse_up(MouseButton::Left, |_, _, _| ARMED.store(false, Relaxed))
    };
    el
}

/// Makes a stateful element a button that stops the press from reaching a title area behind it.
pub fn no_drag<E: StatefulInteractiveElement>(el: E) -> E {
    el.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// Runs the system's double-click action on the window. Does nothing in full screen.
#[cfg(not(target_os = "windows"))]
fn title_double_click(position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    use signet_core::titlebar::TitlebarAction as A;
    if window.is_fullscreen() {
        return;
    }
    let action = api(cx).titlebar_double_click_action();
    // Outside the event dispatch: the window may resize or minimize under the handler.
    window.defer(cx, move |window, _| match action {
        A::Zoom if window.is_resizable() => window.zoom_window(),
        A::Fill if cfg!(target_os = "macos") => window.titlebar_double_click(),
        A::Fill if window.is_resizable() => window.zoom_window(),
        A::Minimize if window.is_minimizable() => window.minimize_window(),
        A::Menu => window.show_window_menu(position),
        _ => {}
    });
}

//! Keyboard hand-off between GPUI elements and the native CEF views.
//!
//! GPUI's focus is a purely internal notion: giving it to an input (a form that opens, an inline rename) does not
//! change which native view the OS sends key events to. When a page had the keyboard, the CEF view keeps it, so the
//! caret blinks in the GPUI input while the keys go to the page (macOS beeps). Mouse clicks on GPUI areas are
//! handled by the platform layer (`signet-web/src/mac.rs`); this watcher covers every focus change that has no click
//! behind it, in all windows, without any code in the screens:
//!
//! - GPUI focus lands on another element: the GPUI window gets the native keyboard focus (macOS `makeFirstResponder:`
//!   on the `GPUIView`, Win32 `SetFocus` on the window, X11 `XSetInputFocus`);
//! - a text input of the main window stops being focused (form closed, rename done or cancelled) while the GPUI
//!   view owns the keyboard: the active page gets it back (macOS only for now: the check needs the platform text
//!   input state, other platforms keep the keyboard on the GPUI side until the page is clicked).
//!
//! A click on the page needs nothing: the CEF view takes the native focus itself and the GPUI focus stays stale
//! until the next change.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::{AnyView, AnyWindowHandle, App, AppContext as _, WeakFocusHandle, WindowId};
use raw_window_handle::HasWindowHandle;

use crate::browser::Browser;

/// Poll period (ms): short enough that the first key stroke after a form opens already goes to GPUI.
const POLL_MS: u64 = 25;

#[derive(Default)]
struct Seen {
    focus: Option<WeakFocusHandle>,
    text: bool,
}

pub fn install(browser: &Rc<Browser>, cx: &mut App) {
    let browser = browser.clone();
    cx.spawn(async move |cx| {
        let mut seen: HashMap<WindowId, Seen> = HashMap::new();
        loop {
            cx.background_executor().timer(Duration::from_millis(POLL_MS)).await;
            let handles: Vec<AnyWindowHandle> = cx.update(|cx| cx.windows());
            seen.retain(|id, _| handles.iter().any(|h| h.window_id() == *id));
            for h in handles {
                let Ok((focus, raw)) = cx.update_window(h, |_: AnyView, window, cx| (window.focused(cx).map(|f| f.downgrade()), window.window_handle().ok().map(|w| w.as_raw()))) else {
                    continue;
                };
                let Some(raw) = raw else { continue };
                let text = signet_web::overlay::gpui_text_input_active(raw);
                let Some(prev) = seen.insert(h.window_id(), Seen { focus: focus.clone(), text }) else { continue };
                let changed = match (&prev.focus, &focus) {
                    (None, None) => false,
                    (Some(x), Some(y)) => x != y,
                    _ => true,
                };
                if changed && focus.is_some() {
                    signet_web::overlay::focus_parent_window(raw);
                } else if prev.text && !text && signet_web::overlay::gpui_has_keyboard(raw) {
                    // The main window only: the other windows have no page below their inputs.
                    if let Ok(mut core) = browser.core.try_borrow_mut()
                        && core.views.parent == Some(raw)
                    {
                        core.request_focus();
                        core.changed_view();
                    }
                }
            }
        }
    })
    .detach();
}

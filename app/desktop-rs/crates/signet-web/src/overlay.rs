//! Overlay windows above the native CEF views, and keyboard focus hand-off.
//!
//! Native CEF views are drawn above every GPUI element of their window, so anything that must float over a
//! page (command bar, popovers, toasts, mini player) is its own transparent GPUI window, attached to the
//! main window as a child (macOS), an owned window (Windows) or a transient window (X11). Handles come
//! from GPUI: `window.window_handle()?.as_raw()`.
//!
//! * macOS: `addChildWindow:ordered:` (drawn above the parent's content, moves and hides with it).
//! * Windows: the overlay gets the parent as owner (`GWLP_HWNDPARENT`): an owned window always stays above its
//!   owner and is minimized with it. Untested.
//! * Linux (X11 or XWayland): `XSetTransientForHint`. Untested. Wayland surfaces cannot be parented.

use raw_window_handle::RawWindowHandle;

/// Attach `child` above `parent` (see the module doc). Call it once the child window exists.
pub fn attach_child_window(parent: RawWindowHandle, child: RawWindowHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::mac::attach_child_window(parent, child)
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWLP_HWNDPARENT};
        let (p, c) = (hwnd(parent)?, hwnd(child)?);
        unsafe { SetWindowLongPtrW(c, GWLP_HWNDPARENT, p as isize) };
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        x11::set_transient(xwindow(child)?, Some(xwindow(parent)?))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (parent, child);
        Err("overlay windows are not supported on this platform".into())
    }
}

/// Undo [`attach_child_window`].
pub fn detach_child_window(parent: RawWindowHandle, child: RawWindowHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::mac::detach_child_window(parent, child)
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWLP_HWNDPARENT};
        let _ = parent;
        unsafe { SetWindowLongPtrW(hwnd(child)?, GWLP_HWNDPARENT, 0) };
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let _ = parent;
        x11::set_transient(xwindow(child)?, None)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (parent, child);
        Err("overlay windows are not supported on this platform".into())
    }
}

/// Let mouse events fall through `window` to what is below it (pass-through surfaces: toasts, hover previews).
pub fn set_click_through(window: RawWindowHandle, on: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::mac::set_click_through(window, on)
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let h = hwnd(window)?;
        unsafe {
            let style = GetWindowLongPtrW(h, GWL_EXSTYLE);
            let flags = (WS_EX_LAYERED | WS_EX_TRANSPARENT) as isize;
            if on {
                SetWindowLongPtrW(h, GWL_EXSTYLE, style | flags);
                SetLayeredWindowAttributes(h, 0, 255, LWA_ALPHA);
            } else {
                SetWindowLongPtrW(h, GWL_EXSTYLE, style & !(WS_EX_TRANSPARENT as isize));
            }
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        x11::set_click_through(xwindow(window)?, on)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (window, on);
        Err("not supported on this platform".into())
    }
}

/// Make the window a transparent, shadowless overlay (macOS: not opaque, clear background; elsewhere the
/// GPUI window options decide, so this is a no-op).
pub fn configure_overlay_window(window: RawWindowHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::mac::configure_overlay_window(window)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(())
    }
}

/// Bring an overlay above its siblings.
pub fn raise_window(window: RawWindowHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::mac::raise_window(window)
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        unsafe { SetWindowPos(hwnd(window)?, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        x11::raise(xwindow(window)?)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = window;
        Ok(())
    }
}

/// Whether the GPUI view of the window of `parent` owns the native keyboard focus (macOS only, false elsewhere).
pub fn gpui_has_keyboard(parent: RawWindowHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        if let RawWindowHandle::AppKit(h) = parent {
            return crate::mac::gpui_has_keyboard(h.ns_view.as_ptr());
        }
    }
    let _ = parent;
    false
}

/// Whether the GPUI window of `parent` has a focused text input (macOS only, false elsewhere).
pub fn gpui_text_input_active(parent: RawWindowHandle) -> bool {
    #[cfg(target_os = "macos")]
    {
        if let RawWindowHandle::AppKit(h) = parent {
            return crate::mac::gpui_text_input_active(h.ns_view.as_ptr());
        }
    }
    let _ = parent;
    false
}

/// Give the keyboard back to the GPUI window of `parent` (the window the CEF views are children of):
/// the page loses the caret and key events go to GPUI. Clicks on GPUI areas do it automatically on
/// macOS (see `mac.rs`); call this when GPUI wants the keyboard without a click (a shortcut that opens
/// the command bar, for example).
pub fn focus_parent_window(parent: RawWindowHandle) {
    #[cfg(target_os = "macos")]
    {
        if let RawWindowHandle::AppKit(h) = parent {
            crate::mac::focus_parent_view(h.ns_view.as_ptr());
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(h) = hwnd(parent) {
            unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus(h) };
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(w) = xwindow(parent) {
            x11::focus(w);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = parent;
    }
}

#[cfg(target_os = "windows")]
fn hwnd(h: RawWindowHandle) -> Result<windows_sys::Win32::Foundation::HWND, String> {
    match h {
        RawWindowHandle::Win32(h) => Ok(h.hwnd.get() as windows_sys::Win32::Foundation::HWND),
        other => Err(format!("not a Win32 handle: {other:?}")),
    }
}

#[cfg(target_os = "linux")]
fn xwindow(h: RawWindowHandle) -> Result<std::os::raw::c_ulong, String> {
    match h {
        RawWindowHandle::Xlib(h) => Ok(h.window as std::os::raw::c_ulong),
        RawWindowHandle::Xcb(h) => Ok(h.window.get() as std::os::raw::c_ulong),
        other => Err(format!("overlay windows need X11 or XWayland, got {other:?}")),
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use std::os::raw::c_ulong;

    fn with_display<R>(f: impl FnOnce(&x11_dl::xlib::Xlib, *mut x11_dl::xlib::Display) -> R) -> Result<R, String> {
        let xlib = x11_dl::xlib::Xlib::open().map_err(|e| e.to_string())?;
        unsafe {
            let dpy = (xlib.XOpenDisplay)(std::ptr::null());
            if dpy.is_null() {
                return Err("cannot open the X display".into());
            }
            let r = f(&xlib, dpy);
            (xlib.XFlush)(dpy);
            (xlib.XCloseDisplay)(dpy);
            Ok(r)
        }
    }

    pub fn set_transient(child: c_ulong, parent: Option<c_ulong>) -> Result<(), String> {
        with_display(|x, d| unsafe {
            // Transient-for with the parent id, or the root-less value 0 to clear it.
            (x.XSetTransientForHint)(d, child, parent.unwrap_or(0));
        })
    }

    pub fn raise(w: c_ulong) -> Result<(), String> {
        with_display(|x, d| unsafe {
            (x.XRaiseWindow)(d, w);
        })
    }

    pub fn focus(w: c_ulong) {
        let _ = with_display(|x, d| unsafe {
            // RevertToParent = 2, CurrentTime = 0
            (x.XSetInputFocus)(d, w, 2, 0);
        });
    }

    /// Empty input shape = click-through; `None` region restores the default.
    pub fn set_click_through(w: c_ulong, on: bool) -> Result<(), String> {
        let xfixes = x11_dl::xfixes::Xlib::open().map_err(|e| e.to_string())?;
        with_display(|_, d| unsafe {
            const SHAPE_INPUT: i32 = 2;
            if on {
                let region = (xfixes.XFixesCreateRegion)(d, std::ptr::null_mut(), 0);
                (xfixes.XFixesSetWindowShapeRegion)(d, w, SHAPE_INPUT, 0, 0, region);
                (xfixes.XFixesDestroyRegion)(d, region);
            } else {
                (xfixes.XFixesSetWindowShapeRegion)(d, w, SHAPE_INPUT, 0, 0, 0);
            }
        })
    }
}

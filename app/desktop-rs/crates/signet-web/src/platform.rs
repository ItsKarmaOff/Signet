//! Native child view placement per platform. All coordinates are window points, top-left origin.

use crate::types::Bounds;
use cef::ImplBrowserHost;

/// Convert a raw-window-handle parent into the integer used internally.
pub(crate) fn parent_to_usize(h: raw_window_handle::RawWindowHandle) -> Result<usize, String> {
    use raw_window_handle::RawWindowHandle as R;
    match h {
        #[cfg(target_os = "macos")]
        R::AppKit(h) => Ok(h.ns_view.as_ptr() as usize),
        #[cfg(target_os = "windows")]
        R::Win32(h) => Ok(h.hwnd.get() as usize),
        #[cfg(target_os = "linux")]
        R::Xlib(h) => Ok(h.window as usize),
        #[cfg(target_os = "linux")]
        R::Xcb(h) => Ok(h.window.get() as usize),
        #[cfg(target_os = "linux")]
        R::Wayland(_) => Err("Wayland surfaces cannot host CEF child windows; run under XWayland (GDK_BACKEND=x11 style, WAYLAND_DISPLAY unset)".into()),
        other => Err(format!("unsupported window handle: {other:?}")),
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn native_parent(parent: usize) -> *mut std::ffi::c_void {
    parent as *mut std::ffi::c_void
}
#[cfg(target_os = "windows")]
pub(crate) fn native_parent(parent: usize) -> cef::sys::cef_window_handle_t {
    cef::sys::HWND(parent as *mut _)
}
#[cfg(target_os = "linux")]
pub(crate) fn native_parent(parent: usize) -> cef::sys::cef_window_handle_t {
    parent as _
}

/// Takes the native view of a closing browser out of its parent. `DoClose` returns true (the app owns the window), so
/// CEF waits for the view to go away before it calls `OnBeforeClose`; without this the browser lingers until
/// `cef::shutdown` (seconds at quit).
pub(crate) fn destroy_native(host: &cef::BrowserHost) {
    #[cfg(target_os = "macos")]
    {
        let handle = host.window_handle();
        if !handle.is_null() {
            crate::mac::remove_view(handle);
        }
    }
    #[cfg(target_os = "windows")]
    {
        let handle = host.window_handle();
        if !handle.0.is_null() {
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(handle.0 as windows_sys::Win32::Foundation::HWND) };
        }
    }
    #[cfg(target_os = "linux")]
    {
        // The browser window is an X11 child of the app window: unmap then destroy it.
        let handle = host.window_handle();
        if handle != 0 {
            if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
                unsafe {
                    let dpy = (xlib.XOpenDisplay)(std::ptr::null());
                    if !dpy.is_null() {
                        let w = handle as std::os::raw::c_ulong;
                        (xlib.XUnmapWindow)(dpy, w);
                        (xlib.XDestroyWindow)(dpy, w);
                        (xlib.XFlush)(dpy);
                        (xlib.XCloseDisplay)(dpy);
                    }
                }
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = host;
}

/// Place and show/hide the native view of a browser.
pub(crate) fn apply(host: &cef::BrowserHost, parent: usize, b: Bounds, visible: bool) {
    use cef::*;
    let handle = host.window_handle();
    #[cfg(target_os = "macos")]
    {
        if handle.is_null() {
            return;
        }
        crate::mac::set_view_frame(handle, parent as *mut _, b.x, b.y, b.width, b.height);
        crate::mac::set_view_hidden(handle, !visible);
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let hwnd = handle.0 as windows_sys::Win32::Foundation::HWND;
        if hwnd.is_null() {
            return;
        }
        let dpi = unsafe { GetDpiForWindow(parent as windows_sys::Win32::Foundation::HWND) };
        let s = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                (b.x * s).round() as i32,
                (b.y * s).round() as i32,
                (b.width * s).round() as i32,
                (b.height * s).round() as i32,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
        let _ = host;
    }
    #[cfg(target_os = "linux")]
    {
        // X11 window coordinates are in pixels; GPUI reports points, scale with GDK_SCALE if set.
        let scale: f32 = std::env::var("GDK_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
        let _ = parent;
        if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
            unsafe {
                let dpy = (xlib.XOpenDisplay)(std::ptr::null());
                if !dpy.is_null() {
                    let w = handle as std::os::raw::c_ulong;
                    if visible {
                        (xlib.XMoveResizeWindow)(
                            dpy,
                            w,
                            (b.x * scale) as i32,
                            (b.y * scale) as i32,
                            (b.width * scale).max(1.0) as u32,
                            (b.height * scale).max(1.0) as u32,
                        );
                        (xlib.XMapWindow)(dpy, w);
                    } else {
                        (xlib.XUnmapWindow)(dpy, w);
                    }
                    (xlib.XFlush)(dpy);
                    (xlib.XCloseDisplay)(dpy);
                }
            }
        }
    }
    let _ = (host, parent, b, visible, handle);
}

/// Moves the browser's native view under another parent window (coordinates are applied afterwards by [`apply`]).
pub(crate) fn reparent(host: &cef::BrowserHost, parent: usize, b: Bounds) {
    use cef::ImplBrowserHost as _;
    let handle = host.window_handle();
    #[cfg(target_os = "macos")]
    {
        if !handle.is_null() {
            crate::mac::reparent_view(handle, parent as *mut _);
        }
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::SetParent;
        let hwnd = handle.0 as windows_sys::Win32::Foundation::HWND;
        if !hwnd.is_null() {
            unsafe { SetParent(hwnd, parent as windows_sys::Win32::Foundation::HWND) };
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(xlib) = x11_dl::xlib::Xlib::open() {
            unsafe {
                let dpy = (xlib.XOpenDisplay)(std::ptr::null());
                if !dpy.is_null() {
                    (xlib.XReparentWindow)(dpy, handle as std::os::raw::c_ulong, parent as std::os::raw::c_ulong, b.x as i32, b.y as i32);
                    (xlib.XFlush)(dpy);
                    (xlib.XCloseDisplay)(dpy);
                }
            }
        }
    }
    let _ = (host, parent, b, handle);
}

/// Give keyboard focus to the browser's native view.
pub(crate) fn focus(host: &cef::BrowserHost) {
    #[cfg(target_os = "macos")]
    {
        let h = host.window_handle();
        if !h.is_null() {
            crate::mac::focus_view(h);
        }
    }
    host.set_focus(1);
}

/// Native views (`NSView*`) of every live app browser; used by the macOS focus hand-off.
#[cfg(target_os = "macos")]
pub(crate) fn native_view_handles() -> Vec<*mut std::ffi::c_void> {
    use cef::ImplBrowser as _;
    crate::runtime::with_state(|st| {
        st.views.values().filter_map(|e| e.browser.as_ref().and_then(|b| b.host()).map(|h| h.window_handle())).filter(|h| !h.is_null()).collect()
    })
    .unwrap_or_default()
}

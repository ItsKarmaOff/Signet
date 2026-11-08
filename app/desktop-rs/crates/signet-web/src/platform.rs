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

// ---------------------------------------------------------------------------------------------
// Test helpers (self-test): what the system says about a browser's native window. Linux and Windows.
// ---------------------------------------------------------------------------------------------

/// Native window id of the browser as an integer (X11 window, HWND), 0 without one.
#[cfg(not(target_os = "macos"))]
pub(crate) fn native_id(host: &cef::BrowserHost) -> usize {
    #[cfg(target_os = "windows")]
    {
        host.window_handle().0 as usize
    }
    #[cfg(not(target_os = "windows"))]
    {
        host.window_handle() as usize
    }
}

/// (x, y, width, height, hidden, parent width, parent height) in window points, asked to the system.
#[cfg(target_os = "linux")]
pub(crate) fn native_geometry(id: usize) -> Option<(f64, f64, f64, f64, bool, f64, f64)> {
    if id == 0 {
        return None;
    }
    let xlib = x11_dl::xlib::Xlib::open().ok()?;
    let scale: f64 = std::env::var("GDK_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    unsafe {
        let dpy = (xlib.XOpenDisplay)(std::ptr::null());
        if dpy.is_null() {
            return None;
        }
        let mut a: x11_dl::xlib::XWindowAttributes = std::mem::zeroed();
        let ok = (xlib.XGetWindowAttributes)(dpy, id as std::os::raw::c_ulong, &mut a) != 0;
        let mut root: std::os::raw::c_ulong = 0;
        let mut parent: std::os::raw::c_ulong = 0;
        let mut kids: *mut std::os::raw::c_ulong = std::ptr::null_mut();
        let mut n: std::os::raw::c_uint = 0;
        let mut pa: x11_dl::xlib::XWindowAttributes = std::mem::zeroed();
        if ok && (xlib.XQueryTree)(dpy, id as std::os::raw::c_ulong, &mut root, &mut parent, &mut kids, &mut n) != 0 {
            if !kids.is_null() {
                (xlib.XFree)(kids as *mut _);
            }
            if parent != 0 {
                (xlib.XGetWindowAttributes)(dpy, parent, &mut pa);
            }
        }
        (xlib.XCloseDisplay)(dpy);
        if !ok {
            crate::app::rlog(&format!("native_geometry: XGetWindowAttributes failed for {id:#x}"));
            return None;
        }
        let hidden = a.map_state != x11_dl::xlib::IsViewable;
        Some((a.x as f64 / scale, a.y as f64 / scale, a.width as f64 / scale, a.height as f64 / scale, hidden, pa.width as f64 / scale, pa.height as f64 / scale))
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn native_geometry(id: usize) -> Option<(f64, f64, f64, f64, bool, f64, f64)> {
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if id == 0 {
        return None;
    }
    unsafe {
        let hwnd = id as HWND;
        if IsWindow(hwnd) == 0 {
            return None;
        }
        let parent = GetParent(hwnd);
        let dpi = GetDpiForWindow(if parent.is_null() { hwnd } else { parent });
        let s = if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 };
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut r);
        let mut tl = POINT { x: r.left, y: r.top };
        let mut pc: RECT = std::mem::zeroed();
        if !parent.is_null() {
            ScreenToClient(parent, &mut tl);
            GetClientRect(parent, &mut pc);
        }
        Some((tl.x as f64 / s, tl.y as f64 / s, (r.right - r.left) as f64 / s, (r.bottom - r.top) as f64 / s, IsWindowVisible(hwnd) == 0, pc.right as f64 / s, pc.bottom as f64 / s))
    }
}

/// "frame=(x,y,w,h) hidden=bool parent=WxH", the format the macOS description uses.
#[cfg(not(target_os = "macos"))]
pub(crate) fn describe(id: usize) -> String {
    match native_geometry(id) {
        Some((x, y, w, h, hidden, pw, ph)) => format!("frame=({x:.0},{y:.0},{w:.0},{h:.0}) hidden={hidden} parent={pw:.0}x{ph:.0}"),
        None => format!("null (native window {id:#x} unavailable)"),
    }
}

/// (parent window, "Cef", native id, hidden) of every live browser window.
#[cfg(not(target_os = "macos"))]
pub(crate) fn live_views() -> Vec<(usize, String, usize, bool)> {
    use cef::ImplBrowser as _;
    let hosts: Vec<(usize, usize)> = crate::runtime::with_state(|st| {
        st.views.values().filter(|e| !e.closing).filter_map(|e| e.browser.as_ref().and_then(|b| b.host()).map(|h| (e.parent, native_id(&h)))).collect()
    })
    .unwrap_or_default();
    hosts
        .into_iter()
        .filter(|(_, id)| *id != 0)
        .map(|(parent, id)| (parent, "CefBrowserView".to_string(), id, native_geometry(id).map(|g| g.4).unwrap_or(true)))
        .collect()
}

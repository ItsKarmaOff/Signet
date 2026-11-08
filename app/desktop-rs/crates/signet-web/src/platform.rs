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

/// GPUI windows on X11 use a 32-bit ARGB visual; a CEF child window created straight in one fails (BadMatch, so the
/// browser window never exists). Each browser therefore lives in a small wrapper window created with the screen's default
/// depth, visual and colormap, itself a child of the GPUI window. The connection stays open: X resources die with it.
#[cfg(target_os = "linux")]
pub(crate) mod x11host {
    use std::collections::HashMap;
    use std::os::raw::{c_int, c_uint, c_ulong};
    use std::sync::{Mutex, OnceLock};

    pub struct Conn {
        pub xlib: x11_dl::xlib::Xlib,
        pub dpy: *mut x11_dl::xlib::Display,
    }
    // SAFETY: the connection is only used from the main thread (CEF runs on it, see `assert_main_thread`).
    unsafe impl Send for Conn {}
    unsafe impl Sync for Conn {}

    pub fn conn() -> Option<&'static Conn> {
        static CONN: OnceLock<Option<Conn>> = OnceLock::new();
        CONN.get_or_init(|| {
            let xlib = x11_dl::xlib::Xlib::open().ok()?;
            let dpy = unsafe { (xlib.XOpenDisplay)(std::ptr::null()) };
            if dpy.is_null() { None } else { Some(Conn { xlib, dpy }) }
        })
        .as_ref()
    }

    static PENDING: Mutex<Option<HashMap<u64, c_ulong>>> = Mutex::new(None);
    static WRAPPERS: Mutex<Option<HashMap<usize, c_ulong>>> = Mutex::new(None);

    pub fn scale() -> f32 {
        std::env::var("GDK_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0)
    }

    /// Creates the wrapper of view `view` under `parent`; returns the window CEF must use as its parent.
    pub fn create_wrapper(view: u64, parent: usize, b: crate::types::Bounds) -> usize {
        let Some(c) = conn() else { return parent };
        let s = scale();
        unsafe {
            let screen = (c.xlib.XDefaultScreen)(c.dpy);
            let mut attrs: x11_dl::xlib::XSetWindowAttributes = std::mem::zeroed();
            attrs.colormap = (c.xlib.XDefaultColormap)(c.dpy, screen);
            attrs.border_pixel = 0;
            let w = (c.xlib.XCreateWindow)(
                c.dpy,
                parent as c_ulong,
                (b.x * s) as c_int,
                (b.y * s) as c_int,
                (b.width * s).max(1.0) as c_uint,
                (b.height * s).max(1.0) as c_uint,
                0,
                (c.xlib.XDefaultDepth)(c.dpy, screen),
                x11_dl::xlib::InputOutput as c_uint,
                (c.xlib.XDefaultVisual)(c.dpy, screen),
                x11_dl::xlib::CWColormap | x11_dl::xlib::CWBorderPixel,
                &mut attrs,
            );
            if w == 0 {
                return parent;
            }
            (c.xlib.XMapWindow)(c.dpy, w);
            (c.xlib.XSync)(c.dpy, 0);
            PENDING.lock().unwrap().get_or_insert_with(HashMap::new).insert(view, w);
            w as usize
        }
    }

    /// Links the wrapper of `view` to the window CEF made for the browser.
    pub fn adopt(view: u64, handle: usize) {
        if handle == 0 {
            return;
        }
        if let Some(w) = PENDING.lock().unwrap().as_mut().and_then(|m| m.remove(&view)) {
            WRAPPERS.lock().unwrap().get_or_insert_with(HashMap::new).insert(handle, w);
        }
    }

    pub fn wrapper_of(handle: usize) -> Option<c_ulong> {
        WRAPPERS.lock().unwrap().as_ref().and_then(|m| m.get(&handle).copied())
    }

    pub fn destroy_wrapper(handle: usize) {
        let w = WRAPPERS.lock().unwrap().as_mut().and_then(|m| m.remove(&handle));
        if let (Some(w), Some(c)) = (w, conn()) {
            unsafe {
                (c.xlib.XDestroyWindow)(c.dpy, w);
                (c.xlib.XFlush)(c.dpy);
            }
        }
    }
}

/// Window CEF should use as the parent of the browser of `view` (see `x11host`); the parent itself elsewhere.
pub(crate) fn host_parent(view: u64, parent: usize, b: Bounds) -> usize {
    #[cfg(target_os = "linux")]
    {
        x11host::create_wrapper(view, parent, b)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (view, b);
        parent
    }
}

/// Size the browser is created with. macOS takes points; X11 and Win32 windows are in pixels, so the page is created
/// at its final pixel size instead of a point-sized (too small on HiDPI) one that only `apply` corrects afterwards.
pub(crate) fn initial_size(parent: usize, b: Bounds) -> (i32, i32) {
    #[allow(unused_mut)]
    let mut scale = 1.0f32;
    #[cfg(target_os = "linux")]
    {
        let _ = parent;
        scale = x11host::scale();
    }
    #[cfg(target_os = "windows")]
    {
        let dpi = unsafe { windows_sys::Win32::UI::HiDpi::GetDpiForWindow(parent as windows_sys::Win32::Foundation::HWND) };
        if dpi != 0 {
            scale = dpi as f32 / 96.0;
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let _ = parent;
    ((b.width * scale).round().max(1.0) as i32, (b.height * scale).round().max(1.0) as i32)
}

/// Called once the browser exists: remembers which wrapper belongs to its native window.
pub(crate) fn adopt(view: u64, host: &cef::BrowserHost) {
    #[cfg(target_os = "linux")]
    {
        use cef::ImplBrowserHost as _;
        x11host::adopt(view, host.window_handle() as usize);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (view, host);
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
            x11host::destroy_wrapper(handle as usize);
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
        let scale = x11host::scale();
        let _ = parent;
        if let Some(c) = x11host::conn() {
            unsafe {
                let wrapper = x11host::wrapper_of(handle as usize);
                let target = wrapper.unwrap_or(handle as std::os::raw::c_ulong);
                let (w, h) = ((b.width * scale).max(1.0) as u32, (b.height * scale).max(1.0) as u32);
                if visible {
                    (c.xlib.XMoveResizeWindow)(c.dpy, target, (b.x * scale) as i32, (b.y * scale) as i32, w, h);
                    (c.xlib.XMapWindow)(c.dpy, target);
                    if wrapper.is_some() {
                        // The browser fills its wrapper.
                        (c.xlib.XMoveResizeWindow)(c.dpy, handle as std::os::raw::c_ulong, 0, 0, w, h);
                        (c.xlib.XMapWindow)(c.dpy, handle as std::os::raw::c_ulong);
                    }
                } else {
                    (c.xlib.XUnmapWindow)(c.dpy, target);
                }
                (c.xlib.XFlush)(c.dpy);
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
                    let target = x11host::wrapper_of(handle as usize).unwrap_or(handle as std::os::raw::c_ulong);
                    (xlib.XReparentWindow)(dpy, target, parent as std::os::raw::c_ulong, b.x as i32, b.y as i32);
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

#[cfg(target_os = "linux")]
static LAST_X_ERROR: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

#[cfg(target_os = "linux")]
unsafe extern "C" fn record_x_error(_: *mut x11_dl::xlib::Display, e: *mut x11_dl::xlib::XErrorEvent) -> std::os::raw::c_int {
    LAST_X_ERROR.store(unsafe { (*e).error_code } as i32, std::sync::atomic::Ordering::Relaxed);
    0
}

/// Logs the window tree below `w` (id, geometry, map state), three levels deep. Diagnostic for the self-test.
#[cfg(target_os = "linux")]
unsafe fn dump_x_tree(xlib: &x11_dl::xlib::Xlib, dpy: *mut x11_dl::xlib::Display, w: std::os::raw::c_ulong, depth: usize) {
    unsafe {
        let mut a: x11_dl::xlib::XWindowAttributes = std::mem::zeroed();
        if (xlib.XGetWindowAttributes)(dpy, w, &mut a) != 0 {
            crate::app::rlog(&format!("x tree {}{:#x} {}x{}+{}+{} map_state={}", "  ".repeat(depth), w, a.width, a.height, a.x, a.y, a.map_state));
        }
        if depth >= 3 {
            return;
        }
        let (mut root, mut parent) = (0, 0);
        let mut kids: *mut std::os::raw::c_ulong = std::ptr::null_mut();
        let mut n: std::os::raw::c_uint = 0;
        if (xlib.XQueryTree)(dpy, w, &mut root, &mut parent, &mut kids, &mut n) != 0 && !kids.is_null() {
            for i in 0..n as usize {
                dump_x_tree(xlib, dpy, *kids.add(i), depth + 1);
            }
            (xlib.XFree)(kids as *mut _);
        }
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
    // The wrapper holds the position inside the app window (the browser window fills it).
    let id = x11host::wrapper_of(id).map(|w| w as usize).unwrap_or(id);
    unsafe {
        let dpy = (xlib.XOpenDisplay)(std::ptr::null());
        if dpy.is_null() {
            return None;
        }
        let mut a: x11_dl::xlib::XWindowAttributes = std::mem::zeroed();
        LAST_X_ERROR.store(0, std::sync::atomic::Ordering::Relaxed);
        let previous = (xlib.XSetErrorHandler)(Some(record_x_error));
        let ok = (xlib.XGetWindowAttributes)(dpy, id as std::os::raw::c_ulong, &mut a) != 0;
        (xlib.XSync)(dpy, 0);
        let code = LAST_X_ERROR.load(std::sync::atomic::Ordering::Relaxed);
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
        if !ok {
            static DUMPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !DUMPED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                dump_x_tree(&xlib, dpy, (xlib.XDefaultRootWindow)(dpy), 0);
            }
            crate::app::rlog(&format!("native_geometry: XGetWindowAttributes failed for {id:#x}, x error {code}"));
        }
        (xlib.XSetErrorHandler)(previous);
        (xlib.XCloseDisplay)(dpy);
        if !ok {
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

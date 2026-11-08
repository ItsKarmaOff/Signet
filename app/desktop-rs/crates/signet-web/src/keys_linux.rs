//! Linux keyboard glue for the hold-to-quit gauge: the system key repeat settings (XKB).

/// X server key repeat settings (the app runs on X11 or XWayland): (delay until repeat ms, repeat interval ms).
pub fn repeat_settings() -> Option<(u32, u32)> {
    use std::os::raw::c_uint;
    let xlib = x11_dl::xlib::Xlib::open().ok()?;
    unsafe {
        let dpy = (xlib.XOpenDisplay)(std::ptr::null());
        if dpy.is_null() {
            return None;
        }
        let (mut timeout, mut interval): (c_uint, c_uint) = (0, 0);
        // XkbUseCoreKbd = 0x0100.
        let ok = (xlib.XkbGetAutoRepeatRate)(dpy, 0x0100, &mut timeout, &mut interval) != 0;
        (xlib.XCloseDisplay)(dpy);
        ok.then_some((timeout, interval))
    }
}

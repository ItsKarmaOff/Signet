//! macOS glue: the NSApplication protocols CEF requires, and native child view helpers.
//!
//! CEF asserts that `NSApp` conforms to `CefAppProtocol` (`CrAppProtocol` + `CrAppControlProtocol`:
//! `isHandlingSendEvent`, `setHandlingSendEvent:`) and expects `-sendEvent:` to track the flag.
//! GPUI registers its own `NSApplication` subclass (`GPUIApplication`, created in a `ctor` before
//! `main`) and instantiates it, so we cannot subclass NSApplication ourselves. Instead, at runtime,
//! before `cef::initialize`, we patch the class of the live `NSApp` instance with the objc runtime:
//!
//! 1. register the three protocols dynamically (`objc_allocateProtocol`) because no Objective-C code
//!    in this process declares them, and add them to the class (`class_addProtocol`);
//! 2. add `isHandlingSendEvent` / `setHandlingSendEvent:` backed by an atomic flag;
//! 3. add a `sendEvent:` override that sets the flag, calls `NSApplication`'s implementation, then
//!    restores the flag. GPUI does not override `sendEvent:`, so nothing is lost; if it ever does,
//!    the existing implementation is wrapped instead of replaced.

use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

type Id = *mut c_void;
type Sel = *mut c_void;
type Imp = *const c_void;

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn object_getClass(obj: Id) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn class_addMethod(cls: Id, sel: Sel, imp: Imp, types: *const c_char) -> bool;
    fn class_getInstanceMethod(cls: Id, sel: Sel) -> *mut c_void;
    fn method_getImplementation(m: *mut c_void) -> Imp;
    fn method_setImplementation(m: *mut c_void, imp: Imp) -> Imp;
    fn objc_allocateProtocol(name: *const c_char) -> *mut c_void;
    fn objc_registerProtocol(p: *mut c_void);
    fn objc_getProtocol(name: *const c_char) -> *mut c_void;
    fn protocol_addProtocol(p: *mut c_void, other: *mut c_void);
    fn protocol_addMethodDescription(p: *mut c_void, sel: Sel, types: *const c_char, required: bool, instance: bool);
    fn class_addProtocol(cls: Id, p: *mut c_void) -> bool;
    fn objc_msgSend();
}

#[cfg(target_arch = "aarch64")]
const BOOL_ENC: &CStr = c"B";
#[cfg(not(target_arch = "aarch64"))]
const BOOL_ENC: &CStr = c"c";

/// One alphabetic character outside the Latin blocks (up to U+024F).
fn is_non_latin_letter(chars: &str) -> bool {
    let mut it = chars.chars();
    matches!((it.next(), it.next()), (Some(c), None) if c.is_alphabetic() && (c as u32) > 0x24F)
}

static HANDLING_SEND_EVENT: AtomicBool = AtomicBool::new(false);
static ORIGINAL_SEND_EVENT: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

extern "C" fn is_handling_send_event(_this: Id, _sel: Sel) -> bool {
    HANDLING_SEND_EVENT.load(Ordering::Relaxed)
}

extern "C" fn set_handling_send_event(_this: Id, _sel: Sel, value: bool) {
    HANDLING_SEND_EVENT.store(value, Ordering::Relaxed);
}

extern "C" fn send_event(this: Id, sel: Sel, event: Id) {
    focus_handoff_on_click(event);
    let previous = HANDLING_SEND_EVENT.swap(true, Ordering::Relaxed);
    let original = ORIGINAL_SEND_EVENT.load(Ordering::Relaxed);
    if !original.is_null() {
        // SAFETY: the pointer is the IMP of `-[NSApplication sendEvent:]` (or the class's own).
        let f: extern "C" fn(Id, Sel, Id) = unsafe { std::mem::transmute(original) };
        f(this, sel, event);
    }
    HANDLING_SEND_EVENT.store(previous, Ordering::Relaxed);
}

fn protocol(name: &CStr, parent: Option<*mut c_void>, methods: &[(&CStr, String)]) -> *mut c_void {
    unsafe {
        let existing = objc_getProtocol(name.as_ptr());
        if !existing.is_null() {
            return existing;
        }
        let p = objc_allocateProtocol(name.as_ptr());
        if p.is_null() {
            return std::ptr::null_mut();
        }
        if let Some(parent) = parent {
            protocol_addProtocol(p, parent);
        }
        for (sel, types) in methods {
            let types = std::ffi::CString::new(types.as_str()).unwrap_or_default();
            protocol_addMethodDescription(p, sel_registerName(sel.as_ptr()), types.as_ptr(), true, true);
        }
        objc_registerProtocol(p);
        p
    }
}

/// Make the class of the live `NSApp` conform to `CefAppProtocol`. Idempotent. Returns `false` when
/// there is no NSApplication instance yet.
pub fn install_cef_app_protocols() -> bool {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.load(Ordering::SeqCst) {
        return true;
    }
    unsafe {
        let ns_app_class = objc_getClass(c"NSApplication".as_ptr());
        if ns_app_class.is_null() {
            return false;
        }
        // [NSApplication sharedApplication]: returns the existing instance (GPUIApplication under GPUI).
        let shared: extern "C" fn(Id, Sel) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let app = shared(ns_app_class, sel_registerName(c"sharedApplication".as_ptr()));
        if app.is_null() {
            return false;
        }
        let cls = object_getClass(app);
        let bool_enc = BOOL_ENC.to_str().unwrap_or("c");
        let cr = protocol(c"CrAppProtocol", None, &[(c"isHandlingSendEvent", format!("{bool_enc}@:"))]);
        let ctrl = protocol(
            c"CrAppControlProtocol",
            Some(cr),
            &[(c"setHandlingSendEvent:", format!("v@:{bool_enc}"))],
        );
        let cef = protocol(c"CefAppProtocol", Some(ctrl), &[]);

        let is_sel = sel_registerName(c"isHandlingSendEvent".as_ptr());
        let set_sel = sel_registerName(c"setHandlingSendEvent:".as_ptr());
        let send_sel = sel_registerName(c"sendEvent:".as_ptr());
        let is_types = std::ffi::CString::new(format!("{bool_enc}@:")).unwrap_or_default();
        let set_types = std::ffi::CString::new(format!("v@:{bool_enc}")).unwrap_or_default();
        class_addMethod(cls, is_sel, is_handling_send_event as Imp, is_types.as_ptr());
        class_addMethod(cls, set_sel, set_handling_send_event as Imp, set_types.as_ptr());

        // Wrap sendEvent:. The method may be inherited (NSApplication) or defined on the class.
        let m = class_getInstanceMethod(cls, send_sel);
        if !m.is_null() {
            let orig = method_getImplementation(m);
            ORIGINAL_SEND_EVENT.store(orig as *mut c_void, Ordering::Relaxed);
            // Adds when inherited; when the class defines its own, replace it (original kept above).
            if !class_addMethod(cls, send_sel, send_event as Imp, c"v@:@".as_ptr()) {
                method_setImplementation(m, send_event as Imp);
            }
        }
        for p in [cr, ctrl, cef] {
            if !p.is_null() {
                class_addProtocol(cls, p);
            }
        }
    }
    DONE.store(true, Ordering::SeqCst);
    true
}

/// Convert a bounds rectangle (window points, top-left origin) into the parent view's coordinates
/// and apply it to the native browser view.
pub fn set_view_frame(view: *mut c_void, parent: *mut c_void, x: f32, y: f32, w: f32, h: f32) {
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    unsafe {
        let view = &*(view as *mut objc2::runtime::AnyObject);
        let parent = &*(parent as *mut objc2::runtime::AnyObject);
        let flipped: bool = objc2::msg_send![parent, isFlipped];
        let parent_frame: NSRect = objc2::msg_send![parent, bounds];
        let fy = if flipped { y as f64 } else { parent_frame.size.height - (y + h) as f64 };
        let frame = NSRect::new(NSPoint::new(x as f64, fy), NSSize::new(w as f64, h as f64));
        let _: () = objc2::msg_send![view, setFrame: frame];
    }
}

/// A key event seen by the local monitor ([`install_key_monitor`]).
#[derive(Clone, Debug)]
pub struct NativeKey {
    /// 0 key down, 1 key up, 2 modifier flags changed.
    pub kind: u8,
    pub repeat: bool,
    /// Characters without modifiers, lowercase (empty for flags changes).
    pub chars: String,
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Virtual key code (kVK_*), 0 for flags changes.
    pub key_code: u16,
}

/// kVK_ANSI_V.
const KEY_CODE_V: u16 = 9;

/// Responder-chain action a paste chord stands for: Cmd+V is `paste:`, Alt+Shift+Cmd+V is `pasteAsPlainText:`
/// (Chromium's paste and match style). Key down only; layouts whose letters are not Latin go by the V key.
pub fn paste_action(k: &NativeKey) -> Option<&'static str> {
    if k.kind != 0 || !k.cmd || k.ctrl {
        return None;
    }
    let is_v = k.chars == "v" || (k.key_code == KEY_CODE_V && is_non_latin_letter(&k.chars));
    match (is_v, k.alt, k.shift) {
        (true, false, false) => Some("paste:"),
        (true, true, true) => Some("pasteAsPlainText:"),
        _ => None,
    }
}

/// Sends `action` (a selector such as `paste:`) to the first responder of the key window when that is a page view, so
/// that Chromium itself reads the pasteboard (every type, `org.nspasteboard.ConcealedType` items such as one-time codes
/// included). Returns false when the focus is not on a page (GPUI inputs handle their own paste).
pub fn send_action_to_page(action: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(action) else { return false };
    unsafe {
        let app: ObjPtr = objc2::msg_send![objc_getClass(c"NSApplication".as_ptr()) as ObjPtr, sharedApplication];
        let key: ObjPtr = objc2::msg_send![obj(app), keyWindow];
        if key.is_null() {
            return false;
        }
        let responder: ObjPtr = objc2::msg_send![obj(key), firstResponder];
        if responder.is_null() || !crate::platform::native_view_handles().iter().any(|v| is_descendant(responder, *v)) {
            return false;
        }
        let sel = objc2::runtime::Sel::register(&name);
        let handled: bool = objc2::msg_send![obj(app), sendAction: sel, to: obj(responder), from: std::ptr::null_mut::<objc2::runtime::AnyObject>()];
        handled
    }
}

/// Installs a monitor on the key events of the whole application, called on the main thread before the menu and the
/// views see them. The handler returns true to swallow the event. Installed once for the life of the process.
pub fn install_key_monitor(handler: impl Fn(NativeKey) -> bool + 'static) {
    use objc2::runtime::AnyObject;
    let block = block2::RcBlock::new(move |ev: *mut AnyObject| -> *mut AnyObject {
        if ev.is_null() {
            return ev;
        }
        let key = unsafe {
            let ty: usize = objc2::msg_send![&*ev, type];
            let flags: usize = objc2::msg_send![&*ev, modifierFlags];
            let kind = match ty {
                10 => 0u8,
                11 => 1,
                _ => 2,
            };
            let key_code: u16 = if kind == 2 { 0 } else { objc2::msg_send![&*ev, keyCode] };
            let (repeat, chars) = if kind == 2 {
                (false, String::new())
            } else {
                let repeat: bool = objc2::msg_send![&*ev, isARepeat];
                let s: *mut AnyObject = objc2::msg_send![&*ev, charactersIgnoringModifiers];
                let chars = if s.is_null() {
                    String::new()
                } else {
                    let u: *const std::ffi::c_char = objc2::msg_send![&*s, UTF8String];
                    if u.is_null() { String::new() } else { CStr::from_ptr(u).to_string_lossy().to_lowercase() }
                };
                // Non-Latin layouts (Cyrillic, Arabic, Hebrew...): Cmd+Q still means the Q key (kVK_ANSI_Q = 12). Latin
                // layouts (AZERTY, Dvorak) keep going by character.
                let chars = if key_code == 12 && flags & (1 << 20) != 0 && is_non_latin_letter(&chars) { "q".to_string() } else { chars };
                (repeat, chars)
            };
            NativeKey { kind, repeat, chars, key_code, cmd: flags & (1 << 20) != 0, ctrl: flags & (1 << 18) != 0, alt: flags & (1 << 19) != 0, shift: flags & (1 << 17) != 0 }
        };
        if handler(key) { std::ptr::null_mut() } else { ev }
    });
    unsafe {
        let cls = objc_getClass(c"NSEvent".as_ptr()) as *mut AnyObject;
        let mask: u64 = (1 << 10) | (1 << 11) | (1 << 12);
        let _: *mut AnyObject = objc2::msg_send![cls, addLocalMonitorForEventsMatchingMask: mask, handler: &*block];
    }
    // The monitor keeps its own copy of the block.
}

/// Hides every window of the application at once (quit: the window goes away before the slow teardown starts).
pub fn hide_all_windows() {
    unsafe {
        let Some(cls) = objc2::runtime::AnyClass::get(c"NSApplication") else { return };
        let app: *mut objc2::runtime::AnyObject = objc2::msg_send![cls, sharedApplication];
        if app.is_null() {
            return;
        }
        let windows: *mut objc2::runtime::AnyObject = objc2::msg_send![&*app, windows];
        if windows.is_null() {
            return;
        }
        let n: usize = objc2::msg_send![&*windows, count];
        for i in 0..n {
            let w: *mut objc2::runtime::AnyObject = objc2::msg_send![&*windows, objectAtIndex: i];
            if !w.is_null() {
                let _: () = objc2::msg_send![&*w, orderOut: std::ptr::null_mut::<objc2::runtime::AnyObject>()];
            }
        }
    }
}

/// Takes a native view out of its parent (it is released once nothing else holds it).
pub fn remove_view(view: *mut c_void) {
    unsafe {
        let view = &*(view as *mut objc2::runtime::AnyObject);
        let _: () = objc2::msg_send![view, removeFromSuperview];
    }
}

/// Moves a native view to another parent view (`addSubview:` removes it from the old one).
pub fn reparent_view(view: *mut c_void, parent: *mut c_void) {
    unsafe {
        let view = &*(view as *mut objc2::runtime::AnyObject);
        let parent = &*(parent as *mut objc2::runtime::AnyObject);
        let _: *mut objc2::runtime::AnyObject = objc2::msg_send![view, retain];
        let _: () = objc2::msg_send![view, removeFromSuperview];
        let _: () = objc2::msg_send![parent, addSubview: view];
        let _: () = objc2::msg_send![view, release];
    }
}

/// Rounds the corners of a native view (its layer clips the page), like `WebContentsView.setBorderRadius`.
pub fn set_view_corner_radius(view: *mut c_void, radius: f32) {
    unsafe {
        let view = &*(view as *mut objc2::runtime::AnyObject);
        let _: () = objc2::msg_send![view, setWantsLayer: true];
        let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![view, layer];
        if let Some(layer) = layer.as_ref() {
            let _: () = objc2::msg_send![layer, setCornerRadius: radius as f64];
            let _: () = objc2::msg_send![layer, setMasksToBounds: radius > 0.0];
        }
    }
}

pub fn set_view_hidden(view: *mut c_void, hidden: bool) {
    unsafe {
        let view = &*(view as *mut objc2::runtime::AnyObject);
        let _: () = objc2::msg_send![view, setHidden: hidden];
    }
}

/// Pins a native view to the top-left of its parent with a fixed size (`NSViewMaxXMargin | NSViewMinYMargin`):
/// AppKit no longer resizes it with the window (CEF creates its host view width and height sizable).
pub fn pin_view_top_left(view: *mut c_void) {
    if view.is_null() {
        return;
    }
    unsafe {
        let view = &*(view as *mut objc2::runtime::AnyObject);
        let _: () = objc2::msg_send![view, setAutoresizingMask: 4usize | 8usize];
    }
}

/// Make the browser's native view first responder of its window (keyboard focus).
pub fn focus_view(view: *mut c_void) {
    unsafe {
        let view = &*(view as *mut objc2::runtime::AnyObject);
        let window: *mut objc2::runtime::AnyObject = objc2::msg_send![view, window];
        if !window.is_null() {
            let _: bool = objc2::msg_send![&*window, makeFirstResponder: view];
        }
    }
}

/// Debug helper: "x,y,w,h hidden=.. superview_ok=.." of a native view.
pub fn describe_view(view: *mut c_void) -> String {
    use objc2_foundation::NSRect;
    if view.is_null() {
        return "null".into();
    }
    unsafe {
        let v = &*(view as *mut objc2::runtime::AnyObject);
        let f: NSRect = objc2::msg_send![v, frame];
        let hidden: bool = objc2::msg_send![v, isHidden];
        let sup: *mut objc2::runtime::AnyObject = objc2::msg_send![v, superview];
        let (sw, sh) = if sup.is_null() {
            (0.0, 0.0)
        } else {
            let sf: NSRect = objc2::msg_send![&*sup, frame];
            (sf.size.width, sf.size.height)
        };
        format!("frame=({:.0},{:.0},{:.0},{:.0}) hidden={hidden} parent={:.0}x{:.0}", f.origin.x, f.origin.y, f.size.width, f.size.height, sw, sh)
    }
}

// ---------------------------------------------------------------------------------------------
// Keyboard focus between GPUI and the CEF views
// ---------------------------------------------------------------------------------------------
//
// A click on a GPUI area does not move the AppKit first responder: the CEF view keeps it and keeps
// receiving key strokes. The patched `-[NSApplication sendEvent:]` therefore looks at every mouse
// down: when the click lands outside every CEF view of the window and the first responder is
// inside one, the window's `GPUIView` becomes first responder. The reverse needs
// nothing: CEF's view takes the first responder itself when clicked (`FocusGained` is emitted).

type ObjPtr = *mut objc2::runtime::AnyObject;

fn obj<'a, T>(p: *mut T) -> &'a objc2::runtime::AnyObject {
    // SAFETY: callers pass live Objective-C objects obtained from AppKit during the current call.
    unsafe { &*(p as *mut objc2::runtime::AnyObject) }
}

fn is_descendant(view: ObjPtr, ancestor: *mut c_void) -> bool {
    if view.is_null() || ancestor.is_null() {
        return false;
    }
    let r: bool = unsafe { objc2::msg_send![obj(view), isDescendantOf: obj(ancestor)] };
    r
}

/// The view of `window` that handles GPUI's keyboard input. It is the `GPUIView` subview of the content view (the
/// content view itself is a plain `NSView`, wrapped by the accessibility adapter, and ignores key events); falls back
/// to the content view for a window that GPUI did not create.
fn keyboard_view_of(window: ObjPtr) -> ObjPtr {
    unsafe {
        let content: ObjPtr = objc2::msg_send![obj(window), contentView];
        if content.is_null() {
            return content;
        }
        let cls = objc_getClass(c"GPUIView".as_ptr());
        if !cls.is_null() {
            let subs: ObjPtr = objc2::msg_send![obj(content), subviews];
            let count: usize = objc2::msg_send![obj(subs), count];
            for i in 0..count {
                let v: ObjPtr = objc2::msg_send![obj(subs), objectAtIndex: i];
                let is: bool = objc2::msg_send![obj(v), isKindOfClass: obj(cls)];
                if is {
                    return v;
                }
            }
        }
        content
    }
}

fn focus_handoff_on_click(event: Id) {
    use objc2_foundation::NSPoint;
    if event.is_null() {
        return;
    }
    unsafe {
        let ty: usize = objc2::msg_send![obj(event), type];
        // LeftMouseDown, RightMouseDown, OtherMouseDown
        if ty != 1 && ty != 3 && ty != 25 {
            return;
        }
        let window: ObjPtr = objc2::msg_send![obj(event), window];
        if window.is_null() {
            return;
        }
        let cef_views = crate::platform::native_view_handles();
        if cef_views.is_empty() {
            return;
        }
        let content: ObjPtr = objc2::msg_send![obj(window), contentView];
        if content.is_null() {
            return;
        }
        let responder: ObjPtr = objc2::msg_send![obj(window), firstResponder];
        let in_cef_focus = cef_views.iter().any(|v| is_descendant(responder, *v));
        if !in_cef_focus {
            return;
        }
        let loc: NSPoint = objc2::msg_send![obj(event), locationInWindow];
        let hit: ObjPtr = objc2::msg_send![obj(content), hitTest: loc];
        if hit.is_null() || cef_views.iter().any(|v| is_descendant(hit, *v)) {
            return;
        }
        let _: bool = objc2::msg_send![obj(window), makeFirstResponder: obj(keyboard_view_of(window))];
    }
}

/// Whether the GPUI view of the window of `parent` (an `NSView*`, the `GPUIView` itself or any view of the window)
/// has a focused text input: GPUI's `selectedRange` answers `NSNotFound` when no input handler is registered.
pub fn gpui_text_input_active(parent: *mut c_void) -> bool {
    if parent.is_null() {
        return false;
    }
    unsafe {
        let window: ObjPtr = objc2::msg_send![obj(parent), window];
        if window.is_null() {
            return false;
        }
        let view = keyboard_view_of(window);
        if view.is_null() {
            return false;
        }
        let range: objc2_foundation::NSRange = objc2::msg_send![obj(view), selectedRange];
        range.location != isize::MAX as usize
    }
}

/// Whether the GPUI view of the window of `parent` is the first responder (the keyboard goes to GPUI).
pub fn gpui_has_keyboard(parent: *mut c_void) -> bool {
    if parent.is_null() {
        return false;
    }
    unsafe {
        let window: ObjPtr = objc2::msg_send![obj(parent), window];
        if window.is_null() {
            return false;
        }
        let responder: ObjPtr = objc2::msg_send![obj(window), firstResponder];
        !responder.is_null() && responder == keyboard_view_of(window)
    }
}

/// Take the keyboard focus away from the CEF views of the window of `parent` (an `NSView*`): the
/// GPUI view becomes first responder. Call it when GPUI wants the keyboard without a click
/// (opening the command bar from a shortcut while a page has focus).
pub fn focus_parent_view(parent: *mut c_void) {
    if parent.is_null() {
        return;
    }
    unsafe {
        let window: ObjPtr = objc2::msg_send![obj(parent), window];
        if !window.is_null() {
            let _: bool = objc2::msg_send![obj(window), makeFirstResponder: obj(keyboard_view_of(window))];
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Overlay windows (child windows above the native CEF views)
// ---------------------------------------------------------------------------------------------

fn window_of(handle: raw_window_handle::RawWindowHandle) -> Result<ObjPtr, String> {
    match handle {
        raw_window_handle::RawWindowHandle::AppKit(h) => {
            let view = h.ns_view.as_ptr() as ObjPtr;
            let window: ObjPtr = unsafe { objc2::msg_send![obj(view), window] };
            if window.is_null() { Err("the view has no window yet".into()) } else { Ok(window) }
        }
        other => Err(format!("not an AppKit handle: {other:?}")),
    }
}

/// Make the window of `child` a child window of the window of `parent`, ordered above it. A child
/// window is always drawn above the parent's content, native CEF views included, moves with the parent
/// and is hidden with it. Handles come from GPUI: `window.window_handle()?.as_raw()`.
pub fn attach_child_window(parent: raw_window_handle::RawWindowHandle, child: raw_window_handle::RawWindowHandle) -> Result<(), String> {
    let (p, c) = (window_of(parent)?, window_of(child)?);
    unsafe {
        // NSWindowAbove = 1
        let _: () = objc2::msg_send![obj(p), addChildWindow: obj(c), ordered: 1isize];
    }
    Ok(())
}

/// Undo [`attach_child_window`].
pub fn detach_child_window(parent: raw_window_handle::RawWindowHandle, child: raw_window_handle::RawWindowHandle) -> Result<(), String> {
    let (p, c) = (window_of(parent)?, window_of(child)?);
    unsafe {
        let _: () = objc2::msg_send![obj(p), removeChildWindow: obj(c)];
    }
    Ok(())
}

/// Bring an attached overlay back above the other child windows.
pub fn raise_window(window: raw_window_handle::RawWindowHandle) -> Result<(), String> {
    let w = window_of(window)?;
    unsafe {
        let _: () = objc2::msg_send![obj(w), orderFront: None::<&objc2::runtime::AnyObject>];
    }
    Ok(())
}

/// Let mouse events fall through the window to what is below (pass-through surfaces such as toasts).
pub fn set_click_through(window: raw_window_handle::RawWindowHandle, on: bool) -> Result<(), String> {
    let w = window_of(window)?;
    unsafe {
        let _: () = objc2::msg_send![obj(w), setIgnoresMouseEvents: on];
    }
    Ok(())
}

/// Make a window suitable as a floating overlay: transparent background, no shadow, not opaque.
/// The GPUI window must also be created with a transparent background appearance.
pub fn configure_overlay_window(window: raw_window_handle::RawWindowHandle) -> Result<(), String> {
    let w = window_of(window)?;
    unsafe {
        let clear: ObjPtr = objc2::msg_send![objc_getClass(c"NSColor".as_ptr()) as ObjPtr, clearColor];
        let _: () = objc2::msg_send![obj(w), setOpaque: false];
        let _: () = objc2::msg_send![obj(w), setHasShadow: false];
        let _: () = objc2::msg_send![obj(w), setBackgroundColor: obj(clear)];
    }
    Ok(())
}

/// Test helper: post a synthesized key event into the application queue, addressed to the window of
/// `window` (it reaches the first responder through `sendEvent:` like real input, and CEF's
/// `OnPreKeyEvent`). `key_code` is the macOS virtual key code, `flags` the NSEventModifierFlags.
pub fn post_key_event(window: raw_window_handle::RawWindowHandle, key_code: u16, chars: &str, flags: usize, down: bool) -> Result<(), String> {
    post_key_event_ex(window, key_code, chars, flags, down, false)
}

/// Like [`post_key_event`], with the auto-repeat flag.
pub fn post_key_event_ex(window: raw_window_handle::RawWindowHandle, key_code: u16, chars: &str, flags: usize, down: bool, repeat: bool) -> Result<(), String> {
    use objc2_foundation::{NSPoint, NSString};
    let w = window_of(window)?;
    unsafe {
        let number: isize = objc2::msg_send![obj(w), windowNumber];
        let cls = objc_getClass(c"NSEvent".as_ptr()) as *mut objc2::runtime::AnyObject;
        let s = NSString::from_str(chars);
        let ev: ObjPtr = objc2::msg_send![
            cls,
            keyEventWithType: if down { 10usize } else { 11usize },
            location: NSPoint::new(0.0, 0.0),
            modifierFlags: flags,
            timestamp: 0.0f64,
            windowNumber: number,
            context: None::<&objc2::runtime::AnyObject>,
            characters: &*s,
            charactersIgnoringModifiers: &*s,
            isARepeat: repeat,
            keyCode: key_code
        ];
        if ev.is_null() {
            return Err("cannot create the key event".into());
        }
        let app: ObjPtr = objc2::msg_send![objc_getClass(c"NSApplication".as_ptr()) as *mut objc2::runtime::AnyObject, sharedApplication];
        let _: () = objc2::msg_send![obj(app), postEvent: obj(ev), atStart: false];
    }
    Ok(())
}

/// Test helper: post a left mouse down + up at `(x, y)` (window points, top-left origin) in the window of `window`.
pub fn post_click(window: raw_window_handle::RawWindowHandle, x: f64, y: f64) -> Result<(), String> {
    use objc2_foundation::NSPoint;
    let w = window_of(window)?;
    unsafe {
        let number: isize = objc2::msg_send![obj(w), windowNumber];
        let content: ObjPtr = objc2::msg_send![obj(w), contentView];
        let frame: objc2_foundation::NSRect = objc2::msg_send![obj(content), frame];
        let cls = objc_getClass(c"NSEvent".as_ptr()) as *mut objc2::runtime::AnyObject;
        let app: ObjPtr = objc2::msg_send![objc_getClass(c"NSApplication".as_ptr()) as *mut objc2::runtime::AnyObject, sharedApplication];
        for (ty, pressure) in [(1usize, 1.0f32), (2usize, 0.0f32)] {
            let ev: ObjPtr = objc2::msg_send![
                cls,
                mouseEventWithType: ty,
                location: NSPoint::new(x, frame.size.height - y),
                modifierFlags: 0usize,
                timestamp: 0.0f64,
                windowNumber: number,
                context: None::<&objc2::runtime::AnyObject>,
                eventNumber: 1isize,
                clickCount: 1isize,
                pressure: pressure
            ];
            if ev.is_null() {
                return Err("cannot create the mouse event".into());
            }
            let _: () = objc2::msg_send![obj(app), postEvent: obj(ev), atStart: false];
        }
    }
    Ok(())
}

/// Debug: description of the first responder of the window of `window` (class name and whether a CEF view contains it).
pub fn describe_first_responder(window: raw_window_handle::RawWindowHandle) -> String {
    let Ok(w) = window_of(window) else { return "no window".into() };
    unsafe {
        let r: ObjPtr = objc2::msg_send![obj(w), firstResponder];
        if r.is_null() {
            return "none".into();
        }
        let name: *const objc2::runtime::AnyObject = objc2::msg_send![obj(r), className];
        let desc = if name.is_null() { String::new() } else { objc2_foundation::NSString::to_string(&*(name as *const objc2_foundation::NSString)) };
        let in_cef = crate::platform::native_view_handles().iter().any(|v| is_descendant(r, *v));
        format!("{desc} in_cef={in_cef}")
    }
}

/// Test helper: whether the window of `window` is the application's key window.
pub fn is_key_window(window: raw_window_handle::RawWindowHandle) -> bool {
    let Ok(w) = window_of(window) else { return false };
    unsafe { objc2::msg_send![obj(w), isKeyWindow] }
}

/// Test helper: like [`post_key_event`], but addressed to the current key window (what the keyboard does).
/// Returns false when no window is key.
pub fn post_key_to_key_window(key_code: u16, chars: &str, flags: usize, down: bool) -> bool {
    post_key_to_key_window_ex(key_code, chars, flags, down, false)
}

/// Like [`post_key_to_key_window`], with the auto-repeat flag.
pub fn post_key_to_key_window_ex(key_code: u16, chars: &str, flags: usize, down: bool, repeat: bool) -> bool {
    use objc2_foundation::{NSPoint, NSString};
    unsafe {
        let app: ObjPtr = objc2::msg_send![objc_getClass(c"NSApplication".as_ptr()) as *mut objc2::runtime::AnyObject, sharedApplication];
        let key: ObjPtr = objc2::msg_send![obj(app), keyWindow];
        if key.is_null() {
            return false;
        }
        let number: isize = objc2::msg_send![obj(key), windowNumber];
        let cls = objc_getClass(c"NSEvent".as_ptr()) as *mut objc2::runtime::AnyObject;
        let s = NSString::from_str(chars);
        let ev: ObjPtr = objc2::msg_send![
            cls,
            keyEventWithType: if down { 10usize } else { 11usize },
            location: NSPoint::new(0.0, 0.0),
            modifierFlags: flags,
            timestamp: 0.0f64,
            windowNumber: number,
            context: None::<&objc2::runtime::AnyObject>,
            characters: &*s,
            charactersIgnoringModifiers: &*s,
            isARepeat: repeat,
            keyCode: key_code
        ];
        if ev.is_null() {
            return false;
        }
        let _: () = objc2::msg_send![obj(app), postEvent: obj(ev), atStart: false];
        true
    }
}

/// Debug: "title / class / first responder class" of the application's key window.
pub fn describe_key_window() -> String {
    unsafe {
        let app: ObjPtr = objc2::msg_send![objc_getClass(c"NSApplication".as_ptr()) as *mut objc2::runtime::AnyObject, sharedApplication];
        let key: ObjPtr = objc2::msg_send![obj(app), keyWindow];
        if key.is_null() {
            return "no key window".into();
        }
        let name = |o: ObjPtr| -> String {
            if o.is_null() {
                return "none".into();
            }
            let n: *const objc2::runtime::AnyObject = objc2::msg_send![obj(o), className];
            if n.is_null() { String::new() } else { objc2_foundation::NSString::to_string(&*(n as *const objc2_foundation::NSString)) }
        };
        let title: *const objc2::runtime::AnyObject = objc2::msg_send![obj(key), title];
        let title = if title.is_null() { String::new() } else { objc2_foundation::NSString::to_string(&*(title as *const objc2_foundation::NSString)) };
        let r: ObjPtr = objc2::msg_send![obj(key), firstResponder];
        format!("title={title:?} class={} responder={}", name(key), name(r))
    }
}

/// One native subview seen by [`debug_subviews`].
#[derive(Debug, Clone)]
pub struct DebugSubview {
    /// Address of the window (`NSWindow*`) the view hangs in.
    pub window: usize,
    pub class: String,
    pub ptr: usize,
    pub hidden: bool,
}

/// Test helper: the subviews (depth 1 and 2) of the content view of every window of the application.
pub fn debug_subviews() -> Vec<DebugSubview> {
    let mut out = Vec::new();
    unsafe {
        let app: ObjPtr = objc2::msg_send![objc_getClass(c"NSApplication".as_ptr()) as *mut objc2::runtime::AnyObject, sharedApplication];
        let windows: ObjPtr = objc2::msg_send![obj(app), windows];
        let count: usize = objc2::msg_send![obj(windows), count];
        let class_of = |o: ObjPtr| -> String {
            let n: *const objc2::runtime::AnyObject = objc2::msg_send![obj(o), className];
            if n.is_null() { String::new() } else { objc2_foundation::NSString::to_string(&*(n as *const objc2_foundation::NSString)) }
        };
        for i in 0..count {
            let w: ObjPtr = objc2::msg_send![obj(windows), objectAtIndex: i];
            let content: ObjPtr = objc2::msg_send![obj(w), contentView];
            if content.is_null() {
                continue;
            }
            let mut level = vec![content];
            for _ in 0..2 {
                let mut next = Vec::new();
                for v in level {
                    let subs: ObjPtr = objc2::msg_send![obj(v), subviews];
                    let n: usize = objc2::msg_send![obj(subs), count];
                    for j in 0..n {
                        let s: ObjPtr = objc2::msg_send![obj(subs), objectAtIndex: j];
                        let hidden: bool = objc2::msg_send![obj(s), isHidden];
                        out.push(DebugSubview { window: w as usize, class: class_of(s), ptr: s as usize, hidden });
                        next.push(s);
                    }
                }
                level = next;
            }
        }
    }
    out
}

#[cfg(test)]
mod non_latin_tests {
    use super::{is_non_latin_letter, paste_action, NativeKey};

    fn key(chars: &str, key_code: u16, cmd: bool, alt: bool, shift: bool) -> NativeKey {
        NativeKey { kind: 0, repeat: false, chars: chars.into(), cmd, ctrl: false, alt, shift, key_code }
    }

    #[test]
    fn paste_chords() {
        assert_eq!(paste_action(&key("v", 9, true, false, false)), Some("paste:"));
        assert_eq!(paste_action(&key("v", 9, true, true, true)), Some("pasteAsPlainText:"));
        // Cyrillic layout: the V key types a non-Latin letter.
        assert_eq!(paste_action(&key("м", 9, true, false, false)), Some("paste:"));
        // Dvorak: the V key types another letter, Cmd+V is on the character.
        assert_eq!(paste_action(&key(".", 9, true, false, false)), None);
        assert_eq!(paste_action(&key("v", 47, true, false, false)), Some("paste:"));
        assert_eq!(paste_action(&key("v", 9, false, false, false)), None);
        assert_eq!(paste_action(&key("v", 9, true, false, true)), None);
        assert_eq!(paste_action(&key("c", 8, true, false, false)), None);
        let mut up = key("v", 9, true, false, false);
        up.kind = 1;
        assert_eq!(paste_action(&up), None);
    }

    #[test]
    fn detects_non_latin_letters_only() {
        assert!(is_non_latin_letter("й"));
        assert!(is_non_latin_letter("ض"));
        assert!(!is_non_latin_letter("q"));
        assert!(!is_non_latin_letter("a"));
        assert!(!is_non_latin_letter("é"));
        assert!(!is_non_latin_letter(""));
        assert!(!is_non_latin_letter("1"));
    }
}

// ---------------------------------------------------------------------------------------------
// Debug trace (SIGNET_TRACE_VIEWS=1): one line per change of the geometry of every browser view tree.
// ---------------------------------------------------------------------------------------------

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

fn tree_of(view: *mut c_void, depth: usize, out: &mut String) {
    use objc2_foundation::NSRect;
    unsafe {
        let v = &*(view as *mut objc2::runtime::AnyObject);
        let f: NSRect = objc2::msg_send![v, frame];
        let hidden: bool = objc2::msg_send![v, isHidden];
        let layer: *mut objc2::runtime::AnyObject = objc2::msg_send![v, layer];
        let (scale, lw, lh, cw) = if layer.is_null() {
            (0.0f64, 0.0, 0.0, 0.0)
        } else {
            let s: f64 = objc2::msg_send![&*layer, contentsScale];
            let b: NSRect = objc2::msg_send![&*layer, bounds];
            (s, b.size.width, b.size.height, 0.0)
        };
        let _ = cw;
        let win: *mut objc2::runtime::AnyObject = objc2::msg_send![v, window];
        let bs: f64 = if win.is_null() { -1.0 } else { objc2::msg_send![&*win, backingScaleFactor] };
        let cls: *const objc2::runtime::AnyClass = objc2::msg_send![v, class];
        out.push_str(&format!(
            "{}{} ({:.0},{:.0},{:.0}x{:.0}) h={} layer={:.0}x{:.0}@{} win@{}; ",
            " ".repeat(depth),
            (*cls).name().to_str().unwrap_or("?"),
            f.origin.x, f.origin.y, f.size.width, f.size.height, hidden as u8, lw, lh, scale, bs
        ));
        let subs: *mut objc2::runtime::AnyObject = objc2::msg_send![v, subviews];
        let n: usize = objc2::msg_send![&*subs, count];
        for i in 0..n {
            let s: *mut c_void = objc2::msg_send![&*subs, objectAtIndex: i];
            tree_of(s, depth + 1, out);
        }
    }
}

extern "C" fn trace_tick(_ctx: *mut c_void) {
    use std::sync::Mutex;
    static LAST: Mutex<String> = Mutex::new(String::new());
    static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let t0 = *T0.get_or_init(std::time::Instant::now);
    let mut s = String::new();
    for h in crate::platform::native_view_handles() {
        tree_of(h, 0, &mut s);
    }
    let mut last = LAST.lock().unwrap();
    if *last != s {
        let _ = t0;
        let ep = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        eprintln!("[trace {}] {}", ep, s);
        *last = s;
    }
}

/// Starts the trace thread when `SIGNET_TRACE_VIEWS` is set (polls the main thread every 8 ms).
pub fn trace_views_start() {
    if std::env::var_os("SIGNET_TRACE_VIEWS").is_none() {
        return;
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_millis(8));
        unsafe { dispatch_async_f(&raw const _dispatch_main_q, std::ptr::null_mut(), trace_tick) };
    });
}

// ---------------------------------------------------------------------------------------------
// Page view that always fills its host
// ---------------------------------------------------------------------------------------------
//
// Chromium creates a new `RenderWidgetHostViewCocoa` for every renderer swap (first navigation, cross-site
// navigation, reload after a crash) with a small default frame and only sizes it a few frames later: the page shows
// at that size, then jumps to the pane size. Inside a `WebContentsViewCocoa` the page view always fills it, so the
// frame is forced to the superview bounds when the view is attached and on every `setFrame:` (the right value is
// what Chromium sets a moment later anyway).

static ORIGINAL_SET_FRAME: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIGINAL_DID_MOVE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

fn superview_bounds_if_hosted(view: Id) -> Option<objc2_foundation::NSRect> {
    unsafe {
        let v = &*(view as *mut objc2::runtime::AnyObject);
        let sup: *mut objc2::runtime::AnyObject = objc2::msg_send![v, superview];
        if sup.is_null() {
            return None;
        }
        let cls = objc_getClass(c"WebContentsViewCocoa".as_ptr());
        if cls.is_null() {
            return None;
        }
        let ok: bool = objc2::msg_send![&*sup, isKindOfClass: &*(cls as *mut objc2::runtime::AnyObject)];
        if !ok {
            return None;
        }
        let b: objc2_foundation::NSRect = objc2::msg_send![&*sup, bounds];
        (b.size.width >= 1.0 && b.size.height >= 1.0).then_some(b)
    }
}

extern "C" fn rwhv_set_frame(this: Id, sel: Sel, frame: objc2_foundation::NSRect) {
    let orig = ORIGINAL_SET_FRAME.load(Ordering::Relaxed);
    if orig.is_null() {
        return;
    }
    let f: extern "C" fn(Id, Sel, objc2_foundation::NSRect) = unsafe { std::mem::transmute(orig) };
    f(this, sel, superview_bounds_if_hosted(this).unwrap_or(frame));
}

extern "C" fn rwhv_did_move_to_superview(this: Id, sel: Sel) {
    let orig = ORIGINAL_DID_MOVE.load(Ordering::Relaxed);
    if !orig.is_null() {
        let f: extern "C" fn(Id, Sel) = unsafe { std::mem::transmute(orig) };
        f(this, sel);
    }
    if let Some(b) = superview_bounds_if_hosted(this) {
        unsafe {
            let v = &*(this as *mut objc2::runtime::AnyObject);
            let _: () = objc2::msg_send![v, setFrame: b];
        }
    }
}

/// Installs the hooks above. Call once CEF is loaded (the Chromium classes exist from then on). Idempotent.
pub fn install_page_view_fill() {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::SeqCst) || std::env::var_os("SIGNET_NO_PAGE_FILL").is_some() {
        return;
    }
    unsafe {
        let cls = objc_getClass(c"RenderWidgetHostViewCocoa".as_ptr());
        if cls.is_null() {
            return;
        }
        let set_sel = sel_registerName(c"setFrame:".as_ptr());
        let m = class_getInstanceMethod(cls, set_sel);
        if !m.is_null() {
            ORIGINAL_SET_FRAME.store(method_getImplementation(m) as *mut c_void, Ordering::Relaxed);
            // The method may be inherited from NSView: add an override on the class, else replace its own.
            if !class_addMethod(cls, set_sel, rwhv_set_frame as Imp, method_get_types(m)) {
                method_setImplementation(m, rwhv_set_frame as Imp);
            }
        }
        let move_sel = sel_registerName(c"viewDidMoveToSuperview".as_ptr());
        let m = class_getInstanceMethod(cls, move_sel);
        if !m.is_null() {
            ORIGINAL_DID_MOVE.store(method_getImplementation(m) as *mut c_void, Ordering::Relaxed);
            if !class_addMethod(cls, move_sel, rwhv_did_move_to_superview as Imp, method_get_types(m)) {
                method_setImplementation(m, rwhv_did_move_to_superview as Imp);
            }
        }
    }
}

unsafe fn method_get_types(m: *mut c_void) -> *const c_char {
    unsafe extern "C" {
        fn method_getTypeEncoding(m: *mut c_void) -> *const c_char;
    }
    unsafe { method_getTypeEncoding(m) }
}

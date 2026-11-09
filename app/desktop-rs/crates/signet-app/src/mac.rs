//! macOS glue: child windows (floating surfaces move with the main window), window frames, traffic lights and
//! native popup menus. Raw `NSView` pointers come from `raw-window-handle`.

use std::cell::RefCell;
use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

fn window_of(view: *mut c_void) -> *mut AnyObject {
    if view.is_null() {
        return std::ptr::null_mut();
    }
    let v = view as *mut AnyObject;
    unsafe { msg_send![v, window] }
}

/// Makes `child` a child window of `parent` (ordered above, moves with it).
pub fn add_child(parent_view: *mut c_void, child_view: *mut c_void) {
    let (p, c) = (window_of(parent_view), window_of(child_view));
    if p.is_null() || c.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![p, addChildWindow: c, ordered: 1isize];
    }
}

/// Whether the window of `view` is the key window.
pub fn is_key(view: *mut c_void) -> bool {
    let win = window_of(view);
    !win.is_null() && unsafe { msg_send![win, isKeyWindow] }
}

/// Makes the window of `view` the key window (keyboard input goes to it).
pub fn make_key(view: *mut c_void) {
    let win = window_of(view);
    if !win.is_null() {
        unsafe {
            let _: () = msg_send![win, makeKeyWindow];
        }
    }
}

/// Sets the frame of a window from a rectangle in screen points, top-left origin.
pub fn set_frame(view: *mut c_void, x: f64, y: f64, w: f64, h: f64) {
    let win = window_of(view);
    if win.is_null() {
        return;
    }
    unsafe {
        let screen: *mut AnyObject = msg_send![win, screen];
        let sh: f64 = if screen.is_null() {
            0.0
        } else {
            let f: NSRect = msg_send![screen, frame];
            f.size.height
        };
        // The primary screen's top-left is the reference of GPUI global coordinates.
        let main: *mut AnyObject = msg_send![objc2::class!(NSScreen), mainScreen];
        let primary_h: f64 = if main.is_null() {
            sh
        } else {
            let screens: *mut AnyObject = msg_send![objc2::class!(NSScreen), screens];
            let first: *mut AnyObject = msg_send![screens, firstObject];
            let f: NSRect = msg_send![first, frame];
            f.size.height
        };
        let frame = NSRect::new(NSPoint::new(x, primary_h - y - h), NSSize::new(w, h));
        let _: () = msg_send![win, setFrame: frame, display: true];
    }
}

/// Orders a window out (hidden) or back in.
pub fn set_hidden(view: *mut c_void, hidden: bool) {
    let win = window_of(view);
    if win.is_null() {
        return;
    }
    unsafe {
        if hidden {
            let _: () = msg_send![win, orderOut: std::ptr::null_mut::<AnyObject>()];
        } else {
            let _: () = msg_send![win, orderFront: std::ptr::null_mut::<AnyObject>()];
        }
    }
}

/// The window ignores the mouse (transparent surfaces that must not eat clicks).
pub fn set_ignores_mouse(view: *mut c_void, ignore: bool) {
    let win = window_of(view);
    if !win.is_null() {
        unsafe {
            let _: () = msg_send![win, setIgnoresMouseEvents: ignore];
        }
    }
}

/// Shows or hides the three traffic light buttons.
pub fn set_traffic_lights_visible(view: *mut c_void, visible: bool) {
    let win = window_of(view);
    if win.is_null() {
        return;
    }
    for kind in 0isize..3 {
        unsafe {
            let b: *mut AnyObject = msg_send![win, standardWindowButton: kind];
            if !b.is_null() {
                let _: () = msg_send![b, setHidden: !visible];
            }
        }
    }
}

/// Hides the app from the Dock bar (Cmd+H) and others: thin wrappers over `NSApplication`.
pub fn hide_app() {
    unsafe {
        let app: *mut AnyObject = msg_send![objc2::class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, hide: std::ptr::null_mut::<AnyObject>()];
    }
}

thread_local! {
    static PICKED: RefCell<Option<isize>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SignetMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(fire:))]
        fn fire(&self, sender: &NSMenuItem) {
            PICKED.with(|p| *p.borrow_mut() = Some(sender.tag()));
        }
    }
);

/// One entry of a popup menu.
#[derive(Clone, Debug)]
pub enum PopupItem {
    Separator,
    Item { label: String, enabled: bool, checked: bool, submenu: Vec<PopupItem> },
}

fn build(mtm: MainThreadMarker, items: &[PopupItem], target: &MenuTarget, next_tag: &mut isize) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    for it in items {
        match it {
            PopupItem::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            PopupItem::Item { label, enabled, checked, submenu } => {
                let title = NSString::from_str(label);
                let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), &title, Some(sel!(fire:)), &NSString::from_str("")) };
                if submenu.is_empty() {
                    unsafe { item.setTarget(Some(target)) };
                    item.setTag(*next_tag);
                    *next_tag += 1;
                } else {
                    item.setSubmenu(Some(&build(mtm, submenu, target, next_tag)));
                }
                item.setEnabled(*enabled);
                item.setState(if *checked { 1 } else { 0 });
                menu.addItem(&item);
            }
        }
    }
    menu
}

/// Flat index of an activatable item in depth-first order, matching the tags handed out by `build`.
pub fn flat_ids(items: &[PopupItem], out: &mut Vec<usize>, next: &mut usize) {
    for it in items {
        if let PopupItem::Item { submenu, .. } = it {
            if submenu.is_empty() {
                out.push(*next);
            }
            *next += 1;
        }
    }
}

/// Shows a popup menu at (`x`, `y`) in the view's top-left coordinates and returns the tag index of the chosen leaf
/// (depth-first order of leaves), or None if dismissed.
pub fn popup(view: *mut c_void, x: f64, y: f64, items: &[PopupItem]) -> Option<usize> {
    let mtm = MainThreadMarker::new()?;
    let v = view as *mut AnyObject;
    if v.is_null() {
        return None;
    }
    let target: Retained<MenuTarget> = unsafe { msg_send![MenuTarget::alloc(mtm), init] };
    let mut tag = 0isize;
    let menu = build(mtm, items, &target, &mut tag);
    PICKED.with(|p| *p.borrow_mut() = None);
    unsafe {
        let at = if x.is_finite() && y.is_finite() {
            let flipped: bool = msg_send![v, isFlipped];
            let h: f64 = {
                let b: NSRect = msg_send![v, bounds];
                b.size.height
            };
            NSPoint::new(x, if flipped { y } else { h - y })
        } else {
            // No position given: at the pointer, like a native context menu.
            let win: *mut AnyObject = msg_send![v, window];
            if win.is_null() {
                return None;
            }
            let p: NSPoint = msg_send![win, mouseLocationOutsideOfEventStream];
            msg_send![v, convertPoint: p, fromView: std::ptr::null_mut::<AnyObject>()]
        };
        let _: bool = msg_send![&*menu, popUpMenuPositioningItem: std::ptr::null_mut::<AnyObject>(), atLocation: at, inView: v];
    }
    PICKED.with(|p| p.borrow_mut().take()).map(|t| t as usize)
}

/// Looks of a floating surface: no shadow, no traffic lights, not draggable by its background.
pub fn style_floating(view: *mut c_void) {
    let win = window_of(view);
    if win.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![win, setHasShadow: false];
        let _: () = msg_send![win, setMovable: false];
    }
    set_traffic_lights_visible(view, false);
}

/// The system setting "Reduce motion".
pub fn prefers_reduced_motion() -> bool {
    unsafe {
        let ws: *mut AnyObject = msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return false;
        }
        msg_send![ws, accessibilityDisplayShouldReduceMotion]
    }
}

/// Whether the left mouse button is down right now (any app).
pub fn left_button_down() -> bool {
    let mask: usize = unsafe { msg_send![objc2::class!(NSEvent), pressedMouseButtons] };
    mask & 1 != 0
}

/// Pointer position in screen points, top-left origin of the primary screen (the GPUI global coordinates).
pub fn mouse_location() -> (f64, f64) {
    unsafe {
        let p: NSPoint = msg_send![objc2::class!(NSEvent), mouseLocation];
        let screens: *mut AnyObject = msg_send![objc2::class!(NSScreen), screens];
        let first: *mut AnyObject = msg_send![screens, firstObject];
        let h = if first.is_null() {
            0.0
        } else {
            let f: NSRect = msg_send![first, frame];
            f.size.height
        };
        (p.x, h - p.y)
    }
}

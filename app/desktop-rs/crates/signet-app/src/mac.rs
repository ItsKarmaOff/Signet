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
        // Long-lived surfaces (the parked floating sidebar) must not appear in the Window menu or in Cmd+backtick
        // cycling: NSWindowCollectionBehaviorIgnoresCycle.
        let _: () = msg_send![win, setExcludedFromWindowsMenu: true];
        let _: () = msg_send![win, setCollectionBehavior: 1usize << 6];
    }
    set_traffic_lights_visible(view, false);
}

/// Drops the title bar style of a floating card window: no system corner mask or edge (their radius depends on the
/// macOS version), the GPUI content draws its own rounded card.
pub fn make_borderless(view: *mut c_void) {
    let win = window_of(view);
    if win.is_null() {
        return;
    }
    unsafe {
        // NSWindowStyleMaskBorderless (0) | NSWindowStyleMaskFullSizeContentView (1 << 15): GPUI's frame math keeps the
        // content rect equal to the frame.
        let _: () = msg_send![win, setStyleMask: 1usize << 15];
        // A style mask change resets the first responder: give it back to the GPUI view (text fields).
        let _: bool = msg_send![win, makeFirstResponder: view as *mut AnyObject];
    }
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

/// The system setting "Reduce transparency".
pub fn prefers_reduced_transparency() -> bool {
    unsafe {
        let ws: *mut AnyObject = msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        if ws.is_null() {
            return false;
        }
        msg_send![ws, accessibilityDisplayShouldReduceTransparency]
    }
}

/// Work area (menu bar and Dock excluded) of every screen as `(x, y, width, height)` in the GPUI global coordinates:
/// top-left origin of the primary screen. GPUI's own display bounds are local to each display.
pub fn work_areas() -> Vec<(f64, f64, f64, f64)> {
    unsafe {
        let screens: *mut AnyObject = msg_send![objc2::class!(NSScreen), screens];
        let count: usize = msg_send![screens, count];
        let mut out = Vec::with_capacity(count);
        let mut primary_h = 0.0;
        for i in 0..count {
            let sc: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            if i == 0 {
                let f: NSRect = msg_send![sc, frame];
                primary_h = f.size.height;
            }
            let vf: NSRect = msg_send![sc, visibleFrame];
            out.push((vf.origin.x, primary_h - vf.origin.y - vf.size.height, vf.size.width, vf.size.height));
        }
        out
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

/// The system's double-click action on a title bar, read from the global preferences on every call.
pub fn titlebar_action() -> signet_core::titlebar::TitlebarAction {
    use objc2::class;
    unsafe {
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        if defaults.is_null() {
            return signet_core::titlebar::TitlebarAction::Zoom;
        }
        let domain = NSString::from_str("NSGlobalDomain");
        let dict: *mut AnyObject = msg_send![defaults, persistentDomainForName: &*domain];
        if dict.is_null() {
            return signet_core::titlebar::mac_action(None, false);
        }
        let string_class = class!(NSString);
        let get = |key: &str| -> *mut AnyObject {
            let k = NSString::from_str(key);
            msg_send![dict, objectForKey: &*k]
        };
        let value = get("AppleActionOnDoubleClick");
        let action: Option<String> = if !value.is_null() && msg_send![value, isKindOfClass: string_class] {
            let s = &*(value as *const NSString);
            Some(s.to_string())
        } else {
            None
        };
        let legacy = get("AppleMiniaturizeOnDoubleClick");
        let legacy_min: bool = if legacy.is_null() { false } else { msg_send![legacy, boolValue] };
        signet_core::titlebar::mac_action(action.as_deref(), legacy_min)
    }
}

// ------------------------------------------------------------------ floating card: native slide and corner radius

/// How the floating card's layer changes (the window stays in place and clips it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardSlide {
    /// Parked at once, no animation.
    Hide,
    /// Slides in from its edge and fades in.
    Enter,
    /// Slides out to its edge and fades out.
    Exit,
}

unsafe fn ns_number(v: f64) -> *mut AnyObject {
    unsafe { msg_send![objc2::class!(NSNumber), numberWithDouble: v] }
}

unsafe fn ns_str(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

unsafe fn key_value(layer: *mut AnyObject, path: &str) -> Option<f64> {
    if layer.is_null() {
        return None;
    }
    unsafe {
        let n: *mut AnyObject = msg_send![layer, valueForKeyPath: &*ns_str(path)];
        if n.is_null() {
            return None;
        }
        let v: f64 = msg_send![n, doubleValue];
        Some(v)
    }
}

// NSWindowBelow (NSWindowOrderingMode).
const NS_WINDOW_BELOW: isize = -1;
// NSViewWidthSizable | NSViewHeightSizable (NSAutoresizingMaskOptions).
const NS_VIEW_WIDTH_HEIGHT_SIZABLE: usize = 2 | 16;
// NSVisualEffectMaterialUnderWindowBackground, NSVisualEffectBlendingModeBehindWindow, NSVisualEffectStateActive.
const NS_VE_MATERIAL_UNDER_WINDOW_BACKGROUND: isize = 21;
const NS_VE_BLENDING_BEHIND_WINDOW: isize = 0;
const NS_VE_STATE_ACTIVE: isize = 1;
// NSImageResizingModeStretch.
const NS_IMAGE_RESIZING_STRETCH: isize = 1;
const CARD_BLUR_ID: &str = "signet.card-blur";

/// The card's blur view (sibling below the GPUI view), if installed.
unsafe fn card_blur_view(view: *mut AnyObject) -> *mut AnyObject {
    if view.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let sup: *mut AnyObject = msg_send![view, superview];
        if sup.is_null() {
            return std::ptr::null_mut();
        }
        let subs: *mut AnyObject = msg_send![sup, subviews];
        if subs.is_null() {
            return std::ptr::null_mut();
        }
        let count: usize = msg_send![subs, count];
        let want = ns_str(CARD_BLUR_ID);
        for i in 0..count {
            let sub: *mut AnyObject = msg_send![subs, objectAtIndex: i];
            if sub.is_null() {
                continue;
            }
            let id: *mut AnyObject = msg_send![sub, identifier];
            if !id.is_null() {
                let same: bool = msg_send![id, isEqualToString: &*want];
                if same {
                    return sub;
                }
            }
        }
        std::ptr::null_mut()
    }
}

/// Sets the rounded mask image of a blur view: a stretchable rounded rect (cap insets = radius), the documented way to
/// clip a visual effect view.
unsafe fn blur_set_mask(blur: *mut AnyObject, radius: f64) {
    use objc2::runtime::Bool;
    use objc2_foundation::NSEdgeInsets;
    unsafe {
        let side = signet_core::floating::blur_mask_side(radius);
        let r = (side - 1.0) / 2.0;
        let block = block2::RcBlock::new(move |rect: NSRect| -> Bool {
            let color: *mut AnyObject = msg_send![objc2::class!(NSColor), blackColor];
            let _: () = msg_send![color, set];
            let path: *mut AnyObject = msg_send![objc2::class!(NSBezierPath), bezierPathWithRoundedRect: rect, xRadius: r, yRadius: r];
            let _: () = msg_send![path, fill];
            Bool::YES
        });
        let image: *mut AnyObject = msg_send![objc2::class!(NSImage), imageWithSize: NSSize::new(side, side), flipped: false, drawingHandler: &*block];
        if image.is_null() {
            return;
        }
        let _: () = msg_send![image, setCapInsets: NSEdgeInsets { top: r, left: r, bottom: r, right: r }];
        let _: () = msg_send![image, setResizingMode: NS_IMAGE_RESIZING_STRETCH];
        let _: () = msg_send![blur, setMaskImage: image];
    }
}

/// Installs the card's blur view under the GPUI view (hidden, alpha 0), once. Returns whether a blur view is present
/// afterwards. `radius`: corner radius of the card in points.
pub fn card_blur_install(view: *mut c_void, radius: f64) -> bool {
    use objc2::runtime::AnyClass;
    let v = view as *mut AnyObject;
    if v.is_null() {
        return false;
    }
    unsafe {
        let existing = card_blur_view(v);
        if !existing.is_null() {
            blur_set_mask(existing, radius);
            return true;
        }
        let sup: *mut AnyObject = msg_send![v, superview];
        if sup.is_null() {
            return false;
        }
        let frame: NSRect = msg_send![sup, bounds];
        // GPUI's own subclass: same material, same tint and saturation stripping as the docked sidebar's blur.
        let blur: *mut AnyObject = if let Some(cls) = AnyClass::get(c"BlurredView") {
            let a: *mut AnyObject = msg_send![cls, alloc];
            msg_send![a, initWithFrame: frame]
        } else {
            signet_ui::trace::mark(|| "card blur: BlurredView class missing, plain NSVisualEffectView".into());
            let a: *mut AnyObject = msg_send![objc2::class!(NSVisualEffectView), alloc];
            let b: *mut AnyObject = msg_send![a, initWithFrame: frame];
            if !b.is_null() {
                let _: () = msg_send![b, setMaterial: NS_VE_MATERIAL_UNDER_WINDOW_BACKGROUND];
                let _: () = msg_send![b, setBlendingMode: NS_VE_BLENDING_BEHIND_WINDOW];
                let _: () = msg_send![b, setState: NS_VE_STATE_ACTIVE];
            }
            b
        };
        if blur.is_null() {
            return false;
        }
        // Hidden before it joins the view tree: no blurred rectangle can be committed. Never set `appearance`: the view
        // inherits NSApp's, like the docked sidebar.
        let _: () = msg_send![blur, setIdentifier: &*ns_str(CARD_BLUR_ID)];
        let _: () = msg_send![blur, setAutoresizingMask: NS_VIEW_WIDTH_HEIGHT_SIZABLE];
        let _: () = msg_send![blur, setWantsLayer: true];
        let _: () = msg_send![blur, setAlphaValue: 0.0f64];
        let _: () = msg_send![blur, setHidden: true];
        blur_set_mask(blur, radius);
        if let Some(txn) = AnyClass::get(c"CATransaction") {
            let _: () = msg_send![txn, begin];
            let _: () = msg_send![txn, setDisableActions: true];
            let _: () = msg_send![sup, addSubview: blur, positioned: NS_WINDOW_BELOW, relativeTo: v];
            let _: () = msg_send![txn, commit];
        } else {
            let _: () = msg_send![sup, addSubview: blur, positioned: NS_WINDOW_BELOW, relativeTo: v];
        }
        // The superview holds the only lasting reference now.
        let _: () = msg_send![blur, release];
        signet_ui::trace::mark(|| format!("card blur installed r={radius}"));
        true
    }
}

/// Rebuilds the rounded mask of the card's blur view (the radius follows the window corner).
pub fn card_blur_shape(view: *mut c_void, radius: f64) {
    unsafe {
        let bv = card_blur_view(view as *mut AnyObject);
        if !bv.is_null() {
            blur_set_mask(bv, radius);
        }
    }
}

/// Removes the card's blur view.
pub fn card_blur_remove(view: *mut c_void) {
    unsafe {
        let bv = card_blur_view(view as *mut AnyObject);
        if !bv.is_null() {
            let _: () = msg_send![bv, removeFromSuperview];
            signet_ui::trace::mark(|| "card blur removed".into());
        }
    }
}

/// Cold open: the blur view joins the slide and fade already running on the GPUI layer, once the window presented its
/// first frame. No-op when the card is parked.
pub fn card_blur_join(view: *mut c_void) {
    use objc2::runtime::AnyClass;
    let v = view as *mut AnyObject;
    if v.is_null() {
        return;
    }
    unsafe {
        let bv = card_blur_view(v);
        if bv.is_null() {
            return;
        }
        let a: f64 = msg_send![v, alphaValue];
        if a < 0.5 {
            return;
        }
        let layer: *mut AnyObject = msg_send![v, layer];
        let blayer: *mut AnyObject = msg_send![bv, layer];
        let txn = AnyClass::get(c"CATransaction");
        if let Some(t) = txn {
            let _: () = msg_send![t, begin];
            let _: () = msg_send![t, setDisableActions: true];
        }
        let _: () = msg_send![bv, setHidden: false];
        let _: () = msg_send![bv, setAlphaValue: 1.0f64];
        if !layer.is_null() && !blayer.is_null() {
            for key in ["signet.slide", "signet.fade"] {
                let anim: *mut AnyObject = msg_send![layer, animationForKey: &*ns_str(key)];
                if !anim.is_null() {
                    let _: () = msg_send![blayer, addAnimation: anim, forKey: &*ns_str(key)];
                }
            }
        }
        if let Some(t) = txn {
            let _: () = msg_send![t, commit];
        }
        signet_ui::trace::mark(|| "card blur joined".into());
    }
}

/// After an exit is over: hides the blur view if the card is still parked. No-op when it was revealed again.
pub fn card_blur_park(view: *mut c_void) {
    let v = view as *mut AnyObject;
    if v.is_null() {
        return;
    }
    unsafe {
        let bv = card_blur_view(v);
        if bv.is_null() {
            return;
        }
        let a: f64 = msg_send![v, alphaValue];
        if a >= 0.5 {
            return;
        }
        let blayer: *mut AnyObject = msg_send![bv, layer];
        if !blayer.is_null() {
            let _: () = msg_send![blayer, removeAnimationForKey: &*ns_str("signet.slide")];
            let _: () = msg_send![blayer, removeAnimationForKey: &*ns_str("signet.fade")];
        }
        let _: () = msg_send![bv, setHidden: true];
    }
}

/// Slides and fades the GPUI layer of a floating card (and its blur view when `blur`) in the window server (Core
/// Animation), so the motion costs no GPUI frame. `dx`: offset of the parked card in points (minus the width on the
/// left, plus on the right).
pub fn card_slide(view: *mut c_void, how: CardSlide, dx: f64, reduced: bool, blur: bool) {
    use objc2::runtime::AnyClass;
    if view.is_null() {
        return;
    }
    let (Some(txn), Some(kf_cls), Some(basic_cls), Some(timing_cls)) =
        (AnyClass::get(c"CATransaction"), AnyClass::get(c"CAKeyframeAnimation"), AnyClass::get(c"CABasicAnimation"), AnyClass::get(c"CAMediaTimingFunction"))
    else {
        return;
    };
    let v = view as *mut AnyObject;
    unsafe {
        let layer: *mut AnyObject = msg_send![v, layer];
        if layer.is_null() {
            return;
        }
        let _: () = msg_send![txn, begin];
        let _: () = msg_send![txn, setDisableActions: true];
        // Where an interrupted motion stands right now (read once, shared by both targets).
        let pres: *mut AnyObject = msg_send![layer, presentationLayer];
        let src = if pres.is_null() { layer } else { pres };
        let cur_x = key_value(src, "transform.translation.x").unwrap_or(0.0);
        // The GPUI view first, then the blur view.
        let mut targets: Vec<(*mut AnyObject, *mut AnyObject)> = vec![(v, layer)];
        let bv = card_blur_view(v);
        if !bv.is_null() {
            let bl: *mut AnyObject = msg_send![bv, layer];
            if blur && how != CardSlide::Hide && !bl.is_null() {
                let _: () = msg_send![bv, setHidden: false];
                targets.push((bv, bl));
            } else {
                if !bl.is_null() {
                    let _: () = msg_send![bl, removeAnimationForKey: &*ns_str("signet.slide")];
                    let _: () = msg_send![bl, removeAnimationForKey: &*ns_str("signet.fade")];
                }
                let _: () = msg_send![bv, setAlphaValue: 0.0f64];
                let _: () = msg_send![bv, setHidden: true];
            }
        }
        let visible = how == CardSlide::Enter;
        // `+[CAMediaTimingFunction functionWithControlPoints::::]`: unnamed parts, sent by hand.
        let timing = |c1: f32, c2: f32, c3: f32, c4: f32| -> *mut AnyObject {
            let f: unsafe extern "C" fn(*const AnyClass, objc2::runtime::Sel, f32, f32, f32, f32) -> *mut AnyObject = std::mem::transmute(objc2::ffi::objc_msgSend as *const ());
            f(timing_cls, objc2::runtime::Sel::register(c"functionWithControlPoints::::"), c1, c2, c3, c4)
        };
        let ease_out = timing(0.2, 0.8, 0.2, 1.0);
        let mut from_x = cur_x;
        for (i, &(view_i, layer_i)) in targets.iter().enumerate() {
            let pres_i: *mut AnyObject = msg_send![layer_i, presentationLayer];
            let src_i = if pres_i.is_null() { layer_i } else { pres_i };
            let cur_a = key_value(src_i, "opacity").unwrap_or(1.0);
            let hidden_before = cur_a <= 0.001;
            let _: () = msg_send![layer_i, removeAnimationForKey: &*ns_str("signet.slide")];
            let _: () = msg_send![layer_i, removeAnimationForKey: &*ns_str("signet.fade")];
            // NSView alpha is the layer opacity: the model value is the end state, AppKit keeps the frame and position.
            let _: () = msg_send![view_i, setAlphaValue: if visible { 1.0f64 } else { 0.0f64 }];
            if i == 0 {
                from_x = if how == CardSlide::Enter && hidden_before && cur_x.abs() < 0.5 { dx } else { cur_x };
                signet_ui::trace::mark(|| format!("native slide {how:?} from {from_x:.0} dx {dx:.0} alpha {cur_a:.2} reduced {reduced} blur {}", targets.len() > 1));
            }
            match how {
                CardSlide::Hide => {}
                CardSlide::Enter => {
                    if !reduced {
                        let anim: *mut AnyObject = msg_send![kf_cls, animationWithKeyPath: &*ns_str("transform.translation.x")];
                        let values: *mut AnyObject = msg_send![objc2::class!(NSMutableArray), array];
                        let times: *mut AnyObject = msg_send![objc2::class!(NSMutableArray), array];
                        for (t, x) in signet_ui::motion::slide_keyframes(from_x, 0.0) {
                            let _: () = msg_send![values, addObject: ns_number(x)];
                            let _: () = msg_send![times, addObject: ns_number(t)];
                        }
                        let _: () = msg_send![anim, setValues: values];
                        let _: () = msg_send![anim, setKeyTimes: times];
                        let _: () = msg_send![anim, setCalculationMode: &*ns_str("linear")];
                        let _: () = msg_send![anim, setDuration: signet_ui::theme::duration::SLOW.as_secs_f64()];
                        let _: () = msg_send![layer_i, addAnimation: anim, forKey: &*ns_str("signet.slide")];
                    }
                    let fade: *mut AnyObject = msg_send![basic_cls, animationWithKeyPath: &*ns_str("opacity")];
                    let _: () = msg_send![fade, setFromValue: ns_number(if hidden_before { 0.0 } else { cur_a })];
                    let _: () = msg_send![fade, setToValue: ns_number(1.0)];
                    let _: () = msg_send![fade, setDuration: signet_ui::theme::duration::FAST.as_secs_f64()];
                    let _: () = msg_send![fade, setTimingFunction: ease_out];
                    let _: () = msg_send![layer_i, addAnimation: fade, forKey: &*ns_str("signet.fade")];
                }
                CardSlide::Exit => {
                    let dur = if reduced { signet_ui::theme::duration::FAST } else { signet_ui::theme::duration::NORMAL }.as_secs_f64();
                    if !reduced {
                        let anim: *mut AnyObject = msg_send![basic_cls, animationWithKeyPath: &*ns_str("transform.translation.x")];
                        let _: () = msg_send![anim, setFromValue: ns_number(from_x)];
                        let _: () = msg_send![anim, setToValue: ns_number(dx)];
                        let _: () = msg_send![anim, setDuration: dur];
                        let _: () = msg_send![anim, setTimingFunction: ease_out];
                        let _: () = msg_send![layer_i, addAnimation: anim, forKey: &*ns_str("signet.slide")];
                    }
                    let fade: *mut AnyObject = msg_send![basic_cls, animationWithKeyPath: &*ns_str("opacity")];
                    let _: () = msg_send![fade, setFromValue: ns_number(cur_a)];
                    let _: () = msg_send![fade, setToValue: ns_number(0.0)];
                    let _: () = msg_send![fade, setDuration: dur];
                    let _: () = msg_send![fade, setTimingFunction: ease_out];
                    let _: () = msg_send![layer_i, addAnimation: fade, forKey: &*ns_str("signet.fade")];
                }
            }
        }
        let _: () = msg_send![txn, commit];
    }
}

/// Sends a getter selector (no argument) whose return type was checked to be `d`.
unsafe fn send_f64(obj: *mut AnyObject, sel: objc2::runtime::Sel) -> f64 {
    unsafe {
        let f: unsafe extern "C" fn(*mut AnyObject, objc2::runtime::Sel) -> f64 = std::mem::transmute(objc2::ffi::objc_msgSend as *const ());
        f(obj, sel)
    }
}

/// Sends a getter selector (no argument) whose return type was checked to be an object.
unsafe fn send_obj(obj: *mut AnyObject, sel: objc2::runtime::Sel) -> *mut AnyObject {
    unsafe {
        let f: unsafe extern "C" fn(*mut AnyObject, objc2::runtime::Sel) -> *mut AnyObject = std::mem::transmute(objc2::ffi::objc_msgSend as *const ());
        f(obj, sel)
    }
}

/// Reads a CGFloat result of `sel` on `obj` when the object answers it with a `d` return type.
unsafe fn cgfloat_of(obj: *mut AnyObject, name: &std::ffi::CStr) -> Option<f64> {
    use objc2::runtime::Sel;
    use std::ffi::c_char;
    if obj.is_null() {
        return None;
    }
    unsafe {
        let sel = Sel::register(name);
        let ok: bool = msg_send![obj, respondsToSelector: sel];
        if !ok {
            return None;
        }
        let sig: *mut AnyObject = msg_send![obj, methodSignatureForSelector: sel];
        if sig.is_null() {
            return None;
        }
        let ty: *const c_char = msg_send![sig, methodReturnType];
        if ty.is_null() || *ty as u8 != b'd' {
            return None;
        }
        Some(send_f64(obj, sel))
    }
}

/// Corner radius of the window of `view` as AppKit draws it, when the system tells (macOS 26.1+ SPI
/// `-[NSView effectiveCornerRadii]` / `_effectiveCornerRadii` on the content view); None otherwise. `right`: the
/// card sits on the right side. Every step is guarded: any mismatch gives None.
pub fn window_corner_radius(view: *mut c_void, right: bool) -> Option<f64> {
    use objc2::runtime::Sel;
    use std::ffi::c_char;
    let win = window_of(view);
    if win.is_null() {
        return None;
    }
    unsafe {
        // Fullscreen windows have square corners.
        let mask: usize = msg_send![win, styleMask];
        if mask & (1 << 14) != 0 {
            return None;
        }
        let content: *mut AnyObject = msg_send![win, contentView];
        if content.is_null() {
            return None;
        }
        let sup: *mut AnyObject = msg_send![content, superview];
        let (a, b): (&std::ffi::CStr, &std::ffi::CStr) = if right { (c"topRight", c"bottomRight") } else { (c"topLeft", c"bottomLeft") };
        for obj in [content, sup] {
            if obj.is_null() {
                continue;
            }
            for name in [c"effectiveCornerRadii", c"_effectiveCornerRadii"] {
                let sel = Sel::register(name);
                let ok: bool = msg_send![obj, respondsToSelector: sel];
                if !ok {
                    continue;
                }
                let sig: *mut AnyObject = msg_send![obj, methodSignatureForSelector: sel];
                if sig.is_null() {
                    continue;
                }
                let ty: *const c_char = msg_send![sig, methodReturnType];
                // Only an object result (a boxed radii value) is read; a struct return is not touched blindly.
                if ty.is_null() || *ty as u8 != b'@' {
                    continue;
                }
                let radii = send_obj(obj, sel);
                let (Some(x), Some(y)) = (cgfloat_of(radii, a), cgfloat_of(radii, b)) else { continue };
                let r = x.max(y);
                if r.is_nan() || r <= 0.0 {
                    continue;
                }
                signet_ui::trace::mark(|| format!("window corner radius {r} from {}", name.to_string_lossy()));
                return Some(r);
            }
        }
        let r = cgfloat_of(win, c"_cornerRadius").filter(|r| *r > 0.0)?;
        signet_ui::trace::mark(|| format!("window corner radius {r} from _cornerRadius"));
        Some(r)
    }
}

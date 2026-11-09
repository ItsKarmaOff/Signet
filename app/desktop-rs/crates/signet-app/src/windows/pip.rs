//! The mini player windows (`windows/pip.ts`): a frameless panel, always on top and on every Space, hosting the
//! tab's re-parented CEF view, with a transparent child window over it for the controls
//! (`signet_ui::screens::windows::pip::Pip`; native views are drawn above all GPUI content, so the controls cannot
//! live in the panel). The core queues [`PipWindowOp`]s and the gestures of the controls ([`PipInput`]); this file
//! executes them with the pure geometry of `phase3/media/pip.rs`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyView, AnyWindowHandle, App, AppContext as _, Bounds, Context, IntoElement, Render, Styled as _, TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div,
    point, px, size,
};
use gpui_component::Root;
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use signet_core::types::Rect;
use signet_ui::screens::windows::pip::{PipHandle, PipResizePhase};
use signet_web::WebView;

use crate::browser::Browser;
use crate::phase3::{initial_bounds, fit_ratio, release_target, resized_bounds, PipInput, PipWindowOp};

struct Blank;

impl Render for Blank {
    fn render(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(gpui::rgb(0x000000))
    }
}

struct Drag {
    mouse: (f64, f64),
    start: Rect,
    samples: Vec<(std::time::Instant, (f64, f64))>,
}

struct Session {
    tab: String,
    view: WebView,
    panel: AnyWindowHandle,
    panel_raw: Option<RawWindowHandle>,
    controls: AnyWindowHandle,
    controls_raw: Option<RawWindowHandle>,
    bounds: Rect,
    ratio: f64,
    drag: Option<Drag>,
    /// The last frame given to the windows: identical frames are skipped.
    last_applied: std::cell::Cell<Option<Rect>>,
    resize_start: Option<Rect>,
    resize_last: Option<(PipHandle, f64, f64)>,
    /// Bumped when the window closes or a new glide starts: running tasks stop.
    epoch: u64,
}

/// The mini player window of the manager (at most one).
#[derive(Default)]
pub struct PipWindows {
    cur: Option<Rc<RefCell<Session>>>,
}

fn reduced_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::mac::prefers_reduced_motion()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn handle_name(h: PipHandle) -> &'static str {
    match h {
        PipHandle::N => "n",
        PipHandle::S => "s",
        PipHandle::E => "e",
        PipHandle::W => "w",
        PipHandle::NE => "ne",
        PipHandle::NW => "nw",
        PipHandle::SE => "se",
        PipHandle::SW => "sw",
    }
}

fn to_rect(b: Bounds<gpui::Pixels>) -> Rect {
    Rect::new(f64::from(f32::from(b.origin.x)), f64::from(f32::from(b.origin.y)), f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height)))
}

/// Work areas of every screen, and the one of the main window.
fn work_areas(browser: &Rc<Browser>, cx: &App) -> (Vec<Rect>, Rect) {
    // GPUI's macOS display bounds are local to each display: use the global work areas of AppKit.
    #[cfg(target_os = "macos")]
    let all: Vec<Rect> = crate::mac::work_areas().into_iter().map(|(x, y, w, h)| Rect::new(x, y, w, h)).collect();
    #[cfg(not(target_os = "macos"))]
    let all: Vec<Rect> = cx.displays().iter().map(|d| to_rect(d.visible_bounds())).collect();
    let origin = browser.core.borrow().win.as_ref().map(|w| w.origin).unwrap_or((0.0, 0.0));
    let home = all.iter().find(|r| origin.0 >= r.x && origin.0 < r.x + r.width && origin.1 >= r.y && origin.1 < r.y + r.height).or(all.first()).copied().unwrap_or(Rect::new(0.0, 0.0, 1440.0, 900.0));
    (all, home)
}

fn open_window(cx: &mut App, b: Rect, transparent: bool, root: impl FnOnce(&mut Window, &mut App) -> AnyView + 'static) -> Option<(AnyWindowHandle, Option<RawWindowHandle>)> {
    let raw_cell = std::cell::Cell::new(None::<RawWindowHandle>);
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(b.x as f32), px(b.y as f32)), size(px(b.width as f32), px(b.height as f32))))),
            titlebar: Some(TitlebarOptions { title: Some("Signet".into()), appears_transparent: true, traffic_light_position: None }),
            window_background: if transparent { WindowBackgroundAppearance::Transparent } else { WindowBackgroundAppearance::Opaque },
            kind: WindowKind::PopUp,
            focus: false,
            show: true,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        },
        |window, cx| {
            raw_cell.set(window.window_handle().ok().map(|h| h.as_raw()));
            let child = root(window, cx);
            let child = super::factory::with_line_height(child, cx);
            // The Root paints the theme background: the controls must let the video through, the panel is black as in Electron.
            cx.new(|cx| {
                let mut root = Root::new(child, window, cx);
                gpui::Styled::style(&mut root).background = Some(if transparent { gpui::transparent_black() } else { gpui::black() }.into());
                root
            })
        },
    );
    opened.ok().map(|h| (h.into(), raw_cell.take()))
}

fn set_frame(handle: AnyWindowHandle, raw: Option<RawWindowHandle>, r: Rect, cx: &mut App) {
    if !set_native_frame(handle, raw, r, cx) {
        let _ = handle.update(cx, |_, w, _| w.resize(size(px(r.width as f32), px(r.height as f32))));
    }
    // The native resize calls GPUI's size callback synchronously, while the App is borrowed here: GPUI drops that
    // callback, so the window keeps its old viewport (stale layout, stale hitboxes). Tell it the frame changed.
    let _ = handle.update(cx, |_, w, cx| w.bounds_changed(cx));
}

/// Moves the native window; false when only a GPUI resize is possible.
fn set_native_frame(handle: AnyWindowHandle, raw: Option<RawWindowHandle>, r: Rect, cx: &mut App) -> bool {
    #[cfg(target_os = "macos")]
    if let Some(raw) = &raw {
        let _ = (handle, cx);
        crate::mac::set_frame(super::manager::raw_ptr(raw), r.x, r.y, r.width, r.height);
        return true;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let scale = scale_of(handle, cx);
        if let Some(raw) = raw
            && signet_web::overlay::set_window_frame(raw, r.x, r.y, r.width, r.height, scale).is_ok()
        {
            return true;
        }
    }
    let _ = (handle, raw, r, cx);
    false
}

/// Device pixels per logical point of a window.
fn scale_of(handle: AnyWindowHandle, cx: &mut App) -> f64 {
    handle.update(cx, |_, w, _| f64::from(w.scale_factor())).unwrap_or(1.0)
}

fn apply(s: &Session, r: Rect, cx: &mut App) {
    if s.last_applied.get() == Some(r) {
        return;
    }
    s.last_applied.set(Some(r));
    set_frame(s.panel, s.panel_raw, r, cx);
    set_frame(s.controls, s.controls_raw, r, cx);
    s.view.set_bounds(signet_web::Bounds::new(0.0, 0.0, r.width as f32, r.height as f32));
    let vp = |h: AnyWindowHandle, cx: &mut App| h.update(cx, |_, w, _| {
        let v = w.viewport_size();
        (f32::from(v.width), f32::from(v.height))
    }).unwrap_or((0.0, 0.0));
    let seen = (vp(s.controls, cx), vp(s.panel, cx));
    VIEWPORTS.with(|c| c.set(Some(seen)));
}

thread_local! {
    static VIEWPORTS: std::cell::Cell<Option<((f32, f32), (f32, f32))>> = const { std::cell::Cell::new(None) };
}

/// Viewport sizes GPUI holds for the controls and the panel after the last frame change (self-test).
pub fn viewports() -> Option<((f32, f32), (f32, f32))> {
    VIEWPORTS.with(std::cell::Cell::get)
}

/// Global pointer position in logical points and whether the left button is down.
fn pointer(scale: f64) -> Option<((f64, f64), bool)> {
    #[cfg(target_os = "macos")]
    {
        let _ = scale;
        Some((crate::mac::mouse_location(), crate::mac::left_button_down()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        signet_web::overlay::pointer(scale)
    }
}

impl PipWindows {
    /// Executes the queued operations and gestures.
    pub fn pump(&mut self, browser: &Rc<Browser>, cx: &mut App) {
        browser.core.borrow_mut().pip_drain();
        let link = browser.core.borrow().pip_link();
        for op in link.take_ops() {
            match op {
                PipWindowOp::Open { tab_id, ratio, saved, home } => self.open(browser, tab_id, ratio, saved, home, cx),
                PipWindowOp::Ratio { ratio } => {
                    if let Some(s) = &self.cur {
                        let (_, wa) = work_areas(browser, cx);
                        let mut s = s.borrow_mut();
                        let nb = fit_ratio(&s.bounds, ratio, &wa);
                        s.ratio = ratio;
                        s.bounds = nb;
                        apply(&s, nb, cx);
                    }
                }
                PipWindowOp::Close { to } => self.close_to(browser, to, cx),
            }
        }
        for input in link.take_inputs() {
            match input {
                PipInput::Drag(on) => self.drag(browser, on, cx),
                PipInput::Resize(phase) => self.resize(browser, phase, cx),
            }
        }
    }

    fn open(&mut self, browser: &Rc<Browser>, tab: String, ratio: f64, saved: Option<Rect>, home_video: Option<Rect>, cx: &mut App) {
        self.close(browser, cx);
        signet_ui::screens::windows::pip::install_pip_host(cx, crate::phase3::pip_host(&browser.core.borrow()));
        let Some(view) = browser.core.borrow().views.view_of(&tab) else { return };
        let (all, home) = work_areas(browser, cx);
        let bounds = initial_bounds(ratio, saved.as_ref(), &all, &home);
        // Like Electron, the window grows out of the video's place in the page.
        let start = home_video.filter(|_| !reduced_motion()).unwrap_or(bounds);
        let Some((panel, panel_raw)) = open_window(cx, start, false, |_, cx| cx.new(|cx| super::host::QuitKeys(cx.new(|_| Blank).into())).into()) else { return };
        let Some((controls, controls_raw)) = open_window(cx, start, true, |window, cx| { let v: gpui::AnyView = signet_ui::screens::windows::pip::Pip::new(window, cx).into(); cx.new(|_| super::host::QuitKeys(v)).into() }) else {
            let _ = panel.update(cx, |_, w, _| w.remove_window());
            return;
        };
        if let (Some(p), Some(c)) = (panel_raw, controls_raw) {
            #[cfg(target_os = "macos")]
            {
                crate::mac::style_floating(super::manager::raw_ptr(&p));
                crate::mac::style_floating(super::manager::raw_ptr(&c));
                crate::mac::add_child(super::manager::raw_ptr(&p), super::manager::raw_ptr(&c));
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = signet_web::overlay::attach_child_window(p, c);
            }
            if let Err(e) = view.reparent(p) {
                eprintln!("[pip] cannot move the view: {e}");
            }
        }
        let s = Session { tab, view, panel, panel_raw, controls, controls_raw, bounds: start, ratio, drag: None, last_applied: std::cell::Cell::new(None), resize_start: None, resize_last: None, epoch: 0 };
        apply(&s, start, cx);
        view.set_visible(true);
        browser.core.borrow_mut().pip_moved(bounds);
        let sess = Rc::new(RefCell::new(s));
        if start != bounds {
            glide(browser, sess.clone(), start, bounds, 0, false, cx);
        }
        self.cur = Some(sess);
    }

    /// Closes the windows and gives the view back to the main window.
    fn close(&mut self, browser: &Rc<Browser>, cx: &mut App) {
        let Some(s) = self.cur.take() else { return };
        finish(browser, &mut s.borrow_mut(), cx);
    }

    /// Closes after gliding to `to` ("back to tab"), at once without a target or with reduced motion.
    fn close_to(&mut self, browser: &Rc<Browser>, to: Option<Rect>, cx: &mut App) {
        let Some(to) = to.filter(|_| !reduced_motion()) else { return self.close(browser, cx) };
        let Some(sess) = self.cur.take() else { return };
        let (from, gen_) = {
            let mut s = sess.borrow_mut();
            s.epoch += 1;
            s.drag = None;
            (s.bounds, s.epoch)
        };
        glide(browser, sess, from, to, gen_, true, cx);
    }

    fn drag(&mut self, browser: &Rc<Browser>, on: bool, cx: &mut App) {
        let Some(sess) = self.cur.clone() else { return };
        if on {
            let scale = scale_of(sess.borrow().controls, cx);
            let Some((mouse, _)) = pointer(scale) else { return };
            let gen_ = {
                let mut s = sess.borrow_mut();
                s.epoch += 1;
                s.drag = Some(Drag { mouse, start: s.bounds, samples: vec![(std::time::Instant::now(), mouse)] });
                s.epoch
            };
            let b = browser.clone();
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor().timer(Duration::from_millis(8)).await;
                    let alive = cx.update(|cx| {
                        let mut s = sess.borrow_mut();
                        if s.epoch != gen_ || s.drag.is_none() {
                            return false;
                        }
                        // A mouse-up the controls never saw (released outside, or while another window had it) ends the drag.
                        let scale = scale_of(s.controls, cx);
                        let polled = pointer(scale);
                        if let Some((_, false)) = polled {
                            drop(s);
                            release(&b, &sess, cx);
                            return false;
                        }
                        if let Some((m, _)) = polled {
                            let d = s.drag.as_mut().unwrap();
                            d.samples.push((std::time::Instant::now(), m));
                            let keep = d.samples.len().saturating_sub(12);
                            d.samples.drain(..keep);
                            let r = Rect::new((d.start.x + m.0 - d.mouse.0).round(), (d.start.y + m.1 - d.mouse.1).round(), d.start.width, d.start.height);
                            s.bounds = r;
                            apply(&s, r, cx);
                        }
                        true
                    });
                    if !alive {
                        break;
                    }
                }
            })
            .detach();
        } else {
            release(browser, &sess, cx);
        }
    }

    fn resize(&mut self, browser: &Rc<Browser>, phase: PipResizePhase, cx: &mut App) {
        let Some(sess) = self.cur.clone() else { return };
        let (all, home) = work_areas(browser, cx);
        let wa_of = |b: &Rect| all.iter().find(|r| b.x + 1.0 >= r.x && b.x < r.x + r.width).copied().unwrap_or(home);
        match phase {
            PipResizePhase::Start { handle } => {
                let scale = scale_of(sess.borrow().controls, cx);
                let mut s = sess.borrow_mut();
                s.epoch += 1;
                s.resize_start = Some(s.bounds);
                s.resize_last = None;
                // The pointer is polled like in a drag: moves outside the small controls window never reach it.
                let (gen_, start, wa) = (s.epoch, s.bounds, wa_of(&s.bounds));
                let Some((mouse, true)) = pointer(scale) else { return };
                drop(s);
                let b = browser.clone();
                cx.spawn(async move |cx| {
                    loop {
                        cx.background_executor().timer(Duration::from_millis(8)).await;
                        let alive = cx.update(|cx| {
                            let mut s = sess.borrow_mut();
                            if s.epoch != gen_ || s.resize_start.is_none() {
                                return false;
                            }
                            let scale = scale_of(s.controls, cx);
                            match pointer(scale) {
                                Some((_, false)) => {
                                    drop(s);
                                    end_resize(&b, &sess, cx);
                                    false
                                }
                                Some((m, true)) => {
                                    let (dx, dy) = (m.0 - mouse.0, m.1 - mouse.1);
                                    s.resize_last = Some((handle, dx, dy));
                                    let r = resized_bounds(&start, handle_name(handle), dx, dy, s.ratio, &wa, true);
                                    s.bounds = r;
                                    apply(&s, r, cx);
                                    true
                                }
                                None => true,
                            }
                        });
                        if !alive {
                            break;
                        }
                    }
                })
                .detach();
            }
            PipResizePhase::Move { handle, dx, dy } => {
                let mut s = sess.borrow_mut();
                let Some(start) = s.resize_start else { return };
                s.resize_last = Some((handle, f64::from(dx), f64::from(dy)));
                let wa = wa_of(&s.bounds);
                let r = resized_bounds(&start, handle_name(handle), f64::from(dx), f64::from(dy), s.ratio, &wa, true);
                s.bounds = r;
                apply(&s, r, cx);
            }
            PipResizePhase::End => end_resize(browser, &sess, cx),
        }
    }
}

/// Ends a resize (idempotent): the rubber band springs back to the hard limits.
fn end_resize(browser: &Rc<Browser>, sess: &Rc<RefCell<Session>>, cx: &mut App) {
    let (all, home) = work_areas(browser, cx);
    let mut s = sess.borrow_mut();
    let Some(start) = s.resize_start.take() else { return };
    let wa = all.iter().find(|r| start.x + 1.0 >= r.x && start.x < r.x + r.width).copied().unwrap_or(home);
    let to = match s.resize_last.take() {
        Some((h, dx, dy)) => resized_bounds(&start, handle_name(h), dx, dy, s.ratio, &wa, false),
        None => s.bounds,
    };
    s.epoch += 1;
    let (gen_, from) = (s.epoch, s.bounds);
    drop(s);
    if to == from {
        browser.core.borrow_mut().pip_moved(from);
    } else {
        glide(browser, sess.clone(), from, to, gen_, false, cx);
    }
}

/// Ends a drag: the window glides to where its momentum takes it, snapping to a corner when near.
fn release(browser: &Rc<Browser>, sess: &Rc<RefCell<Session>>, cx: &mut App) {
    let (from, v, gen_) = {
        let mut s = sess.borrow_mut();
        let Some(d) = s.drag.take() else { return };
        s.epoch += 1;
        let v = match (d.samples.first(), d.samples.last()) {
            (Some((t0, p0)), Some((t1, p1))) if t1 > t0 => {
                let dt = (*t1 - *t0).as_secs_f64();
                ((p1.0 - p0.0) / dt, (p1.1 - p0.1) / dt)
            }
            _ => (0.0, 0.0),
        };
        (s.bounds, v, s.epoch)
    };
    let (all, home) = work_areas(browser, cx);
    let area_at = move |x: f64, y: f64| all.iter().find(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height).copied().unwrap_or(home);
    // Like Electron's `getDisplayMatching`: the screen under the card, not the one of the main window.
    let here = area_at(from.x + from.width / 2.0, from.y + from.height / 2.0);
    let target = release_target(&from, v, &here, &area_at);
    glide(browser, sess.clone(), from, target, gen_, false, cx);
}

/// Gives the view back to the main window and removes both windows.
fn finish(browser: &Rc<Browser>, s: &mut Session, cx: &mut App) {
    s.epoch += 1;
    let parent = browser.core.borrow().views.parent;
    if let Some(p) = parent {
        let _ = s.view.reparent(p);
    }
    let _ = s.controls.update(cx, |_, w, _| w.remove_window());
    let _ = s.panel.update(cx, |_, w, _| w.remove_window());
}

/// Moves the window from `from` to `to` with an ease-out over 250 ms; the end is remembered.
fn glide(browser: &Rc<Browser>, sess: Rc<RefCell<Session>>, from: Rect, to: Rect, gen_: u64, close_after: bool, cx: &mut App) {
    let b = browser.clone();
    cx.spawn(async move |cx| {
        let started = std::time::Instant::now();
        loop {
            cx.background_executor().timer(Duration::from_millis(8)).await;
            let t = (started.elapsed().as_secs_f64() / 0.25).min(1.0);
            let e = 1.0 - (1.0 - t).powi(3);
            let cont = cx.update(|cx| {
                let mut s = sess.borrow_mut();
                if s.epoch != gen_ {
                    return false;
                }
                let r = Rect::new((from.x + (to.x - from.x) * e).round(), (from.y + (to.y - from.y) * e).round(), (from.width + (to.width - from.width) * e).round(), (from.height + (to.height - from.height) * e).round());
                s.bounds = r;
                apply(&s, r, cx);
                if t >= 1.0 {
                    if close_after {
                        finish(&b, &mut s, cx);
                    } else {
                        b.core.borrow_mut().pip_moved(r);
                    }
                    return false;
                }
                true
            });
            if !cont {
                break;
            }
        }
    })
    .detach();
}

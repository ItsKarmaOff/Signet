//! Opens, moves and closes the GPUI windows on behalf of the browser core.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui::{AnyWindowHandle, App, AppContext as _, AsyncApp, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions, point, px, size};
use gpui_component::Root;
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use signet_core::types::Rect;

use super::factory::{self, Kind};
use super::host::MainHost;
use super::{Cmd, Desired, Entry, Surface, Win};
use signet_core::floating::SidebarWindow;
use crate::app::platform::{window_chrome, IS_MAC};
use crate::browser::Browser;

struct Floating {
    handle: AnyWindowHandle,
    raw: Option<RawWindowHandle>,
    rect: Rect,
    /// Open but waiting: transparent and ignoring the mouse (the floating sidebar on macOS).
    parked: bool,
}

#[derive(Default)]
pub struct Manager {
    main: Option<AnyWindowHandle>,
    floating: HashMap<Surface, Floating>,
    wins: HashMap<Win, AnyWindowHandle>,
    /// Boost editors, one per site.
    boosts: HashMap<String, AnyWindowHandle>,
    desired: Desired,
    aux: super::aux::AuxWindows,
    pip: super::pip::PipWindows,
    /// The GPUI-drawn context menu while it is open (Linux, or `SIGNET_GPUI_MENU=1`).
    menu: Option<AnyWindowHandle>,
}

pub(super) fn raw_ptr(h: &RawWindowHandle) -> *mut std::ffi::c_void {
    match h {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(a) => a.ns_view.as_ptr(),
        _ => std::ptr::null_mut(),
    }
}

impl Manager {
    pub fn handle(&mut self, browser: &Rc<Browser>, cmd: Cmd, cx: &mut AsyncApp) {
        let b = browser.clone();
        match cmd {
            Cmd::SoftApply => {
                cx.update(|app| {
                    app.spawn(async move |cx| {
                        cx.background_executor().timer(Duration::from_millis(32)).await;
                        b.core.borrow_mut().apply();
                    })
                    .detach();
                });
            }
            Cmd::Later(ms, what) => {
                cx.update(|app| {
                    app.spawn(async move |cx| {
                        cx.background_executor().timer(Duration::from_millis(ms)).await;
                        b.core.borrow_mut().on_later(what);
                    })
                    .detach();
                });
            }
            Cmd::Sync(d) => {
                if d != self.desired {
                    self.desired = d.clone();
                    cx.update(|app| self.sync(&b, &d, app));
                }
            }
            Cmd::MakeKey(s) => {
                if let Some(f) = self.floating.get(&s) {
                    #[cfg(target_os = "macos")]
                    if let Some(raw) = &f.raw {
                        crate::mac::make_key(raw_ptr(raw));
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let h = f.handle;
                        cx.update(|app| {
                            let _ = h.update(app, |_, w, _| w.activate_window());
                        });
                    }
                }
            }
            Cmd::RebuildMenu => {
                cx.update(|app| crate::app::menu::install(&b, app));
            }
            Cmd::Popup { entries, at, cb } => {
                // A new menu replaces the open one (which is dismissed: its callback never runs).
                if let Some(old) = self.menu.take() {
                    cx.update(|app| {
                        let _ = old.update(app, |_, w, _| w.remove_window());
                    });
                }
                if super::popup_menu::wanted() {
                    // Asynchronous: the callback runs from the menu window when an item is chosen.
                    self.menu = cx.update(|app| {
                        let g = self.menu_position(&b, at, app);
                        super::popup_menu::open(&b, entries, g, cb, app)
                    });
                } else {
                    // Native menus block until a choice is made.
                    b.core.borrow().note("popup-shown native");
                    let picked = self.popup(&b, &entries, at);
                    b.core.borrow().note(format!("popup-closed picked={}", picked.as_deref().unwrap_or("none")));
                    if let Some(id) = picked {
                        cb(&mut b.core.borrow_mut(), &id);
                    }
                }
            }
            Cmd::ShowMain => {
                cx.update(|app| {
                    // A window closed by the user (macOS keeps the app alive) is no longer there: open a new one.
                    if let Some(h) = self.main.filter(|h| h.update(app, |_, w, _| w.activate_window()).is_ok()) {
                        let _ = h;
                    } else {
                        self.main = None;
                        self.open_main(&b, app);
                    }
                });
            }
            Cmd::MainClosed => {
                cx.update(|app| {
                    self.main = None;
                    self.desired = Desired::default();
                    for (s, f) in std::mem::take(&mut self.floating) {
                        let _ = f.handle.update(app, |_, w, _| w.remove_window());
                        let _ = s;
                    }
                    b.core.borrow_mut().overlay_window_changed(None);
                });
            }
            Cmd::Run(f) => {
                f(&mut b.core.borrow_mut());
            }
            Cmd::PipPump => {
                cx.update(|app| self.pip.pump(&b, app));
            }
            Cmd::CloseMain => {
                cx.update(|app| {
                    if let Some(h) = self.main.take() {
                        let _ = h.update(app, |_, w, _| w.remove_window());
                    }
                });
            }
            Cmd::ToggleFullscreen => {
                cx.update(|app| {
                    if let Some(h) = self.main {
                        let _ = h.update(app, |_, w, _| w.toggle_fullscreen());
                    }
                });
            }
            Cmd::Minimize => {
                cx.update(|app| {
                    if let Some(h) = self.main {
                        let _ = h.update(app, |_, w, _| w.minimize_window());
                    }
                });
            }
            Cmd::ZoomWindow => {
                cx.update(|app| {
                    if let Some(h) = self.main {
                        let _ = h.update(app, |_, w, _| w.zoom_window());
                    }
                });
            }
            Cmd::PageFullscreen(on) => {
                // The page fills the window: take the system window to fullscreen too (and back).
                cx.update(|app| {
                    if let Some(h) = self.main {
                        let _ = h.update(app, |_, w, _| {
                            if w.is_fullscreen() != on {
                                w.toggle_fullscreen();
                            }
                        });
                    }
                });
            }
            Cmd::OpenWin(win, section) => {
                let span = signet_web::startup::enabled().then(std::time::Instant::now);
                cx.update(|app| self.open_win(&b, win, section, app));
                if let Some(t) = span {
                    signet_web::startup::mark_with(|| format!("window {win:?} opened in {:.1} ms", t.elapsed().as_secs_f64() * 1000.0));
                }
            }
            Cmd::OpenAux { id, kind } => {
                cx.update(|app| self.aux.open(&b, id, kind, app));
            }
            Cmd::CloseAux(id) => {
                cx.update(|app| self.aux.close(id, app));
            }
            Cmd::FocusAux(id) => {
                cx.update(|app| self.aux.focus(id, app));
            }
            Cmd::CloseWin(win) => {
                cx.update(|app| {
                    if win == Win::Boost {
                        for (_, h) in self.boosts.drain() {
                            let _ = h.update(app, |_, w, _| w.remove_window());
                        }
                    } else if let Some(h) = self.wins.remove(&win) {
                        let _ = h.update(app, |_, w, _| w.remove_window());
                    }
                });
                if let Ok(mut c) = b.core.try_borrow_mut() {
                    c.on_window_closed(win);
                    c.note(format!("win-closed {win:?}"));
                }
            }
            Cmd::Quit => {
                cx.update(|app| {
                    // The windows disappear first; the flush and the teardown happen out of sight.
                    #[cfg(target_os = "macos")]
                    signet_web::mac::hide_all_windows();
                    b.core.borrow_mut().finish_quit();
                    app.quit();
                });
            }
            Cmd::Urls(urls) => {
                signet_web::startup::once("cmd.urls (first)");
                let mut c = b.core.borrow_mut();
                for u in urls {
                    c.open_external(&u);
                }
            }
        }
    }

    fn popup(&self, browser: &Rc<Browser>, entries: &[Entry], at: (f64, f64)) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            let parent = browser.core.borrow().views.parent;
            let view = parent.as_ref().map(raw_ptr)?;
            let mut ids: Vec<String> = Vec::new();
            let items = to_popup(entries, &mut ids);
            let tag = crate::mac::popup(view, at.0, at.1, &items)?;
            ids.get(tag).cloned()
        }
        #[cfg(target_os = "windows")]
        {
            let parent = browser.core.borrow().views.parent;
            match parent {
                Some(RawWindowHandle::Win32(h)) => crate::win::popup(h.hwnd.get(), at.0, at.1, entries),
                _ => None,
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            // Linux always takes the GPUI-drawn menu (`popup_menu`).
            let _ = (browser, entries, at);
            None
        }
    }

    /// Global screen position of a menu wanted at `at` (window coordinates); NaN = at the pointer.
    fn menu_position(&self, browser: &Rc<Browser>, at: (f64, f64), cx: &mut App) -> (f64, f64) {
        let origin = browser.core.borrow().win.as_ref().map(|w| w.origin).unwrap_or((0.0, 0.0));
        if at.0.is_finite() && at.1.is_finite() {
            return (origin.0 + at.0, origin.1 + at.1);
        }
        #[cfg(target_os = "macos")]
        {
            let _ = cx;
            return crate::mac::mouse_location();
        }
        #[cfg(not(target_os = "macos"))]
        {
            // GPUI sees the pointer only over its own views: the last position it saw in the main window.
            let seen = self.main.and_then(|h| h.update(cx, |_, w, _| w.mouse_position()).ok());
            match seen {
                Some(p) => (origin.0 + f64::from(f32::from(p.x)), origin.1 + f64::from(f32::from(p.y))),
                None => origin,
            }
        }
    }

    // ------------------------------------------------------------------ main window

    pub fn open_main(&mut self, browser: &Rc<Browser>, cx: &mut App) {
        let (saved, transparency) = {
            let c = browser.core.borrow();
            (c.state().bounds, c.state().settings.transparency != Some(false))
        };
        let bounds = match saved.filter(|r| r.width >= 600.0 && r.height >= 400.0) {
            Some(r) => Bounds::new(point(px(r.x as f32), px(r.y as f32)), size(px(r.width as f32), px(r.height as f32))),
            None => Bounds::centered(None, size(px(1280.0), px(820.0)), cx),
        };
        let (titlebar, background) = window_chrome("Signet", Some((16.0, 16.0)), transparency);
        let b = browser.clone();
        signet_web::startup::once("main_window.open_window");
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar,
                window_background: background,
                focus: true,
                show: true,
                window_min_size: Some(size(px(640.0), px(420.0))),
                // macOS: the title areas of the interface move the window and run the double-click action
                // (`signet_ui::title_area`); the native title bar band neither drags nor delays clicks.
                app_owns_titlebar_drag: IS_MAC,
                ..Default::default()
            },
            move |window, cx| {
                signet_web::startup::once("main_window.platform_window_created");
                let raw = window.window_handle().ok().map(|h| h.as_raw());
                let vp = window.viewport_size();
                let wb = window.bounds();
                b.core.borrow_mut().on_main_created(
                    raw,
                    (f64::from(f32::from(vp.width)), f64::from(f32::from(vp.height))),
                    (f64::from(f32::from(wb.origin.x)), f64::from(f32::from(wb.origin.y))),
                );
                signet_web::startup::once("main_window.core_notified");
                let child = factory::build(Kind::Main, window, cx);
                let host = cx.new(|_| MainHost::new(child, b.clone()));
                let root = cx.new(|cx| Root::new(host, window, cx));
                signet_web::startup::once("main_window.views_built");
                if signet_web::startup::enabled() {
                    // The first callback runs when the first frame starts, the nested one when the next frame starts
                    // (the first one was presented by then).
                    window.on_next_frame(|window, _| {
                        signet_web::startup::once("main_window.first_frame_start");
                        window.on_next_frame(|_, _| signet_web::startup::once("main_window.second_frame_start (first presented)"));
                    });
                }
                root
            },
        );
        match opened {
            Ok(h) => {
                let b = browser.clone();
                h.update(cx, |_, w, cx| {
                    w.on_window_should_close(cx, move |_, _| {
                        if IS_MAC {
                            // macOS: the app stays alive without a window; the Dock icon brings it back
                            // (`app.on('window-all-closed', () => {})` and `activate`). The tabs' views go with it.
                            if let Ok(mut c) = b.core.try_borrow_mut() {
                                c.on_main_closed();
                            }
                            let _ = b.core.borrow().cmds.unbounded_send(Cmd::MainClosed);
                            true
                        } else {
                            // Linux and Windows: the app quits when its last window closes.
                            let _ = b.core.borrow().cmds.unbounded_send(Cmd::Quit);
                            false
                        }
                    });
                })
                .ok();
                self.main = Some(h.into());
                signet_web::startup::once("main_window.opened");
            }
            Err(e) => eprintln!("cannot open the main window: {e}"),
        }
        cx.activate(true);
    }

    // ------------------------------------------------------------------ floating surfaces

    fn sync(&mut self, browser: &Rc<Browser>, d: &Desired, cx: &mut App) {
        let Some(main) = self.main else { return };
        let origin = browser.core.borrow().win.as_ref().map(|w| w.origin).unwrap_or((0.0, 0.0));
        // macOS keeps the floating sidebar window open (parked) while the sidebar is hidden; the other platforms create
        // it on reveal and close it on leave.
        const KEEP_ALIVE: bool = cfg!(target_os = "macos");
        for s in Surface::ALL {
            let target = if s == Surface::Sidebar {
                match signet_core::floating::sidebar_window(d.sidebar, d.sidebar_warm, KEEP_ALIVE) {
                    SidebarWindow::Closed => None,
                    SidebarWindow::Parked(r) => Some((r, true)),
                    SidebarWindow::Shown(r) => Some((r, false)),
                }
            } else {
                d.get(s).map(|r| (r, false))
            };
            match target {
                None => {
                    if let Some(f) = self.floating.remove(&s) {
                        // A closing key window gives the keyboard back to the main window, not to a sibling surface.
                        #[cfg(target_os = "macos")]
                        let was_key = f.raw.as_ref().is_some_and(|r| crate::mac::is_key(raw_ptr(r)));
                        if s == Surface::Overlay {
                            browser.core.borrow_mut().overlay_window_changed(None);
                        }
                        let _ = f.handle.update(cx, |_, w, _| w.remove_window());
                        #[cfg(target_os = "macos")]
                        if was_key && let Some(parent) = browser.core.borrow().views.parent.as_ref() {
                            crate::mac::make_key(raw_ptr(parent));
                        }
                        browser.core.borrow().log(format!("surface {} closed", s.name()));
                        browser.core.borrow().note(format!("surface-closed {}", s.name()));
                    }
                }
                Some((rect, parked)) => {
                    let g = Rect::new(origin.0 + rect.x, origin.1 + rect.y, rect.width, rect.height);
                    if let Some(f) = self.floating.get_mut(&s) {
                        // A parked window is invisible: skipping its moves avoids a relayout of the whole sidebar on
                        // every live resize frame; it moves at the reveal.
                        if !parked && f.rect != rect {
                            f.rect = rect;
                            self.move_floating(s, g, cx);
                        }
                    } else {
                        self.open_floating(browser, main, s, rect, g, cx);
                    }
                    #[cfg(target_os = "macos")]
                    if s == Surface::Sidebar && !parked {
                        self.update_window_corner(browser, cx);
                    }
                    self.set_parked(browser, s, parked, cx);
                }
            }
        }
    }

    fn set_parked(&mut self, browser: &Rc<Browser>, s: Surface, parked: bool, cx: &App) {
        let Some(f) = self.floating.get_mut(&s) else { return };
        if f.parked == parked {
            return;
        }
        f.parked = parked;
        signet_ui::trace::mark(|| format!("parked {parked} ({})", s.name()));
        #[cfg(target_os = "macos")]
        if let Some(raw) = &f.raw {
            crate::mac::set_ignores_mouse(raw_ptr(raw), parked);
            // The card is painted at rest; Core Animation slides its layer in and out (no GPUI frame per step).
            if s == Surface::Sidebar {
                let (width, right) = (f.rect.width, browser.core.borrow().sidebar_right());
                let dx = if right { width } else { -width };
                let how = if parked { crate::mac::CardSlide::Exit } else { crate::mac::CardSlide::Enter };
                crate::mac::card_slide(raw_ptr(raw), how, dx, signet_ui::motion::reduced_motion(cx));
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (browser, cx);
    }

    /// Measures the app window's corner radius and hands it to the floating sidebar (concentric card). Logged once
    /// per value.
    #[cfg(target_os = "macos")]
    fn update_window_corner(&mut self, browser: &Rc<Browser>, cx: &mut App) {
        let right = browser.core.borrow().sidebar_right();
        let r = browser.core.borrow().views.parent.as_ref().and_then(|p| crate::mac::window_corner_radius(raw_ptr(p), right)).map(|r| r as f32);
        if r == signet_ui::theme::window_corner(cx) {
            return;
        }
        signet_ui::theme::set_window_corner(cx, r);
        browser.core.borrow().log(format!("window corner radius {r:?}"));
        eprintln!("signet: window corner radius {r:?}");
        if let Some(f) = self.floating.get(&Surface::Sidebar) {
            let _ = f.handle.update(cx, |_, w, _| w.refresh());
        }
    }

    fn move_floating(&mut self, s: Surface, g: Rect, cx: &mut App) {
        let Some(f) = self.floating.get_mut(&s) else { return };
        #[cfg(target_os = "macos")]
        if let Some(raw) = &f.raw {
            crate::mac::set_frame(raw_ptr(raw), g.x, g.y, g.width, g.height);
            // The native resize reaches GPUI's size callback while the App is borrowed and is dropped there: tell
            // the window its frame changed so its viewport and hitboxes follow.
            let _ = f.handle.update(cx, |_, w, cx| w.bounds_changed(cx));
            return;
        }
        // No move API on this platform: reopen at the new place.
        let _ = f.handle.update(cx, |_, w, _| w.resize(size(px(g.width as f32), px(g.height as f32))));
    }

    fn open_floating(&mut self, browser: &Rc<Browser>, main: AnyWindowHandle, s: Surface, rect: Rect, g: Rect, cx: &mut App) {
        // Only one surface takes the keyboard: the overlay opens first, so a toolbar or sidebar opened with it (compact
        // mode shows the toolbar next to a popover) must not steal the key window from the text field.
        let take_focus = s.takes_focus() && (s == Surface::Overlay || !self.floating.contains_key(&Surface::Overlay));
        let b = browser.clone();
        let t_start = std::time::Instant::now();
        let mut raw_out: Option<RawWindowHandle> = None;
        let raw_cell = std::cell::Cell::new(None::<RawWindowHandle>);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(g.x as f32), px(g.y as f32)), size(px(g.width.max(1.0) as f32), px(g.height.max(1.0) as f32))))),
                titlebar: Some(TitlebarOptions { title: Some(s.name().into()), appears_transparent: true, traffic_light_position: None }),
                window_background: WindowBackgroundAppearance::Transparent,
                focus: take_focus,
                show: true,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                ..Default::default()
            },
            |window, cx| {
                raw_cell.set(window.window_handle().ok().map(|h| h.as_raw()));
                let child = factory::build(Kind::Floating(s), window, cx);
                // The Root paints the theme background by default: floating surfaces (scrim, cards) must stay transparent.
                cx.new(|cx| {
                    let mut root = Root::new(child, window, cx);
                    if true {
                        gpui::Styled::style(&mut root).background = Some(gpui::transparent_black().into());
                    }
                    root
                })
            },
        );
        let open_ms = t_start.elapsed().as_secs_f64() * 1000.0;
        raw_out =raw_cell.take().or(raw_out);
        match opened {
            Ok(h) => {
                #[cfg(target_os = "macos")]
                if let (Some(child), Some(parent)) = (&raw_out, b.core.borrow().views.parent.as_ref()) {
                    crate::mac::style_floating(raw_ptr(child));
                    // The sidebar and toolbar cards are 6 px inset cards: no system corner mask (the overlay keeps it).
                    if matches!(s, Surface::Sidebar | Surface::Toolbar) {
                        crate::mac::make_borderless(raw_ptr(child));
                    }
                    // The sidebar card starts parked (hidden, mouse ignored): the reveal that follows slides it in.
                    if s == Surface::Sidebar {
                        crate::mac::card_slide(raw_ptr(child), crate::mac::CardSlide::Hide, 0.0, false);
                        crate::mac::set_ignores_mouse(raw_ptr(child), true);
                    }
                    crate::mac::add_child(raw_ptr(parent), raw_ptr(child));
                    crate::mac::set_frame(raw_ptr(child), g.x, g.y, g.width, g.height);
                    // A style change and a native frame while the App is borrowed: GPUI must learn the new bounds.
                    let _ = h.update(cx, |_, w, cx| w.bounds_changed(cx));
                    // Adding a child window can reorder windows: the surface that wants the keyboard keeps it.
                    if take_focus {
                        crate::mac::make_key(raw_ptr(child));
                    }
                }
                let _ = (&main, IS_MAC);
                signet_ui::trace::mark(|| format!("open {} window {open_ms:.1} ms total {:.1} ms", s.name(), t_start.elapsed().as_secs_f64() * 1000.0));
                b.core.borrow().note(format!("surface-opened {}", s.name()));
                b.core.borrow().log(format!("surface {} opened at ({:.0},{:.0},{:.0},{:.0})", s.name(), g.x, g.y, g.width, g.height));
                #[cfg(not(target_os = "macos"))]
                if let (Some(child), Some(parent)) = (raw_out, b.core.borrow().views.parent) {
                    let _ = signet_web::overlay::attach_child_window(parent, child);
                }
                self.floating.insert(s, Floating { handle: h.into(), raw: raw_out, rect, parked: cfg!(target_os = "macos") && s == Surface::Sidebar });
                // The surfaces that missed an event sent before they existed, and the Peek view that lives in the
                // overlay window.
                match s {
                    Surface::Preview => b.core.borrow_mut().preview_ready(),
                    Surface::Overlay => b.core.borrow_mut().overlay_window_changed(raw_out),
                    _ => {}
                }
            }
            Err(e) => eprintln!("cannot open the {} surface: {e}", s.name()),
        }
    }

    // ------------------------------------------------------------------ secondary windows

    /// Settings opened on a given section (`open_settings(section)`).
    fn show_settings_section(browser: &Rc<Browser>, win: Win, handle: AnyWindowHandle, section: Option<&str>, cx: &mut App) {
        let Some(key) = section.filter(|_| win == Win::Settings) else { return };
        let shown = handle
            .update(cx, |_, window, cx| {
                let Some(Some(root)) = window.root::<Root>() else { return None };
                let mut view = root.read(cx).view().clone();
                // Window roots are wrapped in `LineHeight` (see `factory::build`): look inside it.
                if let Ok(wrapper) = view.clone().downcast::<super::host::LineHeight>() {
                    view = wrapper.read(cx).0.clone();
                }
                let settings = view.downcast::<signet_ui::screens::settings::SettingsWindow>().ok()?;
                settings.update(cx, |s, cx| s.open_section(key, window, cx));
                Some(settings.read(cx).section_key())
            })
            .ok()
            .flatten();
        browser.core.borrow().note(format!("settings-section {}", shown.unwrap_or("none")));
    }

    fn open_win(&mut self, browser: &Rc<Browser>, win: Win, section: Option<String>, cx: &mut App) {
        match win {
            // One editor per site: focus the one of the site if it is there.
            Win::Boost => {
                let site = section.clone().unwrap_or_default();
                if let Some(h) = self.boosts.get(&site).copied() {
                    if h.update(cx, |_, w, _| w.activate_window()).is_ok() {
                        return;
                    }
                    self.boosts.remove(&site);
                }
            }
            // A passkey request always gets a fresh window (it reads the prompt when created).
            Win::Passkey => {
                if let Some(h) = self.wins.remove(&win) {
                    let _ = h.update(cx, |_, w, _| w.remove_window());
                }
            }
            _ => {
                if let Some(h) = self.wins.get(&win).copied()
                    && h.update(cx, |_, w, _| w.activate_window()).is_ok()
                {
                    Self::show_settings_section(browser, win, h, section.as_deref(), cx);
                    browser.core.borrow_mut().on_window_section(win, section);
                    return;
                }
            }
        }
        let site = section.clone().unwrap_or_default();
        // (title, width, height, minimum size, resizable, traffic lights)
        let (title, w, h, min, resizable, lights): (String, f32, f32, Option<(f32, f32)>, bool, Option<(f32, f32)>) = match win {
            Win::Settings => (signet_core::i18n::t("menu.settings"), 780.0, 600.0, None, true, if IS_MAC { Some((16.0, 18.0)) } else { None }),
            Win::Library => (signet_core::i18n::t("menu.navigate.library"), 920.0, 660.0, Some((700.0, 460.0)), true, if IS_MAC { Some((16.0, 18.0)) } else { None }),
            Win::Welcome => ("Signet".to_string(), 640.0, 580.0, None, true, if IS_MAC { Some((16.0, 18.0)) } else { None }),
            Win::Passkey => (signet_core::i18n::t("window.passkey"), 400.0, 520.0, None, false, if IS_MAC { Some((16.0, 18.0)) } else { None }),
            Win::Devtools => (signet_core::i18n::t("devtools.title"), 1000.0, 700.0, Some((480.0, 320.0)), true, if IS_MAC { Some((14.0, 11.0)) } else { None }),
            Win::Boost => (signet_core::i18n::t_args("window.boost", &[("site", site.as_str())]), 400.0, 620.0, Some((360.0, 420.0)), true, if IS_MAC { Some((14.0, 14.0)) } else { None }),
        };
        let (titlebar, background) = window_chrome(&title, lights, true);
        let bounds = match win {
            // Centered over the main window.
            Win::Passkey if browser.core.borrow().win.is_some() => {
                let c = browser.core.borrow();
                let m = c.win.as_ref().unwrap();
                Bounds::new(point(px((m.origin.0 + (m.size.0 - f64::from(w)) / 2.0).round() as f32), px((m.origin.1 + (m.size.1 - f64::from(h)) / 2.0).round() as f32)), size(px(w), px(h)))
            }
            _ => Bounds::centered(None, size(px(w), px(h)), cx),
        };
        let raw_cell = std::cell::Cell::new(None::<RawWindowHandle>);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar,
                window_background: background,
                focus: true,
                show: true,
                is_resizable: resizable,
                is_minimizable: resizable,
                window_min_size: min.map(|(mw, mh)| size(px(mw), px(mh))),
                // Library, Welcome, Passkey and Devtools draw a title area; Settings and Boost rely on the native band.
                app_owns_titlebar_drag: IS_MAC && !matches!(win, Win::Settings | Win::Boost),
                ..Default::default()
            },
            |window, cx| {
                raw_cell.set(window.window_handle().ok().map(|h| h.as_raw()));
                let child = factory::build(Kind::Win(win), window, cx);
                if win == Win::Devtools {
                    let vp = window.viewport_size();
                    let size = (f64::from(f32::from(vp.width)), f64::from(f32::from(vp.height)));
                    if let Ok(mut c) = browser.core.try_borrow_mut() {
                        c.devtools_window_created(raw_cell.get(), size);
                    }
                    let host = cx.new(|_| super::host::ToolsHost::new(child, browser.clone()));
                    return cx.new(|cx| Root::new(host, window, cx));
                }
                cx.new(|cx| Root::new(child, window, cx))
            },
        );
        match opened {
            Ok(handle) => {
                // Closed by the user: the matching feature is told (a programmatic close does not report).
                let b = browser.clone();
                let key = site.clone();
                handle
                    .update(cx, |_, w, cx| {
                        w.on_window_should_close(cx, move |_, _| {
                            if let Ok(mut c) = b.core.try_borrow_mut() {
                                match win {
                                    Win::Passkey => c.passkey_window_closed(),
                                    Win::Boost => c.boost_window_closed(&key),
                                    Win::Devtools => c.devtools_window_closed(),
                                    _ => c.on_window_closed(win),
                                }
                            }
                            true
                        });
                    })
                    .ok();
                // The passkey window stays above the window that asked for it.
                #[cfg(target_os = "macos")]
                if win == Win::Passkey
                    && let (Some(child), Some(parent)) = (raw_cell.get(), browser.core.borrow().views.parent.as_ref())
                {
                    crate::mac::add_child(raw_ptr(parent), raw_ptr(&child));
                }
                if win == Win::Boost {
                    self.boosts.insert(site, handle.into());
                } else {
                    self.wins.insert(win, handle.into());
                }
                Self::show_settings_section(browser, win, handle.into(), section.as_deref(), cx);
                browser.core.borrow_mut().on_window_opened(win, section);
                browser.core.borrow().note(format!("win-opened {win:?}"));
            }
            Err(e) => eprintln!("cannot open the {win:?} window: {e}"),
        }
        cx.activate(true);
    }
}

#[cfg(target_os = "macos")]
fn to_popup(entries: &[Entry], ids: &mut Vec<String>) -> Vec<crate::mac::PopupItem> {
    entries
        .iter()
        .map(|e| match e {
            Entry::Sep => crate::mac::PopupItem::Separator,
            Entry::Item { label, id, enabled, checked } => {
                ids.push(id.clone());
                crate::mac::PopupItem::Item { label: label.clone(), enabled: *enabled, checked: *checked, submenu: Vec::new() }
            }
            Entry::Sub { label, items } => crate::mac::PopupItem::Item { label: label.clone(), enabled: true, checked: false, submenu: to_popup(items, ids) },
        })
        .collect()
}

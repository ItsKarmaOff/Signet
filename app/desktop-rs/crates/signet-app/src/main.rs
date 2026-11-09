//! The `signet` binary: composition root (port of `main/index.ts`). See README.md for the startup sequence.

mod app;
mod browser;
mod dev;
mod focus;
mod syslang;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod win;
mod phase3;
mod quitkeys;
mod scheme;
mod selftest;
mod theme;
mod windows;

use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt as _;
use gpui::App;
use signet_core::api::SignetApi;
use signet_core::core::history::History;
use signet_core::core::store::{Store, StoreOptions};
use crate::syslang::system_languages as system_languages_from_env;
use signet_web::Config;
use signet_web::startup;

use app::single_instance::{self, Role};
use browser::{Browser, Core};
use windows::Cmd;

fn env_flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0")
}

fn main() {
    startup::mark("main.entry");
    signet_web::preload_framework();
    // A CEF subprocess started from this very executable (no helper next to it): never run the app logic (data
    // folder, single instance forwarding) in it.
    #[cfg(not(target_os = "macos"))]
    if std::env::args().any(|a| a.starts_with("--type=")) {
        std::process::exit(signet_web::run_subprocess());
    }
    // CEF child windows are X11 windows: on a Wayland session, run through XWayland (same as the Linux launcher).
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some() {
        // SAFETY: first statement of `main`, no other thread exists yet.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }
    let debug = env_flag("SIGNET_DEBUG");
    let prepare_span = startup::span("data_dir.prepare");

    // Data folder, with the first-run import of the Electron app's files.
    // Dev harness (`SIGNET_DATA_DIR` + `SIGNET_SEED=1`): never import the Electron folder, the sample state replaces it.
    let seeded = dev::flag("SIGNET_SEED");
    let prepared = if seeded {
        signet_core::paths::data_dir().ok_or_else(|| std::io::Error::other("no data folder")).and_then(|d| std::fs::create_dir_all(&d).map(|_| (d, Default::default())))
    } else {
        signet_core::paths::prepare_data_dir()
    };
    let data_dir = match prepared {
        Ok((dir, report)) => {
            if debug {
                println!("[startup] data folder {} ({report:?})", dir.display());
            }
            dir
        }
        Err(e) => {
            eprintln!("cannot prepare the data folder: {e}");
            dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Signet GPUI")
        }
    };

    drop(prepare_span);
    // Single instance: a second launch hands its URLs to the first one and exits.
    let args = single_instance::url_args();
    let single_span = startup::span("single_instance.acquire");
    let forwarded = match single_instance::acquire(&data_dir, &args) {
        Role::Forwarded => {
            if debug {
                println!("[startup] another instance is running: arguments forwarded");
            }
            return;
        }
        Role::First(rx) => rx,
    };
    drop(single_span);

    let store_span = startup::span("store.open");
    let mut store = Store::open(&data_dir, StoreOptions { system_languages: system_languages_from_env(), startup_state: None });
    drop(store_span);
    dev::seed::setup(&mut store);
    let history_span = startup::span("history.new");
    let history = History::new(&data_dir);
    drop(history_span);
    let locale_tag = if signet_core::i18n::current_locale() == signet_core::i18n::Locale::Fr { "fr-FR" } else { "en-US" };
    let accept_language = store.state.settings.languages.join(",");

    // Links the system hands over (default browser, `signet://webapp/<id>`): checked like a second launch's arguments.
    let (open_tx, open_rx) = futures::channel::mpsc::unbounded::<Vec<String>>();
    let gpui_span = startup::span("gpui.application");
    let app = gpui_platform::application().with_assets(signet_ui::SignetAssets);
    drop(gpui_span);
    app.on_open_urls(move |urls| {
        let _ = open_tx.unbounded_send(urls);
    });
    windows::on_reopen(&app);
    startup::mark("gpui.run");
    app.run(move |cx: &mut App| {
        startup::mark("gpui.run.launched");
        signet_web::set_scheme_handler(scheme::handle);
        let mut config = Config::new(data_dir.clone());
        config.locale = locale_tag.to_string();
        if !accept_language.is_empty() {
            config.accept_language = accept_language.clone();
        }
        config.log_severity = if debug { 3 } else { 4 };
        // The built-in developer tools load the DevTools frontend from Chromium's remote debugging server: a random
        // port on 127.0.0.1, fixed for the session. `SIGNET_NO_DEVTOOLS=1` turns the server (and the tools) off.
        if std::env::var_os("SIGNET_NO_DEVTOOLS").is_none_or(|v| v.is_empty() || v == "0") {
            config.remote_debugging_port = signet_web::pick_loopback_port().map_or(0, i32::from);
        }
        let cef_span = startup::span("cef.init (blocks the main thread)");
        let inited = signet_web::init(config);
        drop(cef_span);
        if let Err(e) = inited {
            eprintln!("the web layer failed to start: {e}");
            cx.quit();
            return;
        }
        // Create the request contexts now: they finish initializing while the app keeps starting.
        signet_web::prewarm_profile(&signet_web::ProfileKind::persistent("default"));
        {
            let space = signet_core::model::active_space(&store.state);
            if space.is_isolated() {
                signet_web::prewarm_profile(&signet_web::ProfileKind::persistent(format!("space-{}", space.id)));
            }
        }

        // Key filter: app shortcuts work while a page has focus. The table is rebuilt when shortcuts change.
        let key_table: browser::KeyTable = Arc::new(Mutex::new(browser::commands::build_key_table(store.state.settings.shortcuts.as_ref())));
        let filter_table = key_table.clone();
        signet_web::set_key_filter(move |e| {
            if !e.pressed {
                return false;
            }
            let Some(key) = browser::commands::event_key(e) else { return false };
            let ks = browser::commands::key_string(e.cmd, e.ctrl, e.alt, e.shift, &key);
            filter_table.lock().map(|t| t.contains_key(&ks)).unwrap_or(false)
        });

        // Browser core and API.
        let (tx, rx) = futures::channel::mpsc::unbounded::<Cmd>();
        let core_span = startup::span("core.new");
        let core = Core::new(store, history, tx.clone(), data_dir.clone(), key_table, debug);
        let browser = Rc::new(Browser::new(core));
        drop(core_span);
        let phase3_span = startup::span("phase3.startup");
        browser.core.borrow_mut().phase3_startup();
        phase3::hooks::install();
        drop(phase3_span);

        // CEF events.
        let b = browser.clone();
        signet_web::gpui_pump::install(cx, move |ev, _cx| {
            b.core.borrow_mut().on_web_event(ev);
        });

        // Interface: theme, assets, API.
        let ui_span = startup::span("ui.init (theme, fonts, assets)");
        let settings = theme::settings_from_snapshot(&browser.core.borrow().snapshot());
        signet_ui::init(cx, settings);
        drop(ui_span);
        // Let CEF run its posted tasks now instead of in one go at the first pump (events stay queued).
        signet_web::pump();
        let state_span = startup::span("ui.app_state");
        let api: Rc<dyn SignetApi> = browser.clone();
        signet_ui::app_state::install(api, cx);
        phase3::install_extension_ui(&browser, cx);
        let ui = signet_ui::app_state::ui_state(cx);
        cx.observe(&ui, |_, cx| theme::sync(cx)).detach();
        drop(state_span);

        // Menus and shortcuts.
        let menu_span = startup::span("menus");
        app::menu::register_actions(&browser, cx);
        app::menu::install(&browser, cx);
        drop(menu_span);
        signet_web::pump();

        // Cmd+Q down and up, before the menu: hold-to-quit gauge (see `browser/quit.rs`).
        #[cfg(target_os = "macos")]
        {
            browser.core.borrow_mut().quit_read_system_timings();
            let b = browser.clone();
            signet_web::mac::install_key_monitor(move |k| {
                // Cmd+V on a page: the menu has no key equivalent for Paste, so send `paste:` to the page view
                // ourselves; Chromium reads the pasteboard (concealed one-time codes included).
                if let Some(action) = signet_web::mac::paste_action(&k) {
                    if signet_web::mac::send_action_to_page(action) {
                        return true;
                    }
                }
                let Ok(mut c) = b.core.try_borrow_mut() else { return false };
                match k.kind {
                    0 if k.chars == "q" && k.cmd && !k.ctrl && !k.alt && !k.shift && !c.recording_shortcuts => {
                        c.quit_key_down(k.repeat);
                        true
                    }
                    1 if k.chars == "q" => {
                        c.quit_key_up();
                        false
                    }
                    2 if !k.cmd => {
                        c.quit_key_up();
                        false
                    }
                    _ => false,
                }
            });
        }

        // Ctrl+Q on Windows and Linux: hold-to-quit gauge fed by the keyboard hook (Windows), the page key observer and
        // the GPUI window roots (see `quitkeys.rs`); the main loop drains what they decoded.
        #[cfg(not(target_os = "macos"))]
        {
            browser.core.borrow_mut().quit_read_system_timings();
            #[cfg(target_os = "windows")]
            signet_web::keys_win::install_key_hook(quitkeys::on_win_key);
            signet_web::set_key_observer(quitkeys::on_cef_key);
            let b = browser.clone();
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor().timer(Duration::from_millis(10)).await;
                    let Ok(mut c) = b.core.try_borrow_mut() else { continue };
                    quitkeys::set_paused(c.recording_shortcuts);
                    for k in quitkeys::take() {
                        match k {
                            quitkeys::QuitKey::Down { repeat } => c.quit_key_down(repeat),
                            quitkeys::QuitKey::Up => c.quit_key_up(),
                        }
                    }
                }
            })
            .detach();
        }

        // Keyboard hand-off between GPUI inputs and the native page views.
        focus::install(&browser, cx);

        // Window manager loop.
        let b = browser.clone();
        cx.spawn(async move |cx| windows::run(b, rx, cx).await).detach();

        // Store and history writes are deadline driven (500 ms after a change, 60 s for titles).
        let b = browser.clone();
        cx.spawn(async move |cx| {
            loop {
                // At most 250 ms: the results of background work (phone link, captures, extensions...) are drained
                // by the phase 3 tick.
                let wait = {
                    let c = b.core.borrow();
                    c.next_wake().map(|t| (t - signet_core::ids::now_ms()).clamp(50, 250) as u64).unwrap_or(250)
                };
                cx.background_executor().timer(Duration::from_millis(wait)).await;
                let t = startup::enabled().then(std::time::Instant::now);
                b.core.borrow_mut().tick();
                b.core.borrow_mut().phase3_tick();
                if let Some(t) = t.filter(|t| t.elapsed() > Duration::from_millis(16)) {
                    startup::mark_with(|| format!("core.tick took {:.1} ms", t.elapsed().as_secs_f64() * 1000.0));
                }
            }
        })
        .detach();

        // Links forwarded by a second launch.
        let tx2 = tx.clone();
        cx.spawn(async move |_cx| {
            let mut forwarded = forwarded;
            while let Some(urls) = forwarded.next().await {
                let _ = tx2.unbounded_send(Cmd::Urls(urls));
            }
        })
        .detach();

        let tx2 = tx.clone();
        cx.spawn(async move |_cx| {
            let mut open_rx = open_rx;
            while let Some(urls) = open_rx.next().await {
                let _ = tx2.unbounded_send(Cmd::Urls(urls));
            }
        })
        .detach();

        // Flush on quit (any path: menu, Dock, window).
        let b = browser.clone();
        cx.on_app_quit(move |_| {
            let b = b.clone();
            async move {
                // Quit from the menu or the Dock does not go through `Cmd::Quit`: hide the windows here too.
                #[cfg(target_os = "macos")]
                signet_web::mac::hide_all_windows();
                b.core.borrow_mut().finish_quit();
            }
        })
        .detach();

        // Open the main window, then the arguments given on the command line.
        startup::mark("show_main.sent");
        let _ = tx.unbounded_send(Cmd::ShowMain);
        dev::capture::start(&browser, cx);
        if !args.is_empty() {
            let _ = tx.unbounded_send(Cmd::Urls(args));
        }
        if !browser.core.borrow().state().settings.onboarded.unwrap_or(true) {
            let _ = tx.unbounded_send(Cmd::OpenWin(windows::Win::Welcome, None));
        }

        // Test hook: SIGNET_AUTOQUIT=<seconds> (1 means 8) quits cleanly after that delay.
        if let Some(secs) = std::env::var("SIGNET_AUTOQUIT").ok().and_then(|v| v.parse::<u64>().ok()).filter(|s| *s > 0) {
            let secs = if secs == 1 { 8 } else { secs };
            let tx3 = tx.clone();
            let b = browser.clone();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_secs(secs)).await;
                if b.core.borrow().debug {
                    b.core.borrow().dump();
                }
                println!("[autoquit]");
                let _ = tx3.unbounded_send(Cmd::Quit);
            })
            .detach();
        }
        // Test hook: SIGNET_SELFTEST=1 drives the app and logs PASS/FAIL lines (see selftest.rs).
        if env_flag("SIGNET_SELFTEST") {
            selftest::spawn(browser.clone(), cx);
        }
        cx.activate(true);
        startup::mark("gpui.run.callback.done");
        if startup::enabled() {
            trace_main_thread_stalls(cx);
        }
    });
}

/// `SIGNET_TRACE_STARTUP`: report the main thread being busy for more than 50 ms during the first 15 seconds (a
/// foreground timer of 10 ms that fires late).
fn trace_main_thread_stalls(cx: &mut App) {
    cx.spawn(async move |cx| {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            let t = std::time::Instant::now();
            cx.background_executor().timer(Duration::from_millis(10)).await;
            let late = t.elapsed().as_secs_f64() * 1000.0 - 10.0;
            if late > 50.0 {
                startup::mark_with(|| format!("main thread stall: {late:.0} ms"));
            }
        }
    })
    .detach();
}

//! Port of `browser/window.ts` (main window events), `windows/settings.ts`, `windows/library.ts`, `app/quit.ts`
//! (Cmd+Q confirmation) and the glue that asks the window manager for windows.

use raw_window_handle::RawWindowHandle;
use signet_core::api::UiEvent;
use signet_core::ids::now_ms;
use signet_core::types::{LibrarySection, QuitHudState};

use super::{Core, MainWin};
use crate::windows::{Cmd, Later, Win};

impl Core {
    pub fn on_main_created(&mut self, raw: Option<RawWindowHandle>, size: (f64, f64), origin: (f64, f64)) {
        self.views.parent = raw;
        self.win = Some(MainWin { size, origin, fullscreen: false, active: true });
        self.refresh_key_table();
        self.request_focus();
        self.send(Cmd::Later(60_000, Later::SleepCheck));
        self.send(Cmd::Later(5 * 60_000, Later::Sweep));
        self.log(format!("main window {}x{} at ({}, {})", size.0, size.1, origin.0, origin.1));
        self.changed();
    }

    /// The user closed the main window (macOS: the app stays alive; the Dock icon opens a new one). Port of
    /// `MainWindow.onClosed`: the layers and every tab view go, the store is written.
    pub fn on_main_closed(&mut self) {
        self.win = None;
        self.drop_peek();
        self.ov.payload = None;
        self.ov.leaving = false;
        self.ov.ready = false;
        self.find = Default::default();
        self.hover = Default::default();
        self.fullscreen = None;
        self.stop_slide();
        self.lx.overlay_raw = None;
        self.panes.clear();
        self.dividers.clear();
        self.shown_tabs.clear();
        self.desired = Default::default();
        self.preview = None;
        self.quit = Default::default();
        self.hidden_since.clear();
        let ids = self.views.ids();
        self.views.destroy_all();
        for id in ids {
            if let Some(t) = self.store.data.state.tabs.get_mut(&id) {
                t.loaded = false;
                t.loading = false;
                t.audible = false;
                t.can_go_back = false;
                t.can_go_forward = false;
            }
        }
        self.views.parent = None;
        self.flush();
        self.log("main window closed");
    }

    /// A secondary window (Settings, Library, Welcome) was closed by the user.
    pub fn on_window_closed(&mut self, win: Win) {
        self.open_windows.remove(&format!("{win:?}"));
    }

    pub fn on_main_resized(&mut self, size: (f64, f64), origin: (f64, f64)) {
        let Some(w) = self.win.as_mut() else { return };
        let moved = w.origin != origin;
        let resized = w.size != size;
        if !moved && !resized {
            return;
        }
        w.size = size;
        w.origin = origin;
        if resized {
            self.store.data.state.bounds = Some(signet_core::types::Rect::new(origin.0, origin.1, size.0, size.1));
            self.store.schedule(signet_core::core::store::LAZY_DELAY);
            self.dirty.layout = true;
            self.dirty.needs_full = true;
            self.log(format!("window resized {}x{}", size.0, size.1));
            self.apply();
        } else {
            // Moved only: floating surfaces follow (non-macOS reopen them).
            self.desired_dirty();
        }
    }

    fn desired_dirty(&mut self) {
        // Force the manager to recompute the screen positions.
        let mut d = self.desired.clone();
        d.preview = d.preview.map(|r| signet_core::types::Rect::new(r.x + 0.0, r.y, r.width, r.height));
        self.send(Cmd::Sync(d));
    }

    pub fn open_settings_window(&mut self, section: Option<&str>) {
        self.settings_section = section.map(str::to_string);
        self.send(Cmd::OpenWin(Win::Settings, section.map(str::to_string)));
    }

    pub fn open_library_window(&mut self, section: Option<&str>) {
        self.send(Cmd::OpenWin(Win::Library, section.map(str::to_string)));
    }

    pub fn open_welcome_window(&mut self) {
        self.send(Cmd::OpenWin(Win::Welcome, None));
    }

    pub fn on_window_opened(&mut self, win: Win, section: Option<String>) {
        self.open_windows.insert(format!("{win:?}"));
        self.on_window_section(win, section);
    }

    pub fn on_window_section(&mut self, win: Win, section: Option<String>) {
        if win == Win::Library
            && let Some(s) = section.as_deref().and_then(|s| serde_json::from_value::<LibrarySection>(serde_json::json!(s)).ok())
        {
            self.emit(UiEvent::LibrarySection(s));
        }
    }

    // ------------------------------------------------------------------ quit (app/quit.ts)

    pub fn quit_now(&mut self) {
        self.quit.confirmed = true;
        self.send(Cmd::Quit);
    }

    /// The one quit sequence, whatever the path: module hooks (they still need the views), then the store flush,
    /// then the views come down. Runs once; later calls do nothing.
    pub fn finish_quit(&mut self) {
        if !self.quit_hooks_ran {
            self.quit_hooks_ran = true;
            self.phase3_quit();
        }
        self.prepare_quit();
        // A downloaded update is installed once the profile is released (no cost when none is waiting).
        self.run_pending_update();
    }

    /// Flush the store and take the views down (call `finish_quit` instead, the module hooks come first).
    pub fn prepare_quit(&mut self) {
        if self.quitting {
            return;
        }
        self.quitting = true;
        self.log("quitting: flush");
        let t = std::time::Instant::now();
        self.flush();
        if self.debug {
            println!("[quit] flush: {} ms", t.elapsed().as_millis());
        }
        let t = std::time::Instant::now();
        self.views.destroy_all();
        if self.debug {
            println!("[quit] destroy_all: {} ms", t.elapsed().as_millis());
        }
        crate::app::single_instance::release(&self.data_dir);
    }
}

impl Core {
    /// `SIGNET_DEBUG`: window, views and tabs, for checking without a screenshot.
    pub fn dump(&self) {
        let w = self.win.as_ref();
        println!("[dump] window {:?} origin {:?}", w.map(|w| w.size), w.map(|w| w.origin));
        println!("[dump] tabs {} views {} panes {}", self.state().tabs.len(), self.views.ids().len(), self.panes.len());
        for id in self.views.ids() {
            if let Some(v) = self.views.view_of(&id) {
                let t = self.state().tabs.get(&id);
                println!("[dump] view {} tab {} visible={} {} | {} | {}", v.id(), id, v.is_visible(), v.debug_native_frame(), t.map(|t| t.title.as_str()).unwrap_or(""), t.map(|t| t.url.as_str()).unwrap_or(""));
            }
        }
        println!("[dump] surfaces {:?}", self.desired);
    }
}

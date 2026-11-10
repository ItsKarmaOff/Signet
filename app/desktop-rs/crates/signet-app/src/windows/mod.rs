//! Window manager: the GPUI windows of the app. The browser core never touches GPUI; it sends `Cmd`s here.
//!
//! - The main window hosts the CEF page views (native children).
//! - Floating surfaces (overlay, floating sidebar and toolbar, find bar, toast, tab preview, quit HUD) are
//!   transparent borderless GPUI windows placed over the main window, because native CEF views are drawn above all
//!   GPUI content of their own window (PORTING.md section 7). The core declares which ones it wants and where
//!   (`Desired`, window coordinates); `Manager::sync` opens, moves and closes them. On macOS they are child windows
//!   of the main one (`addChildWindow:`), so they move and order with it. The floating sidebar window is kept open,
//!   parked (transparent, ignoring the mouse), while the sidebar is hidden (macOS), so a reveal does not build one.
//! - Secondary windows (Settings, Library, Welcome) are ordinary windows opened once and focused when asked again.

// "aux" is a reserved device name on Windows: the file cannot be called aux.rs there.
#[path = "aux_window.rs"]
pub mod aux;
pub mod factory;
pub mod host;
pub mod manager;
pub mod pip;
pub mod popup_menu;

use std::rc::Rc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::AsyncApp;
use signet_core::types::Rect;

use crate::browser::{Browser, Core};

/// A floating surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Overlay,
    Sidebar,
    Toolbar,
    Find,
    Toast,
    Preview,
    Quit,
}

impl Surface {
    pub const ALL: [Surface; 7] = [Surface::Overlay, Surface::Sidebar, Surface::Toolbar, Surface::Find, Surface::Toast, Surface::Preview, Surface::Quit];

    /// Surfaces that take the keyboard when they open.
    pub fn takes_focus(self) -> bool {
        matches!(self, Surface::Overlay | Surface::Find)
    }

    pub fn name(self) -> &'static str {
        match self {
            Surface::Overlay => "overlay",
            Surface::Sidebar => "sidebar",
            Surface::Toolbar => "toolbar",
            Surface::Find => "find",
            Surface::Toast => "toast",
            Surface::Preview => "preview",
            Surface::Quit => "quit",
        }
    }
}

/// Where each floating surface must be (window coordinates), `None` = not shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Desired {
    pub overlay: Option<Rect>,
    pub sidebar: Option<Rect>,
    /// Where the floating sidebar window waits, parked, while the sidebar is hidden (macOS keeps it open so a reveal
    /// does not build a window).
    pub sidebar_warm: Option<Rect>,
    pub toolbar: Option<Rect>,
    pub find: Option<Rect>,
    pub toast: Option<Rect>,
    pub preview: Option<Rect>,
    pub quit: Option<Rect>,
}

impl Desired {
    pub fn get(&self, s: Surface) -> Option<Rect> {
        match s {
            Surface::Overlay => self.overlay,
            Surface::Sidebar => self.sidebar,
            Surface::Toolbar => self.toolbar,
            Surface::Find => self.find,
            Surface::Toast => self.toast,
            Surface::Preview => self.preview,
            Surface::Quit => self.quit,
        }
    }
}

/// Secondary windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Win {
    Settings,
    Library,
    Welcome,
    /// QR code of a passkey request (one at a time, always fresh).
    Passkey,
    /// Boost editor; one per site (the site is the `section` of `Cmd::OpenWin`).
    Boost,
    /// Detached developer tools (`#devtools`): a 36 px header over the frontend view.
    Devtools,
}

/// Delayed actions of the core.
#[derive(Clone, Debug)]
pub enum Later {
    UndoExpire(u64),
    SidebarLeave(u64),
    SidebarWarm,
    ToolbarLeave,
    SleepCheck,
    Sweep,
    SlideStep,
    QuitHudHide,
    QuitTick,
    Focus,
}

/// Entry of a native popup menu.
pub enum Entry {
    Sep,
    Item { label: String, id: String, enabled: bool, checked: bool },
    Sub { label: String, items: Vec<Entry> },
}

pub enum Cmd {
    /// Floating surfaces wanted by the core.
    Sync(Desired),
    /// An already open surface (find bar, overlay) must take the keyboard again (Cmd+F after a click in the page, a new
    /// popover with a text field). Nothing happens when the surface does not exist yet: it takes the key when it opens.
    MakeKey(Surface),
    /// Apply the batched broadcast after 32 ms.
    SoftApply,
    RebuildMenu,
    Later(u64, Later),
    Popup { entries: Vec<Entry>, at: (f64, f64), cb: Box<dyn FnOnce(&mut Core, &str)> },
    ShowMain,
    CloseMain,
    /// The main window was closed by the user (macOS: the app stays alive; the manager forgets it and its surfaces).
    MainClosed,
    /// Runs a closure on the core, in the order of the commands: worker threads post their results with it.
    Run(Box<dyn FnOnce(&mut Core) + Send>),
    /// The mini player queues (`Core::pip_link`) have something for the window manager.
    PipPump,
    ToggleFullscreen,
    Minimize,
    ZoomWindow,
    PageFullscreen(bool),
    OpenWin(Win, Option<String>),
    /// Little Signet or private window `id` (see `phase3/windows.rs`).
    OpenAux { id: u64, kind: aux::AuxKind },
    CloseAux(u64),
    FocusAux(u64),
    CloseWin(Win),
    /// Store flush and quit.
    Quit,
    Urls(Vec<String>),
}

thread_local! {
    static REOPEN: std::cell::RefCell<Option<futures::channel::mpsc::UnboundedSender<Cmd>>> = const { std::cell::RefCell::new(None) };
}

/// Dock click with no window open (macOS `activate`): the main window comes back. `Application::on_reopen` only
/// exists before `run`, so `main.rs` calls this on the `Application` and `run` below provides the command channel.
pub fn on_reopen(app: &gpui::Application) {
    app.on_reopen(|_| {
        REOPEN.with(|r| {
            if let Some(tx) = r.borrow().as_ref() {
                let _ = tx.unbounded_send(Cmd::ShowMain);
            }
        });
    });
}

/// The command loop: runs on the GPUI foreground executor for the whole life of the app.
pub async fn run(browser: Rc<Browser>, mut rx: UnboundedReceiver<Cmd>, cx: &mut AsyncApp) {
    let mut mgr = manager::Manager::default();
    REOPEN.with(|r| *r.borrow_mut() = Some(browser.core.borrow().cmds.clone()));
    while let Some(cmd) = rx.next().await {
        mgr.handle(&browser, cmd, cx);
    }
}

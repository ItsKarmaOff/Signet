//! PHASE3: Chrome extensions (`signet-extensions`).
//!
//! CEF 156 has no extension system for the child views Signet uses, so the whole runtime is ours (see the
//! signet-extensions README). The pieces:
//!
//! - `chrome-extension://` pages are served by `files` (registered with `signet_web::set_extension_handler`).
//! - The bundle hook (`wire`) gives every extension frame its page script (`worker` for the hidden background page,
//!   `page` for popups, options and tabs, `content` for content scripts, `webstore` for the Web Store).
//! - `signet-crx*` page messages (seen by `web_event`) become `Call`s of the `ExtensionHost`; the host's `Effect`s are
//!   carried out by `effects`.
//! - The toolbar buttons (`ui`), the popup (`popup`) and the API methods of `SignetApi` (`api`) complete it.
//!
//! Everything but the helper-thread hooks runs on the main thread. Replies and events reach the pages through
//! `window.__signetDispatch`.

mod api;
mod bridge;
mod ctx;
mod dialogs;
mod effects;
mod events;
mod files;
mod popup;
mod shared;
mod tabhost;
mod ui;
mod wire;

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use signet_core::types::ExtensionActionStates;
use signet_extensions::host::{ExtensionHost, HostConfig};
use signet_extensions::reload::ActionRelay;
use signet_web::{ViewId, WebEvent, WebView};

use crate::browser::Core;
use shared::Shared;

pub use ui::install_ui;

/// Where a reply goes.
pub(super) struct ReplyTo {
    pub view: ViewId,
    pub frame: String,
    pub msg_id: String,
}

/// A tab as it was at the last tick, to find what changed.
#[derive(Clone, PartialEq, Default)]
pub(super) struct TabSig {
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
    pub loading: bool,
    pub audible: bool,
    pub muted: bool,
    pub pinned: bool,
}

pub(super) struct Runtime {
    pub shared: Arc<Shared>,
    /// Background view of each extension.
    pub bg: HashMap<String, WebView>,
    /// Extensions whose background waits for the main window.
    pub waiting: Vec<String>,
    /// Events for a worker that is not up yet.
    pub worker_events: HashMap<String, Vec<(String, Vec<Value>)>>,
    /// `StartWorker` tickets waiting for the worker: (extension, ticket, deadline ms).
    pub wake: Vec<(String, u64, i64)>,
    pub replies: HashMap<u64, ReplyTo>,
    pub next_call: u64,
    pub popup: Option<popup::Popup>,
    pub relay: ActionRelay,
    /// Fallback anchors for popups: the last place each button was clicked.
    pub anchors: HashMap<String, signet_core::types::Rect>,
    pub tabs: Option<HashMap<String, TabSig>>,
    pub last_active: Option<String>,
    pub states: ExtensionActionStates,
    /// Offscreen documents: view to (extension, page id) and views that failed.
    pub offscreen: HashMap<ViewId, String>,
    /// Keys added to the shared key table for extension commands.
    pub command_keys: Vec<String>,
    /// Tab creations waiting for the answer to give (call id).
    pub last_url: HashMap<ViewId, String>,
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    pub(super) rt: Option<Runtime>,
    /// Proxy 407 challenges waiting for an extension's answer, by broker key.
    pub(super) proxy_auth: HashMap<String, signet_web::AuthRequest>,
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    let mut cfg = HostConfig::new(core.data_dir.clone());
    cfg.locale = if core.locale() == signet_core::i18n::Locale::Fr { "fr-FR".into() } else { "en-US".into() };
    let keys = core.key_table.clone();
    cfg.signet_keys = Arc::new(move || keys.lock().map(|t| t.iter().filter(|(_, v)| !v.starts_with("ext:")).map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default());
    let host = ExtensionHost::new(cfg);
    let shared = Arc::new(Shared::new(host));
    wire::install(&shared);
    core.phase3.extensions.rt = Some(Runtime {
        shared: shared.clone(),
        bg: HashMap::new(),
        waiting: Vec::new(),
        worker_events: HashMap::new(),
        wake: Vec::new(),
        replies: HashMap::new(),
        next_call: 1,
        popup: None,
        relay: ActionRelay::new(),
        anchors: HashMap::new(),
        tabs: None,
        last_active: None,
        states: Default::default(),
        offscreen: HashMap::new(),
        command_keys: Vec::new(),
        last_url: HashMap::new(),
    });
    let s = &core.state().settings;
    let (unpacked, pinned, order) = (s.unpacked_extensions.clone().unwrap_or_default(), s.pinned_extensions.clone().unwrap_or_default(), s.extension_order.clone().unwrap_or_default());
    let fx = shared.host().startup(unpacked, pinned, order);
    core.ext_apply(fx);
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    if core.phase3.extensions.rt.is_none() {
        return;
    }
    core.ext_web_event(ev);
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    if core.phase3.extensions.rt.is_none() {
        return;
    }
    core.ext_tick();
    core.ext_install_requested();
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    let Some(rt) = core.phase3.extensions.rt.as_mut() else { return };
    if let Some(p) = rt.popup.take() {
        p.view.close();
    }
    for (_, v) in rt.bg.drain() {
        v.close();
    }
    rt.shared.host().flush();
}

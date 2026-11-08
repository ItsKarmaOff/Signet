//! `ExtensionHost`: the object the app drives. Events in, decisions out, no engine calls inside.
//!
//! The host owns every model of this crate (registry, permissions, alarms, settings, DNR, menus, commands, notifications,
//! action state, storage, messaging, event routing). The app (and the web layer under it) feeds it:
//!
//! - **calls** from extension contexts ([`ExtensionHost::call`]): the web layer derives the extension id and the context
//!   from the sender, never from the arguments;
//! - **events** from the browser: tabs, navigations, requests, shortcuts, dialogs, workers, notifications, files;
//! - **time** ([`ExtensionHost::tick`]).
//!
//! and carries out the returned [`Effect`]s. `call` may block on [`TabHost`] (screenshots...): do not call it on the UI
//! thread.

use crate::action::{set_icon, ActionModel, ActionStates, IconUpdate};
use crate::auth::{AuthOutcome, Handled, ProxyAuthBroker, ProxyChallenge};
use crate::commands::{CommandBinding, CommandPlatform, CommandRun, CommandsDeps, ExtensionCommands};
use crate::common::{ApiContext, ApiError, ApiResult, ContextKind, CrxResult, Emitter, Outbox};
use crate::contentscripts::{scripts_for, FrameContext};
use crate::contextmenus::{contexts_of, ContextMenus, MenuNode, MenuTarget, ACTION_CONTEXTS};
use crate::contexts::{background_context, filter_contexts, page_context, ExtensionContext, Offscreen, StubRegistry};
use crate::crx::{install_crx, install_from_store, is_store_url, update_from_store, CrxError, Fetcher};
use crate::dnr::PersistedStore;
use crate::dnr_engine::{ActionCounts, DnrEngine, DnrEvent, GrantSource, RequestDetails, RequestOutcome};
use crate::manifest::{BackgroundEntry, Manifest, RunAt};
use crate::messaging::{Delivery, Kind, Messaging, MsgContext, Target};
use crate::navigation::{FrameInfo, NavigationExtras};
use crate::notifications::{ExtensionNotifications, NotifEffect};
use crate::permissions::{describe_manifest, PermEvent, PermissionGrants, PermissionLine, PermissionSet};
use crate::registry::{ExtensionInfo, ExtensionRegistry, Installed, Source};
use crate::router::{check_call, EventRouter, PermissionTable, RouterAction};
use crate::schedule::Alarms;
use crate::settings::{privacy_defs, proxy_setting, Apply, ChromeSettings};
use crate::shared::{AUTH_LISTENERS_METHOD, AUTH_RESPONSE_METHOD, INSTALLED_METHOD};
use crate::storage::ExtensionStorage;
use crate::tabs::{TabHost, TabsExtras};
use crate::unpacked::Unpacked;
use crate::urlmatch::{extension_file_path, Access};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Permissions whose holders the emitter must know (`extensions_with`).
const WATCHED_PERMISSIONS: [&str; 5] = ["proxy", "privacy", "webNavigation", "webRequest", "webRequestAuthProvider"];

pub struct HostConfig {
    /// Data folder of the app (`Signet GPUI/`): extensions, storage and state files live under it.
    pub data_dir: PathBuf,
    pub platform: CommandPlatform,
    pub app_name: String,
    /// Language of the interface, for `__MSG_key__` (`fr-FR`).
    pub locale: String,
    /// Signet's own shortcuts by identity key (`ctrl+shift+T`) to owner label; extension shortcuts never take these.
    pub signet_keys: Arc<dyn Fn() -> HashMap<String, String> + Send + Sync>,
    /// Chromium version sent to the Web Store (`prodversion`).
    pub prodversion: String,
    /// Milliseconds since the epoch.
    pub now_ms: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl HostConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        HostConfig {
            data_dir: data_dir.into(),
            platform: CommandPlatform::current(),
            app_name: "Signet".into(),
            locale: "en-US".into(),
            signet_keys: Arc::new(HashMap::new),
            prodversion: crate::crx::DEFAULT_PRODVERSION.into(),
            now_ms: Arc::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)),
        }
    }
}

/// A call from an extension context.
#[derive(Debug, Clone)]
pub struct Call {
    /// Id the web layer gave the call, echoed in the `Reply` effect.
    pub id: u64,
    pub extension_id: String,
    pub context: ApiContext,
    pub method: Value,
    pub args: Value,
}

/// What the app must do.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Answer a call: give `result.to_json()` to the context (`window.__signetDispatch("reply", ...)`).
    Reply { call_id: u64, result: CrxResult },
    /// Deliver an event to extension contexts (see [`RouterAction`]).
    Router(RouterAction),
    /// Deliver an event to one context (messaging, ports).
    ToContext { context_id: String, name: String, args: Vec<Value> },
    /// Register the extension with the engine: serve `chrome-extension://<id>/` from `dir`, start its background, inject
    /// content scripts. `manifest` is the raw manifest.
    LoadExtension { id: String, dir: PathBuf, manifest: Value, background: BackgroundEntry },
    /// Stop its background and forget it (the files stay).
    UnloadExtension { id: String },
    /// Unload, wait until its worker stopped, load again (an unpacked folder changed).
    ReloadExtension { id: String, dir: PathBuf },
    /// Show the permission dialog for `need`, then call `permission_answer(ticket, allowed)`.
    PermissionPrompt { ticket: u64, extension_id: String, name: String, lines: Vec<PermissionLine>, need: PermissionSet },
    Notification(NotifEffect),
    /// Make a setting effective (proxy, WebRTC policy).
    ApplySetting(Apply),
    /// Draw this icon on the extension's button.
    SetActionIcon(IconUpdate),
    /// Button state for the toolbar (`{ extId: { enabled?, textColor?, tabs? } }`).
    ActionStates(Value),
    /// Blocked-request counts for the toolbar badges.
    ActionCounts(ActionCounts),
    /// The network hooks that must be wired changed.
    NetworkHooks(Vec<DnrEvent>),
    /// Shortcuts of extensions changed: rebuild the menu (`command_bindings`).
    CommandsChanged,
    /// Open a hidden page for `chrome.offscreen`; report with `offscreen_loaded` / `offscreen_failed`.
    OffscreenOpen { extension_id: String, url: String },
    OffscreenClose { page_id: i64 },
    /// A tab action the host cannot do itself (`create`, `update`, `remove`, `reload`, `executeScript`...): do it, then
    /// call `tab_action_done(call_id, result)`.
    TabAction { call_id: u64, extension_id: String, method: String, args: Vec<Value> },
    /// Open the extension's options page in a tab.
    OpenOptions { extension_id: String },
    /// A shortcut asks to click the extension's button: open `popup_url` under it (ask the toolbar first, see
    /// `reload::ActionRelay`), or nothing to do when the button has no popup (`action.onClicked` was raised).
    RunAction { extension_id: String, popup_url: Option<String> },
    /// The Settings list or the toolbar changed.
    ExtensionsChanged,
    /// New `settings.unpackedExtensions` / `settings.pinnedExtensions` / `settings.extensionOrder` to store.
    UnpackedPathsChanged(Vec<String>),
    PinnedChanged(Vec<String>),
    OrderChanged(Vec<String>),
    /// The outcome of a proxy authentication challenge (`proxy_auth_challenge` returned `Pending(key)`).
    ProxyAuth { key: String, outcome: AuthOutcome },
    /// The Web Store asks to install `extension_id`: show the dialog, then call `install_decision(ticket, allowed)`.
    InstallPrompt { ticket: u64, extension_id: String, name: String, lines: Vec<PermissionLine> },
    /// Download and install from the store (`install_from_store`), then call `store_install_done(call_id, result)`.
    InstallFromStore { call_id: u64, extension_id: String },
    /// Open a URL in a tab (uninstall URL...).
    OpenUrl { url: String },
    /// Development log line.
    Log(String),
}

/// One frame of a page, for content scripts.
#[derive(Debug, Clone)]
pub struct Frame<'a> {
    pub url: &'a str,
    pub is_main_frame: bool,
    pub fallback_url: Option<&'a str>,
}

/// Content scripts for one frame, ready to inject.
#[derive(Debug, Clone, PartialEq)]
pub struct Injection {
    pub extension_id: String,
    pub run_at: RunAt,
    pub css: String,
    pub js: String,
}

/// State the closures given to sub-models read.
#[derive(Default)]
struct Shared {
    dirs: HashMap<String, PathBuf>,
    grants: HashMap<String, (Vec<String>, Vec<String>)>,
    proxy_responder: Option<String>,
}

struct SharedGrants(Arc<Mutex<Shared>>);
impl GrantSource for SharedGrants {
    fn permissions_of(&self, id: &str) -> Vec<String> {
        self.0.lock().map(|s| s.grants.get(id).map(|g| g.0.clone()).unwrap_or_default()).unwrap_or_default()
    }
    fn origins_of(&self, id: &str) -> Vec<String> {
        self.0.lock().map(|s| s.grants.get(id).map(|g| g.1.clone()).unwrap_or_default()).unwrap_or_default()
    }
}

/// `__MSG_key__` in the locale of the interface, read from the extension's `_locales`.
fn localize_in(dir: &Path, locale: &str, text: &str) -> String {
    let default_locale = crate::registry::read_manifest(dir).ok().and_then(|m| m.default_locale);
    let candidates = crate::commands::locale_candidates(locale, default_locale.as_deref());
    let load = |l: &str| -> Option<Value> { serde_json::from_str(&std::fs::read_to_string(dir.join("_locales").join(l).join("messages.json")).ok()?).ok() };
    crate::commands::localize_text(text, &candidates, &load)
}

fn finish(r: ApiResult<Value>) -> CrxResult {
    match r {
        Ok(Value::Null) => CrxResult::Ok(None),
        Ok(v) => CrxResult::ok(v),
        Err(e) => CrxResult::err(e.0),
    }
}

fn reply(call_id: u64, result: CrxResult) -> Effect {
    Effect::Reply { call_id, result }
}

pub struct ExtensionHost {
    cfg: HostConfig,
    pub registry: ExtensionRegistry,
    pub unpacked: Unpacked,
    unpacked_paths: Vec<String>,
    pinned: Vec<String>,
    order: Vec<String>,
    out: Arc<Outbox>,
    shared: Arc<Mutex<Shared>>,
    table: PermissionTable,
    events: EventRouter,
    grants: PermissionGrants,
    alarms: Alarms,
    settings: ChromeSettings,
    pub dnr: DnrEngine,
    menus: ContextMenus,
    commands: ExtensionCommands,
    actions: ActionStates,
    action_model: ActionModel,
    notifications: ExtensionNotifications,
    navigation: NavigationExtras,
    tabs_extras: TabsExtras,
    storage: ExtensionStorage,
    offscreen: Offscreen,
    auth: ProxyAuthBroker,
    messaging: Messaging,
    stubs: StubRegistry,
    uninstall_urls: HashMap<String, String>,
    /// Calls waiting for an answer.
    perm_calls: HashMap<u64, u64>,
    offscreen_calls: HashMap<String, u64>,
    tab_calls: HashSet<u64>,
    /// Extensions currently loaded in the engine.
    active: HashSet<String>,
    install_ticket: u64,
    install_prompts: HashMap<u64, (u64, String)>,
    approved_installs: HashSet<String>,
    last_hooks: Vec<DnrEvent>,
    last_commands_key: String,
}

impl ExtensionHost {
    pub fn new(cfg: HostConfig) -> Self {
        let data = cfg.data_dir.clone();
        let out = Arc::new(Outbox::new());
        let emitter: Arc<dyn Emitter> = out.clone();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let registry = ExtensionRegistry::load(data.join("Extensions"), data.join("extensions-state.json"));

        let keys = cfg.signet_keys.clone();
        let (s1, locale) = (shared.clone(), cfg.locale.clone());
        let commands = ExtensionCommands::new(CommandsDeps {
            platform: cfg.platform,
            emitter: emitter.clone(),
            signet_keys: Box::new(move || keys()),
            localize: Box::new(move |id, text| match s1.lock().ok().and_then(|s| s.dirs.get(id).cloned()) {
                Some(dir) => localize_in(&dir, &locale, text),
                None => text.to_string(),
            }),
            active_tab: Box::new(|_| None),
        });

        let mut defs = vec![proxy_setting()];
        defs.extend(privacy_defs());
        let settings = ChromeSettings::new(data.join("extension-settings.json"), defs, emitter.clone());

        let mut dnr = DnrEngine::new(crate::dnr::DnrStore::new(data.join("extension-dnr.json")));
        dnr.set_grants(Arc::new(SharedGrants(shared.clone())));
        dnr.set_now((cfg.now_ms)());

        let mut menus = ContextMenus::new(emitter.clone());
        menus.persist(data.join("extension-context-menus.json"));

        let s2 = shared.clone();
        let notifications = ExtensionNotifications::new(
            emitter.clone(),
            cfg.platform,
            &cfg.app_name,
            Box::new(move |ext, url| {
                if url.starts_with("data:") {
                    return Some(url.to_string());
                }
                let dir = s2.lock().ok()?.dirs.get(ext).cloned()?;
                let p = extension_file_path(&dir, ext, url)?;
                p.exists().then(|| p.to_string_lossy().into_owned())
            }),
            true,
        );

        let s3 = shared.clone();
        let auth = ProxyAuthBroker::new(emitter.clone(), Box::new(move || s3.lock().ok().and_then(|s| s.proxy_responder.clone())));

        let mut host = ExtensionHost {
            registry,
            unpacked: Unpacked::new(),
            unpacked_paths: vec![],
            pinned: vec![],
            order: vec![],
            table: PermissionTable::default(),
            events: EventRouter::new(),
            grants: PermissionGrants::new(data.join("extension-permissions.json"), emitter.clone()),
            alarms: Alarms::new(data.join("extension-alarms.json"), emitter.clone()),
            settings,
            dnr,
            menus,
            commands,
            actions: ActionStates::new(),
            action_model: ActionModel::new(),
            notifications,
            navigation: NavigationExtras::new(emitter.clone()),
            tabs_extras: TabsExtras::default(),
            storage: ExtensionStorage::new(data.join("Extension Storage"), emitter.clone()),
            offscreen: Offscreen::default(),
            auth,
            messaging: Messaging::new(),
            stubs: StubRegistry::default(),
            uninstall_urls: HashMap::new(),
            perm_calls: HashMap::new(),
            offscreen_calls: HashMap::new(),
            tab_calls: HashSet::new(),
            active: HashSet::new(),
            install_ticket: 0,
            install_prompts: HashMap::new(),
            approved_installs: HashSet::new(),
            last_hooks: vec![],
            last_commands_key: String::new(),
            out,
            shared,
            cfg,
        };
        host.register_methods();
        host
    }

    fn now(&self) -> u64 {
        (self.cfg.now_ms)()
    }

    // ---------------------------------------------------------------- method table

    fn register_methods(&mut self) {
        let t = &mut self.table;
        for m in ["create", "get", "getAll", "clear", "clearAll"] {
            t.register(&["alarms"], &format!("alarms.{m}"));
        }
        for m in ["request", "contains", "getAll", "remove"] {
            t.register(&[""], &format!("permissions.{m}"));
        }
        for key in self.settings.keys() {
            let perm = self.settings.permission_of(&key).unwrap_or("").to_string();
            for op in crate::shared::SETTING_METHODS {
                t.register(&[perm.as_str()], &format!("{key}.{op}"));
            }
        }
        for m in [
            "updateDynamicRules", "getDynamicRules", "updateSessionRules", "getSessionRules", "isRegexSupported", "getEnabledRulesets",
            "updateEnabledRulesets", "getAvailableStaticRuleCount", "getMatchedRules", "testMatchOutcome", "setExtensionActionOptions",
        ] {
            t.register(&["declarativeNetRequest", "declarativeNetRequestWithHostAccess"], &format!("declarativeNetRequest.{m}"));
        }
        t.register(&[""], "commands.getAll");
        for m in ["create", "update", "remove", "removeAll"] {
            t.register(&["contextMenus"], &format!("contextMenus.{m}"));
        }
        for m in ["create", "update", "clear", "getAll", "getPermissionLevel"] {
            t.register(&["notifications"], &format!("notifications.{m}"));
        }
        for ns in ["action", "browserAction"] {
            for m in ["enable", "disable", "isEnabled", "setBadgeTextColor", "getBadgeTextColor", "setIcon", "setBadgeText", "getBadgeText", "setBadgeBackgroundColor", "getBadgeBackgroundColor", "setTitle", "getTitle", "setPopup", "getPopup"] {
                t.register(&[""], &format!("{ns}.{m}"));
            }
        }
        for m in ["captureVisibleTab", "highlight", "insertCSS", "removeCSS", "detectLanguage", "get", "query", "getCurrent", "create", "update", "remove", "reload", "sendMessage", "goBack", "goForward", "duplicate", "move"] {
            t.register(&[""], &format!("tabs.{m}"));
        }
        for m in ["getAll", "getCurrent", "get", "getLastFocused"] {
            t.register(&[""], &format!("windows.{m}"));
        }
        for m in ["createDocument", "closeDocument", "hasDocument"] {
            t.register(&["offscreen"], &format!("offscreen.{m}"));
        }
        for area in ["local", "session", "sync", "managed"] {
            for op in ["get", "set", "remove", "clear", "getBytesInUse", "getKeys", "setAccessLevel"] {
                t.register(&["storage"], &format!("storage.{area}.{op}"));
            }
        }
        for m in ["getContexts", "sendMessage", "connect", "openOptionsPage", "reload", "setUninstallURL", "getPlatformInfo", "__reply", "__accept", "__decline", "__portPost", "__portClose", "__openContext", "__closeContext"] {
            t.register(&[""], &format!("runtime.{m}"));
        }
        t.register(&[""], "i18n.getAllMessages");
        t.register(&[""], "management.getSelf");
        t.register(&[""], INSTALLED_METHOD);
        t.register(&["webRequest", "webRequestAuthProvider"], AUTH_RESPONSE_METHOD);
        t.register(&["webRequest", "webRequestAuthProvider"], AUTH_LISTENERS_METHOD);
    }

    /// Methods an extension may call, with the permissions that unlock them (for the web layer's own filtering).
    pub fn knows_method(&self, method: &str) -> bool {
        self.table.knows(method)
    }

    // ---------------------------------------------------------------- startup, load, unload

    /// Startup: remembered folders and pinned buttons come from `state.json` settings. Returns the load effects of every
    /// enabled extension (store ones first, then unpacked ones, like Electron).
    pub fn startup(&mut self, unpacked_paths: Vec<String>, pinned: Vec<String>, order: Vec<String>) -> Vec<Effect> {
        self.unpacked.restore(&unpacked_paths);
        self.unpacked_paths = unpacked_paths.clone();
        self.pinned = pinned;
        self.order = order;
        let mut fx = Vec::new();
        let ids: Vec<String> = self.registry.enabled().map(|i| i.id.clone()).collect();
        for id in ids {
            self.activate(&id, &mut fx);
        }
        for p in unpacked_paths {
            let path = PathBuf::from(&p);
            fx.extend(self.load_unpacked(&path, false));
        }
        self.collect_garbage();
        for id in self.active.clone() {
            self.out.emit(&id, "runtime.onStartup", vec![], true, true);
        }
        self.pump(&mut fx);
        fx
    }

    fn collect_garbage(&mut self) {
        // Skipped while an unpacked extension failed to load: a missing id then proves nothing.
        if !self.unpacked.failures(&self.unpacked_paths).is_empty() {
            return;
        }
        let installed: HashSet<String> = self.registry.list().iter().map(|i| i.id.clone()).collect();
        let mut known = self.menus.known_ids();
        known.extend(self.settings.known_ids());
        known.extend(self.grants.known_ids());
        known.extend(self.alarms.scheduler.known_ids());
        known.extend(self.storage.known_ids());
        known.extend(PersistedStore::known_ids(&self.dnr.store));
        known.sort();
        known.dedup();
        for id in known.into_iter().filter(|id| !installed.contains(id)) {
            self.forget_data(&id);
        }
    }

    fn activate(&mut self, id: &str, fx: &mut Vec<Effect>) {
        let Some(item) = self.registry.get(id).cloned() else { return };
        let raw = item.manifest.raw.clone();
        {
            let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            s.dirs.insert(id.to_string(), item.dir.clone());
        }
        self.grants.reconcile(id, &raw);
        self.commands.register(id, &item.name, &raw);
        self.menus.restore(id);
        self.settings.reapply(id);
        self.alarms.scheduler.activate(id);
        self.storage.set_unlimited(id, item.manifest.permissions.iter().any(|p| p == "unlimitedStorage"));
        self.dnr.add_extension(id, &item.dir, &item.version, &raw);
        self.active.insert(id.to_string());
        self.refresh_holders();
        fx.push(Effect::LoadExtension { id: id.to_string(), dir: item.dir.clone(), manifest: raw, background: item.manifest.background_entry() });
        for w in &item.manifest.warnings {
            fx.push(Effect::Log(format!("[extensions] {} ({id}): {w}", item.name)));
        }
        fx.push(Effect::ExtensionsChanged);
    }

    /// An unload is not an uninstall (reload of an unpacked extension, store update): only runtime state goes.
    fn deactivate(&mut self, id: &str, fx: &mut Vec<Effect>) {
        if !self.active.remove(id) {
            return;
        }
        self.commands.unregister(id);
        self.notifications.unload(id);
        self.tabs_extras.forget_extension(id);
        self.actions.forget_extension(id);
        self.action_model.forget_extension(id);
        if let Some(page) = self.offscreen.close_for(id) {
            fx.push(Effect::OffscreenClose { page_id: page });
        }
        self.dnr.store.clear_session(id);
        self.dnr.remove_extension(id);
        self.alarms.scheduler.suspend(id);
        self.storage.clear_session(id);
        self.events.forget(id);
        self.refresh_holders();
        fx.push(Effect::UnloadExtension { id: id.to_string() });
        fx.push(Effect::ExtensionsChanged);
    }

    /// Everything kept per extension until it is uninstalled.
    fn forget_data(&mut self, id: &str) {
        self.menus.unload(id);
        self.settings.release(id);
        self.dnr.store.remove(id);
        self.grants.drop_extension(id);
        self.alarms.scheduler.remove(id);
        self.storage.forget(id);
        self.uninstall_urls.remove(id);
    }

    fn refresh_holders(&mut self) {
        for perm in WATCHED_PERMISSIONS {
            let ids: Vec<String> = self
                .registry
                .enabled()
                .filter(|i| self.active.contains(&i.id) && crate::router::has_permission(&i.manifest.raw, perm, &self.grants.permissions_of(&i.id)))
                .map(|i| i.id.clone())
                .collect();
            self.out.set_holders(perm, ids);
        }
        let controller = self.settings.controller_of("proxy.settings").map(String::from);
        let responder = controller.filter(|id| self.out.extensions_with("webRequest").contains(id) || self.out.extensions_with("webRequestAuthProvider").contains(id));
        self.shared.lock().unwrap_or_else(|e| e.into_inner()).proxy_responder = responder;
    }

    fn sync_grants(&mut self) {
        let ids = self.grants.known_ids();
        let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        s.grants = ids.into_iter().map(|id| (id.clone(), (self.grants.permissions_of(&id), self.grants.origins_of(&id)))).collect();
    }

    // ---------------------------------------------------------------- install, remove, update

    /// Lines of the install dialog for a manifest.
    pub fn describe_install(manifest: &Manifest) -> Vec<PermissionLine> {
        describe_manifest(&manifest.raw)
    }

    /// Downloads an extension from the Web Store and installs it. Blocking: call from a worker thread. The user must have
    /// accepted `describe_install` lines before (the app shows the dialog with the manifest from the store page).
    pub fn install_from_store(&mut self, fetcher: &dyn Fetcher, id: &str) -> Result<Vec<Effect>, CrxError> {
        let installed = install_from_store(fetcher, id, &self.cfg.prodversion, self.registry.extensions_dir())?;
        Ok(self.register_installed(installed.id, installed.dir))
    }

    /// Installs a CRX file already in memory (backup restore, drag and drop).
    pub fn install_crx_bytes(&mut self, bytes: &[u8], expected_id: Option<&str>) -> Result<Vec<Effect>, CrxError> {
        let installed = install_crx(bytes, self.registry.extensions_dir(), expected_id)?;
        Ok(self.register_installed(installed.id, installed.dir))
    }

    fn register_installed(&mut self, id: String, dir: PathBuf) -> Vec<Effect> {
        let mut fx = Vec::new();
        let Ok(manifest) = crate::registry::read_manifest(&dir) else { return fx };
        let was = self.active.contains(&id);
        let previous = self.registry.get(&id).map(|i| i.version.clone());
        if was {
            self.deactivate(&id, &mut fx);
        }
        let item = Installed { id: id.clone(), version: manifest.version.clone(), name: manifest.name.clone(), dir, source: Source::Store, enabled: true, installed_at: 0, manifest };
        let now = self.now();
        self.registry.add(item, now);
        self.registry.flush();
        // An installed, imported or restored extension is never pinned by default.
        self.activate(&id, &mut fx);
        let details = match previous {
            Some(v) => json!({ "reason": "update", "previousVersion": v }),
            None => json!({ "reason": "install" }),
        };
        self.out.emit(&id, "runtime.onInstalled", vec![details], true, true);
        self.pump(&mut fx);
        fx
    }

    /// Checks the store for newer versions of the installed store extensions and installs them. Blocking.
    pub fn update_all(&mut self, fetcher: &dyn Fetcher) -> Vec<Result<Vec<Effect>, CrxError>> {
        let installed: Vec<(String, String)> = self.registry.list().iter().filter(|i| i.source == Source::Store).map(|i| (i.id.clone(), i.version.clone())).collect();
        let results = update_from_store(fetcher, &installed, &self.cfg.prodversion, self.registry.extensions_dir());
        results.into_iter().map(|r| r.map(|i| self.register_installed(i.id, i.dir))).collect()
    }

    /// Removes an extension (confirmed by the user): unload, forget its data, erase a store extension's folder.
    pub fn remove_extension(&mut self, id: &str) -> Vec<Effect> {
        let mut fx = Vec::new();
        if let Some(path) = self.unpacked.path_of(id).map(PathBuf::from) {
            return self.remove_unpacked(&path);
        }
        self.deactivate(id, &mut fx);
        if self.registry.remove(id).is_some() {
            self.forget_data(id);
            self.registry.flush();
            if self.pinned.iter().any(|p| p == id) {
                self.pinned = crate::registry::set_pinned(&self.pinned, id, false);
                fx.push(Effect::PinnedChanged(self.pinned.clone()));
            }
            fx.push(Effect::ExtensionsChanged);
        }
        self.pump(&mut fx);
        fx
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.registry.set_enabled(id, enabled) {
            if enabled {
                self.activate(id, &mut fx);
            } else {
                self.deactivate(id, &mut fx);
            }
            self.registry.flush();
        }
        self.pump(&mut fx);
        fx
    }

    // ---------------------------------------------------------------- unpacked

    /// "Load a folder...": the folder must contain `manifest.json`.
    pub fn add_unpacked(&mut self, path: &Path) -> Result<Vec<Effect>, String> {
        Unpacked::check_folder(path)?;
        let (list, loadable) = self.unpacked.add(&self.unpacked_paths.clone(), path);
        self.unpacked_paths = list.clone();
        let mut fx = vec![Effect::UnpackedPathsChanged(list)];
        match loadable {
            Ok(_) => fx.extend(self.load_unpacked(path, false)),
            Err(e) => {
                self.unpacked.load_failed(path, &e);
                fx.push(Effect::ExtensionsChanged);
                return Err(e);
            }
        }
        Ok(fx)
    }

    fn load_unpacked(&mut self, path: &Path, reloading: bool) -> Vec<Effect> {
        let mut fx = Vec::new();
        match self.unpacked.read(path) {
            Ok(l) => {
                let was = self.active.contains(&l.id);
                if was {
                    self.deactivate(&l.id, &mut fx);
                }
                let item = Installed { id: l.id.clone(), version: l.manifest.version.clone(), name: l.manifest.name.clone(), dir: path.to_path_buf(), source: Source::Unpacked, enabled: true, installed_at: 0, manifest: l.manifest };
                let now = self.now();
                self.registry.add(item, now);
                self.activate(&l.id, &mut fx);
                let name = self.registry.get(&l.id).map(|i| i.name.clone()).unwrap_or_default();
                self.unpacked.loaded(path, &l.id, &name, reloading, now as i64);
            }
            Err(e) => {
                self.unpacked.load_failed(path, &e);
                fx.push(Effect::Log(format!("[extensions] unpacked {}: {e}", path.display())));
                fx.push(Effect::ExtensionsChanged);
            }
        }
        self.pump(&mut fx);
        fx
    }

    /// The folder changed (watcher) or the user pressed Reload.
    pub fn reload_unpacked(&mut self, path: &Path) -> Vec<Effect> {
        if !self.unpacked_paths.iter().any(|p| Path::new(p) == path) {
            return vec![];
        }
        let before = self.unpacked.state_of(path);
        let _ = before;
        self.load_unpacked(path, true)
    }

    /// Removes the extension from Signet; its folder stays as is.
    pub fn remove_unpacked(&mut self, path: &Path) -> Vec<Effect> {
        let mut fx = Vec::new();
        let (list, id) = self.unpacked.remove(&self.unpacked_paths.clone(), path);
        self.unpacked_paths = list.clone();
        if let Some(id) = id {
            self.deactivate(&id, &mut fx);
            self.registry.remove(&id);
            self.forget_data(&id);
        }
        fx.push(Effect::UnpackedPathsChanged(list));
        fx.push(Effect::ExtensionsChanged);
        self.pump(&mut fx);
        fx
    }

    // ---------------------------------------------------------------- toolbar, Settings

    /// Settings list.
    pub fn infos(&mut self) -> Vec<ExtensionInfo> {
        let commands: HashMap<String, Vec<Value>> = self.commands.list_all().into_iter().map(|(id, l)| (id, l.iter().map(|c| c.to_json()).collect())).collect();
        let failed = self.unpacked.failures(&self.unpacked_paths);
        let unpacked = &self.unpacked;
        self.registry.infos(&commands, &|id| unpacked.path_of(id).map(|p| unpacked.state_of(p)), failed)
    }

    pub fn pinned(&self) -> &[String] {
        &self.pinned
    }

    pub fn set_pinned(&mut self, id: &str, on: bool) -> Vec<Effect> {
        self.pinned = crate::registry::set_pinned(&self.pinned, id, on);
        vec![Effect::PinnedChanged(self.pinned.clone())]
    }

    pub fn reorder(&mut self, id: &str, before: Option<&str>) -> Vec<Effect> {
        self.order = crate::registry::move_in_order(&self.order, id, before);
        vec![Effect::OrderChanged(self.order.clone())]
    }

    /// Extensions with a toolbar button, in toolbar order.
    pub fn toolbar(&self) -> Vec<String> {
        let has = |id: &str| self.registry.get(id).is_some_and(|i| i.enabled && i.manifest.has_action());
        crate::registry::toolbar_ids(&self.pinned, &self.order, &has)
    }

    /// Extensions that have a button at all (the Extensions menu lists them).
    pub fn with_button(&self) -> Vec<String> {
        self.registry.enabled().filter(|i| i.manifest.has_action()).map(|i| i.id.clone()).collect()
    }

    /// Popup of an extension's button for a tab: the URL to open in the popup view, or None (then `action.onClicked`).
    pub fn popup_url(&self, id: &str, tab: Option<i64>) -> Option<String> {
        let item = self.registry.get(id)?;
        if !self.actions.is_enabled(id, tab) {
            return None;
        }
        let popup = self.action_model.get(id, tab, "popup").and_then(|p| p.as_str().map(String::from)).or_else(|| item.manifest.popup().map(String::from)).filter(|p| !p.is_empty())?;
        Some(format!("chrome-extension://{id}/{}", popup.trim_start_matches('/')))
    }

    /// The button was clicked and has no popup: raise `action.onClicked` with the tab.
    pub fn action_clicked(&mut self, id: &str, tab: Option<Value>) -> Vec<Effect> {
        let mut fx = Vec::new();
        let args = tab.map(|t| vec![t]).unwrap_or_default();
        for name in ["action.onClicked", "browserAction.onClicked"] {
            self.out.emit(id, name, args.clone(), true, true);
        }
        self.pump(&mut fx);
        fx
    }

    /// Badge text, colour, title and icon state for the toolbar.
    pub fn button_state(&self, id: &str, tab: Option<i64>) -> Value {
        let g = |k: &str| self.action_model.get(id, tab, k);
        json!({
            "enabled": self.actions.is_enabled(id, tab),
            "badgeText": g("text").unwrap_or(json!("")),
            "badgeColor": g("color").unwrap_or(json!([217, 48, 37, 255])),
            "badgeTextColor": self.actions.text_color(id, tab),
            "title": g("title").or_else(|| self.registry.get(id).and_then(|i| i.manifest.action.as_ref()?.default_title.clone()).map(Value::String)).unwrap_or_else(|| json!(self.registry.get(id).map(|i| i.name.clone()).unwrap_or_default())),
        })
    }

    pub fn action_counts(&self) -> ActionCounts {
        self.dnr.action_counts()
    }

    // ---------------------------------------------------------------- context menus, commands

    /// Entries added by extensions for a page right-click: `(extension id, extension name, nodes)`.
    pub fn menu_items(&self, target: &MenuTarget) -> Vec<(String, String, Vec<MenuNode>)> {
        let contexts = contexts_of(target);
        self.registry
            .enabled()
            .filter(|i| self.active.contains(&i.id))
            .map(|i| (i.id.clone(), i.name.clone(), self.menus.tree(&i.id, &contexts, Some(target))))
            .filter(|(_, _, n)| !n.is_empty())
            .collect()
    }

    /// Entries an extension added to its own button's menu.
    pub fn button_menu_items(&self, id: &str) -> Vec<MenuNode> {
        let mut contexts: HashSet<String> = HashSet::from(["all".to_string()]);
        contexts.extend(ACTION_CONTEXTS.iter().map(|c| c.to_string()));
        self.menus.tree(id, &contexts, None)
    }

    /// A menu entry was chosen: flips checkboxes and raises `contextMenus.onClicked`.
    pub fn menu_clicked(&mut self, id: &str, item_id: &Value, target: &MenuTarget, tab: Option<Value>) -> Vec<Effect> {
        let info = self.menus.click(id, item_id);
        let mut j = info.to_json();
        if let Some(u) = target.page_url.as_ref() {
            j["pageUrl"] = json!(u);
        }
        if let Some(u) = target.link_url.as_ref() {
            j["linkUrl"] = json!(u);
        }
        if let Some(u) = target.src_url.as_ref() {
            j["srcUrl"] = json!(u);
        }
        if let Some(u) = target.selection_text.as_ref() {
            j["selectionText"] = json!(u);
        }
        if let Some(u) = target.frame_url.as_ref() {
            j["frameUrl"] = json!(u);
        }
        j["editable"] = json!(target.is_editable);
        let mut args = vec![j];
        args.extend(tab);
        self.out.emit(id, crate::contextmenus::MENU_CLICK_EVENT, args, true, true);
        let mut fx = Vec::new();
        self.pump(&mut fx);
        fx
    }

    /// Shortcuts of extensions to register in the application menu.
    pub fn command_bindings(&mut self) -> Vec<CommandBinding> {
        self.commands.bindings()
    }

    /// A shortcut of an extension was pressed on `tab`.
    pub fn run_command(&mut self, extension_id: &str, name: &str, tab: Option<&str>, tab_json: Option<Value>) -> Vec<Effect> {
        let mut fx = Vec::new();
        match self.commands.run(extension_id, name, tab) {
            CommandRun::RunAction { extension_id, .. } => {
                // A disabled button (action.disable) does nothing, shortcuts included.
                if self.actions.is_enabled(&extension_id, None) {
                    match self.popup_url(&extension_id, None) {
                        Some(url) => fx.push(Effect::RunAction { extension_id, popup_url: Some(url) }),
                        None => fx.extend(self.action_clicked(&extension_id, tab_json)),
                    }
                }
            }
            CommandRun::Emitted => {
                // `active_tab` has no tab object here: the event carries it when the app knows it.
                if let Some(t) = tab_json {
                    let mut evs = self.out.drain();
                    for e in evs.iter_mut() {
                        if e.name == crate::commands::COMMAND_EVENT && e.extension_id == extension_id {
                            e.args.push(t.clone());
                        }
                    }
                    for e in evs {
                        self.out.emit(&e.extension_id, &e.name, e.args, e.wake, e.queue);
                    }
                }
            }
        }
        self.pump(&mut fx);
        fx
    }

    // ---------------------------------------------------------------- contexts and workers

    /// A context (worker, extension page, content script) came up. The web layer calls this when its bootstrap script
    /// reports `runtime.__openContext`, or itself when it knows first.
    pub fn context_opened(&mut self, c: MsgContext) {
        self.messaging.open_context(c);
    }

    pub fn context_closed(&mut self, context_id: &str) -> Vec<Effect> {
        let d = self.messaging.close_context(context_id);
        let mut fx = Vec::new();
        let ext = context_id.split(':').next().unwrap_or("").to_string();
        for (e, outcome) in self.auth.context_gone(&ext, context_id) {
            fx.push(Effect::ProxyAuth { key: e, outcome });
        }
        self.deliveries(d, &mut fx);
        fx
    }

    /// The engine started the extension's service worker (or background page).
    pub fn worker_running(&mut self, extension_id: &str, worker_id: &str) -> Vec<Effect> {
        let now = self.now();
        self.events.worker_running(extension_id, worker_id, now);
        vec![]
    }

    pub fn worker_stopped(&mut self, extension_id: &str, worker_id: &str) -> Vec<Effect> {
        self.events.worker_stopped(extension_id, worker_id);
        vec![]
    }

    /// The engine's answer to a `RouterAction::StartWorker`.
    pub fn wake_result(&mut self, ticket: u64, ok: bool) -> Vec<Effect> {
        let now = self.now();
        let mut fx: Vec<Effect> = self.events.wake_result(ticket, ok, now).into_iter().map(Effect::Router).collect();
        self.events.retry_result(ticket, ok, now);
        self.pump(&mut fx);
        fx
    }

    // ---------------------------------------------------------------- tabs and navigation events

    /// A tab was destroyed: forget what extensions set for it.
    pub fn tab_gone(&mut self, tab_id: i64) {
        self.tabs_extras.forget_tab(tab_id);
        self.actions.forget_tab(tab_id);
        self.action_model.forget_tab(tab_id);
        self.dnr.tab_gone(tab_id);
        self.navigation.forget(tab_id);
    }

    /// A tab is tracked for `webNavigation`.
    pub fn tab_created(&mut self, tab_id: i64, url: &str) {
        self.navigation.watch(tab_id, url);
    }

    pub fn frame_navigated(&mut self, tab: i64, frame_key: &str, main: bool, url: &str) {
        self.navigation.frame_navigated(tab, frame_key, main, url);
    }

    pub fn navigated_in_page(&mut self, tab: i64, frame_key: &str, main: bool, url: &str, frame: Option<&FrameInfo>) -> Vec<Effect> {
        let now = self.now();
        self.navigation.navigated_in_page(tab, frame_key, main, url, frame, now);
        let mut fx = Vec::new();
        self.pump(&mut fx);
        fx
    }

    pub fn load_failed(&mut self, tab: i64, code: i64, description: &str, url: &str, frame: Option<&FrameInfo>) -> Vec<Effect> {
        let now = self.now();
        self.navigation.load_failed(tab, code, description, url, frame, now);
        let mut fx = Vec::new();
        self.pump(&mut fx);
        fx
    }

    /// Content scripts for a frame at one phase: the CSS and JS to inject, in manifest order.
    pub fn injections(&self, frame: &Frame, phase: Option<RunAt>) -> Vec<Injection> {
        let ctx = FrameContext { url: frame.url, is_main_frame: frame.is_main_frame, fallback_url: frame.fallback_url };
        let mut out = Vec::new();
        for item in self.registry.enabled().filter(|i| self.active.contains(&i.id)) {
            for plan in scripts_for(&item.id, &item.manifest, &ctx, phase) {
                let (js, css) = plan.load(&item.dir);
                out.push(Injection { extension_id: item.id.clone(), run_at: plan.run_at, css: plan.css_source(&css), js: plan.js_source(&js) });
            }
        }
        out
    }

    /// Network hooks (declarativeNetRequest).
    pub fn before_request(&mut self, d: &RequestDetails) -> RequestOutcome {
        self.dnr.before_request(d)
    }

    // ---------------------------------------------------------------- dialogs and async answers

    /// The user's answer to a `PermissionPrompt`.
    pub fn permission_answer(&mut self, ticket: u64, allowed: bool) -> Vec<Effect> {
        let events = self.grants.answer(ticket, allowed);
        let mut fx = Vec::new();
        self.perm_events(events, &mut fx);
        self.pump(&mut fx);
        fx
    }

    fn perm_events(&mut self, events: Vec<PermEvent>, fx: &mut Vec<Effect>) {
        for e in events {
            match e {
                PermEvent::Prompt { ticket, extension_id, need } => {
                    let name = self.registry.get(&extension_id).map(|i| i.name.clone()).unwrap_or_else(|| extension_id.clone());
                    let lines = crate::permissions::describe_access(&need.permissions, &need.origins);
                    if lines.is_empty() {
                        // Nothing worth warning about (quiet permissions only): granted without a dialog.
                        let more = self.grants.answer(ticket, true);
                        self.perm_events(more, fx);
                    } else {
                        fx.push(Effect::PermissionPrompt { ticket, extension_id, name, lines, need });
                    }
                }
                PermEvent::Resolved { ticket, granted } => {
                    if let Some(call_id) = self.perm_calls.remove(&ticket) {
                        fx.push(reply(call_id, CrxResult::ok(json!(granted))));
                    }
                }
            }
        }
        for id in self.grants.take_changed() {
            self.sync_grants();
            self.dnr.refresh_access(&id);
        }
        self.sync_grants();
        self.refresh_holders();
    }

    pub fn offscreen_loaded(&mut self, extension_id: &str, page_id: i64) -> Vec<Effect> {
        self.offscreen.loaded(extension_id, page_id);
        self.offscreen_calls.remove(extension_id).map(|c| vec![reply(c, CrxResult::Ok(None))]).unwrap_or_default()
    }

    pub fn offscreen_failed(&mut self, extension_id: &str, reason: &str) -> Vec<Effect> {
        let e = self.offscreen.load_failed(extension_id, reason);
        self.offscreen_calls.remove(extension_id).map(|c| vec![reply(c, CrxResult::err(e.0))]).unwrap_or_default()
    }

    pub fn offscreen_page_closed(&mut self, page_id: i64) {
        self.offscreen.page_closed(page_id);
    }

    pub fn is_offscreen_page(&self, page_id: i64) -> bool {
        self.offscreen.owns(page_id)
    }

    /// The answer to an `Effect::TabAction`.
    pub fn tab_action_done(&mut self, call_id: u64, result: Result<Value, String>) -> Vec<Effect> {
        if !self.tab_calls.remove(&call_id) {
            return vec![];
        }
        vec![reply(call_id, match result {
            Ok(Value::Null) => CrxResult::Ok(None),
            Ok(v) => CrxResult::ok(v),
            Err(e) => CrxResult::err(e),
        })]
    }

    pub fn notification_clicked(&mut self, handle: u64) -> Vec<Effect> {
        self.notifications.clicked(handle);
        self.pumped()
    }
    pub fn notification_action(&mut self, handle: u64, index: i64) -> Vec<Effect> {
        self.notifications.action(handle, index);
        self.pumped()
    }
    pub fn notification_closed(&mut self, handle: u64) -> Vec<Effect> {
        self.notifications.closed(handle);
        self.pumped()
    }

    fn pumped(&mut self) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.pump(&mut fx);
        fx
    }

    /// A proxy 407 challenge: `Pending(key)` means an extension was asked; the outcome comes later as `Effect::ProxyAuth`.
    pub fn proxy_auth_challenge(&mut self, c: &ProxyChallenge) -> (Handled, Vec<Effect>) {
        let now = self.now();
        let h = self.auth.handle(c, now);
        (h, self.pumped())
    }

    // ---------------------------------------------------------------- time

    /// Call about every 250 ms (and at `next_wakeup`): alarms, retries, rebuilds, file watchers, persistence.
    pub fn tick(&mut self) -> Vec<Effect> {
        let now = self.now();
        let mut fx = Vec::new();
        self.alarms.tick(now as f64);
        let loaded = |id: &str| self.active.contains(id);
        let actions = self.events.tick(now, &loaded);
        fx.extend(actions.into_iter().map(Effect::Router));
        if let Some(counts) = self.dnr.tick(now) {
            fx.push(Effect::ActionCounts(counts));
        }
        for (key, outcome) in self.auth.tick(now) {
            fx.push(Effect::ProxyAuth { key, outcome });
        }
        for path in self.unpacked.poll(now) {
            if let Some(id) = self.unpacked_id(&path) {
                fx.push(Effect::ReloadExtension { id, dir: path.clone() });
            }
            fx.extend(self.reload_unpacked(&path));
        }
        self.flush_due();
        self.pump(&mut fx);
        fx
    }

    fn unpacked_id(&self, path: &Path) -> Option<String> {
        self.registry.list().iter().find(|i| i.dir == path).map(|i| i.id.clone())
    }

    /// Earliest time (ms since the epoch) something is due; sleep until then.
    pub fn next_wakeup(&self) -> Option<u64> {
        [self.alarms.scheduler.next_due().map(|t| t as u64)].into_iter().flatten().min()
    }

    fn flush_due(&mut self) {
        if self.storage.has_unsaved() {
            self.storage.flush();
        }
        self.alarms.flush();
        self.grants.flush();
        self.settings.flush();
        self.dnr.store.flush();
        self.menus.flush();
        self.registry.flush();
    }

    /// Writes everything now (app quit).
    pub fn flush(&mut self) {
        self.flush_due();
        self.storage.flush();
    }

    // ---------------------------------------------------------------- pumping

    /// Moves what the models produced into effects: events, settings to apply, notifications, button state, hooks.
    fn pump(&mut self, fx: &mut Vec<Effect>) {
        let mut guard = 0;
        loop {
            guard += 1;
            let events = self.out.drain();
            let had = !events.is_empty();
            for e in events {
                fx.extend(self.events.emit(e).into_iter().map(Effect::Router));
            }
            for a in self.settings.take_applies() {
                fx.push(Effect::ApplySetting(a));
            }
            for n in self.notifications.take_effects() {
                fx.push(Effect::Notification(n));
            }
            if self.actions.take_changed() {
                fx.push(Effect::ActionStates(self.actions.snapshot()));
            }
            for l in self.dnr.take_log().into_iter().chain(self.events.take_log()).chain(self.auth.take_log()).chain(self.commands.take_log()) {
                fx.push(Effect::Log(l));
            }
            if !had || guard > 4 {
                break;
            }
        }
        if self.dnr.store.take_changed() || self.dnr.wanted_events() != self.last_hooks {
            self.dnr.rebuild();
        }
        let wanted = self.dnr.wanted_events();
        if wanted != self.last_hooks {
            self.last_hooks = wanted.clone();
            fx.push(Effect::NetworkHooks(wanted));
        }
        let key = self.commands.key();
        if key != self.last_commands_key {
            self.last_commands_key = key;
            fx.push(Effect::CommandsChanged);
        }
        self.refresh_holders();
    }

    fn deliveries(&mut self, ds: Vec<Delivery>, fx: &mut Vec<Effect>) {
        for d in ds {
            match d {
                Delivery::ToContext { context_id, name, args } => fx.push(Effect::ToContext { context_id, name, args }),
                Delivery::Reply { call_id, result } => fx.push(reply(call_id, match result {
                    Ok(Value::Null) => CrxResult::Ok(None),
                    Ok(v) => CrxResult::ok(v),
                    Err(e) => CrxResult::err(e),
                })),
            }
        }
    }

    // ---------------------------------------------------------------- page scripts

    /// Methods an extension context can call (the page-side installer turns each one into a function).
    pub fn implemented_methods(&self) -> Vec<String> {
        self.table.methods().into_iter().filter(|m| !m.contains("__")).collect()
    }

    /// The script to run first in an extension context (`chrome-extension://` page or background page) or in a content
    /// script scope. `kind` is `worker` (the hidden background), `page` or `content`; `context_id` is the id the app gave this
    /// context (it comes back as `Call::context.id`); `tab_id` / `frame_id` describe a content script's frame.
    pub fn page_script(&self, extension_id: &str, kind: &str, context_id: &str, tab_id: Option<i64>, frame_id: i64) -> Option<String> {
        let item = self.registry.get(extension_id).filter(|i| i.enabled)?;
        let mut messages = self.all_messages(extension_id, &item.manifest.raw);
        if messages.is_null() {
            messages = json!({});
        }
        let config = json!({
            "id": extension_id,
            "manifest": item.manifest.raw,
            "messages": messages,
            "implemented": self.implemented_methods(),
            "catalog": crate::catalog::catalog().iter().map(|(k, v)| (k.clone(), catalog_json(v))).collect::<serde_json::Map<_, _>>(),
            "plan": crate::shared::install_plan(),
            "context": { "kind": kind, "id": context_id, "tabId": tab_id, "frameId": frame_id },
        });
        Some(crate::assets::context_script(&config))
    }

    // ---------------------------------------------------------------- Web Store pages

    /// A call from a Web Store page (`chrome.webstorePrivate`). `page_url` is the URL of the calling frame: anything that is
    /// not a store page is refused.
    pub fn webstore_call(&mut self, call_id: u64, page_url: &str, method: &str, args: &[Value]) -> Vec<Effect> {
        if !is_store_url(page_url) {
            return vec![reply(call_id, CrxResult::err("Access to extension API denied."))];
        }
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        match method {
            "webstorePrivate.beginInstallWithManifest3" => {
                let d = a(0);
                let id = d.get("id").and_then(|i| i.as_str()).unwrap_or("");
                let manifest = d.get("manifest").and_then(|m| m.as_str()).map(Manifest::parse);
                match (crate::id::is_valid_id(id), manifest) {
                    (true, Some(Ok(m))) => {
                        self.install_ticket += 1;
                        let ticket = self.install_ticket;
                        self.install_prompts.insert(ticket, (call_id, id.to_string()));
                        let name = d.get("localizedName").and_then(|n| n.as_str()).map(String::from).unwrap_or_else(|| m.name.clone());
                        vec![Effect::InstallPrompt { ticket, extension_id: id.to_string(), name, lines: Self::describe_install(&m) }]
                    }
                    _ => vec![reply(call_id, CrxResult::ok(json!("manifest_error")))],
                }
            }
            "webstorePrivate.completeInstall" | "webstorePrivate.install" => {
                let id = a(0).as_str().unwrap_or("").to_string();
                if !crate::id::is_valid_id(&id) {
                    return vec![reply(call_id, CrxResult::err("Invalid extension id."))];
                }
                if !self.approved_installs.contains(&id) && method == "webstorePrivate.completeInstall" {
                    return vec![reply(call_id, CrxResult::err("The installation was not approved."))];
                }
                self.approved_installs.remove(&id);
                vec![Effect::InstallFromStore { call_id, extension_id: id }]
            }
            "webstorePrivate.getExtensionStatus" => {
                let status = match self.registry.get(a(0).as_str().unwrap_or("")) {
                    Some(i) if i.enabled => "enabled",
                    Some(_) => "disabled",
                    None => "installable",
                };
                vec![reply(call_id, CrxResult::ok(json!(status)))]
            }
            "webstorePrivate.getFullChromeVersion" => vec![reply(call_id, CrxResult::ok(json!({ "version_number": self.cfg.prodversion, "app_name": self.cfg.app_name })))],
            _ => vec![reply(call_id, CrxResult::Ok(None))],
        }
    }

    /// The user's decision on an `InstallPrompt`: answers the store page's `beginInstallWithManifest3`.
    pub fn install_decision(&mut self, ticket: u64, allow: bool) -> Vec<Effect> {
        let Some((call_id, id)) = self.install_prompts.remove(&ticket) else { return vec![] };
        if allow {
            self.approved_installs.insert(id);
        }
        vec![reply(call_id, CrxResult::ok(json!(if allow { "" } else { "user_cancelled" })))]
    }

    /// The result of an `Effect::InstallFromStore` (the app ran `install_from_store` / `install_crx_bytes`).
    pub fn store_install_done(&mut self, call_id: u64, result: Result<Vec<Effect>, String>) -> Vec<Effect> {
        match result {
            Ok(mut fx) => {
                fx.push(reply(call_id, CrxResult::Ok(None)));
                fx
            }
            Err(e) => vec![reply(call_id, CrxResult::err(e))],
        }
    }

    // ---------------------------------------------------------------- calls

    /// Runs a call from an extension context. `tabs` answers tab queries; it may block.
    pub fn call(&mut self, call: Call, tabs: &dyn TabHost) -> Vec<Effect> {
        let now = self.now();
        self.dnr.set_now(now);
        let granted = self.grants.permissions_of(&call.extension_id);
        let item = self.registry.get(&call.extension_id).filter(|i| i.enabled && self.active.contains(&i.id));
        let manifest = item.map(|i| i.manifest.raw.clone());
        let mut fx = Vec::new();
        if let Err(e) = check_call(&self.table, &call.method, &call.args, manifest.as_ref(), &granted) {
            fx.push(reply(call.id, CrxResult::err(e.0)));
            return fx;
        }
        let method = call.method.as_str().unwrap_or("").to_string();
        let args: Vec<Value> = call.args.as_array().cloned().unwrap_or_default();
        let r = self.dispatch(&call, &method, &args, manifest.as_ref().unwrap_or(&Value::Null), tabs, &mut fx);
        if let Some(r) = r {
            fx.push(reply(call.id, r));
        }
        self.pump(&mut fx);
        fx
    }

    fn access_of(&self, ext: &str, manifest: &Value) -> Access {
        let s = self.grants.effective(manifest, ext);
        Access { permissions: s.permissions, origins: s.origins }
    }

    /// Returns the immediate answer, or None when the answer comes later (a reply effect is produced then).
    fn dispatch(&mut self, call: &Call, method: &str, args: &[Value], manifest: &Value, tabs: &dyn TabHost, fx: &mut Vec<Effect>) -> Option<CrxResult> {
        let ext = call.extension_id.as_str();
        let now = self.now();
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        let (ns, _) = method.split_once('.').unwrap_or((method, ""));
        Some(match ns {
            "alarms" => finish(self.alarms.call(ext, method, args, now as f64)),
            "permissions" => match method {
                "permissions.request" => match self.grants.request(ext, manifest, &a(0)) {
                    Ok((ticket, events)) => {
                        self.perm_calls.insert(ticket, call.id);
                        self.perm_events(events, fx);
                        return None;
                    }
                    Err(e) => CrxResult::err(e.0),
                },
                "permissions.contains" => finish(self.grants.contains(manifest, ext, &a(0)).map(|b| json!(b))),
                "permissions.getAll" => ok_val(self.grants.get_all(manifest, ext).to_json()),
                _ => {
                    let r = self.grants.remove(manifest, ext, &a(0)).map(|b| json!(b));
                    self.perm_events(vec![], fx);
                    finish(r)
                }
            },
            "declarativeNetRequest" => finish(self.dnr.call(ext, method, args)),
            "commands" => ok_val(self.commands.get_all(ext)),
            "contextMenus" => finish(self.menus.call(ext, method, args)),
            "notifications" => finish(self.notifications.call(ext, method, args)),
            "action" | "browserAction" => self.action_call(ext, method, args, fx),
            "storage" => finish(self.storage.call(ext, method, args)),
            "offscreen" => match method {
                "offscreen.createDocument" => match self.offscreen.create(ext, &a(0)) {
                    Ok(url) => {
                        self.offscreen_calls.insert(ext.to_string(), call.id);
                        fx.push(Effect::OffscreenOpen { extension_id: ext.to_string(), url });
                        return None;
                    }
                    Err(e) => CrxResult::err(e.0),
                },
                "offscreen.closeDocument" => match self.offscreen.close(ext) {
                    Ok(page) => {
                        if let Some(p) = page {
                            fx.push(Effect::OffscreenClose { page_id: p });
                        }
                        CrxResult::Ok(None)
                    }
                    Err(e) => CrxResult::err(e.0),
                },
                _ => ok_val(json!(self.offscreen.has(ext))),
            },
            "tabs" => return self.tabs_call(call, method, args, manifest, tabs, fx),
            "windows" => finish(self.windows_call(method, args, tabs)),
            "runtime" => return self.runtime_call(call, method, args, manifest, fx),
            "i18n" => ok_val(self.all_messages(ext, manifest)),
            "management" => ok_val(self.registry.get(ext).map(|i| json!({ "id": i.id, "name": i.name, "version": i.version, "enabled": i.enabled, "type": "extension", "installType": if i.source == Source::Unpacked { "development" } else { "normal" }, "description": i.manifest.description, "mayDisable": true })).unwrap_or(Value::Null)),
            _ if method == INSTALLED_METHOD => {
                self.log_installed(ext, &a(0), fx);
                CrxResult::Ok(None)
            }
            _ if method == AUTH_RESPONSE_METHOD => {
                for (key, outcome) in self.auth.respond(ext, &call.context, &a(0), &a(1), now) {
                    fx.push(Effect::ProxyAuth { key, outcome });
                }
                CrxResult::Ok(None)
            }
            _ if method == AUTH_LISTENERS_METHOD => {
                let n = a(0).as_u64().unwrap_or(0) as usize;
                for (key, outcome) in self.auth.set_listeners(ext, &call.context, n) {
                    fx.push(Effect::ProxyAuth { key, outcome });
                }
                CrxResult::Ok(None)
            }
            _ if self.settings.has(method.rsplit_once('.').map(|(k, _)| k).unwrap_or("")) => {
                let r = finish(self.settings.call(ext, method, args));
                self.refresh_holders();
                r
            }
            _ => CrxResult::err(format!("Unknown Extension API - {method}")),
        })
    }

    fn log_installed(&mut self, ext: &str, report: &Value, fx: &mut Vec<Effect>) {
        let version = self.registry.get(ext).map(|i| i.version.clone()).unwrap_or_default();
        let names: Vec<String> = report.get("stubbed").and_then(|s| s.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
        if self.stubs.record(ext, &version, &names) {
            fx.push(Effect::Log(format!("[extension-apis] {ext} stubbed: {}", if names.is_empty() { "none".into() } else { names.join(", ") })));
        }
        let ns = report.get("namespaces").and_then(|n| n.as_array()).map(|a| a.iter().filter_map(|s| s.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        fx.push(Effect::Log(format!("[extension-apis] {ext} {}: {} ({})", report.get("context").and_then(|c| c.as_str()).unwrap_or("worker"), if ns.is_empty() { "none" } else { &ns }, report.get("mode").and_then(|c| c.as_str()).unwrap_or("?"))));
    }

    /// What the placeholders replaced, per extension (a later Settings line).
    pub fn stubbed_of(&self, extension_id: &str) -> Vec<String> {
        self.stubs.of(extension_id, None)
    }

    fn action_call(&mut self, ext: &str, method: &str, args: &[Value], fx: &mut Vec<Effect>) -> CrxResult {
        let name = method.split_once('.').map(|(_, n)| n).unwrap_or("");
        let alias = format!("action.{name}");
        match name {
            "setIcon" => match set_icon(ext, args.first().unwrap_or(&Value::Null)) {
                Ok(u) => {
                    fx.push(Effect::SetActionIcon(u));
                    CrxResult::Ok(None)
                }
                // Icons given by path are drawn by the toolbar from the manifest.
                Err(_) => CrxResult::Ok(None),
            },
            "enable" | "disable" | "isEnabled" | "setBadgeTextColor" | "getBadgeTextColor" => finish(self.actions.call(ext, &alias, args)),
            _ => match self.action_model.call(ext, &alias, args) {
                Ok(Some(v)) => CrxResult::ok(v),
                Ok(None) => CrxResult::Ok(None),
                Err(e) => CrxResult::err(e.0),
            },
        }
    }

    fn windows_call(&mut self, method: &str, args: &[Value], tabs: &dyn TabHost) -> ApiResult<Value> {
        let active = tabs.active_tab(None);
        let window = |id: i64, focused: bool| json!({ "id": id, "focused": focused, "incognito": false, "type": "normal", "state": "normal", "alwaysOnTop": false });
        let current = active.as_ref().map(|t| t.window_id).unwrap_or(1);
        match method {
            "windows.getAll" => Ok(json!([window(current, true)])),
            "windows.getCurrent" | "windows.getLastFocused" => Ok(window(current, true)),
            _ => {
                let id = args.first().and_then(|a| a.as_i64()).unwrap_or(current);
                if id == current { Ok(window(id, true)) } else { Err(ApiError::new(format!("No window with id: {id}."))) }
            }
        }
    }

    fn tabs_call(&mut self, call: &Call, method: &str, args: &[Value], manifest: &Value, tabs: &dyn TabHost, fx: &mut Vec<Effect>) -> Option<CrxResult> {
        let ext = call.extension_id.as_str();
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        let access = self.access_of(ext, manifest);
        // `url`, `title` and `favIconUrl` need the `tabs` permission or host access for the tab.
        let has_tabs = access.permissions.iter().any(|p| p == "tabs");
        let visible = |t: &crate::tabs::TabInfo| has_tabs || crate::urlmatch::can_access_url(&Access { permissions: vec![], origins: access.origins.clone() }, ext, &t.url);
        Some(match method {
            "tabs.captureVisibleTab" | "tabs.highlight" | "tabs.insertCSS" | "tabs.removeCSS" | "tabs.detectLanguage" => finish(self.tabs_extras.call(tabs, &access, ext, method, args)),
            "tabs.get" => match a(0).as_i64().and_then(|id| tabs.tab(id)) {
                Some(t) => CrxResult::ok(t.to_chrome_tab(visible(&t))),
                None => CrxResult::err(format!("No tab with id: {}.", a(0))),
            },
            "tabs.getCurrent" => {
                // Only a context that belongs to a tab has a "current" tab; extension pages in tabs report theirs.
                match call.context.kind {
                    ContextKind::Worker => CrxResult::Ok(None),
                    ContextKind::Page => CrxResult::Ok(None),
                }
            }
            "tabs.query" => {
                let q = a(0);
                let active_tab = tabs.active_tab(None);
                let mut candidates = Vec::new();
                if let Some(t) = &active_tab {
                    candidates.push(t.clone());
                }
                for t in tabs_all(tabs, &active_tab) {
                    if !candidates.iter().any(|c| c.id == t.id) {
                        candidates.push(t);
                    }
                }
                let out: Vec<Value> = candidates.iter().filter(|t| query_matches(&q, t, &active_tab)).map(|t| t.to_chrome_tab(visible(t))).collect();
                CrxResult::ok(Value::Array(out))
            }
            "tabs.sendMessage" => {
                let tab_id = a(0).as_i64().unwrap_or(-1);
                let frame = a(2).get("frameId").and_then(|f| f.as_i64());
                let d = self.messaging.send(&call.context.id, call.id, a(1), Target::Tab { tab_id, frame_id: frame });
                let mut local = Vec::new();
                self.deliveries(d.clone(), &mut local);
                let replied = local.iter().any(|e| matches!(e, Effect::Reply { call_id, .. } if *call_id == call.id));
                fx.extend(local);
                let _ = replied;
                return None;
            }
            // Everything else needs the browser: the app does it and answers with `tab_action_done`.
            _ => {
                self.tab_calls.insert(call.id);
                fx.push(Effect::TabAction { call_id: call.id, extension_id: ext.to_string(), method: method.to_string(), args: args.to_vec() });
                return None;
            }
        })
    }

    fn runtime_call(&mut self, call: &Call, method: &str, args: &[Value], manifest: &Value, fx: &mut Vec<Effect>) -> Option<CrxResult> {
        let ext = call.extension_id.as_str();
        let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        match method {
            "runtime.getContexts" => {
                let mut contexts: Vec<ExtensionContext> = Vec::new();
                if let Some(BackgroundEntry::ServiceWorker { path, .. }) = self.registry.get(ext).map(|i| i.manifest.background_entry()) {
                    if self.events.running_workers(ext) > 0 {
                        contexts.push(background_context(ext, &format!("chrome-extension://{ext}/{path}")));
                    }
                }
                contexts.extend(self.offscreen.contexts(ext));
                for c in self.messaging.contexts_of(ext) {
                    if c.kind == Kind::Page && !self.offscreen.owns(c.id.rsplit('-').next().and_then(|n| n.parse().ok()).unwrap_or(-1)) {
                        contexts.push(page_context(ext, c.id.rsplit('-').next().and_then(|n| n.parse().ok()).unwrap_or(0), &c.url, "TAB"));
                    }
                }
                Some(CrxResult::ok(Value::Array(filter_contexts(contexts, &a(0)))))
            }
            "runtime.__openContext" => {
                let kind = match call.context.kind {
                    ContextKind::Worker => Kind::Worker,
                    ContextKind::Page => Kind::Page,
                };
                let info = a(0);
                let kind = if info.get("contentScript").and_then(|b| b.as_bool()) == Some(true) { Kind::ContentScript } else { kind };
                self.messaging.open_context(MsgContext {
                    id: call.context.id.clone(),
                    kind,
                    extension_id: ext.to_string(),
                    tab_id: info.get("tabId").and_then(|t| t.as_i64()),
                    frame_id: info.get("frameId").and_then(|t| t.as_i64()).unwrap_or(0),
                    url: call.context.url.clone(),
                });
                Some(CrxResult::Ok(None))
            }
            "runtime.__closeContext" => {
                let d = self.messaging.close_context(&call.context.id);
                self.deliveries(d, fx);
                Some(CrxResult::Ok(None))
            }
            "runtime.sendMessage" => {
                // `sendMessage([extensionId], message, [options])`: only messages to the own extension are routed.
                let (message, _) = if a(0).is_string() && args.len() > 1 { (a(1), a(2)) } else { (a(0), a(1)) };
                let d = self.messaging.send(&call.context.id, call.id, message, Target::Extension);
                self.deliveries(d, fx);
                None
            }
            "runtime.__reply" => {
                let token = a(0).as_u64().unwrap_or(0);
                let r = a(1);
                let handled = r.get("handled").and_then(|b| b.as_bool()) == Some(true);
                let listener = r.get("hasListener").and_then(|b| b.as_bool()) == Some(true);
                let d = self.messaging.reply(&call.context.id, token, handled, listener, r.get("response").cloned().unwrap_or(Value::Null));
                self.deliveries(d, fx);
                Some(CrxResult::Ok(None))
            }
            "runtime.connect" => {
                let name = a(1).get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                match self.messaging.connect(&call.context.id, &name, Target::Extension) {
                    Ok((id, d)) => {
                        self.deliveries(d, fx);
                        Some(CrxResult::ok(json!(id)))
                    }
                    Err(e) => Some(CrxResult::err(e)),
                }
            }
            "runtime.__accept" => {
                let d = self.messaging.accept_port(&call.context.id, a(0).as_u64().unwrap_or(0));
                self.deliveries(d, fx);
                Some(CrxResult::Ok(None))
            }
            "runtime.__decline" => {
                let d = self.messaging.decline_port(&call.context.id, a(0).as_u64().unwrap_or(0));
                self.deliveries(d, fx);
                Some(CrxResult::Ok(None))
            }
            "runtime.__portPost" => {
                let d = self.messaging.post(&call.context.id, a(0).as_u64().unwrap_or(0), a(1));
                self.deliveries(d, fx);
                Some(CrxResult::Ok(None))
            }
            "runtime.__portClose" => {
                let d = self.messaging.close_port(a(0).as_u64().unwrap_or(0), &call.context.id);
                self.deliveries(d, fx);
                Some(CrxResult::Ok(None))
            }
            "runtime.openOptionsPage" => {
                fx.push(Effect::OpenOptions { extension_id: ext.to_string() });
                Some(CrxResult::Ok(None))
            }
            "runtime.reload" => {
                if let Some(i) = self.registry.get(ext) {
                    fx.push(Effect::ReloadExtension { id: ext.to_string(), dir: i.dir.clone() });
                }
                Some(CrxResult::Ok(None))
            }
            "runtime.setUninstallURL" => {
                match a(0).as_str() {
                    Some(u) if u.starts_with("http://") || u.starts_with("https://") => {
                        self.uninstall_urls.insert(ext.to_string(), u.to_string());
                    }
                    _ => {
                        self.uninstall_urls.remove(ext);
                    }
                }
                Some(CrxResult::Ok(None))
            }
            "runtime.getPlatformInfo" => {
                let os = match self.cfg.platform {
                    CommandPlatform::Darwin => "mac",
                    CommandPlatform::Win32 => "win",
                    CommandPlatform::Linux => "linux",
                };
                let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x86-64" };
                Some(CrxResult::ok(json!({ "os": os, "arch": arch, "nacl_arch": arch })))
            }
            _ => {
                let _ = manifest;
                Some(CrxResult::err(format!("Unknown Extension API - {method}")))
            }
        }
    }

    /// URL to open after an uninstall (`runtime.setUninstallURL`), if the extension set one.
    pub fn uninstall_url(&self, id: &str) -> Option<&str> {
        self.uninstall_urls.get(id).map(|s| s.as_str())
    }

    /// All the messages of the interface locale for `chrome.i18n.getMessage` (merged with the default locale).
    fn all_messages(&self, ext: &str, manifest: &Value) -> Value {
        let Some(item) = self.registry.get(ext) else { return json!({}) };
        let default_locale = manifest.get("default_locale").and_then(|d| d.as_str());
        let candidates = crate::commands::locale_candidates(&self.cfg.locale, default_locale);
        let mut merged = serde_json::Map::new();
        for locale in candidates.iter().rev() {
            if let Ok(text) = std::fs::read_to_string(item.dir.join("_locales").join(locale).join("messages.json")) {
                if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&crate::manifest::strip_json_comments(&text)) {
                    for (k, v) in m {
                        merged.insert(k.to_lowercase(), v);
                    }
                }
            }
        }
        json!({ "locale": candidates.first().cloned().unwrap_or_default(), "messages": merged })
    }
}

/// The catalog entry in the JSON shape the page-side installer reads (camelCase keys of the TypeScript catalog).
fn catalog_json(spec: &crate::catalog::NamespaceSpec) -> Value {
    let mut v = json!({
        "permissions": spec.permissions, "methods": spec.methods, "events": spec.events,
    });
    let o = v.as_object_mut().expect("object");
    if let Some(k) = &spec.manifest_key {
        o.insert("manifestKey".into(), json!(k));
    }
    if !spec.soft.is_empty() {
        o.insert("soft".into(), json!(spec.soft));
    }
    if !spec.declarative_events.is_empty() {
        o.insert("declarativeEvents".into(), json!(spec.declarative_events));
    }
    if !spec.constants.is_empty() {
        o.insert("constants".into(), json!(spec.constants));
    }
    if !spec.classes.is_empty() {
        o.insert("classes".into(), json!(spec.classes));
    }
    if !spec.crashes.is_empty() {
        o.insert("crashes".into(), json!(spec.crashes));
    }
    if spec.unsupported {
        o.insert("unsupported".into(), json!(true));
    }
    if spec.content_script_only {
        o.insert("contentScriptOnly".into(), json!(true));
    }
    if !spec.children.is_empty() {
        o.insert("children".into(), Value::Object(spec.children.iter().map(|(k, c)| (k.clone(), catalog_json(c))).collect()));
    }
    v
}

fn ok_val(v: Value) -> CrxResult {
    CrxResult::ok(v)
}

/// Tabs the host can enumerate: the TabHost trait only gives single answers, so `query` asks for a window of ids around
/// the active tab. The app should override by implementing `all_tabs`, see [`TabHost`].
fn tabs_all(tabs: &dyn TabHost, active: &Option<crate::tabs::TabInfo>) -> Vec<crate::tabs::TabInfo> {
    let mut out = Vec::new();
    for id in tabs.all_tab_ids() {
        if let Some(t) = tabs.tab(id) {
            out.push(t);
        }
    }
    let _ = active;
    out
}

/// `tabs.query` filter.
pub fn query_matches(q: &Value, t: &crate::tabs::TabInfo, active: &Option<crate::tabs::TabInfo>) -> bool {
    let b = |k: &str| q.get(k).and_then(|v| v.as_bool());
    if b("active").is_some_and(|v| v != t.active) || b("pinned").is_some_and(|v| v != t.pinned) || b("audible").is_some_and(|v| v != t.audible) {
        return false;
    }
    if b("muted").is_some_and(|v| v != t.muted) || b("highlighted").is_some_and(|v| v != (t.highlighted || t.active)) || b("discarded").is_some_and(|v| v != t.discarded) {
        return false;
    }
    if b("currentWindow") == Some(true) || b("lastFocusedWindow") == Some(true) {
        if let Some(a) = active {
            if a.window_id != t.window_id {
                return false;
            }
        }
    }
    if q.get("windowId").and_then(|w| w.as_i64()).is_some_and(|w| w != t.window_id && w != -2) {
        return false;
    }
    if let Some(s) = q.get("status").and_then(|s| s.as_str()) {
        if (s == "loading") != t.loading {
            return false;
        }
    }
    if q.get("index").and_then(|i| i.as_i64()).is_some_and(|i| i != t.index) {
        return false;
    }
    if let Some(title) = q.get("title").and_then(|s| s.as_str()) {
        if !crate::contentscripts::glob_matches(title, &t.title) {
            return false;
        }
    }
    if let Some(url) = q.get("url") {
        let patterns: Vec<&str> = match url {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(|s| s.as_str()).collect(),
            _ => vec![],
        };
        if !patterns.is_empty() && !patterns.iter().filter_map(|p| crate::urlmatch::MatchPattern::parse(p)).any(|p| p.matches(&t.url)) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::TabInfo;
    use std::fs;

    const RSA3: &[u8] = include_bytes!("../tests/fixtures/rsa3.crx");

    struct NoTabs;
    impl TabHost for NoTabs {
        fn tab(&self, id: i64) -> Option<TabInfo> {
            (id == 7).then(|| TabInfo { id: 7, window_id: 1, url: "https://a.test/page".into(), title: "A".into(), active: true, ..Default::default() })
        }
        fn active_tab(&self, _: Option<i64>) -> Option<TabInfo> {
            self.tab(7)
        }
        fn all_tab_ids(&self) -> Vec<i64> {
            vec![7]
        }
        fn capture(&self, _: i64, _: &str, _: i64) -> Result<String, String> {
            Err("no".into())
        }
        fn insert_css(&self, _: i64, _: &str, _: &str) -> Result<String, String> {
            Ok("k".into())
        }
        fn remove_css(&self, _: i64, _: &str) {}
        fn page_language(&self, _: i64) -> Result<String, String> {
            Ok("en".into())
        }
        fn activate_at(&self, _: i64, _: Option<i64>) -> Option<Value> {
            None
        }
        fn read_extension_file(&self, _: &str, _: &str) -> Result<String, String> {
            Err("no".into())
        }
    }

    fn host(dir: &Path) -> ExtensionHost {
        ExtensionHost::new(HostConfig::new(dir))
    }

    /// An unpacked extension with the given manifest keys; returns its id.
    fn dev_ext(h: &mut ExtensionHost, root: &Path, extra: Value) -> (String, PathBuf, Vec<Effect>) {
        let dir = root.join("dev");
        fs::create_dir_all(&dir).unwrap();
        let mut m = json!({ "manifest_version": 3, "name": "Dev", "version": "1.0" });
        for (k, v) in extra.as_object().unwrap() {
            m[k] = v.clone();
        }
        fs::write(dir.join("manifest.json"), m.to_string()).unwrap();
        fs::write(dir.join("content.js"), "window.__ran = true;").unwrap();
        let fx = h.add_unpacked(&dir).unwrap();
        let id = crate::id::id_from_path(&dir);
        (id, dir, fx)
    }

    fn ctx(kind: ContextKind, id: &str) -> ApiContext {
        ApiContext { kind, id: id.into(), url: String::new() }
    }

    fn call(h: &mut ExtensionHost, id: &str, n: u64, ctx_: ApiContext, method: &str, args: Value) -> Vec<Effect> {
        h.call(Call { id: n, extension_id: id.into(), context: ctx_, method: json!(method), args }, &NoTabs)
    }

    fn reply_of(fx: &[Effect], n: u64) -> Option<CrxResult> {
        fx.iter().find_map(|e| if let Effect::Reply { call_id, result } = e { (*call_id == n).then(|| result.clone()) } else { None })
    }

    #[test]
    fn installs_a_store_extension_loads_it_and_lists_it() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let fx = h.install_crx_bytes(RSA3, None).unwrap();
        assert!(fx.iter().any(|e| matches!(e, Effect::LoadExtension { background: BackgroundEntry::None, .. })));
        let infos = h.infos();
        assert_eq!(infos.len(), 1);
        assert_eq!((infos[0].name.as_str(), infos[0].has_action), ("Fixture", true));
        // Never pinned by default; the Extensions menu still lists it.
        assert!(h.toolbar().is_empty());
        assert_eq!(h.with_button().len(), 1);
        let id = infos[0].id.clone();
        assert_eq!(h.set_pinned(&id, true), vec![Effect::PinnedChanged(vec![id.clone()])]);
        assert_eq!(h.toolbar(), [id.clone()]);
        assert_eq!(h.popup_url(&id, None), Some(format!("chrome-extension://{id}/popup.html")));
        // A new host reads the same installed list.
        let mut again = host(d.path());
        let fx = again.startup(vec![], vec![], vec![]);
        assert!(fx.iter().any(|e| matches!(e, Effect::LoadExtension { id: i, .. } if *i == id)));
        // Removing erases the folder.
        let fx = again.remove_extension(&id);
        assert!(fx.iter().any(|e| matches!(e, Effect::UnloadExtension { .. })));
        assert!(again.infos().is_empty());
        assert!(!d.path().join("Extensions").join(&id).exists());
    }

    #[test]
    fn a_call_needs_the_permission_and_the_extension_to_be_loaded() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "permissions": ["storage"] }));
        let fx = call(&mut h, &id, 1, ctx(ContextKind::Worker, "w"), "alarms.getAll", json!([]));
        assert!(matches!(reply_of(&fx, 1), Some(CrxResult::Err(e)) if e.contains("'alarms' permission is required")));
        let fx = call(&mut h, &"b".repeat(32), 2, ctx(ContextKind::Worker, "w"), "storage.local.get", json!([]));
        assert!(matches!(reply_of(&fx, 2), Some(CrxResult::Err(e)) if e.contains("not loaded")));
        let fx = call(&mut h, &id, 3, ctx(ContextKind::Worker, "w"), "nope.nope", json!([]));
        assert!(matches!(reply_of(&fx, 3), Some(CrxResult::Err(e)) if e.contains("Unknown Extension API")));
    }

    #[test]
    fn storage_roundtrip_and_change_event() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "permissions": ["storage"] }));
        let fx = call(&mut h, &id, 1, ctx(ContextKind::Page, "p"), "storage.local.set", json!([{ "k": 1 }]));
        assert_eq!(reply_of(&fx, 1), Some(CrxResult::Ok(None)));
        assert!(fx.iter().any(|e| matches!(e, Effect::Router(RouterAction::DeliverRunning { name, .. }) if name == "storage.onChanged")));
        let fx = call(&mut h, &id, 2, ctx(ContextKind::Page, "p"), "storage.local.get", json!(["k"]));
        assert_eq!(reply_of(&fx, 2), Some(CrxResult::ok(json!({ "k": 1 }))));
    }

    #[test]
    fn optional_permission_request_goes_through_the_dialog() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "optional_permissions": ["cookies"] }));
        let fx = call(&mut h, &id, 1, ctx(ContextKind::Page, "p"), "permissions.request", json!([{ "permissions": ["cookies"] }]));
        let ticket = fx.iter().find_map(|e| if let Effect::PermissionPrompt { ticket, lines, .. } = e { assert_eq!(lines, &vec![PermissionLine::Known("cookies")]); Some(*ticket) } else { None }).expect("a dialog");
        assert!(reply_of(&fx, 1).is_none(), "the call waits for the answer");
        // Before the grant the API is refused.
        let fx2 = call(&mut h, &id, 2, ctx(ContextKind::Page, "p"), "permissions.contains", json!([{ "permissions": ["cookies"] }]));
        assert_eq!(reply_of(&fx2, 2), Some(CrxResult::ok(json!(false))));
        let fx = h.permission_answer(ticket, true);
        assert_eq!(reply_of(&fx, 1), Some(CrxResult::ok(json!(true))));
        let fx2 = call(&mut h, &id, 3, ctx(ContextKind::Page, "p"), "permissions.contains", json!([{ "permissions": ["cookies"] }]));
        assert_eq!(reply_of(&fx2, 3), Some(CrxResult::ok(json!(true))));
        assert!(fx.iter().any(|e| matches!(e, Effect::Router(RouterAction::DeliverRunning { name, .. }) if name == "permissions.onAdded")));
    }

    #[test]
    fn quiet_permissions_are_granted_without_a_dialog() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "optional_permissions": ["alarms"] }));
        let fx = call(&mut h, &id, 1, ctx(ContextKind::Page, "p"), "permissions.request", json!([{ "permissions": ["alarms"] }]));
        assert_eq!(reply_of(&fx, 1), Some(CrxResult::ok(json!(true))));
        assert!(!fx.iter().any(|e| matches!(e, Effect::PermissionPrompt { .. })));
    }

    #[test]
    fn messages_flow_between_contexts_of_an_extension() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({}));
        for (cid, kind) in [("w", ContextKind::Worker), ("p", ContextKind::Page)] {
            let fx = call(&mut h, &id, 0, ctx(kind, cid), "runtime.__openContext", json!([{}]));
            assert!(reply_of(&fx, 0).is_some());
        }
        let fx = call(&mut h, &id, 10, ctx(ContextKind::Page, "p"), "runtime.sendMessage", json!([{ "hello": 1 }]));
        let token = fx.iter().find_map(|e| if let Effect::ToContext { context_id, name, args } = e { assert_eq!((context_id.as_str(), name.as_str()), ("w", "runtime.onMessage")); args[2].as_u64() } else { None }).expect("delivered to the worker");
        assert!(reply_of(&fx, 10).is_none());
        let fx = call(&mut h, &id, 11, ctx(ContextKind::Worker, "w"), "runtime.__reply", json!([token, { "handled": true, "hasListener": true, "response": { "pong": true } }]));
        assert_eq!(reply_of(&fx, 10), Some(CrxResult::ok(json!({ "pong": true }))));
    }

    #[test]
    fn content_scripts_are_prepared_for_a_frame() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "content_scripts": [{ "matches": ["https://a.test/*"], "js": ["content.js"], "run_at": "document_start" }] }));
        let frame = Frame { url: "https://a.test/x", is_main_frame: true, fallback_url: None };
        let inj = h.injections(&frame, Some(RunAt::DocumentStart));
        assert_eq!(inj.len(), 1);
        assert_eq!(inj[0].extension_id, id);
        assert!(inj[0].js.contains("window.__ran = true;"));
        assert!(h.injections(&Frame { url: "https://other.test/", is_main_frame: true, fallback_url: None }, None).is_empty());
        assert!(h.injections(&frame, Some(RunAt::DocumentIdle)).is_empty());
    }

    #[test]
    fn dnr_rules_wire_the_network_hooks_and_block() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "permissions": ["declarativeNetRequest"] }));
        let rule = json!({ "id": 1, "priority": 1, "action": { "type": "block" }, "condition": { "urlFilter": "||ads.example^" } });
        let fx = call(&mut h, &id, 1, ctx(ContextKind::Worker, "w"), "declarativeNetRequest.updateSessionRules", json!([{ "addRules": [rule] }]));
        assert_eq!(reply_of(&fx, 1), Some(CrxResult::Ok(None)));
        assert!(fx.iter().any(|e| matches!(e, Effect::NetworkHooks(v) if v == &vec![DnrEvent::OnBeforeRequest])));
        let req = RequestDetails { url: "https://ads.example/a.js".into(), method: "GET".into(), resource_type: Some("script".into()), tab_id: Some(1), referrer: Some("https://site.org/".into()), ..Default::default() };
        assert_eq!(h.before_request(&req), RequestOutcome::Cancel);
    }

    #[test]
    fn commands_alarms_and_action_state() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, fx) = dev_ext(&mut h, d.path(), json!({ "permissions": ["alarms"], "action": {}, "commands": { "go": { "description": "Go", "suggested_key": { "default": "Alt+Shift+K" } } } }));
        assert!(fx.iter().any(|e| matches!(e, Effect::CommandsChanged)));
        assert_eq!(h.command_bindings().len(), 1);
        let fx = call(&mut h, &id, 1, ctx(ContextKind::Worker, "w"), "commands.getAll", json!([]));
        assert_eq!(reply_of(&fx, 1).unwrap().to_json()["value"][0]["name"], "go");
        let fx = call(&mut h, &id, 2, ctx(ContextKind::Worker, "w"), "alarms.create", json!(["a", { "delayInMinutes": 1 }]));
        assert_eq!(reply_of(&fx, 2), Some(CrxResult::Ok(None)));
        let fx = call(&mut h, &id, 3, ctx(ContextKind::Worker, "w"), "action.disable", json!([]));
        assert!(fx.iter().any(|e| matches!(e, Effect::ActionStates(_))));
        assert!(h.popup_url(&id, None).is_none(), "a disabled button has no popup");
        let fx = call(&mut h, &id, 4, ctx(ContextKind::Worker, "w"), "action.setBadgeText", json!([{ "text": "5" }]));
        assert_eq!(reply_of(&fx, 4), Some(CrxResult::Ok(None)));
        assert_eq!(h.button_state(&id, None)["badgeText"], "5");
    }

    #[test]
    fn unpacked_extensions_load_reload_and_leave_their_folder() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, dir, fx) = dev_ext(&mut h, d.path(), json!({}));
        assert!(fx.iter().any(|e| matches!(e, Effect::UnpackedPathsChanged(l) if l == &vec![dir.to_string_lossy().into_owned()])));
        assert!(fx.iter().any(|e| matches!(e, Effect::LoadExtension { id: i, .. } if *i == id)));
        let fx = h.reload_unpacked(&dir);
        assert!(fx.iter().any(|e| matches!(e, Effect::UnloadExtension { .. })) && fx.iter().any(|e| matches!(e, Effect::LoadExtension { .. })));
        assert!(h.infos()[0].unpacked.as_ref().unwrap().reloaded_at.is_some());
        let fx = h.remove_unpacked(&dir);
        assert!(fx.iter().any(|e| matches!(e, Effect::UnpackedPathsChanged(l) if l.is_empty())));
        assert!(dir.join("manifest.json").exists());
        assert!(h.infos().is_empty());
        // A folder without a manifest is refused with Settings' message.
        let empty = d.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(h.add_unpacked(&empty).unwrap_err().contains("has no manifest.json"));
    }

    #[test]
    fn offscreen_document_flow_and_getcontexts() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "permissions": ["offscreen"], "background": { "service_worker": "sw.js" } }));
        h.worker_running(&id, "1");
        let fx = call(&mut h, &id, 1, ctx(ContextKind::Worker, "w"), "offscreen.createDocument", json!([{ "url": "off.html", "reasons": ["TESTING"], "justification": "t" }]));
        assert!(fx.iter().any(|e| matches!(e, Effect::OffscreenOpen { url, .. } if url == &format!("chrome-extension://{id}/off.html"))));
        assert!(reply_of(&fx, 1).is_none());
        let fx = h.offscreen_loaded(&id, 40);
        assert_eq!(reply_of(&fx, 1), Some(CrxResult::Ok(None)));
        assert!(h.is_offscreen_page(40));
        let fx = call(&mut h, &id, 2, ctx(ContextKind::Worker, "w"), "runtime.getContexts", json!([{}]));
        let v = reply_of(&fx, 2).unwrap().to_json();
        let types: Vec<&str> = v["value"].as_array().unwrap().iter().map(|c| c["contextType"].as_str().unwrap()).collect();
        assert_eq!(types, ["BACKGROUND", "OFFSCREEN_DOCUMENT"]);
        let fx = call(&mut h, &id, 3, ctx(ContextKind::Worker, "w"), "offscreen.closeDocument", json!([]));
        assert!(fx.iter().any(|e| matches!(e, Effect::OffscreenClose { page_id: 40 })));
    }

    #[test]
    fn tab_queries_use_the_tab_host_and_hide_urls_without_permission() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({}));
        let fx = call(&mut h, &id, 1, ctx(ContextKind::Worker, "w"), "tabs.query", json!([{ "active": true }]));
        let v = reply_of(&fx, 1).unwrap().to_json();
        assert_eq!(v["value"][0]["id"], 7);
        assert!(v["value"][0].get("url").is_none(), "no tabs permission, no host access");
        let (id2, _, _) = {
            let mut h2 = host(d.path());
            let r = dev_ext(&mut h2, &d.path().join("two"), json!({ "permissions": ["tabs"] }));
            h = h2;
            r
        };
        let fx = call(&mut h, &id2, 2, ctx(ContextKind::Worker, "w"), "tabs.get", json!([7]));
        assert_eq!(reply_of(&fx, 2).unwrap().to_json()["value"]["url"], "https://a.test/page");
        // Creating a tab is the browser's job.
        let fx = call(&mut h, &id2, 3, ctx(ContextKind::Worker, "w"), "tabs.create", json!([{ "url": "https://x.test/" }]));
        assert!(fx.iter().any(|e| matches!(e, Effect::TabAction { method, .. } if method == "tabs.create")));
        let fx = h.tab_action_done(3, Ok(json!({ "id": 9 })));
        assert_eq!(reply_of(&fx, 3), Some(CrxResult::ok(json!({ "id": 9 }))));
    }

    #[test]
    fn query_filter() {
        let t = TabInfo { id: 1, window_id: 2, active: true, url: "https://a.test/x".into(), title: "Hello world".into(), ..Default::default() };
        let q = |v: Value| query_matches(&v, &t, &Some(t.clone()));
        assert!(q(json!({})) && q(json!({ "active": true })) && !q(json!({ "active": false })));
        assert!(q(json!({ "url": "https://a.test/*" })) && !q(json!({ "url": ["https://b.test/*"] })));
        assert!(q(json!({ "title": "Hello*" })) && !q(json!({ "title": "Nope" })));
        assert!(q(json!({ "currentWindow": true })) && q(json!({ "windowId": 2 })) && !q(json!({ "windowId": 3 })));
        assert!(q(json!({ "status": "complete" })) && !q(json!({ "status": "loading" })));
    }

    #[test]
    fn the_tick_fires_alarms_and_wakes_workers() {
        let d = tempfile::tempdir().unwrap();
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(1_000_000));
        let c2 = clock.clone();
        let mut cfg = HostConfig::new(d.path());
        cfg.now_ms = Arc::new(move || c2.load(std::sync::atomic::Ordering::SeqCst));
        let mut h = ExtensionHost::new(cfg);
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "permissions": ["alarms"] }));
        call(&mut h, &id, 1, ctx(ContextKind::Worker, "w"), "alarms.create", json!(["x", { "delayInMinutes": 1 }]));
        clock.store(1_000_000 + 60_000, std::sync::atomic::Ordering::SeqCst);
        let fx = h.tick();
        assert!(fx.iter().any(|e| matches!(e, Effect::Router(RouterAction::StartWorker { extension_id, .. }) if *extension_id == id)), "{fx:?}");
        let ticket = fx.iter().find_map(|e| if let Effect::Router(RouterAction::StartWorker { ticket, .. }) = e { Some(*ticket) } else { None }).unwrap();
        let fx = h.wake_result(ticket, true);
        assert!(fx.iter().any(|e| matches!(e, Effect::Router(RouterAction::DeliverToWorker { events, .. }) if events[0].0 == "alarms.onAlarm")));
    }

    #[test]
    fn proxy_setting_is_applied_and_survives_a_restart() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "permissions": ["proxy"] }));
        let fx = call(&mut h, &id, 1, ctx(ContextKind::Worker, "w"), "proxy.settings.set", json!([{ "value": { "mode": "direct" } }]));
        assert!(fx.iter().any(|e| matches!(e, Effect::ApplySetting(a) if a.key == "proxy.settings")));
        h.flush();
        let mut again = host(d.path());
        let fx = again.startup(vec![dir_of(d.path())], vec![], vec![]);
        assert!(fx.iter().any(|e| matches!(e, Effect::ApplySetting(a) if a.key == "proxy.settings")), "{fx:?}");
    }

    #[test]
    fn unload_keeps_persisted_data_and_uninstall_drops_it() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "permissions": ["storage", "alarms", "contextMenus"] }));
        call(&mut h, &id, 1, ctx(ContextKind::Worker, "w"), "storage.local.set", json!([{ "k": 1 }]));
        call(&mut h, &id, 2, ctx(ContextKind::Worker, "w"), "alarms.create", json!(["a", { "periodInMinutes": 1 }]));
        call(&mut h, &id, 3, ctx(ContextKind::Worker, "w"), "contextMenus.create", json!([{ "id": "m", "title": "M" }]));
        // Disabling is an unload: the data stays and comes back with the extension.
        let fx = h.set_enabled(&id, false);
        assert!(fx.iter().any(|e| matches!(e, Effect::UnloadExtension { .. })));
        assert_eq!(h.alarms.scheduler.known_ids(), [id.clone()]);
        assert!(h.menus.has(&id));
        h.set_enabled(&id, true);
        let fx = call(&mut h, &id, 4, ctx(ContextKind::Worker, "w"), "storage.local.get", json!([null]));
        assert_eq!(reply_of(&fx, 4), Some(CrxResult::ok(json!({ "k": 1 }))));
        // Removing is an uninstall.
        let dir = h.registry.get(&id).unwrap().dir.clone();
        h.remove_extension(&id);
        assert!(h.alarms.scheduler.known_ids().is_empty());
        assert!(!h.menus.has(&id));
        assert!(h.storage.known_ids().is_empty());
        assert!(dir.join("manifest.json").exists(), "unpacked folders are never erased");
    }

    #[test]
    fn startup_forgets_data_of_extensions_that_vanished() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let (id, _, _) = dev_ext(&mut h, d.path(), json!({ "permissions": ["storage"] }));
        call(&mut h, &id, 1, ctx(ContextKind::Worker, "w"), "storage.local.set", json!([{ "k": 1 }]));
        h.flush();
        // The folder was deleted by hand and is no longer remembered.
        let mut again = host(d.path());
        again.startup(vec![], vec![], vec![]);
        assert!(again.storage.known_ids().is_empty());
    }

    #[test]
    fn the_store_install_flow_asks_then_downloads() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let id = "mpedjdfgkmlmhabmbdmnledbjbnmdfgb";
        let manifest = json!({ "manifest_version": 3, "name": "Store ext", "version": "1.0", "permissions": ["tabs"] }).to_string();
        let page = "https://chromewebstore.google.com/detail/x/mpedjdfgkmlmhabmbdmnledbjbnmdfgb";
        let fx = h.webstore_call(1, "https://evil.example/", "webstorePrivate.beginInstallWithManifest3", &[json!({ "id": id, "manifest": manifest })]);
        assert!(matches!(reply_of(&fx, 1), Some(CrxResult::Err(_))));
        let fx = h.webstore_call(2, page, "webstorePrivate.beginInstallWithManifest3", &[json!({ "id": id, "manifest": manifest })]);
        let ticket = fx.iter().find_map(|e| if let Effect::InstallPrompt { ticket, lines, .. } = e { assert_eq!(lines, &vec![PermissionLine::Known("tabs")]); Some(*ticket) } else { None }).expect("prompt");
        // completeInstall before approval is refused.
        let fx = h.webstore_call(3, page, "webstorePrivate.completeInstall", &[json!(id)]);
        assert!(matches!(reply_of(&fx, 3), Some(CrxResult::Err(_))));
        let fx = h.install_decision(ticket, true);
        assert_eq!(reply_of(&fx, 2), Some(CrxResult::ok(json!(""))));
        let fx = h.webstore_call(4, page, "webstorePrivate.completeInstall", &[json!(id)]);
        assert!(fx.iter().any(|e| matches!(e, Effect::InstallFromStore { call_id: 4, extension_id } if extension_id == id)));
        let fx = h.webstore_call(5, page, "webstorePrivate.getExtensionStatus", &[json!(id)]);
        assert_eq!(reply_of(&fx, 5), Some(CrxResult::ok(json!("installable"))));
        // A refusal is reported to the page.
        let fx = h.webstore_call(6, page, "webstorePrivate.beginInstallWithManifest3", &[json!({ "id": id, "manifest": manifest })]);
        let t2 = fx.iter().find_map(|e| if let Effect::InstallPrompt { ticket, .. } = e { Some(*ticket) } else { None }).unwrap();
        assert_eq!(reply_of(&h.install_decision(t2, false), 6), Some(CrxResult::ok(json!("user_cancelled"))));
        // A broken manifest is a manifest error, not a dialog.
        let fx = h.webstore_call(7, page, "webstorePrivate.beginInstallWithManifest3", &[json!({ "id": id, "manifest": "{ nope" })]);
        assert_eq!(reply_of(&fx, 7), Some(CrxResult::ok(json!("manifest_error"))));
    }

    #[test]
    fn a_store_install_raises_on_installed() {
        let d = tempfile::tempdir().unwrap();
        let mut h = host(d.path());
        let fx = h.install_crx_bytes(RSA3, None).unwrap();
        let ev = fx.iter().find_map(|e| if let Effect::Router(RouterAction::DeliverRunning { name, args, .. }) = e { (name == "runtime.onInstalled").then(|| args.clone()) } else { None });
        assert_eq!(ev, Some(vec![json!({ "reason": "install" })]));
        assert!(fx.iter().any(|e| matches!(e, Effect::Router(RouterAction::StartWorker { .. }))), "the worker is woken to receive it");
    }

    fn dir_of(root: &Path) -> String {
        root.join("dev").to_string_lossy().into_owned()
    }
}

//! State shared between the main thread and the CEF helper threads (bundle hook, request filter, scheme handler).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{Duration, Instant};

use signet_extensions::host::{Effect, ExtensionHost};
use signet_web::ViewId;

use super::ctx::Contexts;
use super::files::Loaded;

/// Answers produced off the main thread (dialogs, downloads), consumed by `tick`.
pub enum Inbox {
    Permission { ticket: u64, allowed: bool },
    Install { ticket: u64, allowed: bool },
    StoreInstall { call_id: u64, result: Result<Vec<Effect>, String> },
    RemoveConfirmed(String),
    StoreUninstall { call_id: u64, id: String, allowed: bool },
    Unpacked(Option<std::path::PathBuf>),
}

type Listener = Arc<dyn Fn() + Send + Sync>;

pub struct Shared {
    pub host: Mutex<ExtensionHost>,
    pub loaded: RwLock<HashMap<String, Loaded>>,
    pub ctxs: Mutex<Contexts>,
    pub inbox: Mutex<Vec<Inbox>>,
    listeners: Mutex<Vec<(u64, Listener)>>,
    next_listener: AtomicU64,
    /// True while an extension wants `onBeforeRequest` decisions (the request filter skips the lock otherwise).
    pub dnr_on: AtomicBool,
    /// View id of the open popup (0: none).
    pub popup_view: AtomicU64,
    /// Views to the extension's tab number, refreshed by the main thread.
    pub view_tab: RwLock<HashMap<ViewId, i64>>,
}

impl Shared {
    pub fn new(host: ExtensionHost) -> Shared {
        Shared {
            host: Mutex::new(host),
            loaded: RwLock::new(HashMap::new()),
            ctxs: Mutex::new(Contexts::default()),
            inbox: Mutex::new(Vec::new()),
            listeners: Mutex::new(Vec::new()),
            next_listener: AtomicU64::new(1),
            dnr_on: AtomicBool::new(false),
            popup_view: AtomicU64::new(0),
            view_tab: RwLock::new(HashMap::new()),
        }
    }

    /// Locks the host, waiting at most `wait` (helper threads must not stall page loads behind a long call).
    pub fn host_within(&self, wait: Duration) -> Option<MutexGuard<'_, ExtensionHost>> {
        let start = Instant::now();
        loop {
            match self.host.try_lock() {
                Ok(g) => return Some(g),
                Err(std::sync::TryLockError::Poisoned(p)) => return Some(p.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => {
                    if start.elapsed() >= wait {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    /// Main thread: the host may be held by an install in progress for a moment.
    pub fn host(&self) -> MutexGuard<'_, ExtensionHost> {
        self.host.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn ctxs(&self) -> MutexGuard<'_, Contexts> {
        self.ctxs.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn push(&self, item: Inbox) {
        if let Ok(mut i) = self.inbox.lock() {
            i.push(item);
        }
    }

    pub fn take_inbox(&self) -> Vec<Inbox> {
        self.inbox.lock().map(|mut i| std::mem::take(&mut *i)).unwrap_or_default()
    }

    pub fn subscribe(self: &Arc<Self>, f: Listener) -> signet_core::api::Subscription {
        let id = self.next_listener.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut l) = self.listeners.lock() {
            l.push((id, f));
        }
        let me = self.clone();
        signet_core::api::Subscription::new(move || {
            if let Ok(mut l) = me.listeners.lock() {
                l.retain(|(i, _)| *i != id);
            }
        })
    }

    /// Tells the toolbars their buttons changed.
    pub fn notify(&self) {
        let list: Vec<Listener> = self.listeners.lock().map(|l| l.iter().map(|(_, f)| f.clone()).collect()).unwrap_or_default();
        for f in list {
            f();
        }
    }
}

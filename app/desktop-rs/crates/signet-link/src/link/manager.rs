//! Sync-friendly front of the phone link (port of features/link/link.ts without the UI parts).
//!
//! Owns a background tokio runtime. The app calls plain methods and reads `LinkEvent`s from a channel.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::runtime::{Handle, Runtime};
use tokio::sync::mpsc;

use super::devservers::{DevServerScanner, TabLike};
use super::discovery::Discovery;
use super::identity::{clean_host_name, computer_name, desktop_platform, lan_addresses};
use super::loopback::to_link_localhost;
use super::protocol::LinkDevice;
use super::service::{LinkHooks, LinkService, LinkServiceOptions, system_now};
use super::status::LinkStatus;
use super::store::{LinkStore, NoVault, StoredLink, Vault};
use crate::qrimage::QrMatrix;

/// Dev servers are re-read at this frequency while a phone is active.
const SCAN_MS: u64 = 5000;

pub struct LinkConfig {
    /// Folder the app passes in; `link.json` lives there.
    pub data_dir: PathBuf,
    /// System keychain for the private keys. `None`: keys are stored in plain text (`raw:`).
    pub vault: Option<Box<dyn Vault>>,
    /// Preferred port (default 47615).
    pub port: Option<u16>,
    /// Announce over mDNS (default true; tests turn it off).
    pub announce: bool,
}

impl LinkConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self { data_dir: data_dir.into(), vault: None, port: None, announce: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub link_id: String,
    pub device: LinkDevice,
    pub enabled: bool,
    pub status: LinkStatus,
    pub created_at: u64,
    pub last_seen: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingInfo {
    /// Text of the QR code (`signet://link?d=...`).
    pub url: String,
    pub expires_at: u64,
    #[serde(skip)]
    pub qr: Option<QrMatrix>,
}

/// State for Settings > Devices and the toolbar indicator.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkState {
    pub general: bool,
    pub name: String,
    pub devices: Vec<DeviceInfo>,
    /// Number of links active on both sides.
    pub active: usize,
    pub pairing: Option<PairingInfo>,
    pub listen_error: bool,
}

#[derive(Debug, Clone)]
pub enum LinkEvent {
    /// The state changed (only sent when it really differs from the last one).
    Changed(Box<LinkState>),
    /// The phone asks to open an address here (the link is active on both sides): open a tab and show the window.
    OpenRequested { url: String, link_id: String, device_name: String },
}

enum Internal {
    Changed,
    Listening(Option<u16>),
    Open(String, StoredLink),
}

struct Hooks(mpsc::UnboundedSender<Internal>);
impl LinkHooks for Hooks {
    fn open(&self, url: &str, link: &StoredLink) {
        let _ = self.0.send(Internal::Open(url.to_string(), link.clone()));
    }
    fn changed(&self) {
        let _ = self.0.send(Internal::Changed);
    }
    fn listening(&self, port: Option<u16>) {
        let _ = self.0.send(Internal::Listening(port));
    }
}

struct Shared {
    name: Mutex<String>,
    tabs: Mutex<Vec<TabLike>>,
    listen_error: Mutex<bool>,
}

pub struct LinkManager {
    rt: Option<Runtime>,
    handle: Handle,
    service: Arc<LinkService>,
    shared: Arc<Shared>,
}

impl LinkManager {
    /// Loads the links and starts listening if the master switch is ON and a link exists.
    pub fn start(config: LinkConfig) -> (LinkManager, std_mpsc::Receiver<LinkEvent>) {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("signet-link").build().expect("tokio runtime");
        let handle = rt.handle().clone();
        let (events_tx, events_rx) = std_mpsc::channel();
        let (int_tx, int_rx) = mpsc::unbounded_channel();

        let shared = Arc::new(Shared {
            name: Mutex::new(clean_host_name(&std::env::var("HOSTNAME").unwrap_or_default())),
            tabs: Mutex::new(vec![]),
            listen_error: Mutex::new(false),
        });
        let store = LinkStore::new(config.data_dir.join("link.json"), config.vault.unwrap_or_else(|| Box::new(NoVault)));
        let name_for_device = shared.clone();
        let desktop_id = store.desktop_id.clone();
        let service = {
            let _g = handle.enter();
            LinkService::new(LinkServiceOptions {
                store,
                desktop: Arc::new(move || LinkDevice { id: desktop_id.clone(), name: name_for_device.name.lock().clone(), platform: desktop_platform() }),
                hosts: Arc::new(lan_addresses),
                port: config.port,
                now: Arc::new(system_now),
                hooks: Arc::new(Hooks(int_tx)),
            })
        };

        let mgr = LinkManager { rt: Some(rt), handle: handle.clone(), service: service.clone(), shared: shared.clone() };
        let coordinator = Coordinator {
            service: service.clone(),
            shared: shared.clone(),
            events: events_tx,
            announce: config.announce,
            discovery: Discovery::default(),
            qr: None,
            last: String::new(),
            scan: None,
            scanner: Arc::new(tokio::sync::Mutex::new(DevServerScanner::new())),
        };
        handle.spawn(coordinator.run(int_rx));
        // Real computer name (System Settings on Mac), then start.
        let (s2, sh2) = (service.clone(), shared.clone());
        handle.spawn(async move {
            if let Ok(name) = tokio::task::spawn_blocking(computer_name).await {
                *sh2.name.lock() = name;
            }
            s2.sync().await;
        });
        (mgr, events_rx)
    }

    /// Runs a future on the link runtime and waits for it. Do not call from a thread of that runtime.
    fn run<T: Send + 'static>(&self, fut: impl std::future::Future<Output = T> + Send + 'static) -> T {
        let (tx, rx) = std_mpsc::channel();
        self.handle.spawn(async move {
            let _ = tx.send(fut.await);
        });
        rx.recv().expect("link runtime stopped")
    }

    pub fn state(&self) -> LinkState {
        build_state(&self.service, &self.shared, &None)
    }

    pub fn set_general(&self, on: bool) {
        *self.shared.listen_error.lock() = false;
        let s = self.service.clone();
        self.run(async move { s.set_general(on).await });
    }

    pub fn set_device(&self, link_id: &str, on: bool) {
        let (s, id) = (self.service.clone(), link_id.to_string());
        self.run(async move { s.set_device(&id, on).await });
    }

    pub fn forget(&self, link_id: &str) {
        let (s, id) = (self.service.clone(), link_id.to_string());
        self.run(async move { s.forget(&id).await });
    }

    /// Shows a new QR code (the state's `pairing`). False if the computer cannot listen.
    pub fn start_pairing(&self) -> bool {
        *self.shared.listen_error.lock() = false;
        let s = self.service.clone();
        let ok = self.run(async move { s.start_pairing().await.is_some() });
        if !ok {
            *self.shared.listen_error.lock() = self.service.general();
            let s = self.service.clone();
            self.run(async move { s.sync().await });
        }
        ok
    }

    pub fn cancel_pairing(&self) {
        let s = self.service.clone();
        self.run(async move { s.cancel_pairing() });
    }

    /// Tabs currently open (all of them): local ones are offered to phones and their ports relayed.
    pub fn set_tabs(&self, tabs: Vec<TabLike>) {
        *self.shared.tabs.lock() = tabs;
    }

    /// Phones a page can be sent to right now: (link id, device name).
    pub fn targets(&self) -> Vec<(String, String)> {
        self.service.active_links().into_iter().map(|l| (l.link_id, l.peer.name)).collect()
    }

    /// The address to send for a tab URL: http(s) only, local addresses go through `localhost`. None if it cannot be sent.
    pub fn url_for_phone(url: &str) -> Option<String> {
        let l = url.to_ascii_lowercase();
        (l.starts_with("http://") || l.starts_with("https://")).then(|| to_link_localhost(url))
    }

    /// "Open on my phone". `link_id`: None = the first active phone. False if there is none or the URL cannot be sent.
    pub fn open_on_phone(&self, url: &str, link_id: Option<&str>) -> bool {
        let Some(url) = Self::url_for_phone(url) else { return false };
        let Some(id) = link_id.map(String::from).or_else(|| self.targets().first().map(|t| t.0.clone())) else { return false };
        self.service.open_on_phone(&id, &url)
    }

    /// Immediate write of the links file.
    pub fn flush(&self) {
        self.service.with_store(|s| s.flush());
    }

    /// App shutdown: announcement withdrawn, connections closed, file written.
    pub fn shutdown(mut self) {
        let s = self.service.clone();
        self.run(async move { s.close().await });
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(Duration::from_secs(2));
        }
    }

    /// Listening port, for tests and diagnostics.
    pub fn port(&self) -> Option<u16> {
        self.service.port()
    }
}

fn build_state(service: &Arc<LinkService>, shared: &Shared, qr: &Option<(String, Option<QrMatrix>)>) -> LinkState {
    let mut devices: Vec<DeviceInfo> = service
        .summaries()
        .into_iter()
        .map(|s| DeviceInfo {
            link_id: s.link.link_id,
            device: s.link.peer,
            enabled: s.link.enabled,
            status: s.status,
            created_at: s.link.created_at,
            last_seen: s.link.last_seen,
        })
        .collect();
    devices.sort_by_key(|d| d.created_at);
    let pairing = service.pending_offer().map(|(offer, url)| {
        let matrix = match qr {
            Some((u, m)) if *u == url => m.clone(),
            _ => QrMatrix::new(&url),
        };
        PairingInfo { url, expires_at: offer.expires_at, qr: matrix }
    });
    LinkState {
        general: service.general(),
        name: shared.name.lock().clone(),
        active: service.active_links().len(),
        devices,
        pairing,
        listen_error: *shared.listen_error.lock(),
    }
}

struct Coordinator {
    service: Arc<LinkService>,
    shared: Arc<Shared>,
    events: std_mpsc::Sender<LinkEvent>,
    announce: bool,
    discovery: Discovery,
    qr: Option<(String, Option<QrMatrix>)>,
    last: String,
    scan: Option<tokio::task::JoinHandle<()>>,
    scanner: Arc<tokio::sync::Mutex<DevServerScanner>>,
}

impl Coordinator {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Internal>) {
        while let Some(msg) = rx.recv().await {
            match msg {
                Internal::Listening(port) => match port {
                    Some(p) if self.announce => {
                        let id = self.service.desktop_id();
                        let name = self.shared.name.lock().clone();
                        self.discovery.publish(&name, &id, p);
                    }
                    _ => self.discovery.unpublish(),
                },
                Internal::Open(url, link) => {
                    let _ = self.events.send(LinkEvent::OpenRequested { url, link_id: link.link_id, device_name: link.peer.name });
                }
                Internal::Changed => self.changed(),
            }
        }
    }

    fn changed(&mut self) {
        self.sync_scan();
        let state = build_state(&self.service, &self.shared, &self.qr);
        if let Some(p) = &state.pairing {
            self.qr = Some((p.url.clone(), p.qr.clone()));
        }
        let json = serde_json::to_string(&state).unwrap_or_default();
        if json == self.last {
            return;
        }
        self.last = json;
        let _ = self.events.send(LinkEvent::Changed(Box::new(state)));
    }

    fn sync_scan(&mut self) {
        let on = !self.service.active_links().is_empty();
        if on && self.scan.is_none() {
            let (service, shared, scanner) = (self.service.clone(), self.shared.clone(), self.scanner.clone());
            self.scan = Some(tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_millis(SCAN_MS));
                loop {
                    tick.tick().await;
                    let tabs = shared.tabs.lock().clone();
                    let mut sc = scanner.lock().await;
                    let list = sc.scan(service.port(), &tabs, system_now()).await;
                    let ports = sc.ports.clone();
                    drop(sc);
                    service.set_servers(list, Some(ports));
                }
            }));
        } else if !on {
            if let Some(t) = self.scan.take() {
                t.abort();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_shows_a_qr_and_cancel_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = LinkConfig::new(dir.path());
        cfg.port = Some(0);
        cfg.announce = false;
        let (mgr, events) = LinkManager::start(cfg);
        assert!(mgr.start_pairing());
        let st = mgr.state();
        let p = st.pairing.expect("pairing");
        assert!(p.url.starts_with("signet://link?d="));
        assert!(p.qr.is_some());
        assert!(mgr.port().is_some());
        // At least one Changed event with a pairing arrived.
        let mut saw = false;
        while let Ok(ev) = events.recv_timeout(Duration::from_millis(500)) {
            if let LinkEvent::Changed(s) = ev {
                saw |= s.pairing.is_some();
            }
        }
        assert!(saw);
        mgr.cancel_pairing();
        assert!(mgr.state().pairing.is_none());
        mgr.shutdown();
    }

    #[test]
    fn url_for_phone_filters_and_maps_localhost() {
        assert_eq!(LinkManager::url_for_phone("http://127.0.0.1:3000/a").as_deref(), Some("http://localhost:3000/a"));
        assert_eq!(LinkManager::url_for_phone("file:///x"), None);
        assert_eq!(LinkManager::url_for_phone("https://example.com/").as_deref(), Some("https://example.com/"));
    }
}

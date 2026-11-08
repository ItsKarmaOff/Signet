//! Link with phones: TLS server, pairing, control connections and tunnels to localhost
//! (port of app/desktop/src/main/features/link/service.ts).
//!
//! No UI dependency: the host (tabs, mDNS, app) is plugged in through `LinkHooks`. All methods run on a tokio runtime.
//! Hooks are called while an internal lock is held: they must be cheap and must not call back into the service
//! (send on a channel, set a flag).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use rustls::ServerConfig;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use super::certs::{CertPair, cert_sha256, make_default_cert, make_link_cert};
use super::protocol::*;
use super::status::{LinkStatus, LinkStatusInput, link_status};
use super::store::{LinkStore, StoredLink, constant_time_eq, hash_token, new_token, token_matches};
use crate::b64;
use crate::rand_bytes;

const FIRST_LINE_MS: u64 = 10_000;
const CONTROL_IDLE_MS: u64 = 90_000;
const CONNECT_MS: u64 = 3_000;
const SERVERS_THROTTLE_MS: u64 = 2_000;
const MAX_BAD_CODES: u32 = 5;
const WRITE_MS: u64 = 5_000;

/// What the service asks of its host.
pub trait LinkHooks: Send + Sync {
    /// The phone asks to open an address (the link is active on both sides).
    fn open(&self, url: &str, link: &StoredLink);
    /// Something changed (list, state, pairing).
    fn changed(&self);
    /// Listening started (port) or stopped (None): mDNS announcement.
    fn listening(&self, port: Option<u16>);
}

pub struct LinkServiceOptions {
    pub store: LinkStore,
    /// This computer as phones see it.
    pub desktop: Arc<dyn Fn() -> LinkDevice + Send + Sync>,
    /// Local network addresses, for the QR code.
    pub hosts: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    /// Preferred port (47615 by default; Some(0) = random, for tests).
    pub port: Option<u16>,
    pub now: Arc<dyn Fn() -> u64 + Send + Sync>,
    pub hooks: Arc<dyn LinkHooks>,
}

pub fn system_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

struct Pending {
    link_id: String,
    code: String,
    expires_at: u64,
    cert: CertPair,
    cert_sha256: String,
    bad_codes: u32,
    timer: JoinHandle<()>,
}

enum Out {
    Msg(LinkMessage),
    /// Optional last message, then close.
    End(Option<LinkMessage>),
}

struct Session {
    id: u64,
    tx: mpsc::UnboundedSender<Out>,
    peer_paused: bool,
    last_servers: String,
    last_sent_at: u64,
    timer_pending: bool,
}

struct TunnelEntry {
    id: u64,
    cancel: watch::Sender<bool>,
}

struct Listener {
    port: u16,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Finished {
    Used,
    Expired,
}

struct State {
    store: LinkStore,
    listener: Option<Listener>,
    pending: Option<Pending>,
    finished: Vec<(String, Finished)>,
    sessions: HashMap<String, Session>,
    tunnels: HashMap<String, Vec<TunnelEntry>>,
    contexts: HashMap<String, Arc<CertifiedKey>>,
    default_key: Option<Arc<CertifiedKey>>,
    servers: Vec<DevServer>,
    ports: Vec<u16>,
    closed: bool,
}

pub struct LinkSummary {
    pub link: StoredLink,
    pub status: LinkStatus,
}

pub struct LinkService {
    st: Mutex<State>,
    listen_lock: tokio::sync::Mutex<()>,
    next_id: AtomicU64,
    desktop: Arc<dyn Fn() -> LinkDevice + Send + Sync>,
    hosts: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    preferred_port: u16,
    now: Arc<dyn Fn() -> u64 + Send + Sync>,
    hooks: Arc<dyn LinkHooks>,
}

/// Reads JSON lines from a byte stream; keeps what follows the lines read as is (raw bytes of a tunnel).
#[derive(Default)]
pub struct LineReader {
    buf: Vec<u8>,
}

impl LineReader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds bytes. False if a line exceeds the allowed size.
    pub fn add(&mut self, chunk: &[u8]) -> bool {
        self.buf.extend_from_slice(chunk);
        let end = self.buf.iter().position(|b| *b == b'\n').map(|n| n + 1).unwrap_or(self.buf.len());
        end <= LINK_MAX_LINE
    }

    /// Next complete line (without line ending), or None.
    pub fn line(&mut self) -> Option<String> {
        let nl = self.buf.iter().position(|b| *b == b'\n')?;
        let rest = self.buf.split_off(nl + 1);
        let mut line = std::mem::replace(&mut self.buf, rest);
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        Some(String::from_utf8_lossy(&line).to_string())
    }

    /// Complete lines received, or None if a line exceeds the allowed size.
    pub fn push(&mut self, chunk: &[u8]) -> Option<Vec<String>> {
        if !self.add(chunk) {
            return None;
        }
        let mut lines = vec![];
        while let Some(l) = self.line() {
            lines.push(l);
            if self.buf.len() > LINK_MAX_LINE && !self.buf.contains(&b'\n') {
                return None;
            }
        }
        Some(lines)
    }

    /// Bytes received after the last line read.
    pub fn rest(&self) -> &[u8] {
        &self.buf
    }
}

type Tls = TlsStream<TcpStream>;

async fn write_all(s: &mut Tls, data: &[u8]) -> bool {
    matches!(tokio::time::timeout(Duration::from_millis(WRITE_MS), s.write_all(data)).await, Ok(Ok(())))
}

/// Last message then close.
async fn send_and_end(mut s: Tls, msg: LinkMessage) {
    if write_all(&mut s, encode_link_message(&msg).as_bytes()).await {
        let _ = tokio::time::timeout(Duration::from_secs(3), s.shutdown()).await;
    }
}

fn certified(cert: &CertPair) -> Option<Arc<CertifiedKey>> {
    let c = CertificateDer::from_pem_slice(cert.cert_pem.as_bytes()).ok()?;
    let k = PrivateKeyDer::from_pem_slice(cert.key_pem.as_bytes()).ok()?;
    let signer = rustls::crypto::ring::sign::any_supported_type(&k).ok()?;
    Some(Arc::new(CertifiedKey::new(vec![c], signer)))
}

struct Resolver(Arc<LinkService>);

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Resolver")
    }
}

impl ResolvesServerCert for Resolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.0.context_for(hello.server_name())
    }
}

impl LinkService {
    pub fn new(o: LinkServiceOptions) -> Arc<Self> {
        Arc::new(Self {
            st: Mutex::new(State {
                store: o.store,
                listener: None,
                pending: None,
                finished: vec![],
                sessions: HashMap::new(),
                tunnels: HashMap::new(),
                contexts: HashMap::new(),
                default_key: None,
                servers: vec![],
                ports: vec![],
                closed: false,
            }),
            listen_lock: tokio::sync::Mutex::new(()),
            next_id: AtomicU64::new(1),
            desktop: o.desktop,
            hosts: o.hosts,
            preferred_port: o.port.unwrap_or(LINK_DEFAULT_PORT),
            now: o.now,
            hooks: o.hooks,
        })
    }

    fn id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn now(&self) -> u64 {
        (self.now)()
    }

    // ----- State -----

    pub fn port(&self) -> Option<u16> {
        self.st.lock().listener.as_ref().map(|l| l.port)
    }

    pub fn general(&self) -> bool {
        self.st.lock().store.general
    }

    pub fn desktop_id(&self) -> String {
        self.st.lock().store.desktop_id.clone()
    }

    /// Runs `f` on the store (read or tweak; call `store.save()` yourself after a change).
    pub fn with_store<R>(&self, f: impl FnOnce(&mut LinkStore) -> R) -> R {
        f(&mut self.st.lock().store)
    }

    fn status_in(st: &State, link: &StoredLink) -> LinkStatus {
        link_status(LinkStatusInput {
            general: st.store.general,
            device: link.enabled,
            connected: st.sessions.contains_key(&link.link_id),
            connecting: false,
            peer_paused: link.peer_paused,
            invalid: false,
        })
    }

    pub fn status_of(&self, link_id: &str) -> Option<LinkStatus> {
        let st = self.st.lock();
        st.store.get(link_id).map(|l| Self::status_in(&st, l))
    }

    pub fn summaries(&self) -> Vec<LinkSummary> {
        let st = self.st.lock();
        st.store.links.iter().map(|l| LinkSummary { link: l.clone(), status: Self::status_in(&st, l) }).collect()
    }

    /// Links active on both sides.
    pub fn active_links(&self) -> Vec<StoredLink> {
        let st = self.st.lock();
        st.store.links.iter().filter(|l| Self::status_in(&st, l) == LinkStatus::Connected).cloned().collect()
    }

    pub fn has_sessions(&self) -> bool {
        !self.st.lock().sessions.is_empty()
    }

    pub fn pending_offer(&self) -> Option<(PairingOffer, String)> {
        let st = self.st.lock();
        st.pending.as_ref().map(|p| self.offer_for(p, st.listener.as_ref().map(|l| l.port).unwrap_or(0)))
    }

    fn offer_for(&self, p: &Pending, port: u16) -> (PairingOffer, String) {
        let offer = PairingOffer {
            v: LINK_PROTOCOL_VERSION,
            desktop_name: (self.desktop)().name,
            link_id: p.link_id.clone(),
            cert_sha256: p.cert_sha256.clone(),
            code: p.code.clone(),
            hosts: (self.hosts)(),
            port,
            expires_at: p.expires_at,
        };
        let url = encode_pairing_url(&offer);
        (offer, url)
    }

    fn should_listen(st: &State) -> bool {
        !st.closed && st.store.general && (!st.store.links.is_empty() || st.pending.is_some())
    }

    // ----- Listening -----

    /// Brings listening (and connections) in line with the switches. Call after any change.
    pub async fn sync(self: &Arc<Self>) {
        if Self::should_listen(&self.st.lock()) {
            self.listen().await;
        } else {
            self.stop_listening().await;
        }
        self.hooks.changed();
    }

    async fn listen(self: &Arc<Self>) -> Option<u16> {
        let _guard = self.listen_lock.lock().await;
        if let Some(p) = self.st.lock().listener.as_ref().map(|l| l.port) {
            return Some(p);
        }
        let default = {
            let mut st = self.st.lock();
            if st.store.default_cert.is_none() {
                st.store.default_cert = make_default_cert().ok();
                st.store.save();
            }
            let dc = st.store.default_cert.clone()?;
            let key = certified(&dc)?;
            st.default_key = Some(key.clone());
            key
        };
        let _ = default;
        let mut config = ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .ok()?
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(Resolver(self.clone())));
        config.alpn_protocols = vec![];
        let acceptor = TlsAcceptor::from(Arc::new(config));

        let preferred = self.preferred_port;
        let v4 = match TcpListener::bind(("0.0.0.0", preferred)).await {
            Ok(l) => l,
            Err(_) if preferred != 0 => TcpListener::bind(("0.0.0.0", 0)).await.ok()?,
            Err(_) => return None,
        };
        let port = v4.local_addr().ok()?.port();
        // Also IPv6 on the same port (an error here is fine: dual-stack systems already cover it).
        let v6 = TcpListener::bind(("::", port)).await.ok();

        if !Self::should_listen(&self.st.lock()) {
            return None;
        }
        let (stop_tx, stop_rx) = watch::channel(false);
        let mut tasks = vec![];
        for l in std::iter::once(v4).chain(v6) {
            tasks.push(tokio::spawn(self.clone().accept_loop(l, acceptor.clone(), stop_rx.clone())));
        }
        self.st.lock().listener = Some(Listener { port, stop: stop_tx, tasks });
        self.hooks.listening(Some(port));
        Some(port)
    }

    async fn accept_loop(self: Arc<Self>, listener: TcpListener, acceptor: TlsAcceptor, mut stop: watch::Receiver<bool>) {
        loop {
            tokio::select! {
                _ = stop.changed() => return,
                r = listener.accept() => {
                    let Ok((tcp, _)) = r else { continue };
                    let me = self.clone();
                    let acc = acceptor.clone();
                    tokio::spawn(async move {
                        let _ = tcp.set_nodelay(true);
                        let Ok(Ok(tls)) = tokio::time::timeout(Duration::from_millis(FIRST_LINE_MS), acc.accept(tcp)).await else { return };
                        me.on_connection(tls).await;
                    });
                }
            }
        }
    }

    async fn stop_listening(self: &Arc<Self>) {
        let _guard = self.listen_lock.lock().await;
        let listener = {
            let mut st = self.st.lock();
            let Some(l) = st.listener.take() else { return };
            // Open connections are warned before closing.
            let ids: Vec<String> = st.sessions.keys().cloned().collect();
            for id in ids {
                Self::end_session(&mut st, &id, true);
            }
            for set in st.tunnels.values() {
                for t in set {
                    let _ = t.cancel.send(true);
                }
            }
            st.tunnels.clear();
            l
        };
        let _ = listener.stop.send(true);
        for t in listener.tasks {
            let _ = t.await;
        }
        self.hooks.listening(None);
    }

    fn context_for(&self, name: Option<&str>) -> Option<Arc<CertifiedKey>> {
        let mut st = self.st.lock();
        if let Some(link_id) = link_id_from_server_name(name) {
            if let Some(c) = st.contexts.get(&link_id) {
                return Some(c.clone());
            }
            let pair = match st.pending.as_ref().filter(|p| p.link_id == link_id) {
                Some(p) => Some(p.cert.clone()),
                None => st.store.get(&link_id).map(|l| CertPair { cert_pem: l.cert_pem.clone(), key_pem: l.key_pem.clone() }),
            };
            if let Some(key) = pair.as_ref().and_then(certified) {
                st.contexts.insert(link_id, key.clone());
                return Some(key);
            }
        }
        st.default_key.clone()
    }

    /// Final stop (app shutdown).
    pub async fn close(self: &Arc<Self>) {
        {
            let mut st = self.st.lock();
            st.closed = true;
            if let Some(p) = st.pending.take() {
                p.timer.abort();
            }
        }
        self.stop_listening().await;
        self.st.lock().store.flush();
    }

    // ----- Settings -----

    pub async fn set_general(self: &Arc<Self>, on: bool) {
        {
            let mut st = self.st.lock();
            if st.store.general == on {
                return;
            }
            st.store.general = on;
            st.store.save();
        }
        if !on {
            self.cancel_pairing_quiet();
        }
        self.sync().await;
    }

    pub async fn set_device(self: &Arc<Self>, link_id: &str, on: bool) {
        {
            let mut st = self.st.lock();
            let Some(link) = st.store.get_mut(link_id) else { return };
            if link.enabled == on {
                return;
            }
            link.enabled = on;
            st.store.save();
            if let Some(s) = st.sessions.get(link_id) {
                let _ = s.tx.send(Out::Msg(LinkMessage::Status { paused: !on }));
                if on {
                    self.send_servers(&mut st, link_id, true);
                }
            }
            if !on {
                Self::close_tunnels(&mut st, link_id);
            }
        }
        self.hooks.changed();
    }

    /// Deletes the link; warns the phone if it is connected.
    pub async fn forget(self: &Arc<Self>, link_id: &str) {
        {
            let mut st = self.st.lock();
            if let Some(s) = st.sessions.get(link_id) {
                let _ = s.tx.send(Out::Msg(LinkMessage::Forget));
            }
            Self::end_session(&mut st, link_id, false);
            Self::drop_link(&mut st, link_id);
        }
        self.sync().await;
    }

    fn drop_link(st: &mut State, link_id: &str) {
        st.store.remove(link_id);
        st.contexts.remove(link_id);
        Self::close_tunnels(st, link_id);
    }

    fn close_tunnels(st: &mut State, link_id: &str) {
        if let Some(set) = st.tunnels.remove(link_id) {
            for t in set {
                let _ = t.cancel.send(true);
            }
        }
    }

    // ----- Pairing -----

    /// Creates a QR code (the previous one is cancelled). None if the computer cannot listen.
    pub async fn start_pairing(self: &Arc<Self>) -> Option<(PairingOffer, String)> {
        {
            let st = self.st.lock();
            if !st.store.general || st.closed {
                return None;
            }
        }
        self.cancel_pairing_quiet();
        let link_id = hex::encode(rand_bytes::<8>());
        let cert = make_link_cert(&link_id).ok()?;
        let sha = cert_sha256(&cert.cert_pem)?;
        let expires_at = self.now() + PAIRING_TTL_MS;
        let me = self.clone();
        let lid = link_id.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(PAIRING_TTL_MS)).await;
            me.expire_pairing(&lid);
        });
        {
            let mut st = self.st.lock();
            st.pending = Some(Pending { link_id: link_id.clone(), code: b64::encode(&rand_bytes::<16>()), expires_at, cert, cert_sha256: sha, bad_codes: 0, timer });
            st.contexts.remove(&link_id);
        }
        if self.listen().await.is_none() {
            self.cancel_pairing();
            return None;
        }
        self.hooks.changed();
        self.pending_offer()
    }

    /// Cancels the QR code and brings listening in line.
    pub fn cancel_pairing(self: &Arc<Self>) {
        if !self.cancel_pairing_quiet() {
            return;
        }
        self.hooks.changed();
        let me = self.clone();
        tokio::spawn(async move { me.sync().await });
    }

    fn cancel_pairing_quiet(&self) -> bool {
        let mut st = self.st.lock();
        let Some(p) = st.pending.take() else { return false };
        p.timer.abort();
        st.contexts.remove(&p.link_id);
        true
    }

    fn finish(st: &mut State, link_id: &str, how: Finished) {
        st.finished.retain(|(id, _)| id != link_id);
        st.finished.push((link_id.to_string(), how));
        if st.finished.len() > 8 {
            st.finished.remove(0);
        }
    }

    fn expire_pairing(self: &Arc<Self>, link_id: &str) {
        {
            let mut st = self.st.lock();
            if st.pending.as_ref().map(|p| p.link_id.as_str()) != Some(link_id) {
                return;
            }
            Self::finish(&mut st, link_id, Finished::Expired);
        }
        self.cancel_pairing();
    }

    // ----- Connections -----

    async fn on_connection(self: Arc<Self>, mut tls: Tls) {
        let sni = tls.get_ref().1.server_name().map(String::from);
        let mut reader = LineReader::new();
        let first = async {
            let mut chunk = [0u8; 4096];
            loop {
                let n = tls.read(&mut chunk).await.ok()?;
                if n == 0 {
                    return None;
                }
                if !reader.add(&chunk[..n]) {
                    return Some(Err(()));
                }
                if let Some(line) = reader.line() {
                    return Some(Ok(line));
                }
            }
        };
        let line = match tokio::time::timeout(Duration::from_millis(FIRST_LINE_MS), first).await {
            Ok(Some(Ok(l))) => l,
            Ok(Some(Err(()))) => return send_and_end(tls, LinkMessage::Error { code: LinkErrorCode::BadRequest }).await,
            _ => return,
        };
        let msg = match parse_link_message(&line) {
            Some(m @ (LinkMessage::Pair { .. } | LinkMessage::HelloControl { .. } | LinkMessage::HelloTunnel { .. })) => m,
            _ => return send_and_end(tls, LinkMessage::Error { code: LinkErrorCode::BadRequest }).await,
        };
        let rest = reader.rest().to_vec();
        self.dispatch(tls, sni, msg, rest).await;
    }

    async fn dispatch(self: Arc<Self>, tls: Tls, sni: Option<String>, msg: LinkMessage, rest: Vec<u8>) {
        let (LinkMessage::Pair { v, .. } | LinkMessage::HelloControl { v, .. } | LinkMessage::HelloTunnel { v, .. }) = &msg else { return };
        if *v != LINK_PROTOCOL_VERSION {
            return send_and_end(tls, LinkMessage::Error { code: LinkErrorCode::Version }).await;
        }
        match msg {
            LinkMessage::Pair { link_id, code, device, .. } => self.on_pair(tls, sni, link_id, code, device).await,
            LinkMessage::HelloControl { link_id, token, device, .. } => match self.authenticate(&link_id, &token) {
                Ok(()) => self.on_control(tls, link_id, device, rest).await,
                Err(code) => send_and_end(tls, LinkMessage::Error { code }).await,
            },
            LinkMessage::HelloTunnel { link_id, token, port, .. } => match self.authenticate(&link_id, &token) {
                Ok(()) => self.on_tunnel(tls, link_id, port, rest).await,
                Err(code) => send_and_end(tls, LinkMessage::Error { code }).await,
            },
            _ => {}
        }
    }

    /// Checks the link and token.
    fn authenticate(&self, link_id: &str, token: &str) -> Result<(), LinkErrorCode> {
        let st = self.st.lock();
        let link = st.store.get(link_id).ok_or(LinkErrorCode::UnknownLink)?;
        if !token_matches(token, &link.token_hash) {
            return Err(LinkErrorCode::BadToken);
        }
        Ok(())
    }

    async fn on_pair(self: Arc<Self>, tls: Tls, sni: Option<String>, link_id: String, code: String, device: LinkDevice) {
        let reply: Result<LinkMessage, LinkErrorCode> = (|| {
            let mut st = self.st.lock();
            let good = st.pending.as_ref().is_some_and(|p| {
                p.link_id == link_id && is_link_id(&link_id) && link_id_from_server_name(sni.as_deref()).as_deref() == Some(link_id.as_str())
            });
            if !good {
                let done = st.finished.iter().find(|(id, _)| *id == link_id).map(|(_, h)| *h);
                return Err(match done {
                    Some(Finished::Used) => LinkErrorCode::BadCode,
                    Some(Finished::Expired) => LinkErrorCode::Expired,
                    None => LinkErrorCode::UnknownLink,
                });
            }
            let (expires_at, same) = {
                let p = st.pending.as_ref().ok_or(LinkErrorCode::UnknownLink)?;
                (p.expires_at, constant_time_eq(code.as_bytes(), p.code.as_bytes()))
            };
            if self.now() > expires_at {
                drop(st);
                self.expire_pairing(&link_id);
                return Err(LinkErrorCode::Expired);
            }
            if !same {
                let cancel = st.pending.as_mut().map(|p| {
                    p.bad_codes += 1;
                    p.bad_codes >= MAX_BAD_CODES
                });
                drop(st);
                if cancel == Some(true) {
                    self.cancel_pairing();
                }
                return Err(LinkErrorCode::BadCode);
            }
            // Good code: single use.
            let p = st.pending.take().ok_or(LinkErrorCode::UnknownLink)?;
            p.timer.abort();
            Self::finish(&mut st, &p.link_id, Finished::Used);
            let token = new_token();
            // A given phone keeps only one link (re-pairing = new link).
            let olds: Vec<String> = st.store.links.iter().filter(|l| l.peer.id == device.id).map(|l| l.link_id.clone()).collect();
            for old in olds {
                Self::end_session(&mut st, &old, false);
                Self::drop_link(&mut st, &old);
            }
            let now = self.now();
            st.store.add(StoredLink {
                link_id: p.link_id.clone(),
                peer: device.clone(),
                enabled: true,
                cert_pem: p.cert.cert_pem.clone(),
                key_pem: p.cert.key_pem.clone(),
                token_hash: hash_token(&token),
                created_at: now,
                last_seen: None,
                peer_paused: false,
            });
            Ok(LinkMessage::Paired { token, device: (self.desktop)() })
        })();
        match reply {
            Ok(m) => {
                send_and_end(tls, m).await;
                self.hooks.changed();
            }
            Err(code) => send_and_end(tls, LinkMessage::Error { code }).await,
        }
    }

    // ----- Control -----

    async fn on_control(self: Arc<Self>, mut tls: Tls, link_id: String, device: LinkDevice, rest: Vec<u8>) {
        let sid = self.id();
        let (tx, mut rx) = mpsc::unbounded_channel::<Out>();
        {
            let mut st = self.st.lock();
            // Only one control connection per link: the new one replaces the old one.
            Self::end_session(&mut st, &link_id, false);
            let now = self.now();
            let paused;
            {
                let general = st.store.general;
                let Some(link) = st.store.get_mut(&link_id) else { return };
                link.peer = LinkDevice { id: link.peer.id.clone(), name: device.name.clone(), platform: device.platform };
                link.last_seen = Some(now);
                // The hello counts as a resume on the phone side.
                link.peer_paused = false;
                paused = !(general && link.enabled);
            }
            st.store.save();
            st.sessions.insert(
                link_id.clone(),
                Session { id: sid, tx: tx.clone(), peer_paused: false, last_servers: String::new(), last_sent_at: 0, timer_pending: false },
            );
            let _ = tx.send(Out::Msg(LinkMessage::Welcome { device: (self.desktop)(), paused }));
            self.send_servers(&mut st, &link_id, true);
            self.hooks.changed();
        }

        let mut reader = LineReader::new();
        let mut alive = true;
        if !rest.is_empty() {
            alive = self.handle_chunk(&mut reader, &rest, &link_id, sid);
        }
        let mut chunk = vec![0u8; 8192];
        while alive {
            tokio::select! {
                out = rx.recv() => match out {
                    Some(Out::Msg(m)) => {
                        if !write_all(&mut tls, encode_link_message(&m).as_bytes()).await { break }
                    }
                    Some(Out::End(last)) => {
                        if let Some(m) = last {
                            write_all(&mut tls, encode_link_message(&m).as_bytes()).await;
                        }
                        let _ = tokio::time::timeout(Duration::from_secs(3), tls.shutdown()).await;
                        break;
                    }
                    None => break,
                },
                r = tokio::time::timeout(Duration::from_millis(CONTROL_IDLE_MS), tls.read(&mut chunk)) => match r {
                    Ok(Ok(n)) if n > 0 => alive = self.handle_chunk(&mut reader, &chunk[..n], &link_id, sid),
                    _ => break,
                },
            }
        }
        // Messages queued by a closing session (e.g. `forget` then End) must still go out.
        while let Ok(out) = rx.try_recv() {
            match out {
                Out::Msg(m) => {
                    write_all(&mut tls, encode_link_message(&m).as_bytes()).await;
                }
                Out::End(last) => {
                    if let Some(m) = last {
                        write_all(&mut tls, encode_link_message(&m).as_bytes()).await;
                    }
                    break;
                }
            }
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), tls.shutdown()).await;
        let mut st = self.st.lock();
        if st.sessions.get(&link_id).is_some_and(|s| s.id == sid) {
            st.sessions.remove(&link_id);
            self.hooks.changed();
        }
    }

    /// False if the connection must end.
    fn handle_chunk(self: &Arc<Self>, reader: &mut LineReader, chunk: &[u8], link_id: &str, sid: u64) -> bool {
        let Some(lines) = reader.push(chunk) else {
            let mut st = self.st.lock();
            if st.sessions.get(link_id).is_some_and(|s| s.id == sid) {
                Self::end_session(&mut st, link_id, false);
            }
            return false;
        };
        for line in lines {
            if let Some(m) = parse_link_message(&line) {
                self.on_control_message(link_id, sid, m);
            }
        }
        true
    }

    fn on_control_message(self: &Arc<Self>, link_id: &str, sid: u64, m: LinkMessage) {
        let mut st = self.st.lock();
        if !st.sessions.get(link_id).is_some_and(|s| s.id == sid) {
            return;
        }
        match m {
            LinkMessage::Ping => {
                if let Some(s) = st.sessions.get(link_id) {
                    let _ = s.tx.send(Out::Msg(LinkMessage::Pong));
                }
            }
            LinkMessage::Status { paused } => {
                let resumed = {
                    let Some(s) = st.sessions.get_mut(link_id) else { return };
                    let r = s.peer_paused && !paused;
                    s.peer_paused = paused;
                    r
                };
                if let Some(l) = st.store.get_mut(link_id) {
                    l.peer_paused = paused;
                }
                st.store.save();
                if resumed {
                    self.send_servers(&mut st, link_id, true);
                }
                self.hooks.changed();
            }
            LinkMessage::Open { url } => {
                // Accepted only if the link is active on both sides.
                let peer_paused = st.sessions.get(link_id).is_some_and(|s| s.peer_paused);
                if let Some(link) = st.store.get(link_id) {
                    if Self::status_in(&st, link) == LinkStatus::Connected && !peer_paused {
                        self.hooks.open(&url, link);
                    }
                }
            }
            LinkMessage::Forget => {
                Self::end_session(&mut st, link_id, false);
                Self::drop_link(&mut st, link_id);
                drop(st);
                let me = self.clone();
                tokio::spawn(async move { me.sync().await });
            }
            _ => {}
        }
    }

    /// Closes a control connection; `notify`: warns of a pause beforehand (listening stopped).
    fn end_session(st: &mut State, link_id: &str, notify: bool) {
        if let Some(s) = st.sessions.remove(link_id) {
            let _ = s.tx.send(Out::End(notify.then_some(LinkMessage::Status { paused: true })));
        }
    }

    // ----- Dev servers and "open on" -----

    /// Current list of dev servers; sent to active phones if it changed.
    pub fn set_servers(self: &Arc<Self>, servers: Vec<DevServer>, ports: Option<Vec<u16>>) {
        let mut st = self.st.lock();
        st.ports = ports.unwrap_or_else(|| servers.iter().map(|s| s.port).collect());
        st.servers = servers;
        let ids: Vec<String> = st.sessions.keys().cloned().collect();
        for id in ids {
            self.send_servers(&mut st, &id, false);
        }
    }

    fn send_servers(self: &Arc<Self>, st: &mut State, link_id: &str, now: bool) {
        let Some(link) = st.store.get(link_id) else { return };
        let Some(session) = st.sessions.get(link_id) else { return };
        if Self::status_in(st, link) != LinkStatus::Connected || session.peer_paused {
            return;
        }
        let json = serde_json::to_string(&(&st.servers, &st.ports)).unwrap_or_default();
        if !now && json == session.last_servers {
            return;
        }
        let t = self.now();
        let wait = (session.last_sent_at + SERVERS_THROTTLE_MS).saturating_sub(t);
        if !now && wait > 0 {
            // Too early: a single deferred send, with the list from that moment.
            let sid = session.id;
            if !session.timer_pending {
                if let Some(s) = st.sessions.get_mut(link_id) {
                    s.timer_pending = true;
                }
                let me = self.clone();
                let lid = link_id.to_string();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                    let mut st = me.st.lock();
                    if let Some(s) = st.sessions.get_mut(&lid) {
                        if s.id == sid {
                            s.timer_pending = false;
                            me.send_servers(&mut st, &lid, false);
                        }
                    }
                });
            }
            return;
        }
        let msg = LinkMessage::Servers { servers: st.servers.clone(), ports: st.ports.clone() };
        if let Some(s) = st.sessions.get_mut(link_id) {
            s.last_servers = json;
            s.last_sent_at = t;
            let _ = s.tx.send(Out::Msg(msg));
        }
    }

    /// "Open on my phone". False if this phone is not active.
    pub fn open_on_phone(&self, link_id: &str, url: &str) -> bool {
        let st = self.st.lock();
        let (Some(session), Some(link)) = (st.sessions.get(link_id), st.store.get(link_id)) else { return false };
        if Self::status_in(&st, link) != LinkStatus::Connected || session.peer_paused {
            return false;
        }
        let l = url.to_ascii_lowercase();
        if !(l.starts_with("http://") || l.starts_with("https://")) || url.chars().count() > 4096 {
            return false;
        }
        session.tx.send(Out::Msg(LinkMessage::Open { url: url.to_string() })).is_ok()
    }

    // ----- Tunnel -----

    async fn on_tunnel(self: Arc<Self>, tls: Tls, link_id: String, port: u16, rest: Vec<u8>) {
        let (own, ok) = {
            let st = self.st.lock();
            let enabled = st.store.get(&link_id).is_some_and(|l| l.enabled);
            (st.listener.as_ref().map(|l| l.port), st.store.general && enabled)
        };
        if !ok {
            return send_and_end(tls, LinkMessage::Error { code: LinkErrorCode::Paused }).await;
        }
        if Some(port) == own {
            return send_and_end(tls, LinkMessage::Error { code: LinkErrorCode::PortRefused }).await;
        }
        let mut target = None;
        for host in ["127.0.0.1", "::1"] {
            if let Ok(Ok(s)) = tokio::time::timeout(Duration::from_millis(CONNECT_MS), TcpStream::connect((host, port))).await {
                target = Some(s);
                break;
            }
        }
        let Some(mut target) = target else {
            return send_and_end(tls, LinkMessage::Error { code: LinkErrorCode::PortClosed }).await;
        };
        let _ = target.set_nodelay(true);
        let tid = self.id();
        let registered = {
            let mut st = self.st.lock();
            // The link may have been cut during the connection.
            let still = st.store.general && st.store.get(&link_id).is_some_and(|l| l.enabled);
            if still {
                let (tx, rx) = watch::channel(false);
                st.tunnels.entry(link_id.clone()).or_default().push(TunnelEntry { id: tid, cancel: tx });
                Some(rx)
            } else {
                None
            }
        };
        let Some(mut cancel_rx) = registered else {
            return send_and_end(tls, LinkMessage::Error { code: LinkErrorCode::Paused }).await;
        };
        let mut tls = tls;
        let relay = async {
            if !write_all(&mut tls, encode_link_message(&LinkMessage::Ok).as_bytes()).await {
                return;
            }
            if !rest.is_empty() && target.write_all(&rest).await.is_err() {
                return;
            }
            let _ = tokio::io::copy_bidirectional(&mut tls, &mut target).await;
        };
        tokio::select! {
            _ = relay => {}
            _ = cancel_rx.wait_for(|c| *c) => {}
        }
        let mut st = self.st.lock();
        if let Some(set) = st.tunnels.get_mut(&link_id) {
            set.retain(|t| t.id != tid);
        }
    }

    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.port().map(|p| SocketAddr::from(([127, 0, 0, 1], p)))
    }
}

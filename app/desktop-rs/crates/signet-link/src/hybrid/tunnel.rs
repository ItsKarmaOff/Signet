//! Tunnel to the phone ("hybrid" transport, caBLE v2): WebSocket to the tunnel server, Noise handshake, post-handshake
//! message (getInfo), then encrypted CTAP2 messages. Implements fido_tunnel_device.cc for a QR-initiated transaction;
//! linking information is ignored. The WebSocket is injectable (`Connector`). Checked against Chromium:
//! device/fido/cable/fido_tunnel_device.cc, websocket_adapter.cc (selected subprotocol, 1 MiB message limit, connect signal
//! frame), v2_constants.h (MessageType values), v2_handshake.cc (DecodePaddedCBORMap for protocol revision 0).
//! Nothing secret or read from the phone is ever logged.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use p256::SecretKey;
use tokio::sync::{mpsc, watch};

use super::cbor::{Cbor, decode_cbor};
use super::eid::Eid;
use super::noise::{Crypter, HandshakeInitiator, qr_psk, tunnel_id, tunnel_url};

pub const SUBPROTOCOL: &str = "fido.cable";
/// Largest message accepted from the tunnel (websocket_adapter.cc: kMaxIncomingMessageSize).
const MAX_MESSAGE: usize = 1 << 20;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

const MSG_SHUTDOWN: u8 = 0;
const MSG_CTAP: u8 = 1;
const MSG_UPDATE: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelStage {
    Connect,
    Handshake,
    PostHandshake,
    Transact,
    Closed,
    Aborted,
}

impl TunnelStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Handshake => "handshake",
            Self::PostHandshake => "post-handshake",
            Self::Transact => "transact",
            Self::Closed => "closed",
            Self::Aborted => "aborted",
        }
    }
}

/// A failure of the tunnel; `stage` says where (safe to log), `message` never carries data from the phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelError {
    pub stage: TunnelStage,
    pub message: String,
}

impl TunnelError {
    fn new(stage: TunnelStage, message: &str) -> Self {
        Self { stage, message: message.into() }
    }
}

impl std::fmt::Display for TunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.stage.as_str())
    }
}
impl std::error::Error for TunnelError {}

// ----- Abort signal -----

/// Triggers an `AbortSignal` (like an AbortController).
#[derive(Clone)]
pub struct AbortHandle(Arc<watch::Sender<bool>>);

#[derive(Clone)]
pub struct AbortSignal(watch::Receiver<bool>);

impl AbortHandle {
    pub fn new() -> (AbortHandle, AbortSignal) {
        let (tx, rx) = watch::channel(false);
        (AbortHandle(Arc::new(tx)), AbortSignal(rx))
    }
    pub fn abort(&self) {
        let _ = self.0.send(true);
    }
}

impl AbortSignal {
    pub fn is_aborted(&self) -> bool {
        *self.0.borrow()
    }
    /// Resolves when aborted (never if the handle is dropped without aborting).
    pub async fn aborted(&self) {
        let mut rx = self.0.clone();
        loop {
            if *rx.borrow() {
                return;
            }
            if rx.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }
}

async fn either_aborted(a: Option<&AbortSignal>, b: Option<&AbortSignal>) {
    let fa = async {
        match a {
            Some(s) => s.aborted().await,
            None => std::future::pending().await,
        }
    };
    let fb = async {
        match b {
            Some(s) => s.aborted().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! { _ = fa => {}, _ = fb => {} }
}

// ----- WebSocket transport -----

pub enum WsOut {
    Binary(Vec<u8>),
    /// Close with this status code (1000).
    Close(u16),
}

pub enum WsIn {
    Binary(Vec<u8>),
    /// A text frame: never valid on this tunnel.
    Text,
    Closed,
    Error,
}

/// An open WebSocket as two channels. Dropping `tx` also closes the connection.
pub struct WsChannels {
    pub tx: mpsc::UnboundedSender<WsOut>,
    pub rx: mpsc::UnboundedReceiver<WsIn>,
    /// Subprotocol the server selected (None: not reported).
    pub protocol: Option<String>,
}

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Opens the WebSocket to the tunnel server (subprotocol `fido.cable`).
pub trait Connector: Send + Sync {
    fn connect(&self, url: String, subprotocol: &'static str) -> BoxFuture<Result<WsChannels, ()>>;
}

/// The real thing: TLS (rustls, Mozilla roots) then tokio-tungstenite.
pub struct WsConnector;

impl Connector for WsConnector {
    fn connect(&self, url: String, subprotocol: &'static str) -> BoxFuture<Result<WsChannels, ()>> {
        Box::pin(async move { connect_ws(&url, subprotocol).await })
    }
}

async fn connect_ws(url: &str, subprotocol: &'static str) -> Result<WsChannels, ()> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};

    let parsed = url::Url::parse(url).map_err(|_| ())?;
    let host = parsed.host_str().ok_or(())?.to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);
    let tcp = tokio::net::TcpStream::connect((host.as_str(), port)).await.map_err(|_| ())?;
    let _ = tcp.set_nodelay(true);

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|_| ())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = rustls_pki_types::ServerName::try_from(host.clone()).map_err(|_| ())?;
    let tls = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await.map_err(|_| ())?;

    let mut request = url.into_client_request().map_err(|_| ())?;
    request.headers_mut().insert("Sec-WebSocket-Protocol", subprotocol.parse().map_err(|_| ())?);
    let ws_config = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
    let (mut ws, response) = tokio_tungstenite::client_async_with_config(request, tls, Some(ws_config)).await.map_err(|_| ())?;
    let protocol = response.headers().get("sec-websocket-protocol").and_then(|v| v.to_str().ok()).map(String::from);

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<WsOut>();
    let (in_tx, in_rx) = mpsc::unbounded_channel::<WsIn>();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                incoming = ws.next() => match incoming {
                    Some(Ok(Message::Binary(b))) => { let _ = in_tx.send(WsIn::Binary(b.to_vec())); }
                    Some(Ok(Message::Text(_))) => { let _ = in_tx.send(WsIn::Text); }
                    Some(Ok(Message::Close(_))) | None => { let _ = in_tx.send(WsIn::Closed); break }
                    Some(Ok(_)) => {}
                    Some(Err(_)) => { let _ = in_tx.send(WsIn::Error); break }
                },
                out = out_rx.recv() => match out {
                    Some(WsOut::Binary(b)) => { if ws.send(Message::Binary(b.into())).await.is_err() { let _ = in_tx.send(WsIn::Error); break } }
                    Some(WsOut::Close(_)) | None => { let _ = ws.close(None).await; break }
                },
            }
        }
    });
    Ok(WsChannels { tx: out_tx, rx: in_rx, protocol })
}

// ----- Tunnel -----

pub type LogFn = Arc<dyn Fn(&str) + Send + Sync>;

pub struct TunnelOptions {
    pub eid: Eid,
    /// QR secret (16 bytes).
    pub secret: [u8; 16],
    /// The identity key pair of the QR code.
    pub identity: SecretKey,
    /// Closes the tunnel and fails whatever is pending when aborted.
    pub signal: Option<AbortSignal>,
    pub connector: Arc<dyn Connector>,
    pub connect_timeout: Option<Duration>,
    pub handshake_timeout: Option<Duration>,
    /// Development log of the stages (no data).
    pub log: Option<LogFn>,
}

/// Padding of revision 0 post-handshake messages: the last 2 bytes (little-endian) or the last byte count the padding.
fn decode_padded_map(buf: &[u8]) -> Option<Cbor> {
    for size in [2usize, 1] {
        if buf.len() < size {
            continue;
        }
        let padding = if size == 2 { u16::from_le_bytes([buf[buf.len() - 2], buf[buf.len() - 1]]) as usize } else { buf[buf.len() - 1] as usize };
        if padding + size > buf.len() {
            continue;
        }
        if let Ok(v) = decode_cbor(&buf[..buf.len() - padding - size]) {
            if v.as_map().is_some() {
                return Some(v);
            }
        }
    }
    None
}

struct PostHandshake {
    revision: u8,
    get_info: Vec<u8>,
}

/// The post-handshake message: a CBOR map {1: getInfo bytes, 2: linking info (ignored), 3: features}.
fn parse_post_handshake(plaintext: &[u8]) -> Option<PostHandshake> {
    let (revision, payload) = match decode_cbor(plaintext) {
        Ok(v) => (1u8, Some(v)),
        Err(_) => (0u8, decode_padded_map(plaintext)),
    };
    let payload = payload?;
    payload.as_map()?;
    let mut ctap = true;
    if let Some(features) = payload.geti(3) {
        ctap = features.as_array()?.iter().any(|f| f.as_text() == Some("ctap"));
    }
    if !ctap {
        return None; // only CTAP2 is used here
    }
    let get_info = payload.geti(1)?.as_bytes()?.to_vec();
    if let Some(linking) = payload.geti(2) {
        linking.as_map()?;
    }
    Some(PostHandshake { revision, get_info })
}

pub struct Tunnel {
    /// The phone's getInfo response (CTAP2 CBOR), from the post-handshake message.
    pub get_info: Vec<u8>,
    tx: mpsc::UnboundedSender<WsOut>,
    rx: mpsc::UnboundedReceiver<WsIn>,
    crypter: Option<Crypter>,
    revision: u8,
    ready: bool,
    closed: bool,
    signal: Option<AbortSignal>,
}

impl Tunnel {
    fn shutdown(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        // A polite end for the phone (revision 1 and later), then the socket.
        if self.ready && self.revision >= 1 {
            if let Some(c) = self.crypter.as_mut() {
                if let Some(m) = c.encrypt(&[MSG_SHUTDOWN]) {
                    let _ = self.tx.send(WsOut::Binary(m));
                }
            }
        }
        let _ = self.tx.send(WsOut::Close(1000));
    }

    /// The next binary message; fails on timeout, abort, a closed tunnel or a bad frame. Any failure closes the tunnel.
    async fn next(&mut self, stage: TunnelStage, timeout: Duration, extra: Option<&AbortSignal>) -> Result<Vec<u8>, TunnelError> {
        if self.closed {
            return Err(TunnelError::new(TunnelStage::Closed, "closed"));
        }
        let own = self.signal.clone();
        let result = tokio::select! {
            ev = self.rx.recv() => match ev {
                Some(WsIn::Binary(d)) if d.len() > MAX_MESSAGE => Err(TunnelError::new(TunnelStage::Closed, "message too large")),
                Some(WsIn::Binary(d)) => Ok(d),
                Some(WsIn::Text) => Err(TunnelError::new(TunnelStage::Closed, "unexpected text message")),
                Some(WsIn::Closed) | None => Err(TunnelError::new(TunnelStage::Closed, "connection closed")),
                Some(WsIn::Error) => Err(TunnelError::new(TunnelStage::Closed, "connection error")),
            },
            _ = tokio::time::sleep(timeout) => Err(TunnelError::new(stage, "timed out")),
            _ = either_aborted(own.as_ref(), extra) => Err(TunnelError::new(TunnelStage::Aborted, "aborted")),
        };
        if result.is_err() {
            self.shutdown();
        }
        result
    }

    /// Sends a CTAP2 command (command byte + CBOR) and resolves with the response (status byte + CBOR).
    /// One request at a time (enforced by `&mut self`). Any failure ends the tunnel.
    pub async fn transact(&mut self, request: &[u8], signal: Option<&AbortSignal>) -> Result<Vec<u8>, TunnelError> {
        if self.closed {
            return Err(TunnelError::new(TunnelStage::Closed, "closed"));
        }
        let message = if self.revision >= 1 { [&[MSG_CTAP][..], request].concat() } else { request.to_vec() };
        let sealed = self.crypter.as_mut().and_then(|c| c.encrypt(&message));
        let Some(sealed) = sealed else {
            self.shutdown();
            return Err(TunnelError::new(TunnelStage::Transact, "sequence number exhausted"));
        };
        let _ = self.tx.send(WsOut::Binary(sealed));
        loop {
            // Bounded by the signal (the session timeout); an hour is only the ceiling of the timer.
            let data = self.next(TunnelStage::Transact, Duration::from_secs(3600), signal).await?;
            let plaintext = self.crypter.as_mut().and_then(|c| c.decrypt(&data));
            let Some(plaintext) = plaintext else {
                self.shutdown();
                return Err(TunnelError::new(TunnelStage::Transact, "decryption failed"));
            };
            if self.revision < 1 {
                return Ok(plaintext);
            }
            let fail = |t: &mut Tunnel, m: &str| {
                t.shutdown();
                Err(TunnelError::new(TunnelStage::Transact, m))
            };
            let Some(&ty) = plaintext.first() else { return fail(self, "empty message") };
            match ty {
                MSG_CTAP => return Ok(plaintext[1..].to_vec()),
                MSG_UPDATE => {
                    // Linking information: not used, but it must be a map.
                    match decode_cbor(&plaintext[1..]) {
                        Ok(v) if v.as_map().is_some() => continue,
                        _ => return fail(self, "invalid update message"),
                    }
                }
                _ => return fail(self, "unexpected message type"),
            }
        }
    }

    /// Tells the phone (kShutdown) and closes the socket. Safe to call twice.
    pub fn close(&mut self) {
        self.shutdown();
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Connects to the tunnel server, runs the handshake with the phone and reads its post-handshake message. Resolves once the
/// phone is ready for CTAP2; fails with a TunnelError otherwise (the socket is closed then).
pub async fn open_tunnel(opts: TunnelOptions) -> Result<Tunnel, TunnelError> {
    let log = |line: &str| {
        if let Some(l) = &opts.log {
            l(line);
        }
    };
    if opts.signal.as_ref().is_some_and(AbortSignal::is_aborted) {
        return Err(TunnelError::new(TunnelStage::Aborted, "aborted"));
    }
    let url = tunnel_url(&opts.eid.tunnel_domain, &opts.eid.routing_id, &tunnel_id(&opts.secret));
    log(&format!("tunnel: connecting to {}", opts.eid.tunnel_domain));

    let connect_timeout = opts.connect_timeout.unwrap_or(CONNECT_TIMEOUT);
    let connected = tokio::select! {
        r = tokio::time::timeout(connect_timeout, opts.connector.connect(url, SUBPROTOCOL)) => match r {
            Ok(Ok(ws)) => Ok(ws),
            Ok(Err(())) => Err(TunnelError::new(TunnelStage::Connect, "connection refused or closed")),
            Err(_) => Err(TunnelError::new(TunnelStage::Connect, "timed out")),
        },
        _ = async { match &opts.signal { Some(s) => s.aborted().await, None => std::future::pending().await } } => Err(TunnelError::new(TunnelStage::Aborted, "aborted")),
    };
    let ws = connected?;
    let mut tunnel = Tunnel { get_info: vec![], tx: ws.tx, rx: ws.rx, crypter: None, revision: 1, ready: false, closed: false, signal: opts.signal.clone() };
    if ws.protocol.as_deref().is_some_and(|p| p != SUBPROTOCOL) {
        tunnel.shutdown();
        return Err(TunnelError::new(TunnelStage::Connect, "unexpected subprotocol"));
    }
    log("tunnel: connected, handshake");

    let mut handshake = HandshakeInitiator::new(qr_psk(&opts.secret, &opts.eid.plaintext), opts.identity.clone());
    let _ = tunnel.tx.send(WsOut::Binary(handshake.build_initial_message()));

    let timeout = opts.handshake_timeout.unwrap_or(HANDSHAKE_TIMEOUT);
    let mut response = tunnel.next(TunnelStage::Handshake, timeout, None).await?;
    // A server that supports it first sends a one-byte zero frame when the phone has connected (never a valid response).
    if response.len() == 1 && response[0] == 0 {
        response = tunnel.next(TunnelStage::Handshake, timeout, None).await?;
    }
    let Some(result) = handshake.process_response(&response) else {
        tunnel.shutdown();
        return Err(TunnelError::new(TunnelStage::Handshake, "handshake failed"));
    };
    tunnel.crypter = Some(result.crypter);
    log("tunnel: handshake done");

    let encrypted = tunnel.next(TunnelStage::PostHandshake, timeout, None).await?;
    let plain = tunnel.crypter.as_mut().and_then(|c| c.decrypt(&encrypted));
    let Some(post) = plain.as_deref().and_then(parse_post_handshake) else {
        tunnel.shutdown();
        return Err(TunnelError::new(TunnelStage::PostHandshake, "invalid post-handshake message"));
    };
    tunnel.revision = post.revision;
    tunnel.get_info = post.get_info;
    tunnel.ready = true;
    log(&format!("tunnel: ready (protocol revision {})", post.revision));
    Ok(tunnel)
}

#[cfg(test)]
pub(crate) mod testing {
    //! The phone's side of the tunnel, behind a fake tunnel server (a `Connector`).
    use super::*;
    use crate::hybrid::cbor::encode_cbor;
    use crate::hybrid::noise::testing::respond_to_handshake;
    use parking_lot::Mutex;

    #[derive(Clone)]
    pub struct FakePhoneOptions {
        pub secret: [u8; 16],
        pub eid: Eid,
        /// Uncompressed public key of the QR identity.
        pub identity: Vec<u8>,
        pub get_info: Vec<u8>,
        /// Replaces the post-handshake message (plaintext before padding); revision 0 pads it the old way.
        pub post_handshake: Option<Vec<u8>>,
        pub revision: u8,
        pub connect_signal: bool,
        /// CTAP commands received; the return value is the response (status byte + CBOR), None: no answer.
        pub on_request: Option<Arc<dyn Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync>>,
        pub silent: bool,
        pub refuse: bool,
        pub hang: bool,
        pub protocol: Option<String>,
    }

    impl FakePhoneOptions {
        pub fn new(secret: [u8; 16], eid: Eid, identity: Vec<u8>, get_info: Vec<u8>) -> Self {
            Self {
                secret,
                eid,
                identity,
                get_info,
                post_handshake: None,
                revision: 1,
                connect_signal: false,
                on_request: None,
                silent: false,
                refuse: false,
                hang: false,
                protocol: None,
            }
        }
    }

    #[derive(Default)]
    pub struct PhoneState {
        pub urls: Vec<String>,
        pub requested_protocols: Vec<String>,
        pub sent: Vec<Vec<u8>>,
        pub requests: Vec<Vec<u8>>,
        pub shutdowns: usize,
        pub closed: usize,
        pub close_code: Option<u16>,
        pub bad_messages: usize,
        crypter: Option<Crypter>,
        inject: Vec<mpsc::UnboundedSender<WsIn>>,
    }

    pub struct FakePhone {
        pub options: FakePhoneOptions,
        pub state: Arc<Mutex<PhoneState>>,
    }

    impl FakePhone {
        pub fn new(options: FakePhoneOptions) -> Arc<Self> {
            Arc::new(Self { options, state: Arc::new(Mutex::new(PhoneState::default())) })
        }

        pub fn expected_url(&self) -> String {
            tunnel_url(&self.options.eid.tunnel_domain, &self.options.eid.routing_id, &tunnel_id(&self.options.secret))
        }

        /// A raw message from the server or phone.
        pub fn deliver(&self, ev: WsIn) {
            if let Some(tx) = self.state.lock().inject.last() {
                let _ = tx.send(ev);
            }
        }

        /// Encrypted message from the phone (type byte for revision 1).
        pub fn say(&self, ty: Option<u8>, payload: &[u8]) {
            let plain = match ty {
                Some(t) => [&[t][..], payload].concat(),
                None => payload.to_vec(),
            };
            let sealed = self.state.lock().crypter.as_mut().and_then(|c| c.encrypt(&plain));
            if let Some(s) = sealed {
                self.deliver(WsIn::Binary(s));
            }
        }

        /// The connection drops.
        pub fn drop_connection(&self) {
            self.deliver(WsIn::Closed);
        }

        pub fn sent_count(&self) -> usize {
            self.state.lock().sent.len()
        }
    }

    impl Connector for Arc<FakePhone> {
        fn connect(&self, url: String, subprotocol: &'static str) -> BoxFuture<Result<WsChannels, ()>> {
            let phone = self.clone();
            Box::pin(async move {
                {
                    let mut st = phone.state.lock();
                    st.urls.push(url);
                    st.requested_protocols.push(subprotocol.into());
                }
                if phone.options.hang {
                    std::future::pending::<()>().await;
                }
                if phone.options.refuse {
                    return Err(());
                }
                let (out_tx, mut out_rx) = mpsc::unbounded_channel::<WsOut>();
                let (in_tx, in_rx) = mpsc::unbounded_channel::<WsIn>();
                phone.state.lock().inject.push(in_tx.clone());
                let p = phone.clone();
                tokio::spawn(async move {
                    while let Some(out) = out_rx.recv().await {
                        match out {
                            WsOut::Close(code) => {
                                let mut st = p.state.lock();
                                st.closed += 1;
                                st.close_code = Some(code);
                            }
                            WsOut::Binary(data) => p.on_message(data, &in_tx),
                        }
                    }
                });
                Ok(WsChannels { tx: out_tx, rx: in_rx, protocol: Some(phone.options.protocol.clone().unwrap_or_else(|| subprotocol.into())) })
            })
        }
    }

    impl FakePhone {
        fn on_message(&self, data: Vec<u8>, in_tx: &mpsc::UnboundedSender<WsIn>) {
            let o = &self.options;
            let first = {
                let mut st = self.state.lock();
                st.sent.push(data.clone());
                st.crypter.is_none()
            };
            if o.silent {
                return;
            }
            if first {
                let psk = qr_psk(&o.secret, &o.eid.plaintext);
                let Some(r) = respond_to_handshake(&psk, &o.identity, &data) else { return };
                self.state.lock().crypter = Some(r.crypter);
                if o.connect_signal {
                    let _ = in_tx.send(WsIn::Binary(vec![0]));
                }
                let _ = in_tx.send(WsIn::Binary(r.response));
                let body = o.post_handshake.clone().unwrap_or_else(|| encode_cbor(&Cbor::map(vec![(Cbor::int(1), Cbor::Bytes(o.get_info.clone()))])).unwrap());
                if o.revision == 1 {
                    self.say(None, &body);
                } else {
                    // Revision 0: padded to 512 bytes, the last two bytes (little-endian) count the padding.
                    let mut padded = vec![0u8; 512];
                    padded[..body.len()].copy_from_slice(&body);
                    padded[510..].copy_from_slice(&((512 - body.len() - 2) as u16).to_le_bytes());
                    self.say(None, &padded);
                }
                return;
            }
            let plain = self.state.lock().crypter.as_mut().and_then(|c| c.decrypt(&data));
            let Some(plain) = plain else {
                self.state.lock().bad_messages += 1;
                return;
            };
            if o.revision == 0 {
                return self.command(&plain, None);
            }
            match plain.first() {
                Some(0) => self.state.lock().shutdowns += 1,
                Some(1) => self.command(&plain[1..], Some(1)),
                _ => {}
            }
        }

        fn command(&self, command: &[u8], ty: Option<u8>) {
            self.state.lock().requests.push(command.to_vec());
            if let Some(reply) = self.options.on_request.as_ref().and_then(|f| f(command)) {
                self.say(ty, &reply);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::hybrid::cbor::encode_cbor;
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    use rand_core::OsRng;

    fn seq(start: u8, n: usize) -> Vec<u8> {
        (0..n).map(|i| start.wrapping_add(i as u8)).collect()
    }

    fn eid() -> Eid {
        Eid {
            plaintext: seq(16, 16).try_into().unwrap(),
            nonce: seq(1, 10).try_into().unwrap(),
            routing_id: [0xaa, 0xbb, 0xcc],
            tunnel_domain_id: 0,
            tunnel_domain: "cable.ua5v.com".into(),
        }
    }
    fn secret() -> [u8; 16] {
        seq(0, 16).try_into().unwrap()
    }
    fn get_info() -> Vec<u8> {
        encode_cbor(&Cbor::map(vec![(Cbor::int(1), Cbor::Array(vec![Cbor::text("FIDO_2_0")]))])).unwrap()
    }

    struct Setup {
        phone: Arc<FakePhone>,
        identity: SecretKey,
    }

    fn setup(f: impl FnOnce(&mut FakePhoneOptions)) -> Setup {
        let identity = SecretKey::random(&mut OsRng);
        let mut o = FakePhoneOptions::new(secret(), eid(), identity.public_key().to_encoded_point(false).as_bytes().to_vec(), get_info());
        f(&mut o);
        Setup { phone: FakePhone::new(o), identity }
    }

    impl Setup {
        fn opts(&self, signal: Option<AbortSignal>) -> TunnelOptions {
            TunnelOptions {
                eid: eid(),
                secret: secret(),
                identity: self.identity.clone(),
                signal,
                connector: Arc::new(self.phone.clone()),
                connect_timeout: None,
                handshake_timeout: None,
                log: None,
            }
        }
        async fn open(&self) -> Result<Tunnel, TunnelError> {
            open_tunnel(self.opts(None)).await
        }
    }

    async fn settle() {
        for _ in 0..5 {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    fn reply(v: Vec<u8>) -> Option<Arc<dyn Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync>> {
        Some(Arc::new(move |_| Some(v.clone())))
    }
    fn never() -> Option<Arc<dyn Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync>> {
        Some(Arc::new(|_| None))
    }

    #[tokio::test(start_paused = true)]
    async fn connects_with_the_subprotocol_and_reads_get_info() {
        let s = setup(|_| {});
        let t = s.open().await.unwrap();
        let st = s.phone.state.lock();
        assert_eq!(st.urls[0], s.phone.expected_url());
        assert!(st.urls[0].starts_with("wss://cable.ua5v.com/cable/connect/AABBCC/"));
        assert_eq!(st.requested_protocols[0], "fido.cable");
        assert_eq!(t.get_info, get_info());
        assert_eq!(st.bad_messages, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn exchanges_a_ctap_request_and_response() {
        let s = setup(|o| o.on_request = reply(vec![0, 0xa1, 1, 2]));
        let mut t = s.open().await.unwrap();
        assert_eq!(t.transact(&[2, 0xa0], None).await.unwrap(), vec![0, 0xa1, 1, 2]);
        let st = s.phone.state.lock();
        assert_eq!(st.requests, vec![vec![2, 0xa0]]);
        assert_eq!((st.sent[1].len() - 16) % 32, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn allows_several_requests_in_a_row() {
        let s = setup(|o| o.on_request = Some(Arc::new(|c| Some([&[0u8][..], c].concat()))));
        let mut t = s.open().await.unwrap();
        assert_eq!(t.transact(&[1], None).await.unwrap(), vec![0, 1]);
        assert_eq!(t.transact(&[2], None).await.unwrap(), vec![0, 2]);
    }

    #[tokio::test(start_paused = true)]
    async fn skips_the_connect_signal() {
        assert!(setup(|o| o.connect_signal = true).open().await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn ignores_linking_info_and_extra_keys() {
        let post = encode_cbor(&Cbor::map(vec![
            (Cbor::int(1), Cbor::bytes(&[0xa0])),
            (Cbor::int(2), Cbor::map(vec![(Cbor::int(1), Cbor::text("x"))])),
            (Cbor::int(3), Cbor::Array(vec![Cbor::text("ctap"), Cbor::text("dc"), Cbor::text("other")])),
            (Cbor::int(9), Cbor::text("x")),
        ]))
        .unwrap();
        let t = setup(|o| o.post_handshake = Some(post)).open().await.unwrap();
        assert_eq!(t.get_info, vec![0xa0]);
    }

    #[tokio::test(start_paused = true)]
    async fn supports_protocol_revision_0() {
        let s = setup(|o| {
            o.revision = 0;
            o.on_request = reply(vec![0, 0xa0]);
        });
        let mut t = s.open().await.unwrap();
        assert_eq!(t.transact(&[1, 2], None).await.unwrap(), vec![0, 0xa0]);
        assert_eq!(s.phone.state.lock().requests, vec![vec![1, 2]]);
    }

    #[tokio::test(start_paused = true)]
    async fn rejects_bad_post_handshake_messages() {
        let bad: Vec<(&str, Vec<u8>)> = vec![
            ("not a map", vec![0x01]),
            ("not CBOR", vec![0xff, 0xff, 0xff]),
            ("missing getInfo", encode_cbor(&Cbor::map(vec![(Cbor::int(3), Cbor::Array(vec![Cbor::text("ctap")]))])).unwrap()),
            ("getInfo not bytes", encode_cbor(&Cbor::map(vec![(Cbor::int(1), Cbor::text("text"))])).unwrap()),
            ("features without ctap", encode_cbor(&Cbor::map(vec![(Cbor::int(1), Cbor::bytes(&[0xa0])), (Cbor::int(3), Cbor::Array(vec![Cbor::text("dc")]))])).unwrap()),
            ("features not an array", encode_cbor(&Cbor::map(vec![(Cbor::int(1), Cbor::bytes(&[0xa0])), (Cbor::int(3), Cbor::text("ctap"))])).unwrap()),
            ("linking info not a map", encode_cbor(&Cbor::map(vec![(Cbor::int(1), Cbor::bytes(&[0xa0])), (Cbor::int(2), Cbor::int(5))])).unwrap()),
        ];
        for (name, post) in bad {
            let s = setup(|o| o.post_handshake = Some(post));
            let e = s.open().await.err().unwrap_or_else(|| panic!("{name}: accepted"));
            assert_eq!(e.stage, TunnelStage::PostHandshake, "{name}");
            settle().await;
            assert_eq!(s.phone.state.lock().closed, 1, "{name}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn refused_connection() {
        assert_eq!(setup(|o| o.refuse = true).open().await.err().unwrap().stage, TunnelStage::Connect);
    }

    #[tokio::test(start_paused = true)]
    async fn connection_that_never_opens_times_out_after_15s() {
        let s = setup(|o| o.hang = true);
        let start = tokio::time::Instant::now();
        let e = s.open().await.err().unwrap();
        assert_eq!(e.stage, TunnelStage::Connect);
        assert_eq!(start.elapsed(), Duration::from_secs(15));
    }

    #[tokio::test(start_paused = true)]
    async fn another_subprotocol_is_refused() {
        let s = setup(|o| o.protocol = Some("other".into()));
        assert_eq!(s.open().await.err().unwrap().stage, TunnelStage::Connect);
        settle().await;
        assert_eq!(s.phone.state.lock().closed, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn no_answer_to_the_handshake_times_out_after_15s() {
        let s = setup(|o| o.silent = true);
        let start = tokio::time::Instant::now();
        let e = s.open().await.err().unwrap();
        assert_eq!(e.stage, TunnelStage::Handshake);
        assert_eq!(start.elapsed(), Duration::from_secs(15));
        settle().await;
        assert_eq!(s.phone.state.lock().closed, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn phone_that_does_not_know_the_psk_never_answers() {
        let s = setup(|o| o.secret = seq(50, 16).try_into().unwrap());
        let e = s.open().await.err().unwrap();
        assert_eq!(e.stage, TunnelStage::Handshake);
        assert_eq!(s.phone.sent_count(), 1);
    }

    /// Opens while a driver injects something once the handshake message was sent.
    async fn open_with(s: &Setup, drive: impl FnOnce(&FakePhone)) -> Result<Tunnel, TunnelError> {
        let open = s.open();
        tokio::pin!(open);
        tokio::select! { _ = &mut open => panic!("opened too early"), _ = tokio::time::sleep(Duration::from_millis(5)) => {} }
        drive(&s.phone);
        open.await
    }

    #[tokio::test(start_paused = true)]
    async fn invalid_handshake_response() {
        let s = setup(|o| o.silent = true);
        let e = open_with(&s, |p| p.deliver(WsIn::Binary(vec![7; 81]))).await.err().unwrap();
        assert_eq!(e.stage, TunnelStage::Handshake);
        settle().await;
        assert_eq!(s.phone.state.lock().closed, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn connection_dropped_during_the_handshake() {
        let s = setup(|o| o.silent = true);
        let e = open_with(&s, |p| p.drop_connection()).await.err().unwrap();
        assert_eq!(e.stage, TunnelStage::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn text_message_ends_the_tunnel() {
        let s = setup(|o| o.silent = true);
        assert!(open_with(&s, |p| p.deliver(WsIn::Text)).await.is_err());
        settle().await;
        assert_eq!(s.phone.state.lock().closed, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn message_over_1_mib_ends_the_tunnel() {
        let s = setup(|o| o.silent = true);
        assert!(open_with(&s, |p| p.deliver(WsIn::Binary(vec![0; (1 << 20) + 1]))).await.is_err());
        settle().await;
        assert_eq!(s.phone.state.lock().closed, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn abort_before_connecting_never_opens_a_socket() {
        let s = setup(|_| {});
        let (h, sig) = AbortHandle::new();
        h.abort();
        let e = open_tunnel(s.opts(Some(sig))).await.err().unwrap();
        assert_eq!(e.stage, TunnelStage::Aborted);
        assert!(s.phone.state.lock().urls.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn abort_during_the_handshake_closes_the_socket() {
        let s = setup(|o| o.silent = true);
        let (h, sig) = AbortHandle::new();
        let open = open_tunnel(s.opts(Some(sig)));
        tokio::pin!(open);
        tokio::select! { _ = &mut open => panic!(), _ = tokio::time::sleep(Duration::from_millis(5)) => {} }
        h.abort();
        let e = open.await.err().unwrap();
        assert_eq!(e.stage, TunnelStage::Aborted);
        settle().await;
        let st = s.phone.state.lock();
        assert_eq!(st.closed, 1);
        assert_eq!(st.shutdowns, 0); // no kShutdown before the tunnel is ready
        assert_eq!(st.sent.len(), 1);
    }

    // ----- transact -----

    async fn pending_transact(s: &Setup) -> (Tunnel, AbortHandle, AbortSignal) {
        let t = s.open().await.unwrap();
        let (h, sig) = AbortHandle::new();
        (t, h, sig)
    }

    #[tokio::test(start_paused = true)]
    async fn update_messages_before_the_answer_are_ignored() {
        let s = setup(|o| o.on_request = never());
        let mut t = s.open().await.unwrap();
        let phone = s.phone.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            phone.say(Some(2), &encode_cbor(&Cbor::map(vec![(Cbor::int(1), Cbor::map(vec![]))])).unwrap());
            phone.say(Some(1), &[0, 0xa0]);
        });
        assert_eq!(t.transact(&[1], None).await.unwrap(), vec![0, 0xa0]);
    }

    async fn transact_with(s: &Setup, drive: impl FnOnce(&FakePhone) + Send + 'static) -> TunnelError {
        let mut t = s.open().await.unwrap();
        let phone = s.phone.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            drive(&phone);
        });
        t.transact(&[1], None).await.unwrap_err()
    }

    #[tokio::test(start_paused = true)]
    async fn invalid_update_message_ends_the_tunnel() {
        let s = setup(|o| o.on_request = never());
        let e = transact_with(&s, |p| p.say(Some(2), &[0xff])).await;
        assert_eq!(e.stage, TunnelStage::Transact);
        settle().await;
        assert_eq!(s.phone.state.lock().closed, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_unknown_type_and_empty_message_end_the_tunnel() {
        for ty in [0u8, 3, 9] {
            let s = setup(|o| o.on_request = never());
            assert_eq!(transact_with(&s, move |p| p.say(Some(ty), &[0])).await.stage, TunnelStage::Transact, "type {ty}");
        }
        let s = setup(|o| o.on_request = never());
        assert_eq!(transact_with(&s, |p| p.say(None, &[])).await.stage, TunnelStage::Transact);
    }

    #[tokio::test(start_paused = true)]
    async fn message_that_does_not_decrypt_ends_the_tunnel() {
        let s = setup(|o| o.on_request = never());
        let e = transact_with(&s, |p| p.deliver(WsIn::Binary(vec![1; 48]))).await;
        assert_eq!(e.stage, TunnelStage::Transact);
        settle().await;
        assert_eq!(s.phone.state.lock().closed, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn phone_dropping_the_connection() {
        let s = setup(|o| o.on_request = never());
        assert_eq!(transact_with(&s, |p| p.drop_connection()).await.stage, TunnelStage::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn bounded_by_the_given_signal_and_closes_the_tunnel() {
        let s = setup(|o| o.on_request = never());
        let (mut t, h, sig) = pending_transact(&s).await;
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            h.abort();
        });
        let e = t.transact(&[1], Some(&sig)).await.unwrap_err();
        assert_eq!(e.stage, TunnelStage::Aborted);
        settle().await;
        let st = s.phone.state.lock();
        assert_eq!(st.closed, 1);
        assert_eq!(st.shutdowns, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn bounded_by_the_tunnel_signal_too() {
        let s = setup(|o| o.on_request = never());
        let (h, sig) = AbortHandle::new();
        let mut t = open_tunnel(s.opts(Some(sig))).await.unwrap();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(5)).await;
            h.abort();
        });
        assert!(t.transact(&[1], None).await.is_err());
        settle().await;
        assert_eq!(s.phone.state.lock().closed, 1);
    }

    // ----- close -----

    #[tokio::test(start_paused = true)]
    async fn close_tells_the_phone_and_closes_once() {
        let s = setup(|_| {});
        let mut t = s.open().await.unwrap();
        t.close();
        t.close();
        settle().await;
        let st = s.phone.state.lock();
        assert_eq!(st.shutdowns, 1);
        assert_eq!(st.closed, 1);
        assert_eq!(st.close_code, Some(1000));
    }

    #[tokio::test(start_paused = true)]
    async fn requests_after_close_fail() {
        let s = setup(|_| {});
        let mut t = s.open().await.unwrap();
        t.close();
        assert_eq!(t.transact(&[1], None).await.unwrap_err().stage, TunnelStage::Closed);
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_the_tunnel_closes_it() {
        let s = setup(|_| {});
        drop(s.open().await.unwrap());
        settle().await;
        assert_eq!(s.phone.state.lock().shutdowns, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn logs_stages_without_data() {
        let s = setup(|_| {});
        let lines = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
        let l2 = lines.clone();
        let mut o = s.opts(None);
        o.log = Some(Arc::new(move |l| l2.lock().push(l.to_string())));
        open_tunnel(o).await.unwrap();
        let all = lines.lock().join("\n");
        assert!(lines.lock().len() > 2);
        for sec in [hex::encode(secret()), hex::encode(eid().plaintext), hex::encode(eid().nonce)] {
            assert!(!all.contains(&sec));
        }
    }
}

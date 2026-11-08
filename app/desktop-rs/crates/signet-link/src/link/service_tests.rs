//! Port of service.test.ts: a real TLS client on loopback plays the phone.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerifier, ServerCertVerified};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use super::protocol::*;
use super::service::*;
use super::status::LinkStatus;
use super::store::{LinkStore, NoVault, StoredLink, hash_token};
use crate::b64;

#[derive(Debug)]
struct AcceptAll;
impl ServerCertVerifier for AcceptAll {
    fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &rustls::crypto::ring::default_provider().signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

fn desktop() -> LinkDevice {
    LinkDevice { id: "desk-1".into(), name: "Mon Mac".into(), platform: LinkPlatform::Macos }
}
fn phone_dev() -> LinkDevice {
    LinkDevice { id: "phone-1".into(), name: "iPhone de test".into(), platform: LinkPlatform::Ios }
}

/// A phone's connection: TLS with the link's SNI, certificate fingerprint captured.
struct Phone {
    stream: TlsStream<TcpStream>,
    reader: LineReader,
    queue: VecDeque<LinkMessage>,
    raw: Vec<u8>,
    closed: bool,
    fingerprint: String,
}

impl Phone {
    async fn open(port: u16, link_id: &str) -> std::io::Result<Phone> {
        let cfg = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAll))
            .with_no_client_auth();
        let tcp = TcpStream::connect(("127.0.0.1", port)).await?;
        let name = ServerName::try_from(server_name_for_link(link_id)).unwrap();
        let stream = TlsConnector::from(Arc::new(cfg)).connect(name, tcp).await?;
        let fingerprint = {
            let certs = stream.get_ref().1.peer_certificates().unwrap();
            use sha2::{Digest, Sha256};
            b64::encode(&Sha256::digest(certs[0].as_ref()))
        };
        Ok(Phone { stream, reader: LineReader::new(), queue: VecDeque::new(), raw: vec![], closed: false, fingerprint })
    }

    async fn write(&mut self, data: &[u8]) {
        let _ = self.stream.write_all(data).await;
    }

    async fn send(&mut self, m: LinkMessage) {
        self.write(encode_link_message(&m).as_bytes()).await;
    }

    /// Next message; None if the connection closes before. Panics after the timeout.
    async fn next(&mut self) -> Option<LinkMessage> {
        // Windows takes about a second per refused connection (IPv4 then IPv6), so the closed-port tests need more time.
        let ms = if cfg!(windows) { 10_000 } else { 3000 };
        self.next_within(ms).await.expect("no message")
    }

    async fn next_within(&mut self, ms: u64) -> Result<Option<LinkMessage>, ()> {
        if let Some(m) = self.queue.pop_front() {
            return Ok(Some(m));
        }
        let fut = async {
            let mut buf = [0u8; 4096];
            loop {
                if self.closed {
                    return None;
                }
                match self.stream.read(&mut buf).await {
                    Ok(n) if n > 0 => {
                        self.raw.extend_from_slice(&buf[..n]);
                        for line in self.reader.push(&buf[..n]).unwrap_or_default() {
                            if let Some(m) = parse_link_message(&line) {
                                self.queue.push_back(m);
                            }
                        }
                        if let Some(m) = self.queue.pop_front() {
                            return Some(m);
                        }
                    }
                    _ => {
                        self.closed = true;
                        return None;
                    }
                }
            }
        };
        tokio::time::timeout(Duration::from_millis(ms), fut).await.map_err(|_| ())
    }

    /// True if nothing arrives during `ms`.
    async fn silent(&mut self, ms: u64) -> bool {
        self.next_within(ms).await.is_err()
    }
}

struct Hooks {
    opened: Mutex<Vec<String>>,
    listening: Mutex<Vec<Option<u16>>>,
}
impl LinkHooks for Hooks {
    fn open(&self, url: &str, _: &StoredLink) {
        self.opened.lock().push(url.to_string());
    }
    fn changed(&self) {}
    fn listening(&self, port: Option<u16>) {
        self.listening.lock().push(port);
    }
}

struct H {
    _dir: tempfile::TempDir,
    service: Arc<LinkService>,
    hooks: Arc<Hooks>,
    clock: Arc<AtomicU64>,
}

fn harness() -> H {
    let dir = tempfile::tempdir().unwrap();
    let hooks = Arc::new(Hooks { opened: Mutex::new(vec![]), listening: Mutex::new(vec![]) });
    let clock = Arc::new(AtomicU64::new(1_000_000));
    let c = clock.clone();
    let service = LinkService::new(LinkServiceOptions {
        store: LinkStore::new(dir.path().join("link.json"), Box::new(NoVault)),
        desktop: Arc::new(desktop),
        hosts: Arc::new(|| vec!["192.168.1.20".into()]),
        port: Some(0),
        now: Arc::new(move || c.load(Ordering::SeqCst)),
        hooks: hooks.clone(),
    });
    H { _dir: dir, service, hooks, clock }
}

async fn wait_for(mut cond: impl FnMut() -> bool) {
    for _ in 0..150 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not met");
}

struct Paired {
    offer: PairingOffer,
    token: String,
    link_id: String,
}

async fn pair_phone(h: &H, device: LinkDevice) -> Paired {
    let (offer, _) = h.service.start_pairing().await.expect("no pairing");
    let mut p = Phone::open(offer.port, &offer.link_id).await.unwrap();
    p.send(LinkMessage::Pair { v: 1, link_id: offer.link_id.clone(), code: offer.code.clone(), device }).await;
    match p.next().await {
        Some(LinkMessage::Paired { token, .. }) => Paired { link_id: offer.link_id.clone(), offer, token },
        other => panic!("pairing refused: {other:?}"),
    }
}

async fn control(pr: &Paired, device: LinkDevice) -> Phone {
    control_on(pr.offer.port, &pr.link_id, &pr.token, device).await
}

async fn control_on(port: u16, link_id: &str, token: &str, device: LinkDevice) -> Phone {
    let mut p = Phone::open(port, link_id).await.unwrap();
    p.send(LinkMessage::HelloControl { v: 1, link_id: link_id.into(), token: token.into(), device }).await;
    p
}

async fn ready_control(pr: &Paired) -> Phone {
    let mut c = control(pr, phone_dev()).await;
    assert!(matches!(c.next().await, Some(LinkMessage::Welcome { .. })));
    c.next().await; // servers
    c
}

fn err(code: LinkErrorCode) -> Option<LinkMessage> {
    Some(LinkMessage::Error { code })
}

async fn web(host: &str, body: &'static str) -> Option<(u16, tokio::task::JoinHandle<()>)> {
    let l = TcpListener::bind((host, 0)).await.ok()?;
    let port = l.local_addr().ok()?.port();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            tokio::spawn(async move {
                let mut b = [0u8; 1024];
                let _ = s.read(&mut b).await;
                let _ = s.write_all(format!("HTTP/1.0 200 OK\r\n\r\n{body}").as_bytes()).await;
                let _ = s.shutdown().await;
            });
        }
    });
    Some((port, task))
}

// ----- listening -----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_listens_while_no_phone_is_paired() {
    let h = harness();
    h.service.sync().await;
    assert_eq!(h.service.port(), None);
    assert!(h.hooks.listening.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pairing_opens_listening_and_cancelling_closes_it() {
    let h = harness();
    let (offer, _) = h.service.start_pairing().await.unwrap();
    assert_eq!(h.service.port(), Some(offer.port));
    assert_eq!(*h.hooks.listening.lock(), vec![Some(offer.port)]);
    h.service.cancel_pairing();
    wait_for(|| h.service.port().is_none() && h.hooks.listening.lock().last() == Some(&None)).await;
    assert_eq!(h.hooks.listening.lock().last(), Some(&None));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_pairing_when_the_master_switch_is_off() {
    let h = harness();
    h.service.set_general(false).await;
    assert!(h.service.start_pairing().await.is_none());
}

// ----- pairing -----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn qr_is_readable_certificate_pinned_and_link_saved() {
    let h = harness();
    let (offer, url) = h.service.start_pairing().await.unwrap();
    let clock = h.clock.load(Ordering::SeqCst);
    match decode_pairing_url(&url, clock) {
        PairingDecode::Ok(d) => {
            assert_eq!(d.link_id, offer.link_id);
            assert_eq!(d.hosts, vec!["192.168.1.20"]);
            assert_eq!(d.desktop_name, desktop().name);
            assert_eq!(d.expires_at, (clock + 5 * 60_000).div_ceil(1000) * 1000);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(offer.code.len(), 22);
    assert!(is_link_id(&offer.link_id));

    let mut p = Phone::open(offer.port, &offer.link_id).await.unwrap();
    assert_eq!(p.fingerprint, offer.cert_sha256);
    p.send(LinkMessage::Pair { v: 1, link_id: offer.link_id.clone(), code: offer.code.clone(), device: phone_dev() }).await;
    let reply = p.next().await;
    let Some(LinkMessage::Paired { token, device }) = reply else { panic!("{reply:?}") };
    assert_eq!(device, desktop());
    assert_eq!(token.len(), 43);
    assert_eq!(p.next().await, None); // the connection closes

    h.service.with_store(|s| {
        let link = s.get(&offer.link_id).unwrap();
        assert_eq!(link.peer, phone_dev());
        assert!(link.enabled && !link.peer_paused);
        assert_eq!(link.token_hash, hash_token(&token));
        assert!(!format!("{link:?}").contains(&token));
    });
    assert!(h.service.pending_offer().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_use_code() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut again = Phone::open(pr.offer.port, &pr.link_id).await.unwrap();
    again.send(LinkMessage::Pair { v: 1, link_id: pr.link_id.clone(), code: pr.offer.code.clone(), device: phone_dev() }).await;
    assert_eq!(again.next().await, err(LinkErrorCode::BadCode));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_code_keeps_qr_until_five_attempts() {
    let h = harness();
    let (offer, _) = h.service.start_pairing().await.unwrap();
    let bad = "A".repeat(22);
    for _ in 0..4 {
        let mut p = Phone::open(offer.port, &offer.link_id).await.unwrap();
        p.send(LinkMessage::Pair { v: 1, link_id: offer.link_id.clone(), code: bad.clone(), device: phone_dev() }).await;
        assert_eq!(p.next().await, err(LinkErrorCode::BadCode));
    }
    assert!(h.service.pending_offer().is_some());
    let mut last = Phone::open(offer.port, &offer.link_id).await.unwrap();
    last.send(LinkMessage::Pair { v: 1, link_id: offer.link_id.clone(), code: bad, device: phone_dev() }).await;
    assert_eq!(last.next().await, err(LinkErrorCode::BadCode));
    assert!(h.service.pending_offer().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_qr_is_refused() {
    let h = harness();
    let (offer, _) = h.service.start_pairing().await.unwrap();
    h.clock.fetch_add(5 * 60_000 + 1, Ordering::SeqCst);
    let mut p = Phone::open(offer.port, &offer.link_id).await.unwrap();
    p.send(LinkMessage::Pair { v: 1, link_id: offer.link_id.clone(), code: offer.code.clone(), device: phone_dev() }).await;
    assert_eq!(p.next().await, err(LinkErrorCode::Expired));
    assert!(h.service.with_store(|s| s.links.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_qr_replaces_the_previous_one() {
    let h = harness();
    let (first, _) = h.service.start_pairing().await.unwrap();
    let (second, _) = h.service.start_pairing().await.unwrap();
    assert_ne!(first.link_id, second.link_id);
    let mut p = Phone::open(first.port, &first.link_id).await.unwrap();
    p.send(LinkMessage::Pair { v: 1, link_id: first.link_id.clone(), code: first.code.clone(), device: phone_dev() }).await;
    assert_eq!(p.next().await, err(LinkErrorCode::UnknownLink));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_link_gets_the_default_certificate() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let ghost = "f".repeat(16);
    let mut stranger = Phone::open(pr.offer.port, &ghost).await.unwrap();
    assert_ne!(stranger.fingerprint, pr.offer.cert_sha256);
    stranger.send(LinkMessage::HelloControl { v: 1, link_id: ghost, token: "A".repeat(43), device: phone_dev() }).await;
    assert_eq!(stranger.next().await, err(LinkErrorCode::UnknownLink));
    assert_eq!(Phone::open(pr.offer.port, &pr.link_id).await.unwrap().fingerprint, pr.offer.cert_sha256);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn protocol_version_is_checked() {
    let h = harness();
    let (offer, _) = h.service.start_pairing().await.unwrap();
    let mut p = Phone::open(offer.port, &offer.link_id).await.unwrap();
    p.send(LinkMessage::Pair { v: 99, link_id: offer.link_id.clone(), code: offer.code.clone(), device: phone_dev() }).await;
    assert_eq!(p.next().await, err(LinkErrorCode::Version));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn re_pairing_the_same_phone_replaces_its_link() {
    let h = harness();
    let a = pair_phone(&h, phone_dev()).await;
    let b = pair_phone(&h, phone_dev()).await;
    let ids: Vec<String> = h.service.with_store(|s| s.links.iter().map(|l| l.link_id.clone()).collect());
    assert_eq!(ids, vec![b.link_id.clone()]);
    assert!(h.service.with_store(|s| s.get(&a.link_id).is_none()));
}

// ----- control -----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn welcome_servers_then_ping_pong() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut c = control(&pr, phone_dev()).await;
    assert_eq!(c.next().await, Some(LinkMessage::Welcome { device: desktop(), paused: false }));
    assert_eq!(c.next().await, Some(LinkMessage::Servers { servers: vec![], ports: vec![] }));
    c.send(LinkMessage::Ping).await;
    assert_eq!(c.next().await, Some(LinkMessage::Pong));
    assert_eq!(h.service.status_of(&pr.link_id), Some(LinkStatus::Connected));
    drop(c);
    wait_for(|| h.service.status_of(&pr.link_id) == Some(LinkStatus::Offline)).await;
    assert!(h.service.with_store(|s| s.get(&pr.link_id).unwrap().last_seen.is_some()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_token_is_refused() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut c = control_on(pr.offer.port, &pr.link_id, &"B".repeat(43), phone_dev()).await;
    assert_eq!(c.next().await, err(LinkErrorCode::BadToken));
    assert_eq!(c.next().await, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn too_long_or_malformed_line_is_refused() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut long = Phone::open(pr.offer.port, &pr.link_id).await.unwrap();
    long.write("x".repeat(LINK_MAX_LINE + 10).as_bytes()).await;
    assert_eq!(long.next().await, err(LinkErrorCode::BadRequest));
    let mut junk = Phone::open(pr.offer.port, &pr.link_id).await.unwrap();
    junk.write(b"pas du json\n").await;
    assert_eq!(junk.next().await, err(LinkErrorCode::BadRequest));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sends_dev_servers_when_the_list_changes() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut c = ready_control(&pr).await;
    let a = DevServer { url: "http://localhost:5173/".into(), port: 5173, title: Some("Vite".into()), from_tab: None };
    let b = DevServer { url: "http://localhost:3000/".into(), port: 3000, title: None, from_tab: None };
    h.service.set_servers(vec![a.clone()], None);
    h.service.set_servers(vec![a.clone(), b.clone()], None); // grouped with the previous one
    h.clock.fetch_add(2000, Ordering::SeqCst); // the test clock is frozen: the delay has elapsed when the deferred send goes out
    let got = c.next_within(4000).await.unwrap();
    assert_eq!(got, Some(LinkMessage::Servers { servers: vec![a.clone(), b.clone()], ports: vec![3000, 5173] }));
    h.service.set_servers(vec![a, b], None); // unchanged: nothing
    assert!(c.silent(300).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phone_open_opens_here_http_only() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut c = ready_control(&pr).await;
    c.send(LinkMessage::Open { url: "https://exemple.com/".into() }).await;
    c.write(b"{\"type\":\"open\",\"url\":\"javascript:alert(1)\"}\n").await;
    c.send(LinkMessage::Ping).await;
    c.next().await; // pong: both opens are handled
    assert_eq!(*h.hooks.opened.lock(), vec!["https://exemple.com/".to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_on_my_phone_reaches_the_active_phone() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    assert!(!h.service.open_on_phone(&pr.link_id, "http://localhost:5173/")); // not connected
    let mut c = ready_control(&pr).await;
    assert!(h.service.open_on_phone(&pr.link_id, "http://localhost:5173/"));
    assert_eq!(c.next().await, Some(LinkMessage::Open { url: "http://localhost:5173/".into() }));
    assert!(!h.service.open_on_phone(&pr.link_id, "file:///etc/passwd"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_on_the_phone_side() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut c = ready_control(&pr).await;
    c.send(LinkMessage::Status { paused: true }).await;
    wait_for(|| h.service.status_of(&pr.link_id) == Some(LinkStatus::PausedThere)).await;
    c.send(LinkMessage::Open { url: "https://exemple.com/".into() }).await;
    c.send(LinkMessage::Ping).await;
    c.next().await;
    assert!(h.hooks.opened.lock().is_empty());
    assert!(!h.service.open_on_phone(&pr.link_id, "https://exemple.com/"));
    drop(c);
    wait_for(|| !h.service.has_sessions()).await;
    // Closed without resume: the paused state stays known.
    assert!(h.service.with_store(|s| s.get(&pr.link_id).unwrap().peer_paused));
    // The next hello counts as a resume.
    let mut again = control(&pr, phone_dev()).await;
    assert!(matches!(again.next().await, Some(LinkMessage::Welcome { .. })));
    assert!(!h.service.with_store(|s| s.get(&pr.link_id).unwrap().peer_paused));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn device_switch_off_here_keeps_connection_but_pushes_nothing() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut c = ready_control(&pr).await;
    h.service.set_device(&pr.link_id, false).await;
    assert_eq!(c.next().await, Some(LinkMessage::Status { paused: true }));
    assert_eq!(h.service.status_of(&pr.link_id), Some(LinkStatus::PausedHere));
    let srv = DevServer { url: "http://localhost:1234/".into(), port: 1234, title: None, from_tab: None };
    h.service.set_servers(vec![srv.clone()], None);
    c.send(LinkMessage::Open { url: "https://exemple.com/".into() }).await;
    c.send(LinkMessage::Ping).await;
    assert_eq!(c.next().await, Some(LinkMessage::Pong));
    assert!(h.hooks.opened.lock().is_empty());
    assert!(!c.closed);
    // Resume: status paused:false then servers.
    h.service.set_device(&pr.link_id, true).await;
    assert_eq!(c.next().await, Some(LinkMessage::Status { paused: false }));
    assert_eq!(c.next().await, Some(LinkMessage::Servers { servers: vec![srv], ports: vec![1234] }));
    // A new hello during the pause receives paused:true.
    h.service.set_device(&pr.link_id, false).await;
    let mut second = control(&pr, phone_dev()).await;
    assert_eq!(second.next().await, Some(LinkMessage::Welcome { device: desktop(), paused: true }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn master_switch_off_closes_everything_and_reopens() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut c = ready_control(&pr).await;
    h.service.set_general(false).await;
    assert_eq!(c.next().await, Some(LinkMessage::Status { paused: true }));
    assert_eq!(c.next().await, None);
    assert_eq!(h.service.port(), None);
    assert_eq!(h.hooks.listening.lock().last(), Some(&None));
    assert!(Phone::open(pr.offer.port, &pr.link_id).await.is_err());
    // The link is kept, and the device switch keeps its position.
    assert!(h.service.with_store(|s| s.get(&pr.link_id).unwrap().enabled));
    assert_eq!(h.service.status_of(&pr.link_id), Some(LinkStatus::PausedHere));
    h.service.set_general(true).await;
    let port = h.service.port().unwrap();
    assert_eq!(h.hooks.listening.lock().last(), Some(&Some(port)));
    let mut back = control_on(port, &pr.link_id, &pr.token, phone_dev()).await;
    assert_eq!(back.next().await, Some(LinkMessage::Welcome { device: desktop(), paused: false }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forgetting_here_warns_the_phone() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut c = ready_control(&pr).await;
    h.service.forget(&pr.link_id).await;
    assert_eq!(c.next().await, Some(LinkMessage::Forget));
    assert!(h.service.with_store(|s| s.get(&pr.link_id).is_none()));
    assert_eq!(h.service.port(), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn phone_forget_deletes_the_link_here() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let keep = pair_phone(&h, LinkDevice { id: "phone-2".into(), name: "Autre".into(), platform: LinkPlatform::Android }).await;
    let mut c = ready_control(&pr).await;
    c.send(LinkMessage::Forget).await;
    wait_for(|| h.service.with_store(|s| s.get(&pr.link_id).is_none())).await;
    assert!(h.service.with_store(|s| s.get(&keep.link_id).is_some()));
    let mut stale = Phone::open(pr.offer.port, &pr.link_id).await.unwrap();
    stale.send(LinkMessage::HelloControl { v: 1, link_id: pr.link_id.clone(), token: pr.token.clone(), device: phone_dev() }).await;
    assert_eq!(stale.next().await, err(LinkErrorCode::UnknownLink));
}

// ----- tunnel -----

async fn tunnel(port: u16, link_id: &str, token: &str, target: u16) -> Phone {
    let mut t = Phone::open(port, link_id).await.unwrap();
    t.send(LinkMessage::HelloTunnel { v: 1, link_id: link_id.into(), token: token.into(), port: target }).await;
    t
}

async fn get(t: &mut Phone) -> String {
    t.write(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").await;
    let mut out = t.raw.clone();
    let mut buf = [0u8; 4096];
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        while let Ok(n) = t.stream.read(&mut buf).await {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
    })
    .await;
    String::from_utf8_lossy(&out).to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tunnel_relays_bytes_to_127_0_0_1() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let (port, _s) = web("127.0.0.1", "bonjour-v4").await.unwrap();
    let mut t = tunnel(pr.offer.port, &pr.link_id, &pr.token, port).await;
    assert_eq!(t.next().await, Some(LinkMessage::Ok));
    assert!(get(&mut t).await.contains("bonjour-v4"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tunnel_tries_ipv6_when_ipv4_is_closed() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let Some((port, _s)) = web("::1", "bonjour-v6").await else { return };
    let mut t = tunnel(pr.offer.port, &pr.link_id, &pr.token, port).await;
    assert_eq!(t.next().await, Some(LinkMessage::Ok));
    assert!(get(&mut t).await.contains("bonjour-v6"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bytes_sent_with_the_first_line_are_not_lost() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let (port, _s) = web("127.0.0.1", "tout-court").await.unwrap();
    let mut t = Phone::open(pr.offer.port, &pr.link_id).await.unwrap();
    let first = encode_link_message(&LinkMessage::HelloTunnel { v: 1, link_id: pr.link_id.clone(), token: pr.token.clone(), port });
    t.write(format!("{first}GET / HTTP/1.0\r\nHost: localhost\r\n\r\n").as_bytes()).await;
    assert_eq!(t.next().await, Some(LinkMessage::Ok));
    let mut buf = [0u8; 4096];
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        while !String::from_utf8_lossy(&t.raw).contains("tout-court") {
            match t.stream.read(&mut buf).await {
                Ok(n) if n > 0 => t.raw.extend_from_slice(&buf[..n]),
                _ => break,
            }
        }
    })
    .await;
    assert!(String::from_utf8_lossy(&t.raw).contains("tout-court"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_port_is_reported() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let mut t = tunnel(pr.offer.port, &pr.link_id, &pr.token, port).await;
    assert_eq!(t.next().await, err(LinkErrorCode::PortClosed));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_links_own_port_is_forbidden() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let mut t = tunnel(pr.offer.port, &pr.link_id, &pr.token, pr.offer.port).await;
    assert_eq!(t.next().await, err(LinkErrorCode::PortRefused));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tunnel_wrong_token_and_unknown_link() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let (port, _s) = web("127.0.0.1", "x").await.unwrap();
    assert_eq!(tunnel(pr.offer.port, &pr.link_id, &"C".repeat(43), port).await.next().await, err(LinkErrorCode::BadToken));
    assert_eq!(tunnel(pr.offer.port, &"e".repeat(16), &"C".repeat(43), port).await.next().await, err(LinkErrorCode::UnknownLink));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn link_paused_here_refuses_and_cuts_tunnels() {
    let h = harness();
    let pr = pair_phone(&h, phone_dev()).await;
    let (port, _s) = web("127.0.0.1", "x").await.unwrap();
    let mut open1 = tunnel(pr.offer.port, &pr.link_id, &pr.token, port).await;
    assert_eq!(open1.next().await, Some(LinkMessage::Ok));
    h.service.set_device(&pr.link_id, false).await;
    assert_eq!(open1.next().await, None);
    let mut t = tunnel(pr.offer.port, &pr.link_id, &pr.token, port).await;
    assert_eq!(t.next().await, err(LinkErrorCode::Paused));
}

// ----- LineReader -----

#[test]
fn line_reader_assembles_lines_and_keeps_the_rest() {
    let mut r = LineReader::new();
    assert_eq!(r.push(b"{\"a\":"), Some(vec![]));
    assert_eq!(r.push(b"1}\n{\"b\":2}\nreste"), Some(vec!["{\"a\":1}".to_string(), "{\"b\":2}".to_string()]));
    assert_eq!(r.rest(), b"reste");
}

#[test]
fn line_reader_refuses_a_line_that_is_too_long() {
    assert_eq!(LineReader::new().push(&vec![97u8; LINK_MAX_LINE + 1]), None);
}

//! "Hybrid" (caBLE v2) sessions: one at a time. Port of hybrid/session.ts and the logic of windows/passkey.ts.
//!
//! The app shows its own window (QR code, status, Cancel) from `HybridHandle`; this module has no UI. See the crate README
//! for the exact JSON that goes in and out.

use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::mpsc;

use super::ble::{BleScanner, BtleplugScanner, NoScanner, OnData, OnState, ScanState, StopFn};
use super::ctap::{CredentialJson, CtapFailure, parse_ctap_response, parse_get_info, plan_ctap};
use super::eid::{Eid, decrypt_advert};
use super::qr::{QrKeys, generate_qr_keys, qr_text};
use super::tunnel::{AbortHandle, AbortSignal, Connector, LogFn, TunnelError, TunnelOptions, WsConnector, open_tunnel};
use super::webauthn_request::{WebAuthnError, WebAuthnInput, WebAuthnRequest, not_allowed, validate_webauthn_request};
use crate::qrimage::QrMatrix;

/// `publicKey.timeout` is clamped like Chrome does (10 s to 10 min); 5 min without one.
const MIN_TIMEOUT: f64 = 10_000.0;
const MAX_TIMEOUT: f64 = 600_000.0;
const DEFAULT_TIMEOUT: f64 = 300_000.0;

pub fn hybrid_timeout(public_key: &Map<String, Value>) -> Duration {
    let ms = match public_key.get("timeout").and_then(Value::as_f64) {
        Some(t) if t.is_finite() => t.clamp(MIN_TIMEOUT, MAX_TIMEOUT),
        _ => DEFAULT_TIMEOUT,
    };
    Duration::from_millis(ms as u64)
}

/// How long the "failed" status stays before the page gets its error.
pub const FAILED_DELAY: Duration = Duration::from_secs(3);

/// What the passkey window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PasskeyStatus {
    Waiting,
    Off,
    NoAdapter,
    Denied,
    Error,
    Unsupported,
    Connecting,
    Confirm,
    Failed,
}

impl From<ScanState> for PasskeyStatus {
    fn from(s: ScanState) -> Self {
        match s {
            ScanState::Waiting => Self::Waiting,
            ScanState::Off => Self::Off,
            ScanState::NoAdapter => Self::NoAdapter,
            ScanState::Denied => Self::Denied,
            ScanState::Error => Self::Error,
            ScanState::Unsupported => Self::Unsupported,
        }
    }
}

/// Answer for the page: an error (the page rejects with a DOMException / TypeError) or the credential.
#[derive(Debug, Clone, PartialEq)]
pub enum WebAuthnAnswer {
    Error(WebAuthnError),
    Credential(CredentialJson),
}

impl WebAuthnAnswer {
    /// `{"error":{"name","message"}}` or `{"credential":{...toJSON() form...}}`: what the page script expects.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Error(e) => json!({ "error": e }),
            Self::Credential(c) => json!({ "credential": c }),
        }
    }
}

pub fn not_allowed_answer() -> WebAuthnAnswer {
    WebAuthnAnswer::Error(not_allowed())
}

#[derive(Debug, Clone, PartialEq)]
pub enum HybridEvent {
    /// The status shown to the user changed (deduplicated).
    Status(PasskeyStatus),
    /// The phone's advert was found and decrypted (tunnel domain only, no secret).
    AdvertSeen { tunnel_domain: String },
    /// Last event: the answer for the page. Nothing follows.
    Done(WebAuthnAnswer),
}

/// A running session. Dropping it cancels the session.
pub struct HybridHandle {
    /// Request type (`get` or `create`) and relying party, for the window's text.
    pub request_type: &'static str,
    pub rp_id: String,
    /// `FIDO:/...` text of the QR code.
    pub qr: String,
    /// The same code as a module matrix to draw (None only if the text could not be encoded).
    pub qr_matrix: Option<QrMatrix>,
    pub events: std_mpsc::Receiver<HybridEvent>,
    abort: AbortHandle,
}

impl HybridHandle {
    /// Cancel button, Escape, window closed, page aborted, tab navigated away.
    pub fn cancel(&self) {
        self.abort.abort();
    }
}

impl Drop for HybridHandle {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

/// What the sessions use from the outside world.
#[derive(Clone)]
pub struct HybridDeps {
    pub scanner: Arc<dyn BleScanner>,
    pub connector: Arc<dyn Connector>,
    pub failed_delay: Duration,
    /// Development log of the stages and of why a request failed (never data or keys).
    pub log: Option<LogFn>,
}

impl HybridDeps {
    pub fn real(handle: &Handle) -> Self {
        Self { scanner: Arc::new(BtleplugScanner::new(handle.clone())), connector: Arc::new(WsConnector), failed_delay: FAILED_DELAY, log: None }
    }
    /// No Bluetooth scan (`Unsupported`), real tunnel server.
    pub fn without_ble() -> Self {
        Self { scanner: Arc::new(NoScanner), connector: Arc::new(WsConnector), failed_delay: FAILED_DELAY, log: None }
    }
}

enum ScanMsg {
    Data(Vec<u8>),
    State(ScanState),
}

struct StopOnDrop(Option<StopFn>);
impl StopOnDrop {
    fn stop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Status {
    events: std_mpsc::Sender<HybridEvent>,
    current: Option<PasskeyStatus>,
}
impl Status {
    fn set(&mut self, s: PasskeyStatus) {
        if self.current != Some(s) {
            self.current = Some(s);
            let _ = self.events.send(HybridEvent::Status(s));
        }
    }
}

enum FlowError {
    Tunnel(TunnelError),
    Ctap(CtapFailure),
}

struct Session {
    deps: HybridDeps,
    request: WebAuthnRequest,
    keys: QrKeys,
    timeout: Duration,
}

impl Session {
    fn log(&self, line: &str) {
        if let Some(l) = &self.deps.log {
            l(line);
        }
    }

    async fn run(self, cancel: AbortSignal, events: std_mpsc::Sender<HybridEvent>) {
        let answer = self.run_inner(&cancel, &events).await;
        let _ = events.send(HybridEvent::Done(answer));
    }

    async fn run_inner(&self, cancel: &AbortSignal, events: &std_mpsc::Sender<HybridEvent>) -> WebAuthnAnswer {
        let status = Arc::new(Mutex::new(Status { events: events.clone(), current: None }));
        let sleep = tokio::time::sleep(self.timeout);
        tokio::pin!(sleep);

        // Look for the phone's advert; the first one that decrypts with the QR secret is the phone.
        let (scan_tx, mut scan_rx) = mpsc::unbounded_channel::<ScanMsg>();
        let (d, s) = (scan_tx.clone(), scan_tx);
        let on_data: OnData = Arc::new(move |b| {
            let _ = d.send(ScanMsg::Data(b));
        });
        let on_state: OnState = Arc::new(move |st| {
            let _ = s.send(ScanMsg::State(st));
        });
        let mut scan = StopOnDrop(Some(self.deps.scanner.start(on_data, on_state)));
        let eid: Eid = loop {
            tokio::select! {
                biased;
                _ = cancel.aborted() => return not_allowed_answer(),
                _ = &mut sleep => return not_allowed_answer(),
                msg = scan_rx.recv() => match msg {
                    Some(ScanMsg::State(st)) => status.lock().set(st.into()),
                    Some(ScanMsg::Data(bytes)) => {
                        if let Some(eid) = decrypt_advert(&self.keys.secret, &bytes) {
                            break eid;
                        }
                    }
                    None => return not_allowed_answer(),
                },
            }
        };
        scan.stop();
        drop(scan_rx);
        let _ = events.send(HybridEvent::AdvertSeen { tunnel_domain: eid.tunnel_domain.clone() });
        status.lock().set(PasskeyStatus::Connecting);

        let flow = self.connect(eid, status.clone());
        tokio::pin!(flow);
        let result = tokio::select! {
            biased;
            _ = cancel.aborted() => return not_allowed_answer(),
            _ = &mut sleep => return not_allowed_answer(),
            r = &mut flow => r,
        };
        match result {
            Ok(c) => WebAuthnAnswer::Credential(c),
            Err(e) => {
                // Only our own errors carry a message that is safe to log (fixed texts, no data).
                self.log(&match &e {
                    FlowError::Tunnel(t) => format!("failed ({}): {}", t.stage.as_str(), t.message),
                    FlowError::Ctap(c) => format!("failed: {}", c.reason),
                });
                let answer = match e {
                    FlowError::Ctap(c) => WebAuthnAnswer::Error(c.error),
                    FlowError::Tunnel(_) => not_allowed_answer(),
                };
                status.lock().set(PasskeyStatus::Failed);
                // "Failed" stays a moment, then the page gets its error. Cancel and timeout end it at once.
                tokio::select! {
                    biased;
                    _ = cancel.aborted() => not_allowed_answer(),
                    _ = &mut sleep => not_allowed_answer(),
                    _ = tokio::time::sleep(self.deps.failed_delay) => answer,
                }
            }
        }
    }

    /// Tunnel, handshake, then the CTAP2 request.
    async fn connect(&self, eid: Eid, status: Arc<Mutex<Status>>) -> Result<CredentialJson, FlowError> {
        let log = self.deps.log.clone();
        let mut tunnel = open_tunnel(TunnelOptions {
            eid,
            secret: self.keys.secret,
            identity: self.keys.identity.clone(),
            signal: None,
            connector: self.deps.connector.clone(),
            connect_timeout: None,
            handshake_timeout: None,
            log,
        })
        .await
        .map_err(FlowError::Tunnel)?;
        status.lock().set(PasskeyStatus::Confirm);
        let info = parse_get_info(&tunnel.get_info).map_err(FlowError::Ctap)?;
        let plan = plan_ctap(&self.request, &info).map_err(FlowError::Ctap)?;
        if !plan.skipped.is_empty() {
            self.log(&format!("ignored extensions: {}", plan.skipped.join(", ")));
        }
        self.log(&format!("sending {}", if plan.ty == super::qr::RequestType::Get { "getAssertion" } else { "makeCredential" }));
        let response = tunnel.transact(&plan.bytes, None).await.map_err(FlowError::Tunnel)?;
        let credential = parse_ctap_response(&plan, &response).map_err(FlowError::Ctap)?;
        self.log("credential received");
        Ok(credential)
    }
}

/// Runs hybrid passkey sessions, one at a time. Owns a small runtime unless built with `with_handle`.
pub struct HybridManager {
    _rt: Option<Runtime>,
    handle: Handle,
    deps: HybridDeps,
    current: Mutex<Option<AbortHandle>>,
}

impl HybridManager {
    /// Own runtime, real Bluetooth scan and tunnel server.
    pub fn new() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("signet-hybrid").build().expect("tokio runtime");
        let handle = rt.handle().clone();
        let deps = HybridDeps::real(&handle);
        Self { _rt: Some(rt), handle, deps, current: Mutex::new(None) }
    }

    /// Own runtime with custom parts (no Bluetooth scan, a development log...).
    pub fn with_deps(make: impl FnOnce(&Handle) -> HybridDeps) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().thread_name("signet-hybrid").build().expect("tokio runtime");
        let handle = rt.handle().clone();
        let deps = make(&handle);
        Self { _rt: Some(rt), handle, deps, current: Mutex::new(None) }
    }

    /// On an existing runtime (tests).
    pub fn with_handle(handle: Handle, deps: HybridDeps) -> Self {
        Self { _rt: None, handle, deps, current: Mutex::new(None) }
    }

    /// Starts a session for a validated, non-conditional request (a conditional one is answered NotAllowed by the caller:
    /// `webauthn_precheck`). The previous session, if any, ends with NotAllowedError. Returns at once.
    pub fn start_hybrid(&self, request: WebAuthnRequest) -> HybridHandle {
        self.start_with_keys(request, generate_qr_keys())
    }

    pub(crate) fn start_with_keys(&self, request: WebAuthnRequest, keys: QrKeys) -> HybridHandle {
        let (abort, cancel) = AbortHandle::new();
        if let Some(prev) = self.current.lock().replace(abort.clone()) {
            prev.abort();
        }
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let qr = qr_text(&keys, request.ty, now_ms);
        let (tx, rx) = std_mpsc::channel();
        let timeout = hybrid_timeout(&request.public_key);
        let handle = HybridHandle {
            request_type: request.ty.as_str(),
            rp_id: request.rp_id.clone(),
            qr_matrix: QrMatrix::new(&qr),
            qr,
            events: rx,
            abort,
        };
        let session = Session { deps: self.deps.clone(), request, keys, timeout };
        self.handle.spawn(session.run(cancel, tx));
        handle
    }

    /// Ends the current session, if any (same as `HybridHandle::cancel`).
    pub fn cancel(&self) {
        if let Some(a) = self.current.lock().take() {
            a.abort();
        }
    }
}

impl Default for HybridManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Everything between the page's `webauthn:request` message and a hybrid session, for a CEF integration:
/// validates like a browser, then either answers at once (error, conditional mediation) or returns the request to start.
pub enum Precheck {
    /// Answer the page with this, no session.
    Answer(WebAuthnAnswer),
    /// Call `HybridManager::start_hybrid` with this.
    Start(WebAuthnRequest),
}

/// `origin`, `top_origin` and `is_main_frame` come from the browser process (never from the page); `payload` is the page's
/// `{type, mediation?, publicKey}` object.
pub fn webauthn_precheck(origin: Option<&str>, top_origin: Option<&str>, is_main_frame: bool, payload: &Value) -> Precheck {
    match validate_webauthn_request(WebAuthnInput { origin, top_origin, is_main_frame, payload }) {
        Err(e) => Precheck::Answer(WebAuthnAnswer::Error(e)),
        // A conditional request stays pending in the page; here it just fails (no autofill UI for passkeys).
        Ok(r) if r.mediation.as_deref() == Some("conditional") => Precheck::Answer(not_allowed_answer()),
        Ok(r) => Precheck::Start(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hybrid::cbor::{Cbor, encode_cbor};
    use crate::hybrid::eid::tests::{advert, plain};
    use crate::hybrid::noise::uncompressed;
    use crate::hybrid::qr::RequestType;
    use crate::hybrid::tunnel::testing::{FakePhone, FakePhoneOptions};
    use parking_lot::Mutex as PMutex;
    use sha2::{Digest, Sha256};

    fn request(public_key: Value, ty: RequestType) -> WebAuthnRequest {
        WebAuthnRequest {
            ty,
            mediation: None,
            rp_id: "example.com".into(),
            origin: "https://example.com".into(),
            public_key: public_key.as_object().cloned().unwrap_or_default(),
        }
    }
    fn get_request(extra: Value) -> WebAuthnRequest {
        let mut pk = json!({ "challenge": "AQID", "rpId": "example.com", "userVerification": "preferred" });
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            pk[k] = v;
        }
        request(pk, RequestType::Get)
    }

    #[derive(Default)]
    struct FakeScanner {
        on_data: PMutex<Option<OnData>>,
        on_state: PMutex<Option<OnState>>,
        starts: PMutex<usize>,
        stops: Arc<PMutex<usize>>,
    }
    impl BleScanner for Arc<FakeScanner> {
        fn start(&self, on_data: OnData, on_state: OnState) -> StopFn {
            *self.on_data.lock() = Some(on_data);
            *self.on_state.lock() = Some(on_state);
            *self.starts.lock() += 1;
            let stops = self.stops.clone();
            Box::new(move || *stops.lock() += 1)
        }
    }
    impl FakeScanner {
        fn data(&self, b: Vec<u8>) {
            if let Some(f) = self.on_data.lock().as_ref() {
                f(b);
            }
        }
        fn state(&self, s: ScanState) {
            if let Some(f) = self.on_state.lock().as_ref() {
                f(s);
            }
        }
    }

    struct Rig {
        manager: HybridManager,
        scanner: Arc<FakeScanner>,
        logs: Arc<PMutex<Vec<String>>>,
        phone: Option<Arc<FakePhone>>,
    }

    /// Manager with a fake scanner and, if given, a fake phone behind the tunnel server.
    fn rig(keys: Option<&QrKeys>, tweak: impl FnOnce(&mut FakePhoneOptions)) -> Rig {
        let scanner = Arc::new(FakeScanner::default());
        let logs = Arc::new(PMutex::new(vec![]));
        let l2 = logs.clone();
        let phone = keys.map(|k| {
            let advert_plain = plain(0, 0);
            let eid = decrypt_advert(&k.secret, &advert(&k.secret, &advert_plain)).unwrap();
            let mut o = FakePhoneOptions::new(k.secret, eid, uncompressed(&k.identity.public_key()), vec![0xa0]);
            o.get_info = encode_cbor(&Cbor::map(vec![
                (Cbor::int(1), Cbor::Array(vec![Cbor::text("FIDO_2_0")])),
                (Cbor::int(4), Cbor::map(vec![(Cbor::text("uv"), Cbor::Bool(true)), (Cbor::text("rk"), Cbor::Bool(true))])),
            ]))
            .unwrap();
            tweak(&mut o);
            FakePhone::new(o)
        });
        struct Never;
        impl Connector for Never {
            fn connect(&self, _: String, _: &'static str) -> crate::hybrid::tunnel::BoxFuture<Result<crate::hybrid::tunnel::WsChannels, ()>> {
                Box::pin(async { Err(()) })
            }
        }
        let connector: Arc<dyn Connector> = match &phone {
            Some(p) => Arc::new(p.clone()),
            None => Arc::new(Never),
        };
        let deps = HybridDeps {
            scanner: Arc::new(scanner.clone()),
            connector,
            failed_delay: FAILED_DELAY,
            log: Some(Arc::new(move |l| l2.lock().push(l.to_string()))),
        };
        Rig { manager: HybridManager::with_handle(Handle::current(), deps), scanner, logs, phone }
    }

    async fn settle() {
        for _ in 0..8 {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }

    fn drain(h: &HybridHandle) -> Vec<HybridEvent> {
        h.events.try_iter().collect()
    }
    fn last_done(evs: &[HybridEvent]) -> Option<&WebAuthnAnswer> {
        evs.iter().rev().find_map(|e| if let HybridEvent::Done(a) = e { Some(a) } else { None })
    }
    fn is_not_allowed(a: Option<&WebAuthnAnswer>) -> bool {
        matches!(a, Some(WebAuthnAnswer::Error(e)) if e.name == "NotAllowedError" && e.message.contains("timed out or was not allowed"))
    }

    #[test]
    fn timeout_is_clamped_like_chrome() {
        let t = |v: Value| hybrid_timeout(v.as_object().unwrap());
        assert_eq!(t(json!({})), Duration::from_millis(300_000));
        assert_eq!(t(json!({ "timeout": "x" })), Duration::from_millis(300_000));
        assert_eq!(t(json!({ "timeout": 1 })), Duration::from_millis(10_000));
        assert_eq!(t(json!({ "timeout": 60_000 })), Duration::from_millis(60_000));
        assert_eq!(t(json!({ "timeout": 1_000_000_000u64 })), Duration::from_millis(600_000));
    }

    #[tokio::test(start_paused = true)]
    async fn shows_a_qr_then_ends_cancelled() {
        let r = rig(None, |_| {});
        let h = r.manager.start_hybrid(request(json!({}), RequestType::Create));
        assert!(h.qr.starts_with("FIDO:/"));
        assert!(h.qr_matrix.is_some());
        assert_eq!((h.request_type, h.rp_id.as_str()), ("create", "example.com"));
        settle().await;
        assert_eq!(*r.scanner.starts.lock(), 1);
        h.cancel();
        settle().await;
        let evs = drain(&h);
        assert!(is_not_allowed(last_done(&evs)));
        assert_eq!(*r.scanner.stops.lock(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn ends_on_timeout() {
        let r = rig(None, |_| {});
        let h = r.manager.start_hybrid(get_request(json!({ "timeout": 30_000 })));
        tokio::time::sleep(Duration::from_millis(29_999)).await;
        assert!(last_done(&drain(&h)).is_none());
        tokio::time::sleep(Duration::from_millis(2)).await;
        settle().await;
        assert!(is_not_allowed(last_done(&drain(&h))));
        assert_eq!(*r.scanner.stops.lock(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_request_ends_the_previous_one() {
        let r = rig(None, |_| {});
        let first = r.manager.start_hybrid(get_request(json!({})));
        settle().await;
        let second = r.manager.start_hybrid(request(json!({}), RequestType::Create));
        settle().await;
        assert!(is_not_allowed(last_done(&drain(&first))));
        assert!(last_done(&drain(&second)).is_none());
        r.manager.cancel();
        settle().await;
        assert!(is_not_allowed(last_done(&drain(&second))));
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_the_handle_cancels() {
        let r = rig(None, |_| {});
        let h = r.manager.start_hybrid(get_request(json!({})));
        settle().await;
        drop(h);
        settle().await;
        assert_eq!(*r.scanner.stops.lock(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn answers_once() {
        let r = rig(None, |_| {});
        let h = r.manager.start_hybrid(get_request(json!({})));
        settle().await;
        h.cancel();
        h.cancel();
        tokio::time::sleep(Duration::from_secs(700)).await;
        let evs = drain(&h);
        assert_eq!(evs.iter().filter(|e| matches!(e, HybridEvent::Done(_))).count(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn forwards_scan_states_once_each() {
        let r = rig(None, |_| {});
        let h = r.manager.start_hybrid(get_request(json!({})));
        settle().await;
        r.scanner.state(ScanState::Waiting);
        r.scanner.state(ScanState::Waiting);
        r.scanner.state(ScanState::Off);
        r.scanner.state(ScanState::Waiting);
        settle().await;
        assert_eq!(
            drain(&h),
            vec![
                HybridEvent::Status(PasskeyStatus::Waiting),
                HybridEvent::Status(PasskeyStatus::Off),
                HybridEvent::Status(PasskeyStatus::Waiting)
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn ignores_adverts_that_are_not_for_this_session() {
        let keys = generate_qr_keys();
        let r = rig(None, |_| {});
        let h = r.manager.start_with_keys(get_request(json!({})), keys.clone());
        settle().await;
        let other = [9u8; 16];
        r.scanner.data(advert(&other, &plain(0, 0)));
        r.scanner.data(vec![1, 2, 3]);
        settle().await;
        assert!(drain(&h).is_empty());
        assert_eq!(*r.scanner.stops.lock(), 0);
    }

    // ----- tunnel and CTAP -----

    fn assertion_response(rp_id: &str) -> Vec<u8> {
        let mut auth = Sha256::digest(rp_id.as_bytes()).to_vec();
        auth.push(0x05);
        auth.extend_from_slice(&[0, 0, 0, 1]);
        let map = Cbor::map(vec![
            (Cbor::int(1), Cbor::map(vec![(Cbor::text("id"), Cbor::bytes(&[7; 16])), (Cbor::text("type"), Cbor::text("public-key"))])),
            (Cbor::int(2), Cbor::Bytes(auth)),
            (Cbor::int(3), Cbor::bytes(&[0x30, 1, 2])),
            (Cbor::int(4), Cbor::map(vec![(Cbor::text("id"), Cbor::bytes(&[9, 9]))])),
        ]);
        let mut out = vec![0];
        out.extend(encode_cbor(&map).unwrap());
        out
    }

    fn statuses(evs: &[HybridEvent]) -> Vec<PasskeyStatus> {
        evs.iter().filter_map(|e| if let HybridEvent::Status(s) = e { Some(*s) } else { None }).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn success_confirm_on_the_phone_then_the_credential_ends_the_session() {
        let keys = generate_qr_keys();
        let r = rig(Some(&keys), |o| o.on_request = Some(Arc::new(|_| Some(assertion_response("example.com")))));
        let h = r.manager.start_with_keys(get_request(json!({})), keys.clone());
        settle().await;
        r.scanner.state(ScanState::Waiting);
        r.scanner.data(advert(&keys.secret, &plain(0, 0)));
        settle().await;
        let evs = drain(&h);
        assert_eq!(statuses(&evs), vec![PasskeyStatus::Waiting, PasskeyStatus::Connecting, PasskeyStatus::Confirm]);
        assert!(evs.contains(&HybridEvent::AdvertSeen { tunnel_domain: "cable.ua5v.com".into() }));
        let Some(WebAuthnAnswer::Credential(c)) = last_done(&evs).cloned() else { panic!("no credential: {evs:?}") };
        assert_eq!(c.id, crate::b64::encode(&[7; 16]));
        // The scan stopped at the advert, the tunnel was shut down politely.
        assert_eq!(*r.scanner.stops.lock(), 1);
        let st = r.phone.as_ref().unwrap().state.lock();
        assert_eq!(st.requests.len(), 1);
        assert_eq!(st.requests[0][0], 0x02);
        assert_eq!(st.shutdowns, 1);
        // The JSON for the page.
        drop(st);
        let j = last_done(&evs).unwrap().to_json();
        assert_eq!(j["credential"]["type"], "public-key");
        assert_eq!(j["credential"]["response"]["userHandle"], crate::b64::encode(&[9, 9]));
    }

    #[tokio::test(start_paused = true)]
    async fn tunnel_that_cannot_open_fails_for_3s_then_not_allowed() {
        let keys = generate_qr_keys();
        let r = rig(Some(&keys), |o| o.refuse = true);
        let h = r.manager.start_with_keys(get_request(json!({})), keys.clone());
        settle().await;
        r.scanner.data(advert(&keys.secret, &plain(0, 0)));
        settle().await;
        let evs = drain(&h);
        assert_eq!(statuses(&evs), vec![PasskeyStatus::Connecting, PasskeyStatus::Failed]);
        assert!(last_done(&evs).is_none());
        tokio::time::sleep(Duration::from_millis(2900)).await;
        assert!(last_done(&drain(&h)).is_none());
        tokio::time::sleep(Duration::from_millis(200)).await;
        settle().await;
        assert!(is_not_allowed(last_done(&drain(&h))));
        assert!(r.logs.lock().iter().any(|l| l.starts_with("failed (connect)")));
    }

    #[tokio::test(start_paused = true)]
    async fn excluded_credential_maps_to_invalid_state_error() {
        let keys = generate_qr_keys();
        let r = rig(Some(&keys), |o| o.on_request = Some(Arc::new(|_| Some(vec![0x19]))));
        let h = r.manager.start_with_keys(get_request(json!({})), keys.clone());
        settle().await;
        r.scanner.data(advert(&keys.secret, &plain(0, 0)));
        settle().await;
        tokio::time::sleep(FAILED_DELAY + Duration::from_millis(1)).await;
        settle().await;
        let evs = drain(&h);
        assert!(matches!(last_done(&evs), Some(WebAuthnAnswer::Error(e)) if e.name == "InvalidStateError"));
    }

    #[tokio::test(start_paused = true)]
    async fn invalid_response_fails() {
        let keys = generate_qr_keys();
        let r = rig(Some(&keys), |o| o.on_request = Some(Arc::new(|_| Some(assertion_response("evil.example")))));
        let h = r.manager.start_with_keys(get_request(json!({})), keys.clone());
        settle().await;
        r.scanner.data(advert(&keys.secret, &plain(0, 0)));
        settle().await;
        assert!(statuses(&drain(&h)).contains(&PasskeyStatus::Failed));
        assert!(r.logs.lock().iter().any(|l| l == "failed: rpIdHash mismatch"));
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_while_failed_ends_at_once() {
        let keys = generate_qr_keys();
        let r = rig(Some(&keys), |o| o.refuse = true);
        let h = r.manager.start_with_keys(get_request(json!({})), keys.clone());
        settle().await;
        r.scanner.data(advert(&keys.secret, &plain(0, 0)));
        settle().await;
        h.cancel();
        settle().await;
        assert!(is_not_allowed(last_done(&drain(&h))));
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_while_confirming_closes_the_tunnel() {
        let keys = generate_qr_keys();
        let r = rig(Some(&keys), |o| o.on_request = Some(Arc::new(|_| None)));
        let h = r.manager.start_with_keys(get_request(json!({})), keys.clone());
        settle().await;
        r.scanner.data(advert(&keys.secret, &plain(0, 0)));
        settle().await;
        assert!(statuses(&drain(&h)).contains(&PasskeyStatus::Confirm));
        h.cancel();
        settle().await;
        assert!(is_not_allowed(last_done(&drain(&h))));
        let st = r.phone.as_ref().unwrap().state.lock();
        assert_eq!((st.shutdowns, st.closed), (1, 1));
    }

    #[tokio::test(start_paused = true)]
    async fn session_timeout_ends_a_request_waiting_for_the_phone() {
        let keys = generate_qr_keys();
        let r = rig(Some(&keys), |o| o.on_request = Some(Arc::new(|_| None)));
        let h = r.manager.start_with_keys(get_request(json!({ "timeout": 20_000 })), keys.clone());
        settle().await;
        r.scanner.data(advert(&keys.secret, &plain(0, 0)));
        settle().await;
        tokio::time::sleep(Duration::from_millis(20_001)).await;
        settle().await;
        assert!(is_not_allowed(last_done(&drain(&h))));
        assert_eq!(r.phone.as_ref().unwrap().state.lock().closed, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn request_without_challenge_fails() {
        let keys = generate_qr_keys();
        let r = rig(Some(&keys), |_| {});
        let h = r.manager.start_with_keys(request(json!({ "rpId": "example.com" }), RequestType::Get), keys.clone());
        settle().await;
        r.scanner.data(advert(&keys.secret, &plain(0, 0)));
        settle().await;
        assert!(statuses(&drain(&h)).contains(&PasskeyStatus::Failed));
    }

    #[tokio::test(start_paused = true)]
    async fn logs_stages_without_keys_or_data() {
        let keys = generate_qr_keys();
        let r = rig(Some(&keys), |o| o.on_request = Some(Arc::new(|_| Some(assertion_response("example.com")))));
        let h = r.manager.start_with_keys(get_request(json!({})), keys.clone());
        settle().await;
        r.scanner.data(advert(&keys.secret, &plain(0, 0)));
        settle().await;
        drop(drain(&h));
        let all = r.logs.lock().join("\n");
        assert!(all.contains("sending getAssertion") && all.contains("credential received"));
        assert!(!all.contains(&hex::encode(keys.secret)));
        assert!(!all.contains(&hex::encode(plain(0, 0))));
    }

    #[test]
    fn precheck_answers_errors_and_conditional_requests() {
        let p = json!({ "type": "get", "publicKey": { "challenge": "abc" } });
        assert!(matches!(webauthn_precheck(Some("https://example.com"), Some("https://example.com"), true, &p), Precheck::Start(_)));
        assert!(matches!(
            webauthn_precheck(Some("http://example.com"), None, true, &p),
            Precheck::Answer(WebAuthnAnswer::Error(e)) if e.name == "SecurityError"
        ));
        let cond = json!({ "type": "get", "mediation": "conditional", "publicKey": { "challenge": "abc" } });
        assert!(matches!(webauthn_precheck(Some("https://example.com"), None, true, &cond), Precheck::Answer(a) if is_not_allowed(Some(&a))));
    }

    #[test]
    fn answer_json_shapes() {
        assert_eq!(not_allowed_answer().to_json()["error"]["name"], "NotAllowedError");
    }
}

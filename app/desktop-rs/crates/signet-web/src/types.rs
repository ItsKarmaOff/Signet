//! Public value types of signet-web: ids, geometry, events and option structs.

use std::fmt;

/// Identifier of a web view, unique for the process lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViewId(pub u64);

impl fmt::Display for ViewId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "view-{}", self.0)
    }
}

/// Rectangle in window points (logical pixels), origin at the top left of the window content.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Bounds {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }
}

/// How a page asked a new browsing context to be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    CurrentTab,
    ForegroundTab,
    BackgroundTab,
    Popup,
    NewWindow,
    Other,
}

/// A profile: persistent (own cache folder) or in-memory (private windows).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ProfileKind {
    Persistent(String),
    /// In-memory profile; the string is a private session id (no disk state).
    Private(String),
}

impl ProfileKind {
    pub fn persistent(id: impl Into<String>) -> Self {
        Self::Persistent(id.into())
    }
    pub fn private(id: impl Into<String>) -> Self {
        Self::Private(id.into())
    }
    pub fn key(&self) -> String {
        match self {
            Self::Persistent(id) => format!("p:{id}"),
            Self::Private(id) => format!("m:{id}"),
        }
    }
}

/// Permissions a page can ask for. Several can be requested at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Permission {
    Camera,
    Microphone,
    ScreenCapture,
    TabCapture,
    Geolocation,
    Notifications,
    ClipboardRead,
    MidiSysex,
    Midi,
    MultipleDownloads,
    StorageAccess,
    IdleDetection,
    WindowManagement,
    LocalFonts,
    ProtectedMedia,
    Other(u32),
}

/// A permission request. Answer it with [`PermissionRequest::allow`] or [`PermissionRequest::deny`]
/// (dropping it without answering denies it).
pub struct PermissionRequest {
    pub origin: String,
    pub permissions: Vec<Permission>,
    pub(crate) responder: Option<Box<dyn FnOnce(bool)>>,
}

impl PermissionRequest {
    pub fn allow(mut self) {
        if let Some(r) = self.responder.take() {
            r(true);
        }
    }
    pub fn deny(mut self) {
        if let Some(r) = self.responder.take() {
            r(false);
        }
    }
}

impl Drop for PermissionRequest {
    fn drop(&mut self) {
        if let Some(r) = self.responder.take() {
            r(false);
        }
    }
}

impl fmt::Debug for PermissionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PermissionRequest")
            .field("origin", &self.origin)
            .field("permissions", &self.permissions)
            .finish()
    }
}

/// A download about to start. Choose a path with [`DownloadRequest::save_to`] or [`DownloadRequest::cancel`].
pub struct DownloadRequest {
    pub id: u32,
    pub url: String,
    pub suggested_name: String,
    pub mime_type: String,
    pub total_bytes: i64,
    pub(crate) responder: Option<Box<dyn FnOnce(Option<String>)>>,
}

impl DownloadRequest {
    pub fn save_to(mut self, path: impl Into<String>) {
        if let Some(r) = self.responder.take() {
            r(Some(path.into()));
        }
    }
    pub fn cancel(mut self) {
        if let Some(r) = self.responder.take() {
            r(None);
        }
    }
}

impl Drop for DownloadRequest {
    fn drop(&mut self) {
        if let Some(r) = self.responder.take() {
            r(None);
        }
    }
}

impl fmt::Debug for DownloadRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DownloadRequest")
            .field("id", &self.id)
            .field("url", &self.url)
            .field("suggested_name", &self.suggested_name)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadState {
    InProgress,
    Complete,
    Canceled,
    Interrupted,
}

#[derive(Clone, Debug)]
pub struct DownloadProgress {
    pub id: u32,
    pub url: String,
    pub path: String,
    pub state: DownloadState,
    pub received_bytes: i64,
    pub total_bytes: i64,
    pub percent: i32,
    pub speed_bps: i64,
    pub paused: bool,
}

/// Key event delivered before the page sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyEvent {
    /// `true` for key down (raw or rawdown), `false` for key up.
    pub pressed: bool,
    pub windows_key_code: i32,
    pub native_key_code: i32,
    /// The typed character when there is one (lowercase for letters).
    pub character: Option<char>,
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub is_repeat: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ContextMenuInfo {
    pub x: i32,
    pub y: i32,
    pub link_url: Option<String>,
    pub image_url: Option<String>,
    pub media_url: Option<String>,
    pub page_url: String,
    pub frame_url: String,
    pub selection_text: String,
    pub misspelled_word: Option<String>,
    pub suggestions: Vec<String>,
    pub editable: bool,
    pub has_selection: bool,
    pub can_copy: bool,
    pub can_cut: bool,
    pub can_paste: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub can_select_all: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Pointer,
    Hand,
    IBeam,
    Cross,
    Wait,
    Help,
    Move,
    ResizeColumn,
    ResizeRow,
    ResizeDiagonal,
    NotAllowed,
    Grab,
    Grabbing,
    ZoomIn,
    ZoomOut,
    None,
    Other,
}

/// Events from the web layer, delivered on the main thread (see `Runtime::drain_events`).
#[derive(Debug)]
pub enum WebEvent {
    Created { view: ViewId },
    Closed { view: ViewId },
    TitleChanged { view: ViewId, title: String },
    AddressChanged { view: ViewId, url: String, is_main_frame: bool },
    FaviconUrls { view: ViewId, urls: Vec<String> },
    LoadingState { view: ViewId, is_loading: bool, can_go_back: bool, can_go_forward: bool },
    LoadProgress { view: ViewId, progress: f64 },
    LoadStart { view: ViewId, is_main_frame: bool, url: String },
    LoadEnd { view: ViewId, is_main_frame: bool, http_status: i32, url: String },
    LoadError { view: ViewId, code: i32, text: String, url: String, is_main_frame: bool },
    /// A page asked to open a new browsing context. The popup itself is always cancelled in CEF:
    /// the app opens `url` as a tab or window.
    NewWindowRequested { view: ViewId, url: String, disposition: Disposition, user_gesture: bool, target_name: String },
    FullscreenChanged { view: ViewId, fullscreen: bool },
    AudioPlayingChanged { view: ViewId, playing: bool },
    RenderProcessGone { view: ViewId, status: i32, error_code: i32, text: String },
    Console { view: ViewId, level: ConsoleLevel, message: String, source: String, line: i32 },
    /// Key event before the page. Return `true` from the key filter (`set_key_filter`) to swallow it.
    PreKey { view: ViewId, event: KeyEvent, handled: bool },
    FocusGained { view: ViewId },
    ContextMenu { view: ViewId, info: ContextMenuInfo },
    StatusText { view: ViewId, url: String },
    FindResult { view: ViewId, request_id: i32, count: i32, active_ordinal: i32, final_update: bool },
    CursorChanged { view: ViewId, cursor: CursorKind },
    Permission { view: ViewId, request: PermissionRequest },
    DownloadStarting { view: ViewId, request: DownloadRequest },
    DownloadProgress { view: ViewId, progress: DownloadProgress },
    JsDialog { view: ViewId, kind: JsDialogKind, origin: String, message: String },
    CertificateError { view: ViewId, code: i32, url: String },
    /// Message sent by page scripts through `window.__signetPost(channel, json)`.
    /// The sender is untrusted: validate `view` and `frame_url`.
    PageMessage {
        view: ViewId,
        frame_url: String,
        is_main_frame: bool,
        channel: String,
        payload: String,
        /// CEF frame identifier of the sender, usable with [`FrameTarget::Id`] to reply.
        frame_id: String,
        /// URL of the top (main) frame of the view, from the browser process.
        top_url: String,
    },
    /// A network request was blocked by the request filter (`set_request_filter`). For counters.
    RequestBlocked { view: ViewId, url: String, resource_type: ResourceType, is_main_frame: bool },
    /// A main frame response arrived (status, content type). Useful to trigger viewers (JSON...).
    MainFrameResponse { view: ViewId, url: String, status: i32, mime_type: String },
    /// Result of an asynchronous CDP call is delivered to its closure; this is a subscribed CDP event
    /// (see `WebView::devtools_subscribe`). `params` is JSON.
    DevToolsEvent { view: ViewId, method: String, params: String },
    /// Media session of a page (title, artist, artwork, state). `json` is the object described in the README.
    MediaSession { view: ViewId, info: MediaSessionInfo },
    /// A page script asked for a JS dialog and `set_js_dialog_mode(JsDialogMode::Events)` is on.
    /// Answer it with [`JsDialogRequest::accept`] / [`JsDialogRequest::dismiss`] (dropping dismisses it).
    JsDialogRequest { view: ViewId, request: JsDialogRequest },
    /// HTTP authentication (basic, digest, proxy). Answer with [`AuthRequest::submit`] / [`AuthRequest::cancel`].
    AuthRequest { view: ViewId, request: AuthRequest },
    /// Certificate error when the certificate handler answered [`CertificateDecision::Ask`].
    CertificateErrorPrompt { view: ViewId, request: CertificateErrorRequest },
}

impl WebEvent {
    pub fn view(&self) -> ViewId {
        use WebEvent::*;
        match self {
            Created { view } | Closed { view } | TitleChanged { view, .. } | AddressChanged { view, .. }
            | FaviconUrls { view, .. } | LoadingState { view, .. } | LoadProgress { view, .. }
            | LoadStart { view, .. } | LoadEnd { view, .. } | LoadError { view, .. }
            | NewWindowRequested { view, .. } | FullscreenChanged { view, .. }
            | AudioPlayingChanged { view, .. } | RenderProcessGone { view, .. } | Console { view, .. }
            | PreKey { view, .. } | FocusGained { view } | ContextMenu { view, .. }
            | StatusText { view, .. } | FindResult { view, .. } | CursorChanged { view, .. }
            | Permission { view, .. } | DownloadStarting { view, .. } | DownloadProgress { view, .. }
            | JsDialog { view, .. } | CertificateError { view, .. } | PageMessage { view, .. }
            | RequestBlocked { view, .. } | MainFrameResponse { view, .. } | DevToolsEvent { view, .. }
            | MediaSession { view, .. } | JsDialogRequest { view, .. } | AuthRequest { view, .. }
            | CertificateErrorPrompt { view, .. } => *view,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsDialogKind {
    Alert,
    Confirm,
    Prompt,
    BeforeUnload,
}

/// A script run at document start in every frame (or only the main frame).
#[derive(Clone, Debug)]
pub struct PageScript {
    pub name: String,
    pub source: String,
    pub main_frame_only: bool,
}

/// Response of a custom scheme handler.
#[derive(Clone, Debug)]
pub struct SchemeResponse {
    pub mime: String,
    pub body: Vec<u8>,
    pub status: u16,
}

pub type SchemeHandlerFn = dyn Fn(&str) -> Option<SchemeResponse> + Send + Sync;

/// Options for creating a view.
#[derive(Clone, Debug)]
pub struct ViewOptions {
    pub profile: ProfileKind,
    pub url: String,
    pub bounds: Bounds,
    pub visible: bool,
    pub transparent_background: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            profile: ProfileKind::persistent("default"),
            url: "about:blank".into(),
            bounds: Bounds::new(0.0, 0.0, 800.0, 600.0),
            visible: true,
            transparent_background: false,
        }
    }
}

#[derive(Debug)]
pub enum Error {
    NotInitialized,
    AlreadyInitialized,
    LibraryLoad(String),
    Init(String),
    Parent(String),
    Create(String),
    UnknownView(ViewId),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInitialized => write!(f, "CEF is not initialized"),
            Self::AlreadyInitialized => write!(f, "CEF is already initialized"),
            Self::LibraryLoad(s) => write!(f, "cannot load the CEF framework: {s}"),
            Self::Init(s) => write!(f, "CEF initialization failed: {s}"),
            Self::Parent(s) => write!(f, "invalid parent window: {s}"),
            Self::Create(s) => write!(f, "cannot create browser: {s}"),
            Self::UnknownView(v) => write!(f, "unknown view {v}"),
        }
    }
}

impl std::error::Error for Error {}


// ---------------------------------------------------------------------------------------------
// Frames, page bundle, network hooks
// ---------------------------------------------------------------------------------------------

/// Which frame of a view a call targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameTarget {
    /// The main frame.
    Main,
    /// The frame that has the focus (falls back to the main frame).
    Focused,
    /// A frame by CEF identifier (`WebEvent::PageMessage::frame_id`).
    Id(String),
    /// The first frame whose current URL equals this string.
    Url(String),
    /// Every frame of the view (main and sub frames).
    All,
}

/// What the page-script bundle hook knows about a document that is about to run.
#[derive(Clone, Debug)]
pub struct FrameInfo {
    pub view: ViewId,
    pub profile: ProfileKind,
    pub url: String,
    pub is_main_frame: bool,
    pub is_private: bool,
    /// CEF frame identifier.
    pub frame_id: String,
}

pub type FrameBundleFn = dyn Fn(&FrameInfo) -> Option<String> + Send + Sync;

/// Kind of resource of a network request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceType {
    MainFrame,
    SubFrame,
    Stylesheet,
    Script,
    Image,
    Font,
    SubResource,
    Object,
    Media,
    Worker,
    SharedWorker,
    Prefetch,
    Favicon,
    Xhr,
    Ping,
    ServiceWorker,
    CspReport,
    PluginResource,
    Other,
}

impl ResourceType {
    pub(crate) fn from_raw(v: i32) -> Self {
        match v {
            0 | 19 => Self::MainFrame,
            1 | 20 => Self::SubFrame,
            2 => Self::Stylesheet,
            3 => Self::Script,
            4 => Self::Image,
            5 => Self::Font,
            6 => Self::SubResource,
            7 => Self::Object,
            8 => Self::Media,
            9 => Self::Worker,
            10 => Self::SharedWorker,
            11 => Self::Prefetch,
            12 => Self::Favicon,
            13 => Self::Xhr,
            14 => Self::Ping,
            15 => Self::ServiceWorker,
            16 => Self::CspReport,
            17 => Self::PluginResource,
            _ => Self::Other,
        }
    }

    /// Name used by Chrome's declarativeNetRequest (`main_frame`, `sub_frame`, `stylesheet`...).
    pub fn dnr_name(&self) -> &'static str {
        match self {
            Self::MainFrame => "main_frame",
            Self::SubFrame => "sub_frame",
            Self::Stylesheet => "stylesheet",
            Self::Script => "script",
            Self::Image | Self::Favicon => "image",
            Self::Font => "font",
            Self::Object | Self::PluginResource => "object",
            Self::Media => "media",
            Self::Xhr | Self::SubResource | Self::Prefetch => "xmlhttprequest",
            Self::Ping | Self::CspReport => "ping",
            Self::Worker | Self::SharedWorker | Self::ServiceWorker => "other",
            Self::Other => "other",
        }
    }
}

/// A network request about to be sent. Built on the CEF IO thread.
#[derive(Clone, Debug)]
pub struct RequestInfo {
    pub view: ViewId,
    pub url: String,
    pub method: String,
    pub resource_type: ResourceType,
    /// URL of the top frame of the view (the page the user sees), `url` for a main frame request.
    pub first_party_url: String,
    pub is_main_frame: bool,
    /// Origin or URL of the document that started the request, when CEF knows it.
    pub initiator: String,
}

/// What to do with a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestDecision {
    Allow,
    /// Cancel the request (the page sees a network error).
    Block,
    /// Load this URL instead (internal redirect, e.g. an HTTPS upgrade).
    Redirect(String),
    /// Send these headers too (overwriting same-named ones), e.g. `Sec-GPC: 1`.
    AddHeaders(Vec<(String, String)>),
}

pub type RequestFilterFn = dyn Fn(&RequestInfo) -> RequestDecision + Send + Sync;

/// Response metadata of a main frame (or sub frame) resource, built on the IO thread.
#[derive(Clone, Debug)]
pub struct ResponseInfo {
    pub view: ViewId,
    pub url: String,
    pub status: i32,
    pub mime_type: String,
    pub headers: Vec<(String, String)>,
    pub is_main_frame: bool,
}

/// Answer of the response hook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResponseDecision {
    Continue,
    /// Load this URL instead of showing the response (e.g. to a viewer page).
    Redirect(String),
}

pub type ResponseHookFn = dyn Fn(&ResponseInfo) -> ResponseDecision + Send + Sync;

// ---------------------------------------------------------------------------------------------
// JS dialogs, auth, certificates
// ---------------------------------------------------------------------------------------------

/// Whether JS dialogs are drawn by the app (events) or by the platform (default).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum JsDialogMode {
    #[default]
    Native,
    Events,
}

/// A JS dialog to show. Answer with [`accept`](Self::accept) or [`dismiss`](Self::dismiss).
pub struct JsDialogRequest {
    pub kind: JsDialogKind,
    pub origin: String,
    pub message: String,
    /// Initial text of a prompt.
    pub default_prompt: String,
    /// For `BeforeUnload`: the dialog comes from a reload rather than a close or navigation.
    pub is_reload: bool,
    pub(crate) responder: Option<Box<dyn FnOnce(bool, String)>>,
}

impl JsDialogRequest {
    /// OK / leave. `text` is the prompt answer (ignored for alert and confirm).
    pub fn accept(mut self, text: Option<String>) {
        if let Some(r) = self.responder.take() {
            r(true, text.unwrap_or_default());
        }
    }
    /// Cancel / stay on the page.
    pub fn dismiss(mut self) {
        if let Some(r) = self.responder.take() {
            r(false, String::new());
        }
    }
}

impl Drop for JsDialogRequest {
    fn drop(&mut self) {
        if let Some(r) = self.responder.take() {
            r(false, String::new());
        }
    }
}

impl fmt::Debug for JsDialogRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsDialogRequest").field("kind", &self.kind).field("origin", &self.origin).finish()
    }
}

/// An HTTP authentication challenge.
pub struct AuthRequest {
    pub origin: String,
    pub host: String,
    pub port: i32,
    pub realm: String,
    pub scheme: String,
    pub is_proxy: bool,
    pub(crate) responder: Option<Box<dyn FnOnce(Option<(String, String)>)>>,
}

impl AuthRequest {
    pub fn submit(mut self, username: impl Into<String>, password: impl Into<String>) {
        if let Some(r) = self.responder.take() {
            r(Some((username.into(), password.into())));
        }
    }
    pub fn cancel(mut self) {
        if let Some(r) = self.responder.take() {
            r(None);
        }
    }
}

impl Drop for AuthRequest {
    fn drop(&mut self) {
        if let Some(r) = self.responder.take() {
            r(None);
        }
    }
}

impl fmt::Debug for AuthRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthRequest").field("host", &self.host).field("realm", &self.realm).finish()
    }
}

/// What the certificate handler decides for a failing certificate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateDecision {
    /// Cancel the load; CEF shows its error page (the default without handler).
    Cancel,
    /// Load anyway (the user already accepted this host).
    Proceed,
    /// Emit [`WebEvent::CertificateErrorPrompt`] and wait for the user.
    Ask,
}

/// Certificate error as given to the certificate handler.
#[derive(Clone, Debug)]
pub struct CertificateErrorInfo {
    pub view: ViewId,
    pub code: i32,
    pub url: String,
    /// Chromium `net::CertStatus` bit set of the certificate.
    pub cert_status: u32,
    pub subject: String,
    pub issuer: String,
}

pub type CertificateHandlerFn = dyn Fn(&CertificateErrorInfo) -> CertificateDecision;

/// A certificate error waiting for the user: [`proceed`](Self::proceed) loads the page anyway
/// (for this request only: remember the host in the app and answer `Proceed` next time).
pub struct CertificateErrorRequest {
    pub info: CertificateErrorInfo,
    pub(crate) responder: Option<Box<dyn FnOnce(bool)>>,
}

impl CertificateErrorRequest {
    pub fn proceed(mut self) {
        if let Some(r) = self.responder.take() {
            r(true);
        }
    }
    pub fn cancel(mut self) {
        if let Some(r) = self.responder.take() {
            r(false);
        }
    }
}

impl Drop for CertificateErrorRequest {
    fn drop(&mut self) {
        if let Some(r) = self.responder.take() {
            r(false);
        }
    }
}

impl fmt::Debug for CertificateErrorRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CertificateErrorRequest").field("info", &self.info).finish()
    }
}

// ---------------------------------------------------------------------------------------------
// Media, navigation history, CDP helpers
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaArtwork {
    pub src: String,
    pub sizes: String,
    pub mime: String,
}

/// State of the page's media session (`navigator.mediaSession`), reported by the injected media script.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaSessionInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: Vec<MediaArtwork>,
    /// `playing`, `paused` or `none`.
    pub playback_state: String,
    /// True when at least one media element of the frame is audibly playing.
    pub playing: bool,
    /// Actions the page registered a handler for (`play`, `pause`, `nexttrack`, `previoustrack`, `seekto`...).
    pub actions: Vec<String>,
    /// Frame that owns the session (target of `WebView::media_control`).
    pub frame_id: String,
    /// Duration and position in seconds when the page reports them, else 0.
    pub duration: f64,
    pub position: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    SeekBackward,
    SeekForward,
}

impl MediaCommand {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Toggle => "toggle",
            Self::Next => "nexttrack",
            Self::Previous => "previoustrack",
            Self::SeekBackward => "seekbackward",
            Self::SeekForward => "seekforward",
        }
    }
}

/// One entry of the back/forward list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Position in the list, oldest first (0-based).
    pub index: usize,
    pub url: String,
    pub display_url: String,
    pub title: String,
    pub current: bool,
    pub http_status: i32,
}

/// Device emulation for the responsive mode (CDP `Emulation`).
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceEmulation {
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: f64,
    pub mobile: bool,
    pub user_agent: Option<String>,
    pub touch: bool,
}

/// Image format of a screenshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg(u8),
    Webp(u8),
}

/// Options of `WebView::capture_screenshot` (CDP `Page.captureScreenshot`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenshotOptions {
    /// Region in page CSS pixels; `None` is the visible viewport.
    pub clip: Option<Bounds>,
    pub format: ImageFormat,
    /// Scale of the clip (1.0 = CSS pixels); ignored without `clip`.
    pub scale: f32,
    /// Also capture content outside the viewport (full page needs a `clip` of the page size).
    pub capture_beyond_viewport: bool,
}

impl Default for ScreenshotOptions {
    fn default() -> Self {
        Self { clip: None, format: ImageFormat::Png, scale: 1.0, capture_beyond_viewport: false }
    }
}

/// Which parts of a profile `clear_profile_data` wipes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClearData {
    pub cookies: bool,
    pub cache: bool,
    pub http_auth: bool,
    pub certificate_exceptions: bool,
    pub connections: bool,
}

impl Default for ClearData {
    fn default() -> Self {
        Self { cookies: true, cache: true, http_auth: true, certificate_exceptions: true, connections: true }
    }
}

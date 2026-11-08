//! Process-wide hooks and cross-thread plumbing.
//!
//! The request filter, the response hook and the frame bundle hook run on CEF threads other than the
//! main one (IO thread, bundle server threads), so they live in statics instead of the main-thread
//! `State`. Events produced on those threads go through [`push_io_event`] and join the normal event
//! queue in `drain_events`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::types::*;

pub(crate) static REQUEST_FILTER: RwLock<Option<Arc<RequestFilterFn>>> = RwLock::new(None);
pub(crate) static RESPONSE_HOOK: RwLock<Option<Arc<ResponseHookFn>>> = RwLock::new(None);
pub(crate) static FRAME_BUNDLE: RwLock<Option<Arc<FrameBundleFn>>> = RwLock::new(None);

static JS_DIALOG_MODE: AtomicU8 = AtomicU8::new(0);

pub(crate) fn js_dialog_mode() -> JsDialogMode {
    if JS_DIALOG_MODE.load(Ordering::Relaxed) == 1 { JsDialogMode::Events } else { JsDialogMode::Native }
}

/// Choose who draws `alert`, `confirm`, `prompt` and `beforeunload` dialogs. Default: the platform.
pub fn set_js_dialog_mode(mode: JsDialogMode) {
    JS_DIALOG_MODE.store(matches!(mode, JsDialogMode::Events) as u8, Ordering::Relaxed);
}

/// Install the request filter. It runs for every network request of every app view, synchronously on
/// the CEF IO thread: keep it fast (hash lookups), never block, never call back into signet-web.
///
pub fn set_request_filter(f: impl Fn(&RequestInfo) -> RequestDecision + Send + Sync + 'static) {
    if let Ok(mut g) = REQUEST_FILTER.write() {
        *g = Some(Arc::new(f) as Arc<RequestFilterFn>);
    }
}

/// Remove the request filter.
pub fn clear_request_filter() {
    if let Ok(mut g) = REQUEST_FILTER.write() {
        *g = None;
    }
}

/// Install the response hook (IO thread, same rules as the request filter). Called for main frame
/// and sub frame responses; the common case (`Continue`) must be cheap.
pub fn set_response_hook(f: impl Fn(&ResponseInfo) -> ResponseDecision + Send + Sync + 'static) {
    if let Ok(mut g) = RESPONSE_HOOK.write() {
        *g = Some(Arc::new(f) as Arc<ResponseHookFn>);
    }
}

/// Remove the response hook.
pub fn clear_response_hook() {
    if let Ok(mut g) = RESPONSE_HOOK.write() {
        *g = None;
    }
}

/// Install the page-script bundle hook, evaluated once per document before the page's own scripts, right
/// after `window.__signetPost` exists. The returned program runs in the page's main world. With
/// `signet_scripts` the closure is `|f| registry.bundle_for(&ctx_of(f), RunAt::DocumentStart)`.
/// It runs on a helper thread of the browser process (the renderer waits for the answer): `Send + Sync`,
/// fast, no call into signet-web.
pub fn set_frame_bundle(f: impl Fn(&FrameInfo) -> Option<String> + Send + Sync + 'static) {
    if let Ok(mut g) = FRAME_BUNDLE.write() {
        *g = Some(Arc::new(f) as Arc<FrameBundleFn>);
    }
}

/// Remove the frame bundle.
pub fn clear_frame_bundle() {
    if let Ok(mut g) = FRAME_BUNDLE.write() {
        *g = None;
    }
}

/// Events built off the main thread. Only `Send` data.
pub(crate) enum IoEvent {
    RequestBlocked { view: ViewId, url: String, resource_type: ResourceType, is_main_frame: bool },
    MainFrameResponse { view: ViewId, url: String, status: i32, mime_type: String },
    Auth { view: ViewId, request: SendAuth },
}

/// An `AuthRequest` crossing from the CEF IO thread to the main thread. The responder only holds a
/// ref-counted CEF `AuthCallback` (safe to use from any thread) and is run on the main thread.
pub(crate) struct SendAuth(pub AuthRequest);
// SAFETY: see above; the request is created on the IO thread and only consumed on the main thread.
unsafe impl Send for SendAuth {}

static IO_EVENTS: Mutex<VecDeque<IoEvent>> = Mutex::new(VecDeque::new());
const IO_EVENTS_MAX: usize = 4096;

pub(crate) fn push_io_event(e: IoEvent) {
    if let Ok(mut q) = IO_EVENTS.lock() {
        if q.len() >= IO_EVENTS_MAX {
            q.pop_front();
        }
        q.push_back(e);
    }
}

pub(crate) fn take_io_events() -> Vec<WebEvent> {
    let drained: Vec<IoEvent> = IO_EVENTS.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
    drained
        .into_iter()
        .map(|e| match e {
            IoEvent::RequestBlocked { view, url, resource_type, is_main_frame } => {
                WebEvent::RequestBlocked { view, url, resource_type, is_main_frame }
            }
            IoEvent::MainFrameResponse { view, url, status, mime_type } => {
                WebEvent::MainFrameResponse { view, url, status, mime_type }
            }
            IoEvent::Auth { view, request } => WebEvent::AuthRequest { view, request: request.0 },
        })
        .collect()
}

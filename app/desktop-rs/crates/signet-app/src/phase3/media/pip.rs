//! Port of `windows/pip.ts`: the custom mini player.
//!
//! In Electron the tab's real page view was moved into a small always-on-top window, where the video was made to
//! fill a full-screen container, and the `#pip` controls view lay over it. This port keeps the same page scripts,
//! the same state machine and the same geometry: `windows/pip.rs` re-parents the CEF view into a panel window
//! (`WebView::reparent`) and executes the [`PipWindowOp`]s and gestures defined here.
//!
//! What the window manager needs (see [`PipWindowOp`], [`PipLink`]):
//! - a frameless, transparent-capable, always-on-top, all-workspaces panel window ("floating", not in the Dock),
//!   showing the tab's CEF view (re-parented, `WebView::reparent` to add in signet-web) with
//!   `signet_ui::screens::windows::pip::Pip` in a transparent window laid over it;
//! - it drains [`PipLink::take_ops`] to open, resize (ratio change) and close the window, and
//!   [`PipLink::take_inputs`] for the drag and resize gestures coming from the controls (`PipHost`), which it
//!   executes with the pure functions of this file ([`initial_bounds`], [`resized_bounds`], [`release_target`]);
//! - it reports the window rectangle back with `Core::pip_moved`, to be remembered in `state.pipBounds`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::Value;
use signet_core::api::UiEvent;
use signet_core::types::{PipCommand, PipState, Rect};
use signet_ui::screens::windows::pip::{PipHost, PipResizePhase};
use signet_web::WebView;

use super::Done;
use crate::browser::Core;

/// Set to true when the window manager can show a `#pip` window with a re-parented tab view.
pub const PIP_WINDOWS: bool = true;

pub const DEFAULT_WIDTH: f64 = 400.0;
pub const MIN_WIDTH: f64 = 240.0;
pub const MARGIN: f64 = 16.0;
pub const SNAP: f64 = 48.0;
const STEP: f64 = 15.0;
const TICK: Duration = Duration::from_millis(500);
const MAX_VELOCITY: f64 = 8000.0;

// ---------------------------------------------------------------------------------------------- window protocol

/// What the window manager must do.
#[derive(Clone, Debug, PartialEq)]
pub enum PipWindowOp {
    /// Opens the window for the tab's view. `home`: the video rectangle in screen coordinates when it was displayed
    /// (the window is born there and glides to its place); `saved`: the remembered rectangle.
    Open { tab_id: String, ratio: f64, saved: Option<Rect>, home: Option<Rect> },
    /// The video proportions changed: resize keeping the width within the limits (see [`fit_ratio`]).
    Ratio { ratio: f64 },
    /// Closes the window and gives the view back to the main window. `to`: rectangle the window glides to first
    /// ("back to tab").
    Close { to: Option<Rect> },
}

/// Gesture coming from the controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PipInput {
    /// A press started (true) or ended (false) outside the controls: move the window with the pointer.
    Drag(bool),
    Resize(PipResizePhase),
}

/// Queues between the core, the controls (`PipHost`) and the window manager. Main thread only.
#[derive(Clone, Default)]
pub struct PipLink {
    ops: Rc<RefCell<VecDeque<PipWindowOp>>>,
    inputs: Rc<RefCell<VecDeque<PipInput>>>,
    /// Wakes the window manager (`Cmd::PipPump`) when something is queued.
    wake: Option<futures::channel::mpsc::UnboundedSender<crate::windows::Cmd>>,
}

impl PipLink {
    pub fn take_ops(&self) -> Vec<PipWindowOp> {
        self.ops.borrow_mut().drain(..).collect()
    }

    pub fn take_inputs(&self) -> Vec<PipInput> {
        self.inputs.borrow_mut().drain(..).collect()
    }

    fn op(&self, op: PipWindowOp) {
        self.ops.borrow_mut().push_back(op);
    }

    fn pump(&self) {
        if let Some(w) = &self.wake {
            let _ = w.unbounded_send(crate::windows::Cmd::PipPump);
        }
    }
}

/// `PipHost` of the controls: records the gestures for the window manager.
pub struct PipHostLink(PipLink);

impl PipHost for PipHostLink {
    fn drag(&self, on: bool) {
        self.0.inputs.borrow_mut().push_back(PipInput::Drag(on));
        self.0.pump();
    }

    fn resize(&self, phase: PipResizePhase) {
        self.0.inputs.borrow_mut().push_back(PipInput::Resize(phase));
        self.0.pump();
    }
}

/// The host to give to `signet_ui::screens::windows::pip::install_pip_host`.
pub fn pip_host(core: &Core) -> Rc<dyn PipHost> {
    let mut link = core.phase3.media.pip.link.clone();
    link.wake = Some(core.cmds.clone());
    Rc::new(PipHostLink(link))
}

impl Core {
    /// The queues the window manager drains.
    pub fn pip_link(&self) -> PipLink {
        self.phase3.media.pip.link.clone()
    }

    /// True as long as this tab's view belongs to the mini player: the main window must not display it.
    pub fn pip_owns(&self, tab: &str) -> bool {
        self.phase3.media.pip.owns(tab)
    }

    /// The window manager reports the mini player rectangle (screen coordinates) after a move or a resize.
    pub fn pip_moved(&mut self, bounds: Rect) {
        self.store.data.state.pip_bounds = Some(bounds);
        self.store.schedule(signet_core::core::store::SAVE_DELAY);
    }
}

// ---------------------------------------------------------------------------------------------- page scripts

/// Key of the mini-player state in the page.
const KEY: &str = "window[Symbol.for('signet.pip')]";

/// Finds the page's videos (shadow roots included); `seen`: only those that have an image.
const FIND: &str = "const all = []
  const walk = (root) => {
    for (const v of root.querySelectorAll('video')) all.push(v)
    for (const e of root.querySelectorAll('*')) if (e.shadowRoot) walk(e.shadowRoot)
  }
  walk(document)
  const seen = all.filter((e) => e.readyState > 0 && e.videoWidth > 0 && e.videoHeight > 0)";

/// Marks the video (the one playing, otherwise the largest) and moves it, in the same task, to a full-screen
/// container added to `body`. Returns its proportions and rectangle, or null.
fn open_script() -> String {
    format!(
        "(() => {{
  if ({KEY}) return null
  {FIND}
  const area = (v) => {{ const r = v.getBoundingClientRect(); return r.width * r.height }}
  const best = (list) => list.sort((a, b) => area(b) - area(a))[0]
  const v = best(seen.filter((e) => !e.paused && !e.ended)) || best(seen)
  if (!v) return null
  const wasPlaying = !v.paused
  const rb = v.getBoundingClientRect()
  const st = {{ v, parent: v.parentNode, next: v.nextSibling, style: v.getAttribute('style'), controls: v.controls }}
  const box = document.createElement('div')
  box.setAttribute('data-signet-pip', '')
  box.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#000;overflow:hidden;transform:none;margin:0;padding:0;border:0'
  const css = document.createElement('style')
  css.textContent = 'html, body {{ overflow: hidden !important; }} [data-signet-pip] > video {{ position: absolute !important; inset: 0 !important; left: 0 !important; top: 0 !important; width: 100% !important; height: 100% !important; max-width: none !important; max-height: none !important; object-fit: contain !important; transform: none !important; margin: 0 !important; }}'
  st.box = box
  st.css = css
  ;(document.head || document.documentElement).appendChild(css)
  ;(document.body || document.documentElement).appendChild(box)
  box.appendChild(v)
  v.controls = false
  {KEY} = st
  if (wasPlaying && v.paused) v.play().catch(() => {{}})
  return {{ w: v.videoWidth, h: v.videoHeight, rect: {{ x: rb.left, y: rb.top, width: rb.width, height: rb.height }} }}
}})()"
    )
}

/// Puts the video back exactly in its place (parent and next sibling) and removes the container.
fn restore_script(pause: bool) -> String {
    format!(
        "(() => {{
  const s = {KEY}
  if (!s) return false
  delete window[Symbol.for('signet.pip')]
  const v = s.v
  if ({pause}) v.pause()
  if (s.css) s.css.remove()
  if (v.parentNode !== s.box) {{
    s.box.remove()
    return true
  }}
  const wasPlaying = !v.paused
  if (s.style === null) v.removeAttribute('style'); else v.setAttribute('style', s.style)
  if (v.controls !== s.controls) v.controls = s.controls
  if (s.parent && s.parent.isConnected) s.parent.insertBefore(v, s.next && s.next.parentNode === s.parent ? s.next : null)
  s.box.remove()
  if (wasPlaying && v.paused) v.play().catch(() => {{}})
  return true
}})()"
    )
}

/// Video state; null if the marked element disappeared or was moved by the page.
fn read_script() -> String {
    format!(
        "(() => {{
  const s = {KEY}
  if (!s) return null
  const v = s.v
  if (!v.isConnected || !s.box.isConnected || v.parentNode !== s.box) return null
  return {{ playing: !v.paused && !v.ended, muted: v.muted || v.volume === 0, currentTime: v.currentTime, duration: Number.isFinite(v.duration) ? v.duration : 0, w: v.videoWidth, h: v.videoHeight }}
}})()"
    )
}

/// Script of a controls command (`Return` and `Close` are not page commands).
pub(super) fn command_script(command: PipCommand, value: f64) -> Option<String> {
    let act = |body: &str| format!("(() => {{ const s = {KEY}; const v = s && s.v; if (!v) return; {body} }})()");
    Some(match command {
        PipCommand::Toggle => act("if (v.paused) v.play().catch(() => {}); else v.pause()"),
        PipCommand::Back => act(&format!("v.currentTime = Math.max(0, v.currentTime - {STEP})")),
        PipCommand::Forward => act(&format!("v.currentTime = Number.isFinite(v.duration) ? Math.min(v.duration, v.currentTime + {STEP}) : v.currentTime + {STEP}")),
        PipCommand::Seek => act(&format!("v.currentTime = {}", if value.is_finite() { value.max(0.0) } else { 0.0 })),
        PipCommand::Mute => act("v.muted = !(v.muted || v.volume === 0); if (!v.muted && v.volume === 0) v.volume = 1"),
        PipCommand::Return | PipCommand::Close => return None,
    })
}

/// Evaluates an expression in the main frame and hands over the JSON value (`Null` if none).
fn eval(view: WebView, expression: &str, done: impl FnOnce(Value) + 'static) {
    let params = serde_json::json!({ "expression": expression, "returnByValue": true }).to_string();
    view.execute_devtools_method("Runtime.evaluate", &params, move |r| {
        let value = r.ok().and_then(|json| serde_json::from_str::<Value>(&json).ok()).map(|v| v["result"]["value"].clone()).unwrap_or(Value::Null);
        done(value);
    });
}

// ---------------------------------------------------------------------------------------------- state

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Found {
    pub w: f64,
    pub h: f64,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Reading {
    pub playing: bool,
    pub muted: bool,
    pub current_time: f64,
    pub duration: f64,
    pub w: f64,
    pub h: f64,
}

pub(super) enum PipDone {
    Opened { tab: String, auto: bool, away: bool, found: Option<Found> },
    /// `seq`: the command counter when the read started; a read older than the last command is stale.
    Read { tab: String, seq: u64, reading: Option<Reading> },
}

fn num(v: &Value, k: &str) -> f64 {
    v[k].as_f64().filter(|n| n.is_finite()).unwrap_or(0.0)
}

pub(super) fn parse_found(v: &Value) -> Option<Found> {
    if v.is_null() {
        return None;
    }
    let r = &v["rect"];
    Some(Found { w: num(v, "w"), h: num(v, "h"), rect: Rect::new(num(r, "x"), num(r, "y"), num(r, "width"), num(r, "height")) })
}

pub(super) fn parse_reading(v: &Value) -> Option<Reading> {
    if v.is_null() {
        return None;
    }
    Some(Reading {
        playing: v["playing"].as_bool().unwrap_or(false),
        muted: v["muted"].as_bool().unwrap_or(false),
        current_time: num(v, "currentTime"),
        duration: num(v, "duration"),
        w: num(v, "w"),
        h: num(v, "h"),
    })
}

struct Session {
    tab_id: String,
    ratio: f64,
    away: bool,
    home: Option<Rect>,
    opened_at: Instant,
    /// Replays of the state for the controls, which subscribe a little after their load.
    replays: u8,
    last_json: String,
    read_at: Option<Instant>,
    reading: bool,
    /// Bumped by every command sent to the page.
    seq: u64,
}

#[derive(Default)]
pub(super) struct Pip {
    link: PipLink,
    session: Option<Session>,
    /// Tab whose video is being moved right now.
    opening: Option<String>,
    prev_shown: Vec<String>,
    blurred_since: Option<Instant>,
}

impl Pip {
    /// True as long as this tab's view belongs to the mini player: the main window must not display it.
    pub(super) fn owns(&self, tab: &str) -> bool {
        self.session.as_ref().is_some_and(|s| s.tab_id == tab)
    }
}

/// Video rectangle (page CSS pixels) converted to screen coordinates; `None` if too little of it is visible.
/// `view`: the tab's view in window coordinates, `origin`: screen position of the window's content area.
pub(super) fn video_rect(rect: &Rect, view: &Rect, zoom: f64, origin: (f64, f64)) -> Option<Rect> {
    let x0 = (rect.x * zoom).max(0.0);
    let y0 = (rect.y * zoom).max(0.0);
    let x1 = ((rect.x + rect.width) * zoom).min(view.width);
    let y1 = ((rect.y + rect.height) * zoom).min(view.height);
    if x1 - x0 < 40.0 || y1 - y0 < 24.0 {
        return None;
    }
    Some(Rect::new((origin.0 + view.x + x0).round(), (origin.1 + view.y + y0).round(), (x1 - x0).round(), (y1 - y0).round()))
}

/// Proportions of a video, kept in a usable range.
pub(super) fn ratio_of(w: f64, h: f64) -> Option<f64> {
    (w > 0.0 && h > 0.0).then(|| (w / h).clamp(0.4, 3.0))
}

/// Queues an operation for the window manager and wakes it.
fn pip_op(core: &mut Core, op: PipWindowOp) {
    core.phase3.media.pip.link.op(op);
    core.send(crate::windows::Cmd::PipPump);
}

fn eligible(core: &Core, id: &str) -> Option<WebView> {
    let tab = core.state().tabs.get(id)?;
    let _ = tab;
    if core.store.data.detached.contains(id) {
        return None;
    }
    core.views.view_of(id)
}

impl Core {
    /// Opens the mini player for the tab's video (the active tab by default).
    pub fn open_pip(&mut self, tab_id: Option<&str>) {
        self.open_pip_with(tab_id, false, false);
    }

    fn open_pip_with(&mut self, tab_id: Option<&str>, auto: bool, away: bool) {
        if !PIP_WINDOWS {
            self.log("pip: no mini player window in this build");
            return;
        }
        let Some(id) = tab_id.map(str::to_string).or_else(|| self.active_tab_id()) else { return };
        let Some(view) = eligible(self, &id) else { return };
        let pip = &mut self.phase3.media.pip;
        if pip.owns(&id) || pip.opening.is_some() {
            return;
        }
        pip.opening = Some(id.clone());
        let outbox = self.phase3.media.outbox.clone();
        eval(view, &open_script(), move |v| outbox.push(Done::Pip(PipDone::Opened { tab: id, auto, away, found: parse_found(&v) })));
    }

    /// Command of the controls.
    pub fn pip_command(&mut self, command: PipCommand, value: Option<f64>) {
        let Some(id) = self.phase3.media.pip.session.as_ref().map(|s| s.tab_id.clone()) else { return };
        match command {
            PipCommand::Return => return_to_tab(self, &id),
            PipCommand::Close => close(self, CloseMode::Keep, None),
            _ => {
                if let (Some(view), Some(script)) = (self.views.view_of(&id), command_script(command, value.unwrap_or(0.0))) {
                    view.execute_js(&script);
                    // Read the result at once (the periodic read would show it up to 500 ms later).
                    if let Some(s) = self.phase3.media.pip.session.as_mut() {
                        s.seq += 1;
                        s.reading = true;
                    }
                    launch_read(self, view, id);
                }
                if let Some(s) = self.phase3.media.pip.session.as_mut() {
                    s.read_at = None;
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum CloseMode {
    /// Pauses the video.
    Pause,
    /// Playback continues in the tab.
    Keep,
    /// The page is no longer there: nothing to put back.
    Dispose,
}

fn close(core: &mut Core, mode: CloseMode, to: Option<Rect>) {
    let Some(s) = core.phase3.media.pip.session.take() else { return };
    pip_op(core, PipWindowOp::Close { to });
    if mode != CloseMode::Dispose
        && let Some(v) = core.views.view_of(&s.tab_id)
    {
        v.execute_js(&restore_script(mode == CloseMode::Pause));
    }
    core.changed_view();
}

fn return_to_tab(core: &mut Core, id: &str) {
    let home = core.phase3.media.pip.session.as_ref().and_then(|s| s.home);
    close(core, CloseMode::Keep, home);
    core.activate_tab(id);
    core.send(crate::windows::Cmd::ShowMain);
}

/// The tab's view is gone (tab closed, render process crashed).
pub(super) fn on_gone(core: &mut Core, tab: &str) {
    if core.phase3.media.pip.owns(tab) {
        close(core, CloseMode::Dispose, None);
    }
}

pub(super) fn on_quit(core: &mut Core) {
    close(core, CloseMode::Dispose, None);
}

pub(super) fn finish(core: &mut Core, done: PipDone) {
    match done {
        PipDone::Opened { tab, auto, away, found } => {
            core.phase3.media.pip.opening = None;
            let Some(found) = found else { return };
            let view = core.views.view_of(&tab);
            // The user came back in the meantime (automatic opening): put everything back.
            let stale = view.is_none() || (away && core.win.as_ref().is_some_and(|w| w.active)) || (auto && !away && core.shown_tabs.contains(&tab));
            if stale {
                if let Some(v) = view {
                    v.execute_js(&restore_script(false));
                }
                return;
            }
            close(core, CloseMode::Pause, None);
            let view_rect = core.panes.iter().find(|p| p.tab_id == tab).map(|p| p.rect);
            let zoom = view.map_or(1.0, |v| 1.2f64.powf(v.zoom_level()));
            let origin = core.win.as_ref().map_or((0.0, 0.0), |w| w.origin);
            let home = view_rect.and_then(|vr| video_rect(&found.rect, &vr, zoom, origin));
            let ratio = ratio_of(found.w, found.h).unwrap_or(16.0 / 9.0);
            let saved = core.state().pip_bounds;
            pip_op(core, PipWindowOp::Open { tab_id: tab.clone(), ratio, saved, home });
            core.phase3.media.pip.session =
                Some(Session { tab_id: tab, ratio, away, home, opened_at: Instant::now(), replays: 0, last_json: String::new(), read_at: None, reading: false, seq: 0 });
            core.changed_view();
        }
        PipDone::Read { tab, seq, reading } => {
            let Some(s) = core.phase3.media.pip.session.as_mut().filter(|s| s.tab_id == tab) else { return };
            // Launched before the last command: it shows the old state, a newer read is on its way.
            if seq < s.seq {
                return;
            }
            s.reading = false;
            let Some(r) = reading else { return close(core, CloseMode::Keep, None) };
            if let Some(ratio) = ratio_of(r.w, r.h)
                && (ratio - s.ratio).abs() >= 0.01
            {
                s.ratio = ratio;
                pip_op(core, PipWindowOp::Ratio { ratio });
            }
            let Some(t) = core.state().tabs.get(&tab) else { return };
            let site = signet_core::site::site_of(&t.url);
            let site = if site.is_empty() { t.url.clone() } else { site };
            let title = t.custom_title.clone().filter(|x| !x.is_empty()).or_else(|| Some(t.title.clone()).filter(|x| !x.is_empty())).unwrap_or_else(|| site.clone());
            let state = PipState { tab_id: tab, site, title, playing: r.playing, muted: r.muted, current_time: r.current_time, duration: r.duration, hue: core.hue() };
            let json = serde_json::to_string(&state).unwrap_or_default();
            let Some(s) = core.phase3.media.pip.session.as_mut() else { return };
            if json != s.last_json {
                s.last_json = json;
                core.emit(UiEvent::PipState(state));
            }
        }
    }
}

pub(super) fn tick(core: &mut Core) {
    if !PIP_WINDOWS {
        return;
    }
    let now = Instant::now();
    let shown = core.shown_tabs.clone();
    let pip = &mut core.phase3.media.pip;
    let prev = std::mem::replace(&mut pip.prev_shown, shown.clone());

    // The tab comes back to display: it takes its page back.
    if let Some(id) = pip.session.as_ref().map(|s| s.tab_id.clone())
        && shown.contains(&id)
        && !prev.contains(&id)
    {
        close(core, CloseMode::Keep, None);
    }

    let auto = core.state().settings.auto_pip;
    let playing = |core: &Core, id: &str| core.phase3.media.entries.get(id).is_some_and(|e| e.playing && e.heard) && core.views.view_of(id).is_some();

    // Tabs that leave the display while playing.
    if auto {
        for id in prev.iter().filter(|id| !shown.contains(id)) {
            if playing(core, id) && !core.phase3.media.pip.owns(id) {
                core.open_pip_with(Some(id), true, false);
            }
        }
    }

    // The main window loses the focus: mini player for the displayed video, as when switching tabs.
    let active = core.win.as_ref().is_some_and(|w| w.active);
    let pip = &mut core.phase3.media.pip;
    if active {
        pip.blurred_since = None;
        if pip.session.as_ref().is_some_and(|s| s.away) {
            close(core, CloseMode::Keep, None);
        }
    } else if pip.blurred_since.is_none() {
        pip.blurred_since = Some(now);
    } else if auto && pip.session.is_none() && pip.opening.is_none() && pip.blurred_since.is_some_and(|t| now.duration_since(t) >= Duration::from_millis(250)) && core.win.is_some() {
        let active_tab = core.active_tab_id();
        let pick = active_tab.filter(|a| shown.contains(a) && playing(core, a)).or_else(|| shown.iter().find(|id| playing(core, id)).cloned());
        if let Some(id) = pick {
            core.open_pip_with(Some(&id), true, true);
        }
    }

    // Reads the video and pushes the state to the controls.
    let Some(s) = core.phase3.media.pip.session.as_mut() else { return };
    if s.replays < 2 {
        let at = if s.replays == 0 { 300 } else { 1500 };
        if now.duration_since(s.opened_at) >= Duration::from_millis(at) {
            s.replays += 1;
            s.last_json.clear();
            s.read_at = None;
        }
    }
    if s.reading || s.read_at.is_some_and(|t| now.duration_since(t) < TICK) {
        return;
    }
    s.read_at = Some(now);
    s.reading = true;
    let id = s.tab_id.clone();
    let Some(view) = core.views.view_of(&id) else { return close(core, CloseMode::Dispose, None) };
    launch_read(core, view, id);
}

/// Reads the video; the result is delivered to the controls as soon as the window manager is woken.
fn launch_read(core: &mut Core, view: WebView, id: String) {
    let seq = core.phase3.media.pip.session.as_ref().map_or(0, |s| s.seq);
    let outbox = core.phase3.media.outbox.clone();
    let cmds = core.cmds.clone();
    eval(view, &read_script(), move |v| {
        outbox.push(Done::Pip(PipDone::Read { tab: id, seq, reading: parse_reading(&v) }));
        let _ = cmds.unbounded_send(crate::windows::Cmd::PipPump);
    });
}

// ---------------------------------------------------------------------------------------------- geometry

fn clamp(n: f64, min: f64, max: f64) -> f64 {
    n.min(max).max(min)
}

fn overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Distance a momentum (px/s) would cover while slowing down like a native scroll.
pub fn project(velocity: f64) -> f64 {
    let rate = 0.998;
    (velocity / 1000.0) * (rate / (1.0 - rate))
}

/// Progressive resistance beyond a limit.
pub fn rubberband(d: f64, dim: f64) -> f64 {
    let c = 0.55;
    if d == 0.0 || dim <= 0.0 { 0.0 } else { d.signum() * (d.abs() * dim * c) / (dim + c * d.abs()) }
}

/// Inverse of [`rubberband`]: the raw overshoot that produces this displayed overshoot.
pub fn unrubberband(r: f64, dim: f64) -> f64 {
    let c = 0.55;
    let a = r.abs().min(dim * 0.999);
    r.signum() * (a * dim) / (c * (dim - a))
}

/// Largest width the window may have: 60 percent of the work area, and a height of at most 90 percent.
fn max_initial_width(wa: &Rect, ratio: f64) -> f64 {
    MIN_WIDTH.max((wa.width * 0.6).min(wa.height * 0.9 * ratio).round())
}

/// Starting size and position: the remembered ones if still on screen, otherwise at the bottom right of the main
/// screen. `displays`: work areas of every screen; `home`: the main window's screen work area.
pub fn initial_bounds(ratio: f64, saved: Option<&Rect>, displays: &[Rect], home: &Rect) -> Rect {
    let wa = saved.and_then(|s| displays.iter().find(|d| overlap(d, s))).unwrap_or(home);
    let visible = saved.is_some_and(|s| displays.iter().any(|d| overlap(d, s)));
    let width = clamp(saved.map_or(DEFAULT_WIDTH, |s| s.width), MIN_WIDTH, max_initial_width(wa, ratio)).round();
    let height = (width / ratio).round();
    match saved {
        Some(s) if visible => Rect::new(clamp(s.x, wa.x, wa.x + wa.width - width), clamp(s.y, wa.y, wa.y + wa.height - height), width, height),
        _ => Rect::new(wa.x + wa.width - MARGIN - width, wa.y + wa.height - MARGIN - height, width, height),
    }
}

/// New size after a video ratio change: width kept within the limits, position kept on screen.
pub fn fit_ratio(b: &Rect, ratio: f64, wa: &Rect) -> Rect {
    let width = MIN_WIDTH.max(b.width.min((wa.height * 0.9 * ratio).floor()));
    let height = (width / ratio).round();
    Rect::new(clamp(b.x, wa.x, wa.x + wa.width - width), clamp(b.y, wa.y, wa.y + wa.height - height), width, height)
}

/// New rectangle for a handle (`n`, `s`, `e`, `w`, `ne`...) dragged by (dx, dy) from the start. Edge anchors: the
/// opposite side stays fixed; the free axis of an edge is centered. Limits [`MIN_WIDTH`], 90 percent of the work
/// area are elastic when `soft` (rubber band); the screen edge stays hard; without `soft`, everything is hard.
pub fn resized_bounds(s: &Rect, handle: &str, dx: f64, dy: f64, ratio: f64, wa: &Rect, soft: bool) -> Rect {
    let east = handle.contains('e');
    let west = handle.contains('w');
    let south = handle.contains('s');
    let north = handle.contains('n');
    let room_x = if east { wa.x + wa.width - s.x } else if west { s.x + s.width - wa.x } else { wa.width };
    let room_y = if south { wa.y + wa.height - s.y } else if north { s.y + s.height - wa.y } else { wa.height };
    let edge = MIN_WIDTH.max(room_x.min(room_y * ratio).floor());
    let max_w = edge.min(MIN_WIDTH.max((wa.width * 0.9).min(wa.height * 0.9 * ratio).floor()));
    let raw = if handle.len() == 2 {
        // The opposite corner (anchor) does not move: anchor to pointer vector, projected onto the diagonal.
        let vx = if east { s.width + dx } else { s.width - dx };
        let vy = if south { s.height + dy } else { s.height - dy };
        ratio * (ratio * vx + vy) / (ratio * ratio + 1.0)
    } else if east || west {
        if east { s.width + dx } else { s.width - dx }
    } else {
        (if south { s.height + dy } else { s.height - dy }) * ratio
    };
    let w = if !soft {
        clamp(raw, MIN_WIDTH, max_w)
    } else if raw > max_w {
        edge.min(max_w + rubberband(raw - max_w, max_w))
    } else if raw < MIN_WIDTH {
        MIN_WIDTH + rubberband(raw - MIN_WIDTH, MIN_WIDTH)
    } else {
        raw
    }
    .round();
    let h = (w / ratio).round();
    let x = if west {
        s.x + s.width - w
    } else if east {
        s.x
    } else {
        clamp((s.x + (s.width - w) / 2.0).round(), wa.x, wa.x + wa.width - w)
    };
    let y = if north {
        s.y + s.height - h
    } else if south {
        s.y
    } else {
        clamp((s.y + (s.height - h) / 2.0).round(), wa.y, wa.y + wa.height - h)
    };
    Rect::new(x, y, w, h)
}

/// Where the window lands after a drag released with velocity `v` (px/s). The projected point is kept [`MARGIN`]
/// inside the work area, then each axis snaps on its own to the margin line of the nearest edge when closer than
/// [`SNAP`]. (Electron's `release` snapped only when both axes were close to one corner and otherwise clamped with
/// no margin, so flicks landed flush against the Dock or the screen edge: this deliberately differs.) `area_at` gives
/// the work area of the screen nearest a point.
pub fn release_target(r: &Rect, v: (f64, f64), here: &Rect, area_at: &dyn Fn(f64, f64) -> Rect) -> Rect {
    let v = (clamp(v.0, -MAX_VELOCITY, MAX_VELOCITY), clamp(v.1, -MAX_VELOCITY, MAX_VELOCITY));
    let px = r.x + clamp(project(v.0), -here.width, here.width);
    let py = r.y + clamp(project(v.1), -here.height, here.height);
    let wa = area_at((px + r.width / 2.0).round(), (py + r.height / 2.0).round());
    let snap = |p: f64, lo: f64, size: f64, len: f64| {
        let (near, far) = (lo + MARGIN, lo + len - MARGIN - size);
        let c = clamp(p, near, near.max(far));
        if (c - near).abs() < SNAP {
            near
        } else if (c - far).abs() < SNAP {
            far.max(near)
        } else {
            c.round()
        }
    };
    Rect::new(snap(px, wa.x, r.width, wa.width), snap(py, wa.y, r.height, wa.height), r.width, r.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WA: Rect = Rect::new(0.0, 0.0, 1920.0, 1080.0);

    #[test]
    fn corner_resize_keeps_the_ratio_and_the_anchor() {
        let s = Rect::new(1500.0, 800.0, 400.0, 225.0);
        let r = resized_bounds(&s, "nw", -100.0, -56.25, 16.0 / 9.0, &WA, false);
        assert_eq!(r.width, 500.0);
        assert_eq!(r.height, 281.0);
        // The opposite (south-east) corner does not move.
        assert_eq!(r.x + r.width, 1900.0);
        assert_eq!(r.y + r.height, 1025.0);
    }

    #[test]
    fn edge_resize_centers_the_free_axis() {
        let s = Rect::new(100.0, 100.0, 400.0, 225.0);
        let r = resized_bounds(&s, "e", 80.0, 0.0, 16.0 / 9.0, &WA, false);
        assert_eq!(r.width, 480.0);
        assert_eq!(r.x, 100.0);
        assert_eq!(r.y, (100.0 + (225.0 - 270.0) / 2.0f64).round());
    }

    #[test]
    fn hard_limits_and_elastic_limits() {
        let s = Rect::new(100.0, 100.0, 400.0, 225.0);
        let hard = resized_bounds(&s, "e", -300.0, 0.0, 16.0 / 9.0, &WA, false);
        assert_eq!(hard.width, MIN_WIDTH);
        let soft = resized_bounds(&s, "e", -300.0, 0.0, 16.0 / 9.0, &WA, true);
        assert!(soft.width < MIN_WIDTH && soft.width > 0.0, "resists but follows: {}", soft.width);
        // The screen edge is hard even when soft.
        let edge = resized_bounds(&Rect::new(1500.0, 100.0, 400.0, 225.0), "e", 900.0, 0.0, 16.0 / 9.0, &WA, true);
        assert!(edge.x + edge.width <= 1920.0);
    }

    #[test]
    fn rubberband_roundtrip() {
        for d in [5.0, 40.0, 200.0] {
            let b = rubberband(d, 300.0);
            assert!((unrubberband(b, 300.0) - d).abs() < 1e-6, "{d}");
        }
        assert_eq!(rubberband(0.0, 300.0), 0.0);
        assert!(rubberband(-10.0, 300.0) < 0.0);
    }

    #[test]
    fn initial_bounds_default_and_remembered() {
        let d = [WA];
        let r = initial_bounds(16.0 / 9.0, None, &d, &WA);
        assert_eq!((r.width, r.height), (400.0, 225.0));
        assert_eq!((r.x, r.y), (1920.0 - 16.0 - 400.0, 1080.0 - 16.0 - 225.0));
        let saved = Rect::new(100.0, 100.0, 600.0, 338.0);
        let r = initial_bounds(16.0 / 9.0, Some(&saved), &d, &WA);
        assert_eq!((r.x, r.y, r.width), (100.0, 100.0, 600.0));
        // Off every screen: back to the corner.
        let gone = Rect::new(9000.0, 9000.0, 600.0, 338.0);
        let r = initial_bounds(16.0 / 9.0, Some(&gone), &d, &WA);
        assert_eq!(r.x, 1920.0 - 16.0 - 600.0);
    }

    #[test]
    fn release_snaps_to_a_corner() {
        let r = Rect::new(1480.0, 810.0, 400.0, 225.0);
        let t = release_target(&r, (0.0, 0.0), &WA, &|_, _| WA);
        assert_eq!((t.x, t.y), (1920.0 - 16.0 - 400.0, 1080.0 - 16.0 - 225.0));
        // Far from any corner: stays where it is.
        let r = Rect::new(800.0, 400.0, 400.0, 225.0);
        let t = release_target(&r, (0.0, 0.0), &WA, &|_, _| WA);
        assert_eq!((t.x, t.y), (800.0, 400.0));
    }

    #[test]
    fn release_keeps_the_margin_and_snaps_each_axis() {
        let r = Rect::new(800.0, 400.0, 400.0, 225.0);
        // A hard throw into the corner lands on the margin lines, never flush with the edges.
        let t = release_target(&r, (3000.0, 3000.0), &WA, &|_, _| WA);
        assert_eq!((t.x, t.y), (1920.0 - 16.0 - 400.0, 1080.0 - 16.0 - 225.0));
        // Close to the right edge on x, overshooting on y: both axes end on the margin.
        let r = Rect::new(1490.0, 700.0, 400.0, 225.0);
        let t = release_target(&r, (0.0, 3000.0), &WA, &|_, _| WA);
        assert_eq!((t.x, t.y), (1920.0 - 16.0 - 400.0, 1080.0 - 16.0 - 225.0));
        // A slow drop in the middle stays where it is.
        let r = Rect::new(800.0, 400.0, 400.0, 225.0);
        let t = release_target(&r, (0.0, 0.0), &WA, &|_, _| WA);
        assert_eq!((t.x, t.y), (800.0, 400.0));
        // Dropped past the left edge: the margin.
        let r = Rect::new(-100.0, 400.0, 400.0, 225.0);
        let t = release_target(&r, (0.0, 0.0), &WA, &|_, _| WA);
        assert_eq!((t.x, t.y), (16.0, 400.0));
    }

    #[test]
    fn ratio_change_keeps_the_window_on_screen() {
        let b = Rect::new(1600.0, 900.0, 400.0, 225.0);
        let r = fit_ratio(&b, 1.0, &WA);
        assert_eq!((r.width, r.height), (400.0, 400.0));
        assert!(r.x + r.width <= 1920.0 && r.y + r.height <= 1080.0);
    }

    #[test]
    fn video_rect_is_clipped_to_the_view() {
        let view = Rect::new(300.0, 80.0, 1000.0, 700.0);
        let r = video_rect(&Rect::new(-20.0, 10.0, 1400.0, 500.0), &view, 1.0, (50.0, 30.0)).unwrap();
        assert_eq!((r.x, r.y, r.width, r.height), (350.0, 120.0, 1000.0, 500.0));
        assert!(video_rect(&Rect::new(990.0, 0.0, 200.0, 200.0), &view, 1.0, (0.0, 0.0)).is_none());
    }

    #[test]
    fn ratio_is_bounded() {
        assert_eq!(ratio_of(1920.0, 1080.0).map(|r| (r * 100.0).round()), Some(178.0));
        assert_eq!(ratio_of(100.0, 1000.0), Some(0.4));
        assert_eq!(ratio_of(1000.0, 100.0), Some(3.0));
        assert!(ratio_of(0.0, 10.0).is_none());
    }

    #[test]
    fn commands_map_to_scripts() {
        assert!(command_script(PipCommand::Toggle, 0.0).unwrap().contains("v.paused"));
        assert!(command_script(PipCommand::Seek, 12.5).unwrap().contains("v.currentTime = 12.5"));
        assert!(command_script(PipCommand::Seek, f64::NAN).unwrap().contains("v.currentTime = 0"));
        assert!(command_script(PipCommand::Return, 0.0).is_none());
        assert!(command_script(PipCommand::Close, 0.0).is_none());
    }

    #[test]
    fn page_replies_are_parsed() {
        let v: Value = serde_json::json!({"w": 1280, "h": 720, "rect": {"x": 1, "y": 2, "width": 3, "height": 4}});
        let f = parse_found(&v).unwrap();
        assert_eq!((f.w, f.rect.width), (1280.0, 3.0));
        assert!(parse_found(&Value::Null).is_none());
        let r = parse_reading(&serde_json::json!({"playing": true, "muted": false, "currentTime": 3.5, "duration": 0, "w": 10, "h": 5})).unwrap();
        assert!(r.playing && r.current_time == 3.5 && r.duration == 0.0);
        assert!(parse_reading(&Value::Null).is_none());
    }
}

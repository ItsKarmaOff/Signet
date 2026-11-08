//! PHASE3: screenshots (capture), tab hover previews, media state and controls, mini player (pip), audio state.
//!
//! Port of `browser/media.ts` (this file), `windows/pip.ts` (`pip.rs`), `features/capture.ts` (`capture.rs`) and
//! `browser/preview.ts` (`preview.rs`).
//!
//! CEF answers asynchronously (screenshots, DevTools calls) through closures that cannot reach `Core`. They push
//! their result into an [`Outbox`] kept in [`State`]; the `web_event` and `tick` hooks drain it.

mod capture;
mod pip;
mod preview;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use signet_core::api::SkipDirection;
use signet_core::ids::now_ms;
use signet_core::types::MediaInfo;
use signet_web::{MediaCommand, MediaSessionInfo, WebEvent};

use crate::browser::Core;

pub use pip::{PipHostLink, PipInput, PipLink, PipWindowOp, fit_ratio, initial_bounds, pip_host, release_target, resized_bounds};

/// A paused tab stays a candidate for 10 minutes.
const PAUSED_TTL_MS: i64 = 10 * 60_000;

/// Results of asynchronous CEF calls, waiting for the next `web_event` or `tick`.
enum Done {
    Capture(capture::Finished),
    Thumb(preview::Thumb),
    Pip(pip::PipDone),
}

/// Shared queue between the CEF closures and the hooks (main thread only).
#[derive(Clone, Default)]
struct Outbox(Rc<RefCell<Vec<Done>>>);

impl Outbox {
    fn push(&self, d: Done) {
        self.0.borrow_mut().push(d);
    }

    fn take(&self) -> Vec<Done> {
        std::mem::take(&mut *self.0.borrow_mut())
    }
}

/// Media state of one tab, plus the facts of the tab needed to build the card without reaching the store.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Entry {
    /// Something is audible in the tab (CEF audio state).
    audible: bool,
    /// The page's media session says it is playing.
    session_playing: bool,
    /// Effective state: `audible || session_playing`.
    playing: bool,
    /// Last start or last pause (ms).
    active_at: i64,
    title: Option<String>,
    artist: Option<String>,
    artwork: Option<String>,
    /// The page declared handlers for previous / next track.
    can_skip: bool,
    /// The tab actually emitted sound (a silent decorative video never counts).
    heard: bool,
    tab_title: String,
    tab_url: String,
    muted: bool,
    /// Detached tabs (Little, private, web apps) never show a card.
    detached: bool,
}

impl Entry {
    fn recompute(&mut self, now: i64) {
        let playing = self.audible || self.session_playing;
        if playing != self.playing {
            self.playing = playing;
            self.active_at = now;
        }
    }

    fn candidate(&self, now: i64) -> bool {
        self.heard && (self.playing || now - self.active_at < PAUSED_TTL_MS)
    }
}

/// Last artwork of the session (the largest, by convention), if any.
pub(super) fn artwork_of(info: &MediaSessionInfo) -> Option<String> {
    info.artwork.iter().rev().map(|a| a.src.clone()).find(|s| !s.is_empty())
}

/// Whether the page declared previous / next track handlers.
pub(super) fn can_skip(actions: &[String]) -> bool {
    actions.iter().any(|a| a == "nexttrack" || a == "previoustrack")
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    entries: HashMap<String, Entry>,
    /// Card last pushed to the interface, to push again only on a change.
    last_card: Option<MediaInfo>,
    outbox: Outbox,
    /// Fake card shown instead of the real one (dev harness, `SIGNET_MEDIA=1`).
    pub debug: Option<MediaInfo>,
    capture: capture::Capture,
    preview: preview::Previews,
    pip: pip::Pip,
}

impl State {
    pub(super) fn on_audio(&mut self, tab: &str, playing: bool, now: i64) {
        let e = self.entries.entry(tab.to_string()).or_default();
        e.audible = playing;
        if playing {
            e.heard = true;
        }
        e.recompute(now);
    }

    pub(super) fn on_session(&mut self, tab: &str, info: &MediaSessionInfo, now: i64) {
        let e = self.entries.entry(tab.to_string()).or_default();
        if info.playback_state == "none" {
            e.title = None;
            e.artist = None;
            e.artwork = None;
            e.can_skip = false;
            e.session_playing = false;
        } else {
            e.title = Some(info.title.clone()).filter(|s| !s.is_empty());
            e.artist = Some(info.artist.clone()).filter(|s| !s.is_empty());
            e.artwork = artwork_of(info);
            e.can_skip = can_skip(&info.actions);
            e.session_playing = info.playing || info.playback_state == "playing";
        }
        e.recompute(now);
    }

    /// Most recent candidate among the tabs that are not displayed.
    pub(super) fn current(&self, displayed: &[String], now: i64) -> Option<MediaInfo> {
        let (tab, e) = self
            .entries
            .iter()
            .filter(|(id, e)| !e.detached && !displayed.contains(id) && e.candidate(now))
            .max_by(|a, b| a.1.active_at.cmp(&b.1.active_at).then_with(|| a.1.playing.cmp(&b.1.playing)).then_with(|| b.0.cmp(a.0)))?;
        let site = signet_core::site::site_of(&e.tab_url);
        Some(MediaInfo {
            tab_id: tab.clone(),
            title: e.title.clone().or_else(|| Some(e.tab_title.clone()).filter(|t| !t.is_empty())).unwrap_or_else(|| e.tab_url.clone()),
            artist: e.artist.clone(),
            artwork: e.artwork.clone(),
            site: if site.is_empty() { e.tab_url.clone() } else { site },
            playing: e.playing,
            muted: e.muted,
            can_skip: e.can_skip,
        })
    }

    /// True if the tab is playing, or played sound recently: not to be unloaded.
    pub(super) fn is_active(&self, tab: &str, now: i64) -> bool {
        self.entries.get(tab).is_some_and(|e| e.playing || e.candidate(now))
    }
}

/// Most recent media of a tab that is not displayed (`Phase3::media_for`).
pub(super) fn media_for(state: &State, displayed: &[String]) -> Option<MediaInfo> {
    if state.debug.is_some() {
        return state.debug.clone();
    }
    state.current(displayed, now_ms())
}

/// True if the tab must not be put to sleep: mini player, media playing or played recently (`Phase3::keeps_awake`).
pub(super) fn keeps_awake(state: &State, tab: &str) -> bool {
    state.pip.owns(tab) || state.is_active(tab, now_ms())
}

impl Core {
    /// Play / pause of the tab's media (through its media session, else its media elements).
    pub fn media_toggle(&mut self, tab_id: &str) {
        if let Some(v) = self.views.view_of(tab_id) {
            v.media_control(MediaCommand::Toggle);
        }
    }

    /// Next / previous track, through the page's media session handlers.
    pub fn media_skip(&mut self, tab_id: &str, direction: SkipDirection) {
        if let Some(v) = self.views.view_of(tab_id) {
            v.media_control(if direction == SkipDirection::Next { MediaCommand::Next } else { MediaCommand::Previous });
        }
    }
}

/// Refreshes the facts copied from the store; forgets tabs that are gone.
fn sync_facts(core: &mut Core) {
    let tabs = &core.store.data.state.tabs;
    let detached = &core.store.data.detached;
    let views = &core.views;
    core.phase3.media.entries.retain(|id, _| tabs.contains_key(id) && (views.view_of(id).is_some() || tabs.get(id).is_some_and(|t| t.loaded)));
    for (id, e) in core.phase3.media.entries.iter_mut() {
        if let Some(t) = tabs.get(id) {
            e.tab_title = if t.title.is_empty() { String::new() } else { t.title.clone() };
            e.tab_url = t.url.clone();
            e.muted = t.muted;
            e.detached = detached.contains(id);
        }
    }
}

/// Sends the media card to the interface when it changed (no layout, no save).
fn refresh_card(core: &mut Core) {
    sync_facts(core);
    let card = media_for(&core.phase3.media, &core.shown_tabs);
    if card != core.phase3.media.last_card {
        core.phase3.media.last_card = card;
        core.dirty.needs_full = true;
        core.apply();
    }
}

/// Keeps a muted tab muted when its view was created after the mute.
fn apply_mute(core: &Core, tab: &str) {
    if core.state().tabs.get(tab).is_some_and(|t| t.muted)
        && let Some(v) = core.views.view_of(tab)
        && !v.is_audio_muted()
    {
        v.set_audio_muted(true);
    }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(_core: &mut Core) {}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    drain(core);
    let Some(tab) = core.views.tab_of(ev.view()).map(str::to_string) else { return };
    let now = now_ms();
    match ev {
        WebEvent::AudioPlayingChanged { playing, .. } => {
            core.phase3.media.on_audio(&tab, *playing, now);
            refresh_card(core);
        }
        WebEvent::MediaSession { info, .. } => {
            core.phase3.media.on_session(&tab, info, now);
            refresh_card(core);
        }
        WebEvent::LoadStart { is_main_frame: true, .. } => apply_mute(core, &tab),
        WebEvent::LoadEnd { is_main_frame: true, .. } => preview::on_loaded(core, &tab),
        WebEvent::Closed { .. } => {
            core.phase3.media.entries.remove(&tab);
            core.phase3.media.preview.forget(&tab);
            if core.phase3.media.pip.owns(&tab) {
                pip::on_gone(core, &tab);
            }
            refresh_card(core);
        }
        WebEvent::RenderProcessGone { .. } => {
            if core.phase3.media.pip.owns(&tab) {
                pip::on_gone(core, &tab);
            }
        }
        _ => {}
    }
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    drain(core);
    refresh_card(core);
    preview::tick(core);
    pip::tick(core);
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    pip::on_quit(core);
}

/// Handles the results of asynchronous calls.
fn drain(core: &mut Core) {
    for done in core.phase3.media.outbox.take() {
        match done {
            Done::Capture(f) => capture::finish(core, f),
            Done::Thumb(t) => preview::store(core, t),
            Done::Pip(p) => pip::finish(core, p),
        }
    }
}

/// Standard base64 (RFC 4648, padded), for data URLs.
pub(super) fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_web::MediaArtwork;

    fn session(state: &str, playing: bool, actions: &[&str]) -> MediaSessionInfo {
        MediaSessionInfo {
            title: "Song".into(),
            artist: "Band".into(),
            artwork: vec![MediaArtwork { src: "a.png".into(), sizes: "96x96".into(), mime: String::new() }, MediaArtwork { src: "b.png".into(), sizes: "512x512".into(), mime: String::new() }],
            playback_state: state.into(),
            playing,
            actions: actions.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn facts(s: &mut State, tab: &str, title: &str, url: &str) {
        let e = s.entries.get_mut(tab).unwrap();
        e.tab_title = title.into();
        e.tab_url = url.into();
    }

    #[test]
    fn base64_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn silent_video_is_never_a_candidate() {
        let mut s = State::default();
        s.on_session("t1", &session("playing", true, &[]), 1000);
        facts(&mut s, "t1", "Video", "https://example.com/v");
        assert!(s.current(&[], 1000).is_none());
        assert!(s.entries["t1"].playing);
    }

    #[test]
    fn heard_tab_gives_a_card_and_expires_after_a_pause() {
        let mut s = State::default();
        s.on_audio("t1", true, 1000);
        s.on_session("t1", &session("playing", true, &["nexttrack"]), 1000);
        facts(&mut s, "t1", "Tab title", "https://www.music.example.co.uk/x");
        let c = s.current(&[], 2000).unwrap();
        assert_eq!(c.title, "Song");
        assert_eq!(c.artist.as_deref(), Some("Band"));
        assert_eq!(c.artwork.as_deref(), Some("b.png"));
        assert!(c.playing && c.can_skip);
        assert_eq!(c.site, "example.co.uk");
        // Paused: stays 10 minutes.
        s.on_audio("t1", false, 5000);
        s.on_session("t1", &session("paused", false, &["nexttrack"]), 5000);
        assert!(!s.current(&[], 5000).unwrap().playing);
        assert!(s.current(&[], 5000 + PAUSED_TTL_MS - 1).is_some());
        assert!(s.current(&[], 5000 + PAUSED_TTL_MS).is_none());
    }

    #[test]
    fn displayed_and_detached_tabs_are_skipped() {
        let mut s = State::default();
        s.on_audio("a", true, 1000);
        s.on_audio("b", true, 2000);
        facts(&mut s, "a", "A", "https://a.example/");
        facts(&mut s, "b", "B", "https://b.example/");
        assert_eq!(s.current(&[], 3000).unwrap().tab_id, "b");
        assert_eq!(s.current(&["b".to_string()], 3000).unwrap().tab_id, "a");
        s.entries.get_mut("a").unwrap().detached = true;
        assert!(s.current(&["b".to_string()], 3000).is_none());
    }

    #[test]
    fn title_falls_back_to_the_tab_then_the_address() {
        let mut s = State::default();
        s.on_audio("a", true, 1);
        facts(&mut s, "a", "Tab title", "https://a.example/");
        assert_eq!(s.current(&[], 2).unwrap().title, "Tab title");
        facts(&mut s, "a", "", "https://a.example/");
        assert_eq!(s.current(&[], 2).unwrap().title, "https://a.example/");
    }

    #[test]
    fn session_none_clears_the_metadata() {
        let mut s = State::default();
        s.on_audio("a", true, 1);
        s.on_session("a", &session("playing", true, &["nexttrack"]), 1);
        s.on_session("a", &MediaSessionInfo { playback_state: "none".into(), ..Default::default() }, 2);
        let e = &s.entries["a"];
        assert!(e.title.is_none() && !e.can_skip);
        assert!(e.playing, "still audible");
    }

    #[test]
    fn keeps_awake_while_playing_or_recently_paused() {
        let mut s = State::default();
        s.on_audio("a", true, 1000);
        assert!(s.is_active("a", 1000));
        s.on_audio("a", false, 2000);
        assert!(s.is_active("a", 2000 + PAUSED_TTL_MS - 1));
        assert!(!s.is_active("a", 2000 + PAUSED_TTL_MS));
        assert!(!s.is_active("zz", 0));
    }

    #[test]
    fn skip_needs_a_track_handler() {
        assert!(can_skip(&["play".into(), "nexttrack".into()]));
        assert!(!can_skip(&["play".into(), "pause".into()]));
    }
}

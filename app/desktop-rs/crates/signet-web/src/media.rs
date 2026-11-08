//! Audio state and media session of pages, reported by the injected `media.js` (see README:
//! CEF only offers audio through a capturing `AudioHandler`, which would silence the page).

use std::collections::HashMap;

use crate::runtime::{push_event, with_state};
use crate::types::*;

#[derive(Default)]
pub(crate) struct AudioState {
    /// Per frame: is something audibly playing.
    pub frames: HashMap<String, bool>,
    pub playing: bool,
    /// Frame that owns the last media session (target of `media_control`).
    pub session_frame: Option<String>,
}

fn json_str(v: &serde_json::Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

pub(crate) fn on_report(view: ViewId, frame_id: &str, kind: &str, json: &str) {
    match kind {
        "audio" => {
            let playing = json == "1";
            let changed = with_state(|st| {
                let a = st.audio.entry(view).or_default();
                a.frames.insert(frame_id.to_string(), playing);
                let any = a.frames.values().any(|p| *p);
                let changed = any != a.playing;
                a.playing = any;
                changed.then_some(any)
            })
            .flatten();
            if let Some(playing) = changed {
                push_event(WebEvent::AudioPlayingChanged { view, playing });
            }
        }
        "session" => {
            let info = if json == "null" {
                MediaSessionInfo { playback_state: "none".into(), frame_id: frame_id.into(), ..Default::default() }
            } else {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return };
                MediaSessionInfo {
                    title: json_str(&v, "title"),
                    artist: json_str(&v, "artist"),
                    album: json_str(&v, "album"),
                    artwork: v["artwork"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|x| MediaArtwork { src: json_str(x, "src"), sizes: json_str(x, "sizes"), mime: json_str(x, "type") })
                                .collect()
                        })
                        .unwrap_or_default(),
                    playback_state: json_str(&v, "state"),
                    playing: v["playing"].as_bool().unwrap_or(false),
                    actions: v["actions"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
                    frame_id: frame_id.into(),
                    duration: v["duration"].as_f64().unwrap_or(0.0),
                    position: v["position"].as_f64().unwrap_or(0.0),
                }
            };
            with_state(|st| {
                let a = st.audio.entry(view).or_default();
                if json == "null" {
                    if a.session_frame.as_deref() == Some(frame_id) {
                        a.session_frame = None;
                    }
                } else {
                    a.session_frame = Some(frame_id.to_string());
                }
            });
            push_event(WebEvent::MediaSession { view, info });
        }
        "dbg" => {
            if std::env::var_os("SIGNET_WEB_DEBUG").is_some() {
                eprintln!("[signet-web] media script: {json}");
            }
        }
        _ => {}
    }
}

/// Forget the audio state of a view (navigation, crash, close) and tell the app if it was playing.
pub(crate) fn reset(view: ViewId) {
    let was_playing = with_state(|st| st.audio.remove(&view).map(|a| a.playing)).flatten().unwrap_or(false);
    if was_playing {
        push_event(WebEvent::AudioPlayingChanged { view, playing: false });
        push_event(WebEvent::MediaSession {
            view,
            info: MediaSessionInfo { playback_state: "none".into(), ..Default::default() },
        });
    }
}

/// Frame that owns the media session of a view.
pub(crate) fn session_frame(view: ViewId) -> Option<String> {
    with_state(|st| st.audio.get(&view).and_then(|a| a.session_frame.clone())).flatten()
}

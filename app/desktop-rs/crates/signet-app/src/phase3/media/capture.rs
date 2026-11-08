//! Port of `features/capture.ts`: the screenshot tool (area, visible part or whole page; copy, save or send to an Easel).
//!
//! The tool UI (`screens::overlay::Capture`) calls `take_capture` and expects a synchronous answer, but CEF
//! screenshots are asynchronous. The result is therefore handled by the app once the image arrives:
//! - `Save`: the PNG is written to the download folder (no file dialog exists in this port) and revealed.
//! - `Easel`: a new Easel with the image is created and opened in a tab.
//! - `Copy`: the app puts the PNG on the system clipboard itself and answers `ok` without `png`.

use std::path::Path;

use signet_core::api::{CaptureAction, CaptureResult};
use signet_core::types::{CaptureTarget, OverlayPayload, Rect};
use signet_web::{ImageFormat, ScreenshotOptions, WebView};

use super::{base64, Done};
use crate::browser::Core;

/// Longest side of a full-page capture (px): beyond it, the engine fails or the image becomes huge.
const MAX_SIDE: f64 = 16384.0;
/// Longest side of an image sent to an Easel (the limit for images in an Easel).
const EASEL_SIDE: f64 = 1600.0;

/// Page measures read in one call before the screenshot.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub(super) struct Metrics {
    pub x: f64,
    pub y: f64,
    pub vw: f64,
    pub vh: f64,
    pub cw: f64,
    pub ch: f64,
    pub dpr: f64,
}

pub(super) const METRICS_JS: &str = "JSON.stringify({x:(window.visualViewport?visualViewport.pageLeft:scrollX)||0,y:(window.visualViewport?visualViewport.pageTop:scrollY)||0,vw:innerWidth,vh:innerHeight,cw:Math.max(document.documentElement.scrollWidth,document.body?document.body.scrollWidth:0),ch:Math.max(document.documentElement.scrollHeight,document.body?document.body.scrollHeight:0),dpr:window.devicePixelRatio||1})";

/// What to ask the engine, and the size of the image that comes back.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Plan {
    pub opts: ScreenshotOptions,
    pub width: u32,
    pub height: u32,
}

/// Pixel density of the clip: the page's own density, reduced if the image would exceed `limit` on a side.
pub(super) fn scale_for(css_w: f64, css_h: f64, dpr: f64, limit: f64) -> f32 {
    let dpr = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
    let longest = css_w.max(css_h).max(1.0);
    dpr.min(limit / longest).max(0.1) as f32
}

/// Requested area, brought within the view bounds (view coordinates, in points). `None` if smaller than 2 px.
pub(super) fn clamp_rect(rect: &Rect, view: (f64, f64)) -> Option<Rect> {
    let x = rect.x.round().max(0.0);
    let y = rect.y.round().max(0.0);
    let width = rect.width.round().min(view.0 - x);
    let height = rect.height.round().min(view.1 - y);
    (width >= 2.0 && height >= 2.0).then(|| Rect::new(x, y, width, height))
}

/// Builds the engine request. `view`: size of the web view in points; `zoom`: page zoom factor (1.0 = 100 percent);
/// `limit`: longest side allowed for the image.
pub(super) fn plan(target: &CaptureTarget, m: &Metrics, view: (f64, f64), zoom: f64, limit: f64) -> Option<Plan> {
    let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
    let (clip, beyond) = match target {
        CaptureTarget::Region { rect } => {
            let r = clamp_rect(rect, view)?;
            ((m.x + r.x / zoom, m.y + r.y / zoom, r.width / zoom, r.height / zoom), false)
        }
        CaptureTarget::Visible => {
            if view.0 < 2.0 || view.1 < 2.0 || m.vw < 1.0 || m.vh < 1.0 {
                return None;
            }
            ((m.x, m.y, m.vw, m.vh), false)
        }
        CaptureTarget::Full => {
            if m.cw < 1.0 || m.ch < 1.0 {
                return None;
            }
            ((0.0, 0.0, m.cw.ceil().min(MAX_SIDE), m.ch.ceil().min(MAX_SIDE)), true)
        }
    };
    let scale = scale_for(clip.2, clip.3, m.dpr, limit);
    let opts = ScreenshotOptions {
        clip: Some(signet_web::Bounds::new(clip.0 as f32, clip.1 as f32, clip.2 as f32, clip.3 as f32)),
        format: ImageFormat::Png,
        scale,
        capture_beyond_viewport: beyond,
    };
    Some(Plan { opts, width: (clip.2 * f64::from(scale)).round() as u32, height: (clip.3 * f64::from(scale)).round() as u32 })
}

/// Reads the page measures, then takes the screenshot; `done` gets the PNG and its size. Runs on the main thread, in
/// callbacks of the DevTools calls (which may send new calls).
pub(super) fn grab(view: WebView, target: CaptureTarget, limit: f64, done: impl FnOnce(Result<(Vec<u8>, u32, u32), String>) + 'static) {
    let view_size = {
        let b = view.bounds();
        (f64::from(b.width), f64::from(b.height))
    };
    let zoom = 1.2f64.powf(view.zoom_level());
    let params = serde_json::json!({ "expression": METRICS_JS, "returnByValue": true }).to_string();
    view.execute_devtools_method("Runtime.evaluate", &params, move |r| {
        let metrics = r.and_then(|json| {
            let v: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let text = v["result"]["value"].as_str().ok_or("no page measures")?;
            serde_json::from_str::<Metrics>(text).map_err(|e| e.to_string())
        });
        let plan = metrics.and_then(|m| plan(&target, &m, view_size, zoom, limit).ok_or_else(|| "nothing to capture".to_string()));
        match plan {
            Err(e) => done(Err(e)),
            Ok(p) => {
                let (w, h) = (p.width, p.height);
                view.capture_screenshot(&p.opts, move |r| done(r.map(|png| (png, w, h))));
            }
        }
    });
}

/// File name of a capture: "Screenshot example.com 2026-10-24 at 16.42.05.png".
pub(super) fn file_name(host: &str, date: &str, time: &str) -> String {
    let site = if host.is_empty() { "Signet" } else { host };
    let name = signet_core::i18n::t_args("capture.fileName", &[("site", site), ("date", date), ("time", time)]);
    let name = if name == "capture.fileName" { format!("Screenshot {site} {date} at {time}") } else { name };
    format!("{}.png", super::super::page_tools::safe_file_base(&name))
}

fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.strip_prefix("www.").unwrap_or(h).to_string())).unwrap_or_default()
}

/// A capture that came back.
pub(super) struct Finished {
    action: CaptureAction,
    url: String,
    result: Result<(Vec<u8>, u32, u32), String>,
}

#[derive(Default)]
pub(super) struct Capture {
    busy: bool,
}

impl Core {
    /// Starts the tool on the tab (default: the active one). In the main window the overlay offers area, visible part
    /// or whole page; elsewhere the visible part is saved right away.
    pub fn start_capture(&mut self, tab: Option<&str>) {
        let Some(id) = tab.map(str::to_string).or_else(|| self.active_tab_id()) else { return };
        if self.views.view_of(&id).is_none() {
            return;
        }
        if let Some(pane) = self.win.as_ref().and(self.panes.iter().find(|p| p.tab_id == id)).cloned() {
            self.ov.payload = Some(OverlayPayload::Capture { tab_id: id, rect: pane.rect, hue: self.hue() });
            self.ov.leaving = false;
            self.changed_view();
            self.emit_overlay();
            self.request_focus();
            return;
        }
        self.take_capture(&id, CaptureTarget::Visible, CaptureAction::Save);
    }

    /// Takes the capture requested by the overlay. The image arrives later (see the module comment); the answer only
    /// says whether it was started.
    pub fn take_capture(&mut self, tab_id: &str, target: CaptureTarget, action: CaptureAction) -> CaptureResult {
        let Some(view) = self.views.view_of(tab_id) else { return CaptureResult::default() };
        if self.phase3.media.capture.busy {
            return CaptureResult::default();
        }
        self.phase3.media.capture.busy = true;
        if action != CaptureAction::Copy {
            self.close_overlay(false);
        }
        let url = self.state().tabs.get(tab_id).map(|t| t.url.clone()).unwrap_or_default();
        let outbox = self.phase3.media.outbox.clone();
        let limit = if action == CaptureAction::Easel { EASEL_SIDE } else { MAX_SIDE };
        grab(view, target, limit, move |result| outbox.push(Done::Capture(Finished { action, url, result })));
        CaptureResult { ok: true, png: None }
    }
}

pub(super) fn finish(core: &mut Core, f: Finished) {
    core.phase3.media.capture.busy = false;
    core.note(format!("capture-done action={:?} ok={}", f.action, f.result.as_ref().map(|r| !r.0.is_empty()).unwrap_or(false)));
    let (png, w, h) = match f.result {
        Ok(r) if !r.0.is_empty() => r,
        Ok(_) => return core.log("capture: empty image"),
        Err(e) => return core.log(format!("capture failed: {e}")),
    };
    match f.action {
        CaptureAction::Copy => copy_png(png),
        CaptureAction::Save => {
            let now = chrono::Local::now();
            let name = file_name(&host_of(&f.url), &now.format("%Y-%m-%d").to_string(), &now.format("%H.%M.%S").to_string());
            let dir = core.download_dir();
            let _ = std::fs::create_dir_all(&dir);
            let path = signet_features::downloads::unique_path(&dir, &name, &Default::default(), &|p| p.exists());
            match std::fs::write(&path, &png) {
                Ok(()) => crate::app::platform::reveal(&path.to_string_lossy()),
                Err(e) => core.log(format!("capture: cannot save {}: {e}", path.display())),
            }
        }
        CaptureAction::Easel => {
            let url = format!("data:image/png;base64,{}", base64(&png));
            if let Some(address) = core.create_easel_with_image(&url, f64::from(w), f64::from(h)) {
                core.open_tab(&address, signet_core::model::OpenOptions { activate: true, after_tab_id: None, opener_id: None });
            }
        }
    }
}

/// Puts a PNG on the system clipboard (in a thread: the helper programs can be slow).
fn copy_png(png: Vec<u8>) {
    std::thread::spawn(move || {
        let path = std::env::temp_dir().join(format!("signet-capture-{}.png", std::process::id()));
        if std::fs::write(&path, &png).is_err() {
            return;
        }
        run_clipboard_helper(&path);
        let _ = std::fs::remove_file(&path);
    });
}

fn run_clipboard_helper(path: &Path) {
    use std::process::{Command, Stdio};
    let p = path.to_string_lossy().into_owned();
    #[cfg(target_os = "macos")]
    {
        let script = format!("set the clipboard to (read (POSIX file \"{}\") as \u{ab}class PNGf\u{bb})", p.replace('\\', "\\\\").replace('"', "\\\""));
        let _ = Command::new("osascript").arg("-e").arg(script).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(target_os = "windows")]
    {
        let script = format!("Add-Type -AssemblyName System.Windows.Forms,System.Drawing; [System.Windows.Forms.Clipboard]::SetImage([System.Drawing.Image]::FromFile('{}'))", p.replace('\'', "''"));
        let _ = Command::new("powershell").args(["-STA", "-NoProfile", "-NonInteractive", "-Command", &script]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        if let Ok(f) = std::fs::File::open(path)
            && Command::new("wl-copy").args(["--type", "image/png"]).stdin(f).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
        {
            return;
        }
        let _ = Command::new("xclip").args(["-selection", "clipboard", "-t", "image/png", "-i", &p]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> Metrics {
        Metrics { x: 0.0, y: 300.0, vw: 1000.0, vh: 600.0, cw: 1000.0, ch: 5000.0, dpr: 2.0 }
    }

    #[test]
    fn clamp_keeps_the_area_in_the_view() {
        let r = clamp_rect(&Rect::new(-5.0, 10.4, 2000.0, 100.0), (800.0, 600.0)).unwrap();
        assert_eq!(r, Rect::new(0.0, 10.0, 800.0, 100.0));
        assert!(clamp_rect(&Rect::new(10.0, 10.0, 1.0, 50.0), (800.0, 600.0)).is_none());
        assert!(clamp_rect(&Rect::new(799.0, 0.0, 50.0, 50.0), (800.0, 600.0)).is_none());
    }

    #[test]
    fn visible_uses_the_scroll_position_and_density() {
        let p = plan(&CaptureTarget::Visible, &m(), (1000.0, 600.0), 1.0, MAX_SIDE).unwrap();
        let c = p.opts.clip.unwrap();
        assert_eq!((c.x, c.y, c.width, c.height), (0.0, 300.0, 1000.0, 600.0));
        assert_eq!(p.opts.scale, 2.0);
        assert!(!p.opts.capture_beyond_viewport);
        assert_eq!((p.width, p.height), (2000, 1200));
    }

    #[test]
    fn region_is_converted_to_css_pixels() {
        let t = CaptureTarget::Region { rect: Rect::new(120.0, 60.0, 240.0, 120.0) };
        let p = plan(&t, &m(), (1000.0, 600.0), 1.2, MAX_SIDE).unwrap();
        let c = p.opts.clip.unwrap();
        assert_eq!((c.x, c.y, c.width, c.height), (100.0, 350.0, 200.0, 100.0));
    }

    #[test]
    fn full_page_is_capped_and_goes_beyond_the_viewport() {
        let mut met = m();
        met.ch = 40000.0;
        let p = plan(&CaptureTarget::Full, &met, (1000.0, 600.0), 1.0, MAX_SIDE).unwrap();
        let c = p.opts.clip.unwrap();
        assert_eq!((c.x, c.y, c.width, c.height), (0.0, 0.0, 1000.0, 16384.0));
        assert!(p.opts.capture_beyond_viewport);
        assert_eq!(p.opts.scale, 1.0, "16384 / 16384");
        assert!(p.height <= 16384);
    }

    #[test]
    fn easel_images_stay_under_1600_px() {
        let p = plan(&CaptureTarget::Visible, &m(), (1000.0, 600.0), 1.0, EASEL_SIDE).unwrap();
        assert!(p.width.max(p.height) <= 1600);
        assert_eq!(p.width, 1600);
    }

    #[test]
    fn scale_handles_bad_density() {
        assert_eq!(scale_for(100.0, 100.0, 0.0, MAX_SIDE), 1.0);
        assert_eq!(scale_for(100.0, 100.0, f64::NAN, MAX_SIDE), 1.0);
        assert_eq!(scale_for(1e9, 1.0, 2.0, MAX_SIDE), 0.1);
    }

    #[test]
    fn empty_page_gives_no_plan() {
        let mut met = m();
        met.cw = 0.0;
        assert!(plan(&CaptureTarget::Full, &met, (1000.0, 600.0), 1.0, MAX_SIDE).is_none());
        assert!(plan(&CaptureTarget::Visible, &m(), (1.0, 1.0), 1.0, MAX_SIDE).is_none());
    }

    #[test]
    fn file_name_is_safe() {
        let n = file_name("example.com", "2026-10-24", "16.42.05");
        assert!(n.ends_with(".png"));
        assert!(n.contains("example.com") && n.contains("2026-10-24"));
        assert!(!n.contains(':'));
        assert!(file_name("", "d", "t").contains("Signet"));
    }

    #[test]
    fn host_strips_www() {
        assert_eq!(host_of("https://www.example.com/a"), "example.com");
        assert_eq!(host_of("nonsense"), "");
    }
}

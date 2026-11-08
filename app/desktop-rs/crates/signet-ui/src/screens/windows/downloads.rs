//! Port of `windows/Downloads.tsx` + `styles/downloads-list.css`: download rows (shared with the Library), the sidebar
//! panel, and the helpers the bottom bar uses (progress ring value, "unseen" dot, new download detection).
//!
//! Not ported: `DownloadFlight` (the dot that arcs from the card to the button), a pure animation: the sidebar owner may add it.

use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::{IconButton, ProgressBar};
use gpui::{App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px};
use gpui_component::Icon;
use signet_core::i18n::{Locale, current_locale};
use signet_core::types::{DownloadInfo, DownloadStatus};
use std::collections::HashSet;

/// Progressing or paused.
pub fn live(d: &DownloadInfo) -> bool {
    matches!(d.status, DownloadStatus::Progressing | DownloadStatus::Paused)
}

/// Size with the language's short units, base 1024 (`Intl.NumberFormat` `unit` style).
pub fn format_bytes(n: u64, locale: Locale) -> String {
    let units: [&str; 5] = match locale {
        Locale::Fr => ["o", "ko", "Mo", "Go", "To"],
        Locale::En => ["byte", "kB", "MB", "GB", "TB"],
    };
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let (group, decimal) = match locale {
        Locale::Fr => ('\u{202f}', ','),
        Locale::En => (',', '.'),
    };
    let rounded = if i == 0 { v.round() } else { (v * 10.0).round() / 10.0 };
    let int = rounded.trunc() as u64;
    let mut digits = int.to_string();
    let mut grouped = String::new();
    while digits.len() > 3 {
        let tail = digits.split_off(digits.len() - 3);
        grouped = format!("{group}{tail}{grouped}");
    }
    let mut out = format!("{digits}{grouped}");
    let tenth = ((rounded.fract() * 10.0).round() as u32) % 10;
    if i > 0 && tenth != 0 {
        out.push(decimal);
        out.push_str(&tenth.to_string());
    }
    let space = if locale == Locale::Fr { '\u{a0}' } else { ' ' };
    format!("{out}{space}{}", units[i])
}

/// Second line of a row: progress, size, or the failure.
pub fn subtitle(d: &DownloadInfo, locale: Locale) -> String {
    let fmt = |n: u64| format_bytes(n, locale);
    match d.status {
        DownloadStatus::Progressing | DownloadStatus::Paused => {
            let base = if d.total_bytes > 0 { tr_args("downloads.progress", &[("received", &fmt(d.received_bytes)), ("total", &fmt(d.total_bytes))]) } else { fmt(d.received_bytes) };
            if d.status == DownloadStatus::Paused { tr_args("downloads.paused", &[("size", &base)]) } else { base }
        }
        DownloadStatus::Completed => tr_args("downloads.completed", &[("size", &fmt(if d.total_bytes > 0 { d.total_bytes } else { d.received_bytes }))]),
        DownloadStatus::Cancelled => tr("downloads.cancelled"),
        DownloadStatus::Interrupted => tr("downloads.interrupted"),
    }
}

/// File icon by extension.
pub fn icon_for(name: &str) -> IconName {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let is = |list: &[&str]| list.contains(&ext.as_str());
    if is(&["png", "jpg", "jpeg", "gif", "webp", "svg", "heic", "avif", "bmp", "tiff"]) {
        IconName::FileImage
    } else if is(&["zip", "gz", "tgz", "tar", "rar", "7z", "dmg", "pkg", "bz2", "xz"]) {
        IconName::FileArchive
    } else if is(&["mp4", "mov", "mkv", "avi", "webm", "m4v"]) {
        IconName::FileVideo
    } else if is(&["mp3", "wav", "flac", "aac", "m4a", "ogg", "aiff"]) {
        IconName::FileAudio
    } else if is(&["pdf", "txt", "md", "doc", "docx", "rtf", "csv", "xls", "xlsx", "ppt", "pptx", "pages", "numbers", "key", "json"]) {
        IconName::FileText
    } else {
        IconName::File
    }
}

/// Splits a name for middle truncation: the last 8 characters stay visible (the extension).
pub fn split_mid(name: &str) -> (String, String) {
    let chars: Vec<char> = name.chars().collect();
    let cut = chars.len().saturating_sub(8);
    (chars[..cut].iter().collect(), chars[cut..].iter().collect())
}

/// Bottom bar button: is anything running, and the combined fraction (None when a size is unknown).
pub fn progress_of(downloads: &[DownloadInfo]) -> (bool, Option<f32>) {
    let running: Vec<&DownloadInfo> = downloads.iter().filter(|d| live(d)).collect();
    if running.is_empty() {
        return (false, None);
    }
    if running.iter().any(|d| d.total_bytes == 0) {
        return (true, None);
    }
    let total: u64 = running.iter().map(|d| d.total_bytes).sum();
    let got: u64 = running.iter().map(|d| d.received_bytes).sum();
    (true, if total > 0 { Some((got as f32 / total as f32).min(1.0)) } else { None })
}

/// Finished downloads the user has not seen yet (`useUnseenDownloads`). Kept in memory: the sidebar owns one instance.
pub struct UnseenTracker {
    seen: HashSet<String>,
}

impl UnseenTracker {
    /// Everything already finished counts as seen.
    pub fn new(downloads: &[DownloadInfo]) -> Self {
        Self { seen: downloads.iter().filter(|d| d.status == DownloadStatus::Completed).map(|d| d.id.clone()).collect() }
    }

    /// While the panel is `open`, what finishes counts as seen. Returns true if the "unseen" dot must show.
    pub fn update(&mut self, downloads: &[DownloadInfo], open: bool) -> bool {
        if open {
            self.seen.extend(downloads.iter().filter(|d| d.status == DownloadStatus::Completed).map(|d| d.id.clone()));
        }
        !open && downloads.iter().any(|d| d.status == DownloadStatus::Completed && !self.seen.contains(&d.id))
    }
}

/// Detects the arrival of a new live download: an id absent from the previous list (the first list does not count).
#[derive(Default)]
pub struct NewDownloadDetector {
    known: Option<HashSet<String>>,
}

impl NewDownloadDetector {
    pub fn update<'a>(&mut self, downloads: &'a [DownloadInfo]) -> Option<&'a DownloadInfo> {
        let before = self.known.replace(downloads.iter().map(|d| d.id.clone()).collect())?;
        downloads.iter().find(|d| !before.contains(&d.id) && live(d))
    }
}

/// A download row (sidebar panel and Library).
#[derive(IntoElement)]
pub struct DownloadRow {
    d: DownloadInfo,
}

impl DownloadRow {
    pub fn new(d: DownloadInfo) -> Self {
        Self { d }
    }
}

impl RenderOnce for DownloadRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let d = self.d;
        let a = api(cx);
        let running = live(&d);
        let done = d.status == DownloadStatus::Completed;
        let dead = matches!(d.status, DownloadStatus::Cancelled | DownloadStatus::Interrupted);
        let fraction = (d.total_bytes > 0).then(|| (d.received_bytes as f32 / d.total_bytes as f32).clamp(0.0, 1.0));
        let (head, tail) = split_mid(&d.filename);
        let id: ElementId = SharedString::from(d.id.clone()).into();
        let (a_open, a_retry, a_reveal, a_cancel) = (a.clone(), a.clone(), a.clone(), a.clone());
        let (id_open, id_retry, id_reveal, id_cancel) = (d.id.clone(), d.id.clone(), d.id.clone(), d.id.clone());
        let hover = t.surface_2;

        let main = div()
            .id(ElementId::from((id.clone(), "main")))
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(space::S3)
            .rounded(radius::ROW)
            .when(done, |m| m.cursor_pointer().on_click(move |_, _, _| a_open.open_download(&id_open)))
            .child(Icon::from(icon_for(&d.filename)).size(px(18.0)).text_color(t.text_2).flex_none())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(div().flex().min_w_0().child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(head)).child(div().flex_none().whitespace_nowrap().child(tail)))
                    .child(div().truncate().text_size(t.type_scale.cap).text_color(t.text_2).child(subtitle(&d, current_locale())))
                    .when(running, |c| {
                        c.child(div().mt(space::S1).child(match fraction {
                            Some(f) => ProgressBar::new(ElementId::from((id.clone(), "bar"))).value(f),
                            None => ProgressBar::new(ElementId::from((id.clone(), "bar"))),
                        }))
                    }),
            );

        div()
            .id(ElementId::from((id.clone(), "row")))
            .flex_none()
            .flex()
            .items_center()
            .gap(space::S3)
            .min_h(px(44.0))
            .py(space::S1)
            .pr(space::S1)
            .pl(space::S2)
            .rounded(radius::ROW)
            .text_size(t.type_scale.base)
            .text_color(t.text)
            .when(dead, |r| r.opacity(0.6))
            .hover(move |s| s.bg(hover))
            .child(main)
            .when(dead, |r| {
                r.child(IconButton::new(ElementId::from((id.clone(), "retry")), IconName::RotateCw).small().icon_size(13.0).tooltip(tr("common.retry")).on_click(move |_, _, _| a_retry.retry_download(&id_retry)))
            })
            .when(done, |r| {
                r.child(
                    IconButton::new(ElementId::from((id.clone(), "reveal")), IconName::Search)
                        .small()
                        .icon_size(13.0)
                        .tooltip(tr("downloads.reveal"))
                        .on_click(move |_, _, _| a_reveal.reveal_download(&id_reveal)),
                )
            })
            .when(running, |r| {
                r.child(IconButton::new(ElementId::from((id.clone(), "cancel")), IconName::X).small().icon_size(13.0).tooltip(tr("downloads.cancel")).on_click(move |_, _, _| a_cancel.cancel_download(&id_cancel)))
            })
    }
}

/// Sidebar downloads panel: header ("Downloads", "Clear") and the list. The popover owner closes it on click outside or Escape.
#[derive(IntoElement)]
pub struct DownloadsPanel {
    downloads: Vec<DownloadInfo>,
}

impl DownloadsPanel {
    pub fn new(downloads: Vec<DownloadInfo>) -> Self {
        Self { downloads }
    }
}

impl RenderOnce for DownloadsPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let a = api(cx);
        let clearable = self.downloads.iter().any(|d| !live(d));
        let hover = t.surface_2;
        let (text, text_2) = (t.text, t.text_2);
        div()
            .flex()
            .flex_col()
            .size_full()
            .text_size(t.type_scale.base)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(space::S8)
                    .pl(space::S2)
                    .pr(space::S1)
                    .child(div().font_weight(weight::STRONG).child(tr("downloads.title")))
                    .child(
                        div()
                            .id("dl-clear")
                            .flex()
                            .items_center()
                            .h(space::S6)
                            .px(space::S2)
                            .rounded(radius::CONTROL)
                            .text_size(t.type_scale.sub)
                            .text_color(text_2)
                            .when(!clearable, |c| c.opacity(0.35))
                            .when(clearable, |c| c.cursor_pointer().hover(move |s| s.bg(hover).text_color(text)).on_click(move |_, _, _| a.clear_downloads()))
                            .child(tr("downloads.clear")),
                    ),
            )
            .child(
                div()
                    .id("dl-list")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(space::HALF)
                    .overflow_y_scroll()
                    .when(self.downloads.is_empty(), |l| {
                        l.child(
                            div()
                                .mt(px(80.0))
                                .mx_auto()
                                .max_w(px(380.0))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(space::S3)
                                .text_center()
                                .text_color(t.text_2)
                                .line_height(gpui::relative(1.5))
                                .child(Icon::from(IconName::Download).size(px(28.0)).text_color(t.text_3))
                                .child(tr("downloads.empty")),
                        )
                    })
                    .children(self.downloads.into_iter().map(DownloadRow::new)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dl(id: &str, status: DownloadStatus, got: u64, total: u64) -> DownloadInfo {
        DownloadInfo { id: id.into(), filename: format!("{id}.zip"), status, received_bytes: got, total_bytes: total, ..Default::default() }
    }

    #[test]
    fn bytes_use_short_units_per_language() {
        assert_eq!(format_bytes(0, Locale::En), "0 byte");
        assert_eq!(format_bytes(1023, Locale::En), "1,023 byte");
        assert_eq!(format_bytes(1536, Locale::En), "1.5 kB");
        assert_eq!(format_bytes(5 * 1024 * 1024, Locale::En), "5 MB");
        assert_eq!(format_bytes(1536, Locale::Fr), "1,5\u{a0}ko");
        assert_eq!(format_bytes(12, Locale::Fr), "12\u{a0}o");
    }

    #[test]
    fn file_icons_follow_the_extension() {
        assert_eq!(icon_for("a.PNG"), IconName::FileImage);
        assert_eq!(icon_for("a.tar"), IconName::FileArchive);
        assert_eq!(icon_for("a.mkv"), IconName::FileVideo);
        assert_eq!(icon_for("a.flac"), IconName::FileAudio);
        assert_eq!(icon_for("a.pdf"), IconName::FileText);
        assert_eq!(icon_for("noext"), IconName::File);
    }

    #[test]
    fn names_keep_their_last_eight_characters() {
        assert_eq!(split_mid("report-final.pdf"), ("report-f".to_string(), "inal.pdf".to_string()));
        assert_eq!(split_mid("a.zip"), (String::new(), "a.zip".to_string()));
    }

    #[test]
    fn progress_combines_running_downloads() {
        assert_eq!(progress_of(&[dl("a", DownloadStatus::Completed, 5, 5)]), (false, None));
        assert_eq!(progress_of(&[dl("a", DownloadStatus::Progressing, 50, 100), dl("b", DownloadStatus::Paused, 50, 300)]), (true, Some(0.25)));
        assert_eq!(progress_of(&[dl("a", DownloadStatus::Progressing, 50, 0)]), (true, None));
    }

    #[test]
    fn unseen_dot_shows_for_downloads_that_finish_while_closed() {
        let mut t = UnseenTracker::new(&[dl("a", DownloadStatus::Completed, 1, 1)]);
        assert!(!t.update(&[dl("a", DownloadStatus::Completed, 1, 1)], false));
        let list = [dl("a", DownloadStatus::Completed, 1, 1), dl("b", DownloadStatus::Completed, 1, 1)];
        assert!(t.update(&list, false));
        assert!(!t.update(&list, true));
        assert!(!t.update(&list, false));
    }

    #[test]
    fn new_download_ignores_the_first_list() {
        let mut det = NewDownloadDetector::default();
        assert!(det.update(&[dl("a", DownloadStatus::Progressing, 0, 0)]).is_none());
        let list = [dl("b", DownloadStatus::Progressing, 0, 0), dl("a", DownloadStatus::Progressing, 0, 0)];
        assert_eq!(det.update(&list).map(|d| d.id.as_str()), Some("b"));
        assert!(det.update(&list).is_none());
    }
}

//! Downloads panel of the tab-row popover (port of `DownloadsPanel` in `windows/Downloads.tsx`).

use super::common::lucide;
use crate::app_state::{api, snapshot, tr, tr_args};
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::{EmptyState, ProgressBar};
use crate::IconName;
use gpui::{AnyElement, App, InteractiveElement, IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px};
use signet_core::types::{DownloadInfo, DownloadStatus};

pub fn live(d: &DownloadInfo) -> bool {
    matches!(d.status, DownloadStatus::Progressing | DownloadStatus::Paused)
}

/// Size with the language's units, base 1024.
pub fn format_bytes(n: u64, fr: bool) -> String {
    let units: [&str; 5] = if fr { ["o", "ko", "Mo", "Go", "To"] } else { ["B", "KB", "MB", "GB", "TB"] };
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let num = if i == 0 { format!("{}", v.round() as u64) } else { format!("{:.1}", v).trim_end_matches('0').trim_end_matches('.').to_string() };
    let num = if fr { num.replace('.', ",") } else { num };
    format!("{}{}{}", num, if fr { "\u{202f}" } else { " " }, units[i])
}

fn subtitle(d: &DownloadInfo, fr: bool) -> String {
    let fmt = |n: u64| format_bytes(n, fr);
    match d.status {
        DownloadStatus::Progressing | DownloadStatus::Paused => {
            let base = if d.total_bytes > 0 {
                tr_args("downloads.progress", &[("received", &fmt(d.received_bytes)), ("total", &fmt(d.total_bytes))])
            } else {
                fmt(d.received_bytes)
            };
            if d.status == DownloadStatus::Paused { tr_args("downloads.paused", &[("size", &base)]) } else { base }
        }
        DownloadStatus::Completed => tr_args("downloads.completed", &[("size", &fmt(if d.total_bytes > 0 { d.total_bytes } else { d.received_bytes }))]),
        DownloadStatus::Cancelled => tr("downloads.cancelled"),
        DownloadStatus::Interrupted => tr("downloads.interrupted"),
    }
}

fn file_icon(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" | "avif" | "bmp" | "tiff" => "file-image",
        "zip" | "gz" | "tgz" | "tar" | "rar" | "7z" | "dmg" | "pkg" | "bz2" | "xz" => "file-archive",
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "m4v" => "file-video",
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "aiff" => "file-audio",
        "pdf" | "txt" | "md" | "doc" | "docx" | "rtf" | "csv" | "xls" | "xlsx" | "ppt" | "pptx" | "pages" | "numbers" | "key" | "json" => "file-text",
        _ => "file",
    }
}

fn mini(id: SharedString, icon: &str, tip: String, cx: &App) -> gpui::Stateful<gpui::Div> {
    let t = cx.signet();
    let hover = t.border;
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(26.0))
        .rounded(radius::CONTROL)
        .text_color(t.text_2)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(lucide(icon).size(px(13.0)))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
}

/// A download row (popover panel).
pub fn download_row(d: &DownloadInfo, fr: bool, cx: &App) -> AnyElement {
    let t = cx.signet();
    let running = live(d);
    let done = d.status == DownloadStatus::Completed;
    let dead = matches!(d.status, DownloadStatus::Cancelled | DownloadStatus::Interrupted);
    let id = d.id.clone();
    let pct = if d.total_bytes > 0 { Some((d.received_bytes as f32 / d.total_bytes as f32).min(1.0)) } else { None };
    let (open_id, retry_id, reveal_id, cancel_id) = (id.clone(), id.clone(), id.clone(), id.clone());
    let surface_2 = t.surface_2;
    div()
        .id(SharedString::from(format!("dl-{id}")))
        .flex()
        .flex_none()
        .items_center()
        .gap(space::S3)
        .min_h(space::TOOLBAR_H)
        .py(space::S1)
        .pl(space::S2)
        .pr(space::S1)
        .rounded(radius::ROW)
        .hover(move |s| s.bg(surface_2))
        .when(dead, |d| d.opacity(0.6))
        .child(
            div()
                .id(SharedString::from(format!("dl-main-{id}")))
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(space::S3)
                .rounded(radius::ROW)
                .when(done, |d| d.cursor_pointer().on_click(move |_, _, cx| api(cx).open_download(&open_id)))
                .child(div().flex_none().text_color(t.text_2).child(lucide(file_icon(&d.filename)).size(px(18.0))))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .line_height(gpui::relative(1.4))
                        .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().child(d.filename.clone()))
                        .child(div().overflow_hidden().text_ellipsis().whitespace_nowrap().text_size(t.type_scale.cap).text_color(t.text_2).child(subtitle(d, fr)))
                        .when(running, |c| {
                            let bar = ProgressBar::new(SharedString::from(format!("dl-bar-{id}")));
                            c.child(div().mt(space::S1).child(match pct {
                                Some(p) => bar.value(p),
                                None => bar,
                            }))
                        }),
                ),
        )
        .when(dead, |r| r.child(mini(SharedString::from(format!("dl-retry-{id}")), "rotate-cw", tr("common.retry"), cx).on_click(move |_, _, cx| api(cx).retry_download(&retry_id))))
        .when(done, |r| {
            r.child(mini(SharedString::from(format!("dl-reveal-{id}")), "search", tr("downloads.reveal"), cx).on_click(move |_, _, cx| api(cx).reveal_download(&reveal_id)))
        })
        .when(running, |r| r.child(mini(SharedString::from(format!("dl-cancel-{id}")), "x", tr("downloads.cancel"), cx).on_click(move |_, _, cx| api(cx).cancel_download(&cancel_id))))
        .into_any_element()
}

/// Number of rows the panel shows (to size the popover).
pub fn count(cx: &App) -> usize {
    snapshot(cx).downloads.len()
}

pub fn downloads_panel(cx: &App) -> AnyElement {
    let t = cx.signet();
    let s = snapshot(cx);
    let fr = signet_core::i18n::current_locale() == signet_core::i18n::Locale::Fr;
    let clearable = s.downloads.iter().any(|d| !live(d));
    let hover = t.surface_2;
    div()
        .flex()
        .flex_col()
        .p(px(6.0))
        .max_h(px(380.0))
        .rounded(radius::POP)
        .bg(t.surface)
        .border_1()
        .border_color(t.border)
        .shadow(t.shadow_pop.clone())
        .text_color(t.text)
        .child(
            div()
                .flex()
                .flex_none()
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
                        .text_color(t.text_2)
                        .child(tr("downloads.clear"))
                        .when(!clearable, |d| d.opacity(0.35))
                        .when(clearable, |d| d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(|_, _, cx| api(cx).clear_downloads())),
                ),
        )
        .child(
            div()
                .id("dl-list")
                .flex()
                .flex_col()
                .gap(space::HALF)
                .overflow_y_scroll()
                .when(s.downloads.is_empty(), |d| d.child(div().py(space::S5).child(EmptyState::new(tr("downloads.empty")).icon(IconName::Download))))
                .children(s.downloads.iter().map(|d| download_row(d, fr, cx))),
        )
        .into_any_element()
}

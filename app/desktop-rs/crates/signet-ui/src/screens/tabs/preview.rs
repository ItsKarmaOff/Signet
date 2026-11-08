//! Port of `tabs/TabPreview.tsx`: content of the `#preview` window. Preview card of a hovered tab (thumbnail,
//! title, address, state), placed by the app next to the row. It appears from the bar's side (6 px slide + fade) and
//! leaves the same way.
//!
//! The app creates a small transparent window (margin of 14 px around the card for the shadow) and puts a
//! `TabPreviewView` in it; the view listens to `UiEvent::TabPreview` and `UiEvent::TabPreviewHide` itself.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AppContext, Context, ElementId, Entity, IntoElement, ObjectFit, ParentElement, Render, SharedString, Styled, StyledImage, Subscription, Window, div, img,
    prelude::FluentBuilder, px,
};
use gpui_component::Icon;
use signet_core::api::UiEvent;
use signet_core::types::{PreviewFrom, TabPreviewPayload};

use super::custom::{decode_data_url, favicon_source};
use crate::app_state::{tr, ui_state};
use crate::icon::IconName;
use crate::motion::{ease_out, reduced_motion};
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::FaviconSource;

/// Margin around the card (room for its shadow); the app sizes the window as the card plus this margin on each side.
pub const MARGIN: f32 = 14.0;

/// Displayed address of a page: host without `www.`; internal addresses in full; anything else as is.
pub fn domain_of(url: &str) -> String {
    if url.len() >= 7 && url[..7].eq_ignore_ascii_case("signet:") {
        return url.trim_end_matches('/').to_string();
    }
    let Some((_, rest)) = url.split_once("://") else { return url.to_string() };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = if host.starts_with('[') { host.split(']').next().map(|h| format!("{h}]")).unwrap_or_default() } else { host.split(':').next().unwrap_or("").to_string() };
    let host = host.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    if host.is_empty() { url.to_string() } else { host.to_string() }
}

pub struct TabPreviewView {
    payload: Option<TabPreviewPayload>,
    on: bool,
    /// Bumped on each change that restarts the animation.
    epoch: u64,
    animate: bool,
    _sub: Subscription,
}

impl TabPreviewView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let ui = ui_state(cx);
        let sub = cx.subscribe(&ui, |this, _ui, ev: &UiEvent, cx| match ev {
            UiEvent::TabPreview(p) => {
                // Appearance: starts from the hidden state; moving from one tab to another keeps the card.
                this.animate = p.fresh;
                if p.fresh || !this.on {
                    this.epoch += 1;
                }
                this.payload = Some(p.clone());
                this.on = true;
                cx.notify();
            }
            UiEvent::TabPreviewHide => {
                this.epoch += 1;
                this.animate = true;
                this.on = false;
                cx.notify();
            }
            _ => {}
        });
        TabPreviewView { payload: None, on: false, epoch: 0, animate: true, _sub: sub }
    }
}

/// Creates the view (helper for the app).
pub fn new_view(window: &mut Window, cx: &mut gpui::App) -> Entity<TabPreviewView> {
    cx.new(|cx| TabPreviewView::new(window, cx))
}

impl Render for TabPreviewView {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(p) = self.payload.clone() else { return div().into_any_element() };
        let t = cx.signet().clone();
        let reduced = reduced_motion(cx);
        let domain = domain_of(&p.url);
        let domain = if domain.is_empty() { p.url.clone() } else { domain };
        let thumb = p.thumb.as_deref().and_then(decode_data_url);
        let favicon = favicon_source(p.favicon.as_deref());
        let (on, from, animate, epoch) = (self.on, p.from, self.animate, self.epoch);
        let badge = |icon: IconName, text: String| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(3.0))
                .px(px(6.0))
                .py(px(1.0))
                .rounded_full()
                .bg(t.surface_2)
                .text_size(t.type_scale.cap)
                .child(Icon::from(icon).size(px(12.0)))
                .child(text)
        };
        let mut card = div()
            .absolute()
            .when(from == PreviewFrom::Right, |d| d.left(px(0.0)).top(px(MARGIN)).bottom(px(MARGIN)).right(px(MARGIN)))
            .when(from == PreviewFrom::Left, |d| d.right(px(0.0)).top(px(MARGIN)).bottom(px(MARGIN)).left(px(MARGIN)))
            .when(from == PreviewFrom::Below, |d| d.top(px(0.0)).left(px(MARGIN)).right(px(MARGIN)).bottom(px(MARGIN)))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(radius::POP)
            .bg(t.glass_view)
            .shadow(t.shadow_pop.clone())
            .border_1()
            .border_color(t.border)
            .text_size(t.type_scale.base)
            .text_color(t.text);
        if let Some(img_src) = thumb {
            card = card.child(div().flex_none().h(px(158.0)).bg(t.surface_2).border_b_1().border_color(t.border).child(img(img_src).size_full().object_fit(ObjectFit::Cover)));
        }
        let fav = favicon.map(|f| match f {
            FaviconSource::Bytes(b) => img(b).size(px(14.0)).rounded(px(3.0)).into_any_element(),
            FaviconSource::Url(u) | FaviconSource::Path(u) => img(u).size(px(14.0)).rounded(px(3.0)).into_any_element(),
        });
        card = card.child(
            div()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .px(space::S3)
                .py(space::S2)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .children(fav.map(|f| div().flex_none().child(f)))
                        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(weight::LABEL).child(SharedString::from(p.title.clone()))),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S2)
                        .min_w_0()
                        .text_size(t.type_scale.sub)
                        .text_color(t.text_2)
                        .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(SharedString::from(domain)))
                        .when(p.audible, |d| d.child(badge(IconName::Volume2, tr("preview.playing"))))
                        .when(p.asleep, |d| d.child(badge(IconName::Moon, tr("preview.asleep")))),
                ),
        );
        // Slide of 6 px from the side of the bar, plus a fade; the exit runs the same path in reverse.
        let slide = if reduced { 0.0 } else { 6.0 };
        let id = ElementId::from(SharedString::from(format!("tab-preview-{epoch}-{on}")));
        let wrapper = div().absolute().size_full().child(card);
        if !animate {
            return wrapper.into_any_element();
        }
        wrapper
            .with_animation(id, Animation::new(Duration::from_millis(120)).with_easing(ease_out), move |el, d| {
                let k = if on { d } else { 1.0 - d };
                let off = (1.0 - k) * slide;
                let el = el.opacity(k).relative();
                match from {
                    PreviewFrom::Right => el.left(px(-off)),
                    PreviewFrom::Left => el.left(px(off)),
                    PreviewFrom::Below => el.top(px(-off)),
                }
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_of_strips_www_and_keeps_the_hostname() {
        assert_eq!(domain_of("https://www.example.com/a?b"), "example.com");
        assert_eq!(domain_of("http://sub.example.com:8080/"), "sub.example.com");
        assert_eq!(domain_of("https://user:pw@Example.COM/x"), "example.com");
    }

    #[test]
    fn domain_of_shows_internal_addresses_in_full_without_a_trailing_slash() {
        assert_eq!(domain_of("signet://settings/"), "signet://settings");
        assert_eq!(domain_of("SIGNET://note/abc"), "SIGNET://note/abc");
    }

    #[test]
    fn domain_of_returns_the_string_as_is_if_it_is_not_an_address() {
        assert_eq!(domain_of("pas une url"), "pas une url");
        assert_eq!(domain_of(""), "");
    }

    #[test]
    fn domain_of_returns_the_address_when_there_is_no_host() {
        assert_eq!(domain_of("file:///tmp/x.html"), "file:///tmp/x.html");
    }
}

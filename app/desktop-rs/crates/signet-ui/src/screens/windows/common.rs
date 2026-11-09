//! Pieces shared by the window screens: address helpers, bar colors (`barStyle`), buttons (`.st-btn`), the drag area.

use crate::color::{from_hex, luminance};
use crate::icon::IconName;
use crate::theme::{SignetTheme, radius, space, weight};
use crate::widgets::Favicon;
use gpui::{AnyElement, App, Div, ElementId, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div, hsla, px};
use gpui_component::{Icon, tooltip::Tooltip};

/// Host without `www.`; internal `signet:` addresses are shown in full (`lib/util.ts` `domainOf`).
pub fn domain_of(url: &str) -> String {
    if url.len() >= 7 && url[..7].eq_ignore_ascii_case("signet:") {
        return url.trim_end_matches('/').to_string();
    }
    match url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)) {
        Some(host) if !host.is_empty() => host.strip_prefix("www.").map(str::to_string).unwrap_or(host),
        _ => url.to_string(),
    }
}

pub fn is_https(url: &str) -> bool {
    url.len() >= 6 && url[..6].eq_ignore_ascii_case("https:")
}

/// True if light content contrasts better than dark content on a background of relative luminance `l` (WCAG ratios).
pub fn wants_light_text(l: f32) -> bool {
    1.05 / (l + 0.05) > (l + 0.05) / 0.05
}

/// Colors of a bar painted with the site color (`barStyle`): background, text, secondary text and hover.
#[derive(Clone, Copy)]
pub struct BarColors {
    pub bg: Hsla,
    pub fg: Hsla,
    pub fg2: Hsla,
    pub hover: Hsla,
}

impl BarColors {
    /// Theme tokens (no site color).
    pub fn theme(t: &SignetTheme) -> Self {
        Self { bg: t.surface, fg: t.text, fg2: t.text_2, hover: t.surface_2 }
    }

    /// Site color (`#rrggbb`), with light or dark content depending on contrast; theme tokens if it is unreadable.
    pub fn from_color(color: Option<&str>, t: &SignetTheme) -> Self {
        let Some(rgba) = color.and_then(from_hex) else { return Self::theme(t) };
        let light = wants_light_text(luminance(rgba));
        let alpha2 = if t.settings.high_contrast { 0.75 } else { 0.5 } + if light { 0.05 } else { 0.0 };
        let (fg, fg2, hover) = if light {
            (hsla(0.0, 0.0, 1.0, 0.92), hsla(0.0, 0.0, 1.0, alpha2), hsla(0.0, 0.0, 1.0, 0.1))
        } else {
            (hsla(0.0, 0.0, 0.0, 0.85), hsla(0.0, 0.0, 0.0, alpha2), hsla(0.0, 0.0, 0.0, 0.1))
        };
        Self { bg: rgba.into(), fg, fg2, hover }
    }
}

/// Makes an area move the window (title bar replacement) and run the system double-click action. Buttons inside stop
/// the mouse down themselves.
pub fn drag_area<E: InteractiveElement + Styled>(el: E) -> E {
    crate::title_area::title_area(el)
}

/// A favicon from the string the state carries (URL or `data:` URL).
pub fn favicon(src: Option<&str>, size: f32, loading: bool) -> Favicon {
    Favicon::new(super::data_url::favicon_source(src)).size(size).loading(loading)
}

/// `.st-btn`: 28 px control button (add `.child(label)` and `.on_click`).
pub fn st_btn(id: impl Into<ElementId>, t: &SignetTheme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S2)
        .h(px(28.0))
        .px(space::S3)
        .rounded(radius::CONTROL)
        .whitespace_nowrap()
        .bg(t.ctl)
        .border(px(0.5))
        .border_color(t.ctl_ring)
        .text_color(t.text)
        .text_size(t.type_scale.base)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .hover(|s| s.opacity(0.9))
        .active(|s| s.opacity(0.8))
}

/// `.wl-primary` / accent button: accent fill, 28 px.
pub fn accent_btn(id: impl Into<ElementId>, t: &SignetTheme) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .gap(space::S2)
        .h(px(28.0))
        .px(space::S4)
        .rounded(radius::CONTROL)
        .whitespace_nowrap()
        .bg(t.accent)
        .text_color(t.on_accent)
        .text_size(t.type_scale.base)
        .font_weight(weight::LABEL)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .hover(|s| s.opacity(0.94))
        .active(|s| s.opacity(0.85))
}

/// Pill of a bar (`.priv-badge`, `.wa-origin`, `.lib-badge`): rounded label with a background.
pub fn pill(t: &SignetTheme, bg: Hsla, fg: Hsla) -> Div {
    div().flex_none().flex().items_center().gap(space::S1).h(px(22.0)).px(space::S2).rounded(px(11.0)).bg(bg).text_color(fg).text_size(t.type_scale.sub)
}

/// Icon button drawn with the colors of a bar (`.little-bar button.icon`, `.wa-bar button.icon`).
pub struct BarButton {
    pub id: ElementId,
    pub icon: IconName,
    pub icon_size: f32,
    pub size: f32,
    pub tooltip: String,
    pub disabled: bool,
}

impl BarButton {
    pub fn new(id: impl Into<ElementId>, icon: IconName, tooltip: impl Into<String>) -> Self {
        Self { id: id.into(), icon, icon_size: 16.0, size: 28.0, tooltip: tooltip.into(), disabled: false }
    }
    pub fn icon_size(mut self, s: f32) -> Self {
        self.icon_size = s;
        self
    }
    pub fn size(mut self, s: f32) -> Self {
        self.size = s;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn render(self, colors: BarColors, on_click: impl Fn(&mut Window, &mut App) + 'static) -> AnyElement {
        let tip: SharedString = self.tooltip.into();
        let (fg, fg2, hover, disabled) = (colors.fg, colors.fg2, colors.hover, self.disabled);
        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(self.size))
            .rounded(radius::CONTROL)
            .text_color(fg2)
            .child(Icon::from(self.icon).size(px(self.icon_size)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if disabled {
            el = el.opacity(0.35);
        } else {
            el = el
                .cursor_pointer()
                .hover(move |s| s.bg(hover).text_color(fg))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_click(move |_, window, cx| on_click(window, cx));
        }
        el.into_any_element()
    }
}

/// `.pane-shadow`: card with the theme's shadow, drawn under a native web view.
pub fn pane_card(t: &SignetTheme) -> Div {
    div().absolute().rounded(radius::CARD).bg(t.surface).shadow(t.shadow_card.clone())
}

pub use crate::title_area::no_drag;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_strips_www_and_keeps_internal_addresses() {
        assert_eq!(domain_of("https://www.example.com/a?b=1"), "example.com");
        assert_eq!(domain_of("http://localhost:3000/x"), "localhost");
        assert_eq!(domain_of("signet://note/abc/"), "signet://note/abc");
        assert_eq!(domain_of("not a url"), "not a url");
    }

    #[test]
    fn https_check_is_case_insensitive() {
        assert!(is_https("HTTPS://a.test"));
        assert!(!is_https("http://a.test"));
    }

    #[test]
    fn text_contrast_rule() {
        assert!(wants_light_text(0.0));
        assert!(!wants_light_text(1.0));
        assert!(!wants_light_text(0.3));
    }
}

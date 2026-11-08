use crate::icon::IconName;
use crate::theme::{ThemeExt, weight};
use gpui::{Animation, AnimationExt, App, ElementId, Hsla, ImageSource, IntoElement, ParentElement, RenderOnce, SharedString, Styled, StyledImage, Window, div, img, px, ObjectFit};
use gpui_component::Icon;
use std::{sync::Arc, time::Duration};

/// Where a favicon comes from.
#[derive(Clone)]
pub enum FaviconSource {
    /// Decoded bytes (PNG, ICO converted upstream, SVG...) as stored by the app.
    Bytes(Arc<gpui::Image>),
    /// `http(s)` or `data:`-free URL, loaded by GPUI's HTTP client.
    Url(SharedString),
    /// Local file or embedded asset path.
    Path(SharedString),
}

impl From<FaviconSource> for ImageSource {
    fn from(s: FaviconSource) -> Self {
        match s {
            FaviconSource::Bytes(b) => b.into(),
            FaviconSource::Url(u) | FaviconSource::Path(u) => u.into(),
        }
    }
}

/// Site icon. Without an image (or when it fails to load) it shows a globe, or the first letter of the site on its
/// color when `letter` is set.
#[derive(IntoElement)]
pub struct Favicon {
    source: Option<FaviconSource>,
    size: f32,
    loading: bool,
    letter: Option<(char, Hsla)>,
}

impl Favicon {
    pub fn new(source: Option<FaviconSource>) -> Self {
        Self { source, size: 16.0, loading: false, letter: None }
    }
    pub fn size(mut self, s: f32) -> Self {
        self.size = s;
        self
    }
    /// Pulsing while the page loads.
    pub fn loading(mut self, l: bool) -> Self {
        self.loading = l;
        self
    }
    /// Fallback: a letter on a colored rounded square instead of the globe.
    pub fn letter(mut self, ch: char, color: Hsla) -> Self {
        self.letter = Some((ch.to_uppercase().next().unwrap_or(ch), color));
        self
    }
}

fn fallback(size: f32, letter: Option<(char, Hsla)>, text_2: Hsla, on_accent: Hsla) -> gpui::AnyElement {
    match letter {
        Some((ch, color)) => div()
            .flex()
            .items_center()
            .justify_center()
            .size(px(size))
            .rounded(px((size * 0.25).max(3.0)))
            .bg(color)
            .text_color(on_accent)
            .text_size(px(size * 0.62))
            .font_weight(weight::STRONG)
            .child(ch.to_string())
            .into_any_element(),
        None => Icon::from(IconName::Globe).size(px(size)).text_color(text_2).into_any_element(),
    }
}

thread_local! {
    static RESOLVED: std::cell::RefCell<std::collections::HashMap<String, Arc<gpui::Image>>> = Default::default();
    /// Per URL: number of attempts and time of the last one.
    static TRIES: std::cell::RefCell<std::collections::HashMap<String, (u8, Option<std::time::Instant>)>> = Default::default();
}

/// GPUI has no HTTP client here: a web URL is resolved through the app (`fallback_favicon`, downloaded in the
/// background). On a miss the widget redraws a few times until the download lands.
fn resolve_url(url: &SharedString, w: &mut Window, cx: &mut App) -> Option<Arc<gpui::Image>> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    if let Some(hit) = RESOLVED.with(|r| r.borrow().get(url.as_ref()).cloned()) {
        return Some(hit);
    }
    // Renders run every frame during animations: ask the app at most every 250 ms per URL, and stop after 40 tries.
    let now = std::time::Instant::now();
    let (n, due) = TRIES.with(|t| {
        let mut t = t.borrow_mut();
        let e = t.entry(url.to_string()).or_insert((0, None));
        let due = e.1.is_none_or(|last: std::time::Instant| now.duration_since(last) >= Duration::from_millis(250));
        if due {
            e.0 = e.0.saturating_add(1);
            e.1 = Some(now);
        }
        (e.0, due)
    });
    if !due || n > 40 {
        return None;
    }
    if let Some(img) = crate::app_state::api(cx).fallback_favicon(url).and_then(|d| crate::data_url::image_from_data_url(&d)) {
        RESOLVED.with(|r| {
            let mut r = r.borrow_mut();
            if r.len() > 512 {
                r.clear();
            }
            r.insert(url.to_string(), img.clone());
        });
        return Some(img);
    }
    if n < 40 {
        w.spawn(cx, async move |cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            cx.update(|w, _| w.refresh()).ok();
        })
        .detach();
    }
    None
}

impl RenderOnce for Favicon {
    fn render(self, w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet();
        let (size, letter, text_2, on_accent) = (self.size, self.letter, t.text_2, t.on_accent);
        let source = match self.source {
            Some(FaviconSource::Url(u)) => resolve_url(&u, w, cx).map(FaviconSource::Bytes),
            other => other,
        };
        let body: gpui::AnyElement = match source {
            Some(src) => img(src)
                .size(px(size))
                .flex_none()
                .object_fit(ObjectFit::Contain)
                .with_fallback(move || fallback(size, letter, text_2, on_accent))
                .into_any_element(),
            None => fallback(size, letter, text_2, on_accent),
        };
        let wrapper = div().flex_none().size(px(size)).child(body);
        if self.loading {
            let id = ElementId::from(SharedString::from(format!("favicon-pulse-{size}")));
            wrapper
                .with_animation(id, Animation::new(Duration::from_millis(1100)).repeat().with_easing(gpui::pulsating_between(0.45, 1.0)), |el, d| el.opacity(d))
                .into_any_element()
        } else {
            wrapper.into_any_element()
        }
    }
}

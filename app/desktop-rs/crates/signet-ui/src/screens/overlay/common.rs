//! Small helpers shared by the overlay surfaces.

use crate::color::{accent_color, hsla};
use crate::theme::{ThemeExt, radius, space};
use crate::widgets::{Favicon, FaviconSource};
use gpui::{
    App, Div, ElementId, Hsla, InteractiveElement, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::Icon;

/// Accent color of a view whose hue differs from the current space (`--h` on a surface).
pub fn accent_for(cx: &App, hue: f64) -> Hsla {
    hsla(accent_color(hue as f32, cx.signet().scheme))
}

/// A lucide icon by its kebab-case name (`icons/<name>.svg`, served by `SignetAssets`).
pub fn lucide(name: &str) -> Icon {
    Icon::empty().path(SharedString::from(format!("icons/{name}.svg")))
}

/// Window size in px.
pub fn viewport(window: &Window) -> (f32, f32) {
    let s = window.viewport_size();
    (f32::from(s.width), f32::from(s.height))
}

/// Source of an image given as a data URL or an address (other strings are local paths).
pub fn image_source(src: &str) -> Option<FaviconSource> {
    if src.is_empty() {
        return None;
    }
    if src.starts_with("data:") || src.starts_with("http://") || src.starts_with("https://") {
        return crate::data_url::favicon_source(Some(src));
    }
    Some(FaviconSource::Path(SharedString::from(src.to_string())))
}

/// Site icon (globe when missing).
pub fn favicon(src: Option<&str>, size: f32) -> Favicon {
    Favicon::new(src.and_then(image_source)).size(size)
}

/// Emoji in a fixed box.
pub fn emoji(text: &str, size: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .text_size(px(size - 1.0))
        .line_height(px(size))
        .font_family(crate::theme::emoji_font())
        .child(SharedString::from(text.to_string()))
}

/// Floating panel (`.site-panel`): glass fill, popover shadow, hairline ring. Blocks the mouse behind it.
pub fn panel(cx: &App) -> Div {
    let t = cx.signet();
    div()
        .occlude()
        .flex()
        .flex_col()
        .rounded(radius::POP)
        .bg(t.glass_view)
        .border_1()
        .border_color(t.border)
        .shadow(t.shadow_pop.clone())
        .text_color(t.text)
        .text_size(t.type_scale.base)
}

/// `.site-btn-s`: 24 px pill-ish button on `surface-2`.
pub fn small_btn(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let t = cx.signet();
    let (hover, active) = (t.ctl_ring, t.border);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(5.0))
        .h(px(24.0))
        .px(space::S3)
        .rounded(radius::CONTROL)
        .text_size(t.type_scale.sub)
        .bg(t.surface_2)
        .text_color(t.text)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(active))
}

/// Horizontal hairline.
pub fn hairline(cx: &App) -> Div {
    div().h(px(0.5)).w_full().flex_none().bg(cx.signet().border)
}

/// Host shown under a panel title (`origin` without its scheme).
pub fn host_of(origin: &str) -> String {
    match origin.find("://") {
        Some(i) if origin[..i].chars().all(|c| c.is_alphanumeric() || c == '_') => origin[i + 3..].to_string(),
        _ => origin.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_without_scheme() {
        assert_eq!(host_of("https://example.com"), "example.com");
        assert_eq!(host_of("example.com"), "example.com");
    }
}

/// Form container of the popovers (`.space-form`): opaque surface, padding 12, gap 12.
pub fn form_box(cx: &App) -> Div {
    let t = cx.signet();
    div()
        .occlude()
        .flex()
        .flex_col()
        .gap(space::S3)
        .p(space::S3)
        .rounded(radius::POP)
        .bg(t.surface)
        .border_1()
        .border_color(t.border)
        .shadow(t.shadow_pop.clone())
        .text_color(t.text)
        .text_size(t.type_scale.base)
}

/// Button of a form's `.actions` row: ghost, or accent-filled when `primary`.
pub fn form_btn(id: impl Into<ElementId>, label: impl Into<SharedString>, primary: bool, disabled: bool, cx: &App) -> Stateful<Div> {
    let t = cx.signet();
    let (surface_2, active, text, border) = (t.surface_2, t.active, t.text, t.border);
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(28.0))
        .px(space::S3)
        .rounded(radius::CONTROL)
        .whitespace_nowrap()
        .text_color(text)
        .hover(move |s| s.bg(surface_2))
        .child(label.into());
    // `button.primary`: the `--active` fill with its shadow and a hairline ring (not the accent color).
    let base = if primary {
        let mut shadow = t.active_shadow.clone();
        shadow.push(gpui::BoxShadow { color: border, offset: gpui::point(px(0.0), px(0.0)), blur_radius: px(0.0), spread_radius: px(0.5), inset: false });
        base.bg(active).shadow(shadow)
    } else {
        base
    };
    if disabled { base.opacity(0.45) } else { base.cursor_pointer() }
}

/// Folder of a list of pinned items, anywhere in the tree.
pub fn find_folder<'a>(items: &'a [signet_core::types::Item], id: &str) -> Option<&'a signet_core::types::FolderItem> {
    use signet_core::types::Item;
    for i in items {
        if let Item::Folder(f) = i {
            if f.id == id {
                return Some(f);
            }
            if let Some(inner) = find_folder(&f.items, id) {
                return Some(inner);
            }
        }
    }
    None
}

/// Bookmark folder (or link, with `any`) anywhere in the tree.
pub fn find_bookmark<'a>(items: &'a [signet_core::types::Bookmark], id: &str) -> Option<&'a signet_core::types::Bookmark> {
    use signet_core::types::Bookmark;
    for i in items {
        if i.id() == id {
            return Some(i);
        }
        if let Bookmark::Folder { items, .. } = i
            && let Some(f) = find_bookmark(items, id)
        {
            return Some(f);
        }
    }
    None
}

/// The active space (or the first one).
pub fn active_space(s: &signet_core::types::Snapshot) -> Option<&signet_core::types::Space> {
    s.spaces.iter().find(|x| x.id == s.active_space_id).or(s.spaces.first())
}

/// Title shown for a tab: the user's name, then the page's, then its address.
pub fn display_title(tab: &signet_core::types::TabInfo, fallback: &str) -> String {
    [tab.custom_title.as_deref(), Some(tab.title.as_str()), Some(tab.url.as_str())]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| fallback.to_string())
}

/// Small arrow on top of a panel, pointing up at x = `at` (relative to the panel's left edge).
pub fn arrow_up(cx: &App, at: f32) -> Div {
    let t = cx.signet();
    let (fill, line) = (t.glass_view, t.border);
    div().absolute().top(px(-6.0)).left(px(at - 6.0)).w(px(12.0)).h(px(6.0)).child(
        gpui::canvas(
            |_, _, _| (),
            move |b, _, window, _| {
                let (x, y, w, h) = (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height));
                let tri = |inset: f32| {
                    let mut p = gpui::PathBuilder::fill();
                    p.move_to(gpui::point(px(x + inset), px(y + h)));
                    p.line_to(gpui::point(px(x + w / 2.0), px(y + inset)));
                    p.line_to(gpui::point(px(x + w - inset), px(y + h)));
                    p.close();
                    p.build().ok()
                };
                if let Some(path) = tri(0.0) {
                    window.paint_path(path, line);
                }
                if let Some(path) = tri(0.8) {
                    window.paint_path(path, fill);
                }
            },
        )
        .size_full(),
    )
}

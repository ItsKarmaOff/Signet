//! Port of `windows/WebAppInstall.tsx` + the `.wa-install-*` rules of `webapp.css`: the offer to install a site as an
//! app, the content of the overlay's `webapp` mode (`OverlayPayload::Webapp`).
//!
//! The overlay root builds `WebAppInstall::new(window, cx, token, name, url, icon, maskable, hue, card)` and gives it the
//! whole overlay window. The card is 320 px wide at the top right of `card`. The icon is composed by
//! [`super::icon_compose`] and handed over as a 1024 px PNG data URL (`install_web_app`). An image can be chosen with
//! the button or the icon, dropped on the card, or pasted (Cmd/Ctrl+V).

use super::common::domain_of;
use super::data_url::decode_data_url;
use super::icon_compose::{Fit, MAX_CUSTOM_BYTES, SIZE, auto_fit, compose, encode_png, load_image, to_png_data_url};
use super::site_panel::site_btn;
use crate::app_state::{api, tr};
use crate::icon::IconName;
use crate::motion::{reduced_motion, surface_enter};
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::Segmented;
use gpui::{
    AppContext, ClipboardEntry, Context, Entity, ExternalPaths, FocusHandle, Focusable, Image, ImageFormat, InteractiveElement, IntoElement, MouseButton, ObjectFit, ParentElement, PathPromptOptions, Render,
    StatefulInteractiveElement, Styled, StyledImage, Window, div, img, prelude::FluentBuilder, px,
};
use gpui_component::{
    Icon,
    input::{Input, InputEvent, InputState},
};
use image::RgbaImage;
use signet_core::types::Rect;
use std::sync::Arc;

pub const WIDTH: f32 = 320.0;
/// Preview size in px (shown at 64 logical px, drawn at 2x).
const PREVIEW: u32 = 128;
const NAME_MAX: usize = 60;

pub struct WebAppInstall {
    token: String,
    url: String,
    default_name: String,
    hue: f32,
    maskable: bool,
    card: Rect,
    input: Entity<InputState>,
    site: Option<RgbaImage>,
    /// Image chosen by the user (file, drag and drop or paste), which replaces the site's icon.
    custom: Option<RgbaImage>,
    fit: Fit,
    bad: bool,
    dragging: bool,
    busy: bool,
    preview: Option<Arc<Image>>,
    focus: FocusHandle,
}

/// Card position: top right of the page card, kept inside the window.
pub fn placement(card: &Rect, window_width: f64) -> (f64, f64) {
    let left = (card.x + card.width - f64::from(WIDTH) - 12.0).min(window_width - f64::from(WIDTH) - 8.0).max(8.0);
    (left, card.y + 12.0)
}

impl WebAppInstall {
    #[allow(clippy::too_many_arguments)]
    pub fn new(window: &mut Window, cx: &mut gpui::App, token: String, name: String, url: String, icon: Option<String>, maskable: bool, hue: f64, card: Rect) -> Entity<Self> {
        let site = icon.as_deref().and_then(decode_data_url).and_then(|(_, bytes)| load_image(&bytes));
        let fit = site.as_ref().map(|i| auto_fit(i, maskable)).unwrap_or(Fit::Fit);
        let view = cx.new(|cx: &mut Context<Self>| {
            let initial = name.clone();
            let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
            cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, _window, cx| match ev {
                InputEvent::Change => {
                    // maxLength 60
                    let text = input.read(cx).value().to_string();
                    if text.chars().count() > NAME_MAX {
                        let cut: String = text.chars().take(NAME_MAX).collect();
                        input.update(cx, |i, cx| i.set_value(cut, _window, cx));
                    }
                    this.rebuild_preview(cx);
                }
                InputEvent::PressEnter { .. } => this.install(cx),
                _ => {}
            })
            .detach();
            let mut me = Self { token, url, default_name: name, hue: hue as f32, maskable, card, input, site, custom: None, fit, bad: false, dragging: false, busy: false, preview: None, focus: cx.focus_handle() };
            me.rebuild_preview(cx);
            me
        });
        let input = view.read(cx).input.clone();
        input.update(cx, |i, cx| i.focus(window, cx));
        view
    }

    fn image(&self) -> Option<&RgbaImage> {
        self.custom.as_ref().or(self.site.as_ref())
    }

    fn name_text(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Without an image, the initial follows the typed name.
    fn rebuild_preview(&mut self, cx: &mut Context<Self>) {
        let name = self.name_text(cx);
        let letter_name = if self.image().is_some() { String::new() } else { name };
        let label = if letter_name.trim().is_empty() { self.default_name.clone() } else { letter_name };
        let composed = compose(self.image(), self.fit, &label, self.hue, PREVIEW);
        self.preview = encode_png(&composed).map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
        cx.notify();
    }

    fn take_bytes(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        if bytes.len() > MAX_CUSTOM_BYTES {
            self.bad = true;
            return cx.notify();
        }
        match load_image(bytes) {
            Some(image) => {
                self.bad = false;
                self.fit = auto_fit(&image, false);
                self.custom = Some(image);
            }
            None => self.bad = true,
        }
        self.rebuild_preview(cx);
    }

    fn take_path(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        match std::fs::metadata(path) {
            Ok(m) if m.len() as usize <= MAX_CUSTOM_BYTES => match std::fs::read(path) {
                Ok(bytes) => self.take_bytes(&bytes, cx),
                Err(_) => {
                    self.bad = true;
                    cx.notify();
                }
            },
            _ => {
                self.bad = true;
                cx.notify();
            }
        }
    }

    fn choose_file(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(path) = paths.first()
            {
                this.update(cx, |this, cx| this.take_path(path, cx)).ok();
            }
        })
        .detach();
    }

    fn reset_to_site(&mut self, cx: &mut Context<Self>) {
        self.custom = None;
        self.bad = false;
        self.fit = self.site.as_ref().map(|i| auto_fit(i, self.maskable)).unwrap_or(Fit::Fit);
        self.rebuild_preview(cx);
    }

    fn paste(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else { return false };
        for entry in item.entries() {
            if let ClipboardEntry::Image(image) = entry {
                let bytes = image.bytes.clone();
                self.take_bytes(&bytes, cx);
                return true;
            }
        }
        false
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let typed = self.name_text(cx);
        let name = if typed.trim().is_empty() { return } else { typed.trim().to_string() };
        self.busy = true;
        let icon = to_png_data_url(&compose(self.image(), self.fit, &name, self.hue, SIZE));
        api(cx).install_web_app(&self.token, &name, &icon);
        cx.notify();
    }
}

impl Focusable for WebAppInstall {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for WebAppInstall {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let reduced = reduced_motion(cx);
        let me = cx.entity();
        let (left, top) = placement(&self.card, f64::from(f32::from(window.viewport_size().width)));
        let has_image = self.image().is_some();
        let dragging = self.dragging;
        let name_empty = self.name_text(cx).trim().is_empty();
        let busy = self.busy;
        let hover_badge = t.accent;

        let pick = {
            let me = me.clone();
            let preview = self.preview.clone();
            div()
                .id("wa-pick")
                .group("wa-pick")
                .relative()
                .flex_none()
                .size(px(64.0))
                .rounded(px(16.0))
                .cursor_pointer()
                .tooltip(|w, cx| gpui_component::tooltip::Tooltip::new(tr("webapp.iconHint")).build(w, cx))
                .on_click(move |_, _, cx| me.update(cx, |this, cx| this.choose_file(cx)))
                .children(preview.map(|p| img(p).size(px(64.0)).object_fit(ObjectFit::Contain)))
                .child(
                    div()
                        .absolute()
                        .right(px(-2.0))
                        .bottom(px(-2.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(22.0))
                        .rounded_full()
                        .bg(hover_badge)
                        .text_color(t.on_accent)
                        .border_2()
                        .border_color(t.glass_view)
                        .when(!dragging, |d| d.invisible().group_hover("wa-pick", |s| s.visible()))
                        .child(Icon::from(IconName::ImagePlus).size(px(12.0))),
                )
        };

        let head = div()
            .flex()
            .flex_col()
            .gap(px(1.0))
            .px(space::S2)
            .pt(space::S2)
            .pb(space::S3)
            .child(div().font_weight(weight::STRONG).child(tr("webapp.installTitle")))
            .child(div().text_size(t.type_scale.sub).text_color(t.text_2).child(domain_of(&self.url)));

        let body = div()
            .flex()
            .items_center()
            .gap(space::S3)
            .pt(space::S1)
            .px(space::S2)
            .pb(space::S2)
            .child(pick)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(space::S1)
                    .text_size(t.type_scale.sub)
                    .text_color(t.text_2)
                    .child(tr("webapp.name"))
                    .child(
                        div()
                            .h(px(28.0))
                            .rounded(radius::CONTROL)
                            .bg(t.ctl)
                            .border_1()
                            .border_color(t.ctl_ring)
                            .text_size(t.type_scale.base)
                            .text_color(t.text)
                            .child(Input::new(&self.input).appearance(false)),
                    ),
            );

        let fit_ix = Fit::ALL.iter().position(|f| *f == self.fit).unwrap_or(1);
        let (me_fit, me_choose, me_reset) = (me.clone(), me.clone(), me.clone());
        let controls = div()
            .flex()
            .flex_col()
            .gap(space::S2)
            .px(space::S2)
            .pb(space::S3)
            .when(has_image, |c| {
                c.child(
                    Segmented::new("wa-fit")
                        .item(tr("webapp.fit.fill"))
                        .item(tr("webapp.fit.fit"))
                        .item(tr("webapp.fit.raw"))
                        .selected(fit_ix)
                        .on_change(move |ix, _, cx| me_fit.update(cx, |this, cx| {
                            this.fit = Fit::ALL[ix];
                            this.rebuild_preview(cx);
                        })),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(space::S2)
                    .child(site_btn("wa-choose", &t).on_click(move |_, _, cx| me_choose.update(cx, |this, cx| this.choose_file(cx))).child(tr("webapp.chooseImage")))
                    .when(self.custom.is_some(), |r| {
                        r.child(site_btn("wa-reset", &t).on_click(move |_, _, cx| me_reset.update(cx, |this, cx| this.reset_to_site(cx))).child(if self.site.is_some() { tr("webapp.siteIcon") } else { tr("webapp.noImage") }))
                    })
                    .when(self.bad, |r| r.child(div().text_size(t.type_scale.sub).text_color(t.danger).child(tr("webapp.badImage")))),
            );

        let hint = div().px(space::S2).pb(space::S3).text_size(t.type_scale.sub).line_height(gpui::relative(1.4)).text_color(t.text_2).child(tr("webapp.installHint"));

        let disabled = busy || name_empty;
        let me_install = me.clone();
        let foot = div()
            .flex()
            .items_center()
            .gap(space::S2)
            .pt(space::S2)
            .pb(px(2.0))
            .px(space::S1)
            .border_t(px(0.5))
            .border_color(t.border)
            .child(div().flex_1())
            .child(site_btn("wa-cancel", &t).on_click(|_, _, cx| api(cx).close_overlay(false)).child(tr("common.cancel")))
            .child(
                site_btn("wa-install", &t)
                    .bg(t.accent)
                    .text_color(t.on_accent)
                    .when(disabled, |b| b.opacity(0.5))
                    .when(!disabled, |b| b.on_click(move |_, _, cx| me_install.update(cx, |this, cx| this.install(cx))))
                    .child(tr("webapp.install")),
            );

        let me_drop = me.clone();
        let me_key = me.clone();
        let card = div()
            .id("wa-install")
            .track_focus(&self.focus)
            .absolute()
            .left(px(left as f32))
            .top(px(top as f32))
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .p(px(6.0))
            .rounded(radius::POP)
            .bg(t.glass_view)
            .border(px(if dragging { 2.0 } else { 0.5 }))
            .border_color(if dragging { t.accent } else { t.border })
            .shadow(t.shadow_pop.clone())
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .drag_over::<ExternalPaths>({
                let me = me.clone();
                move |s, _, _, cx| {
                    me.update(cx, |this, cx| {
                        if !this.dragging {
                            this.dragging = true;
                            cx.notify();
                        }
                    });
                    s
                }
            })
            .on_drop(move |paths: &ExternalPaths, _, cx| {
                me_drop.update(cx, |this, cx| {
                    this.dragging = false;
                    if let Some(path) = paths.paths().first() {
                        this.take_path(path, cx);
                    }
                })
            })
            .on_key_down(move |ev, _, cx| {
                let m = ev.keystroke.modifiers;
                if ev.keystroke.key == "escape" {
                    cx.stop_propagation();
                    api(cx).close_overlay(false);
                } else if (m.platform || m.control) && ev.keystroke.key == "v" {
                    // An image on the clipboard becomes the icon; text is left to the name field.
                    let taken = me_key.update(cx, |this, cx| this.paste(cx));
                    if taken {
                        cx.stop_propagation();
                    }
                }
            })
            .child(head)
            .child(body)
            .child(controls)
            .child(hint)
            .child(foot);

        div().id("wa-layer").absolute().inset_0().on_mouse_down(MouseButton::Left, |_, _, cx| api(cx).close_overlay(false)).child(surface_enter(card, "wa-enter", false, reduced))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_sits_at_the_top_right_of_the_page_card() {
        let (left, top) = placement(&Rect::new(200.0, 50.0, 800.0, 600.0), 1000.0);
        assert_eq!((left, top), (668.0, 62.0));
        // never past the window
        let (left, _) = placement(&Rect::new(200.0, 50.0, 2000.0, 600.0), 1000.0);
        assert_eq!(left, 672.0);
        // never before the left margin
        let (left, _) = placement(&Rect::new(0.0, 0.0, 100.0, 100.0), 1000.0);
        assert_eq!(left, 8.0);
    }
}

//! Offer to install the site as an app (port of `windows/WebAppInstall.tsx`, `webapp` mode of the overlay): name field,
//! icon framed on a rounded plate (the 1024 px PNG is composed here), at the top right of the page.

use super::common::{host_of, lucide, panel, small_btn, viewport};
use crate::app_state::{api, tr};
use crate::color::hsl_to_rgb;
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::{Segmented, TextField, text_field_state};
use base64::Engine as _;
use gpui::{
    AppContext, Context, Entity, ImageFormat, InteractiveElement, IntoElement, ObjectFit, ParentElement, Render, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, div, img,
    prelude::FluentBuilder, px,
};
use gpui_component::input::{InputEvent, InputState};
use image::{DynamicImage, Rgba, RgbaImage, imageops};
use signet_core::types::Rect;
use std::sync::Arc;

const WIDTH: f32 = 320.0;
/// macOS icon grid: 1024 px square, 824 px plate with rounded corners, light shadow.
const SIZE: u32 = 1024;
const PLATE: f32 = 824.0;
const RADIUS: f32 = 185.0;
/// Image chosen by the user: 15 MB at most (it is downscaled to 1024 px anyway).
const MAX_CUSTOM_BYTES: u64 = 15_000_000;

/// How the image is framed in the icon: `Fill` the whole rounded plate, `Fit` placed at 60% on a white plate, `Raw` the
/// image as is over the whole square (an icon already drawn for the Mac).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    Fill,
    Fit,
    Raw,
}

const FITS: [Fit; 3] = [Fit::Fill, Fit::Fit, Fit::Raw];

fn fit_key(f: Fit) -> &'static str {
    match f {
        Fit::Fill => "webapp.fit.fill",
        Fit::Fit => "webapp.fit.fit",
        Fit::Raw => "webapp.fit.raw",
    }
}

/// True if the image's corners are opaque (full square, to stretch over the whole plate rather than center in it).
pub fn opaque_corners(img: &RgbaImage) -> bool {
    let small = imageops::resize(img, 32, 32, imageops::FilterType::Triangle);
    [(1, 1), (30, 1), (1, 30), (30, 30)].iter().all(|(x, y)| small.get_pixel(*x, *y)[3] > 240)
}

/// Suggested framing: plate filled for a maskable icon or a crisp opaque square, otherwise placed on a white background.
pub fn auto_fit(img: &RgbaImage, maskable: bool) -> Fit {
    let natural = img.width().min(img.height());
    if maskable || (natural >= 120 && opaque_corners(img)) { Fit::Fill } else { Fit::Fit }
}

/// Coverage (0..1) of a rounded square of side `size` and corner radius `r` at pixel center (x, y), centered in a canvas.
fn plate_cover(x: f32, y: f32, canvas: f32, size: f32, r: f32) -> f32 {
    let (cx, cy) = (canvas / 2.0, canvas / 2.0);
    let (dx, dy) = ((x - cx).abs() - (size / 2.0 - r), (y - cy).abs() - (size / 2.0 - r));
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    let inside = dx.max(dy).min(0.0);
    let dist = outside + inside - r;
    (0.5 - dist).clamp(0.0, 1.0)
}

fn over(dst: &mut Rgba<u8>, rgb: [f32; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let oa = a + da * (1.0 - a);
    for i in 0..3 {
        let d = dst[i] as f32 / 255.0;
        let o = (rgb[i] * a + d * da * (1.0 - a)) / oa.max(1e-6);
        dst[i] = (o * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (oa * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn scaled(img: &RgbaImage, ratio: f32) -> RgbaImage {
    let (w, h) = (((img.width() as f32 * ratio).round() as u32).max(1), ((img.height() as f32 * ratio).round() as u32).max(1));
    imageops::resize(img, w, h, imageops::FilterType::Lanczos3)
}

/// App icon: the image on a rounded plate according to `fit`, or, without an image, a tinted plate. `size` is the side
/// in px (1024 for the real icon, less for the preview).
pub fn compose(img: Option<&RgbaImage>, fit: Fit, hue: f64, size: u32) -> RgbaImage {
    let s = size as f32 / SIZE as f32;
    let mut out = RgbaImage::new(size, size);
    let canvas = size as f32;
    if let (Some(img), Fit::Raw) = (img, fit) {
        // As is: contained in the whole square, no plate or shadow.
        let src = scaled(img, (canvas / img.width() as f32).min(canvas / img.height() as f32));
        imageops::overlay(&mut out, &src, ((size - src.width()) / 2) as i64, ((size - src.height()) / 2) as i64);
        return out;
    }
    let (plate, radius) = (PLATE * s, RADIUS * s);
    let cover: Vec<f32> = (0..size * size).map(|i| plate_cover((i % size) as f32 + 0.5, (i / size) as f32 + 0.5, canvas, plate, radius)).collect();

    // Drop shadow of the plate, like system icons.
    let (offset, sigma) = ((12.0 * s).round() as i64, (14.0 * s).max(0.5));
    let mut shadow = RgbaImage::new(size, size);
    for y in 0..size as i64 {
        for x in 0..size as i64 {
            let src_y = y - offset;
            if src_y >= 0 && src_y < size as i64 {
                let a = cover[(src_y as u32 * size + x as u32) as usize] * 0.3;
                shadow.put_pixel(x as u32, y as u32, Rgba([0, 0, 0, (a * 255.0) as u8]));
            }
        }
    }
    let shadow = imageops::blur(&shadow, sigma);
    imageops::overlay(&mut out, &shadow, 0, 0);

    // Plate.
    let base = if img.is_some() { [1.0, 1.0, 1.0] } else { hsl_to_rgb(hue as f32, 0.5, 0.48) };
    for (i, c) in cover.iter().enumerate() {
        over(out.get_pixel_mut(i as u32 % size, i as u32 / size), base, *c);
    }
    match (img, fit) {
        (Some(img), Fit::Fill) => {
            // Covers the plate while keeping proportions.
            let src = scaled(img, (plate / img.width() as f32).max(plate / img.height() as f32));
            let (ox, oy) = ((canvas - src.width() as f32) / 2.0, (canvas - src.height() as f32) / 2.0);
            blit_masked(&mut out, &src, ox, oy, &cover);
        }
        (Some(img), _) => {
            let boxed = plate * 0.6;
            let src = scaled(img, (boxed / img.width() as f32).min(boxed / img.height() as f32));
            blit_masked(&mut out, &src, (canvas - src.width() as f32) / 2.0, (canvas - src.height() as f32) / 2.0, &cover);
        }
        (None, _) => {
            // Light-to-dark wash on the plate.
            let top = (canvas - plate) / 2.0;
            for (i, c) in cover.iter().enumerate() {
                let (x, y) = (i as u32 % size, i as u32 / size);
                let t = ((y as f32 - top) / plate).clamp(0.0, 1.0);
                let (rgb, a) = if t < 0.5 { ([1.0; 3], 0.18 * (1.0 - t * 2.0)) } else { ([0.0; 3], 0.12 * (t * 2.0 - 1.0)) };
                over(out.get_pixel_mut(x, y), rgb, a * c);
            }
        }
    }
    out
}

fn blit_masked(out: &mut RgbaImage, src: &RgbaImage, ox: f32, oy: f32, cover: &[f32]) {
    let size = out.width();
    for (x, y, p) in src.enumerate_pixels() {
        let (dx, dy) = (x as i64 + ox.round() as i64, y as i64 + oy.round() as i64);
        if dx < 0 || dy < 0 || dx >= size as i64 || dy >= size as i64 {
            continue;
        }
        let c = cover[(dy as u32 * size + dx as u32) as usize];
        over(out.get_pixel_mut(dx as u32, dy as u32), [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0], p[3] as f32 / 255.0 * c);
    }
}

fn png_bytes(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = DynamicImage::ImageRgba8(img.clone()).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
    out
}

/// `data:image/png;base64,...` of an image.
pub fn png_data_url(img: &RgbaImage) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png_bytes(img)))
}

fn decode_data_url(src: &str) -> Option<RgbaImage> {
    let (meta, data) = src.strip_prefix("data:")?.split_once(',')?;
    let bytes = if meta.contains(";base64") { base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()? } else { data.as_bytes().to_vec() };
    image::load_from_memory(&bytes).ok().map(|i| i.to_rgba8()).filter(|i| i.width() > 0 && i.height() > 0)
}

pub struct WebAppInstall {
    token: String,
    url: String,
    fallback_name: String,
    maskable: bool,
    hue: f64,
    card: Rect,
    name: Entity<InputState>,
    site: Option<RgbaImage>,
    /// Image chosen by the user, which replaces the site's icon.
    custom: Option<RgbaImage>,
    fit: Fit,
    bad: bool,
    busy: bool,
    preview: Option<Arc<gpui::Image>>,
    _sub: Subscription,
}

impl WebAppInstall {
    #[allow(clippy::too_many_arguments)]
    pub fn new(token: String, name: String, url: String, icon: Option<String>, maskable: bool, hue: f64, card: Rect, window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let input = text_field_state(window, cx, "", name.clone());
            let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _window, cx| match ev {
                InputEvent::PressEnter { .. } => this.install(cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            });
            input.update(cx, |i, cx| i.focus(window, cx));
            let site = icon.as_deref().and_then(decode_data_url);
            let fit = site.as_ref().map(|i| auto_fit(i, maskable)).unwrap_or(Fit::Fit);
            let mut this = Self { token, url, fallback_name: name, maskable, hue, card, name: input, site, custom: None, fit, bad: false, busy: false, preview: None, _sub: sub };
            this.refresh_preview();
            this
        })
    }

    fn image(&self) -> Option<&RgbaImage> {
        self.custom.as_ref().or(self.site.as_ref())
    }

    fn refresh_preview(&mut self) {
        let icon = compose(self.image(), self.fit, self.hue, 128);
        self.preview = Some(Arc::new(gpui::Image::from_bytes(ImageFormat::Png, png_bytes(&icon))));
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_string();
        let name = if name.is_empty() { self.fallback_name.clone() } else { name };
        if self.busy || name.is_empty() {
            return;
        }
        self.busy = true;
        let icon = png_data_url(&compose(self.image(), self.fit, self.hue, SIZE));
        api(cx).install_web_app(&self.token, &name, &icon);
        cx.notify();
    }

    fn choose_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let picked = std::fs::metadata(&path).ok().filter(|m| m.len() <= MAX_CUSTOM_BYTES).and_then(|_| std::fs::read(&path).ok()).and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgba8());
            this.update(cx, |this, cx| {
                match picked {
                    Some(img) => {
                        this.bad = false;
                        this.fit = auto_fit(&img, false);
                        this.custom = Some(img);
                        this.refresh_preview();
                    }
                    None => this.bad = true,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn reset_to_site(&mut self, cx: &mut Context<Self>) {
        self.custom = None;
        self.bad = false;
        self.fit = self.site.as_ref().map(|i| auto_fit(i, self.maskable)).unwrap_or(Fit::Fit);
        self.refresh_preview();
        cx.notify();
    }
}

impl Render for WebAppInstall {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let (w, _) = viewport(window);
        let c = self.card;
        let left = ((c.x + c.width) as f32 - WIDTH - 12.0).clamp(8.0, (w - WIDTH - 8.0).max(8.0));
        let top = c.y as f32 + 12.0;
        let me = cx.entity();
        let (m_pick, m_pick2, m_reset, m_fit, m_ok) = (me.clone(), me.clone(), me.clone(), me.clone(), me.clone());
        let has_img = self.image().is_some();
        let name_ok = !self.name.read(cx).value().trim().is_empty() || !self.fallback_name.is_empty();
        let host = url::Url::parse(&self.url).ok().and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string())).unwrap_or_else(|| host_of(&self.url));
        let selected = FITS.iter().position(|f| *f == self.fit).unwrap_or(1);
        panel(cx)
            .id("webapp-install")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(WIDTH))
            .p(px(6.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .pt(space::S2)
                    .px(space::S2)
                    .pb(space::S3)
                    .child(div().font_weight(weight::STRONG).child(tr("webapp.installTitle")))
                    .child(div().text_size(t.type_scale.sub).text_color(t.text_2).child(host)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S3)
                    .pt(space::S1)
                    .px(space::S2)
                    .pb(space::S2)
                    .child(
                        div()
                            .id("webapp-pick")
                            .relative()
                            .flex_none()
                            .size(px(64.0))
                            .cursor_pointer()
                            .on_click(move |_, window, cx| m_pick.update(cx, |this, cx| this.choose_image(window, cx)))
                            .when_some(self.preview.clone(), |d, p| d.child(img(p).size(px(64.0)).object_fit(ObjectFit::Contain)))
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
                                    .bg(t.accent)
                                    .text_color(t.on_accent)
                                    .child(lucide("image-plus").size(px(12.0))),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(space::S1)
                            .child(div().text_size(t.type_scale.sub).text_color(t.text_2).child(tr("webapp.name")))
                            .child(TextField::new(&self.name)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .px(space::S2)
                    .pb(space::S3)
                    .when(has_img, |d| {
                        d.child(Segmented::new("webapp-fit").item(tr(fit_key(Fit::Fill))).item(tr(fit_key(Fit::Fit))).item(tr(fit_key(Fit::Raw))).selected(selected).on_change(move |ix, _, cx| {
                            m_fit.update(cx, |this, cx| {
                                this.fit = FITS[ix.min(2)];
                                this.refresh_preview();
                                cx.notify();
                            })
                        }))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(space::S2)
                            .child(small_btn("webapp-choose", cx).child(tr("webapp.chooseImage")).on_click(move |_, window, cx| m_pick2.update(cx, |this, cx| this.choose_image(window, cx))))
                            .when(self.custom.is_some(), |d| {
                                d.child(
                                    small_btn("webapp-reset", cx)
                                        .child(if self.site.is_some() { tr("webapp.siteIcon") } else { tr("webapp.noImage") })
                                        .on_click(move |_, _, cx| m_reset.update(cx, |this, cx| this.reset_to_site(cx))),
                                )
                            })
                            .when(self.bad, |d| d.child(div().text_size(t.type_scale.sub).text_color(t.danger).child(tr("webapp.badImage")))),
                    ),
            )
            .child(div().px(space::S2).pb(space::S3).text_size(t.type_scale.sub).line_height(px(17.0)).text_color(t.text_2).child(tr("webapp.installHint")))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .pt(space::S2)
                    .px(space::S1)
                    .pb(px(2.0))
                    .border_t_1()
                    .border_color(t.border)
                    .child(div().flex_1())
                    .child(small_btn("webapp-cancel", cx).child(tr("common.cancel")).on_click(|_, _, cx| api(cx).close_overlay(false)))
                    .child({
                        let off = self.busy || !name_ok;
                        small_btn("webapp-install-ok", cx)
                            .bg(t.accent)
                            .text_color(t.on_accent)
                            .rounded(radius::CONTROL)
                            .child(tr("webapp.install"))
                            .when(off, |d| d.opacity(0.5))
                            .when(!off, |d| d.on_click(move |_, _, cx| m_ok.update(cx, |this, cx| this.install(cx))))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, a: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([200, 30, 30, a]))
    }

    #[test]
    fn opaque_square_fills_the_plate() {
        assert!(opaque_corners(&solid(200, 200, 255)));
        assert_eq!(auto_fit(&solid(200, 200, 255), false), Fit::Fill);
        // Small or transparent icons sit on a white plate; a maskable icon always fills.
        assert_eq!(auto_fit(&solid(64, 64, 255), false), Fit::Fit);
        assert_eq!(auto_fit(&solid(200, 200, 0), false), Fit::Fit);
        assert_eq!(auto_fit(&solid(64, 64, 0), true), Fit::Fill);
    }

    #[test]
    fn composed_icon_has_the_plate_and_transparent_corners() {
        let icon = compose(Some(&solid(300, 300, 255)), Fit::Fill, 250.0, 128);
        assert_eq!((icon.width(), icon.height()), (128, 128));
        assert_eq!(icon.get_pixel(64, 64)[3], 255);
        assert_eq!(icon.get_pixel(0, 0)[3], 0);
        // The plate is red in the middle, the corner of the plate (inside the rounding) stays outside.
        assert_eq!(icon.get_pixel(64, 64)[0], 200);
    }

    #[test]
    fn raw_keeps_the_image_over_the_whole_square() {
        let icon = compose(Some(&solid(100, 100, 255)), Fit::Raw, 0.0, 64);
        assert_eq!(icon.get_pixel(1, 1)[3], 255);
    }

    #[test]
    fn without_image_the_plate_is_tinted() {
        let icon = compose(None, Fit::Fit, 120.0, 64);
        let p = icon.get_pixel(32, 32);
        assert_eq!(p[3], 255);
        assert!(p[1] > p[0] && p[1] > p[2]);
    }

    #[test]
    fn data_url_roundtrip() {
        let url = png_data_url(&solid(8, 8, 255));
        let back = decode_data_url(&url).unwrap();
        assert_eq!((back.width(), back.height()), (8, 8));
    }
}

use super::favicon::{Favicon, FaviconSource};
use crate::icon::IconName;
use crate::motion::{reduced_motion, surface_enter};
use crate::theme::{ThemeExt, radius, space, weight};
use gpui::{App, ClickEvent, ElementId, ImageSource, InteractiveElement, IntoElement, ObjectFit, ParentElement, RenderOnce, SharedString, StatefulInteractiveElement, Styled, StyledImage, Window, div, img, prelude::FluentBuilder, px};
use super::icon_button::IconButton;
use std::rc::Rc;

/// What the media card shows (port of `MediaInfo`).
#[derive(Clone)]
pub struct MediaInfo {
    pub title: SharedString,
    pub artist: Option<SharedString>,
    pub site: SharedString,
    pub artwork: Option<ImageSource>,
    pub favicon: Option<FaviconSource>,
    pub playing: bool,
    pub muted: bool,
    pub can_skip: bool,
}

/// Translated labels (tooltips and accessible name).
#[derive(Clone)]
pub struct MediaLabels {
    pub previous: SharedString,
    pub next: SharedString,
    pub play: SharedString,
    pub pause: SharedString,
    pub mute: SharedString,
    pub unmute: SharedString,
}

type Cb = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(Default)]
struct Callbacks {
    activate: Option<Cb>,
    toggle: Option<Cb>,
    previous: Option<Cb>,
    next: Option<Cb>,
    mute: Option<Cb>,
}

macro_rules! callbacks {
    ($ty:ty) => {
        impl $ty {
            /// Click on the card (activates the playing tab).
            pub fn on_activate(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
                self.cb.activate = Some(Rc::new(f));
                self
            }
            pub fn on_toggle(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
                self.cb.toggle = Some(Rc::new(f));
                self
            }
            pub fn closing(mut self, c: bool) -> Self {
                self.closing = c;
                self
            }
        }
    };
}

fn art(info: &MediaInfo, size: f32, fav: f32) -> gpui::AnyElement {
    match info.artwork.clone() {
        Some(src) => img(src).size(px(size)).object_fit(ObjectFit::Cover).into_any_element(),
        None => Favicon::new(info.favicon.clone()).size(fav).into_any_element(),
    }
}

/// Sidebar media card: the tab playing elsewhere with its controls. Presentational. Wrap it in
/// [`crate::motion::Presence`] and call `.closing(true)` for the fade out.
#[derive(IntoElement)]
pub struct MediaCard {
    id: ElementId,
    info: MediaInfo,
    labels: MediaLabels,
    cb: Callbacks,
    closing: bool,
}

impl MediaCard {
    pub fn new(id: impl Into<ElementId>, info: MediaInfo, labels: MediaLabels) -> Self {
        Self { id: id.into(), info, labels, cb: Callbacks::default(), closing: false }
    }
    pub fn on_previous(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.cb.previous = Some(Rc::new(f));
        self
    }
    pub fn on_next(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.cb.next = Some(Rc::new(f));
        self
    }
    pub fn on_mute(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.cb.mute = Some(Rc::new(f));
        self
    }
}
callbacks!(MediaCard);

fn wire(b: IconButton, cb: &Option<Cb>) -> IconButton {
    match cb.clone() {
        Some(f) => b.on_click(move |e, w, cx| f(e, w, cx)),
        None => b,
    }
}

impl RenderOnce for MediaCard {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let reduced = reduced_motion(cx);
        let (info, l, cb) = (self.info, self.labels, self.cb);
        let id = self.id.clone();
        let sub = info.artist.clone().unwrap_or_else(|| info.site.clone());
        let btn = |name: &'static str, icon: IconName, tip: SharedString, size: f32| {
            IconButton::new(ElementId::from((id.clone(), name)), icon).small().dims(20.0, 24.0).icon_size(size).tooltip(tip)
        };
        let activate = cb.activate.clone();
        let card = div()
            .id(self.id.clone())
            .flex()
            .flex_none()
            .items_center()
            .gap(space::S2)
            .p(space::S1)
            .rounded(radius::TILE)
            .bg(t.surface_2)
            // CSS: inset 0 0 0 0.5px (no layout space).
            .child(div().absolute().inset_0().rounded(radius::TILE).border_color(t.border).border(px(0.5)))
            .relative()
            .cursor_pointer()
            .active(move |s| s.opacity(0.92))
            .when_some(activate, |d, f| d.on_click(move |e, w, cx| f(e, w, cx)))
            .child(
                div().flex_none().size(px(32.0)).flex().items_center().justify_center().rounded(radius::ROW_IN).overflow_hidden().bg(t.surface_2).child(art(&info, 32.0, 18.0)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .line_height(gpui::relative(1.25))
                    .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(t.type_scale.sub).font_weight(weight::STRONG).text_color(t.text).child(info.title.clone()))
                    .child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(t.type_scale.cap).text_color(t.text_2).child(sub)),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .when(info.can_skip, |d| d.child(wire(btn("prev", IconName::SkipBack, l.previous.clone(), 13.0), &cb.previous)))
                    .child(wire(
                        btn("toggle", if info.playing { IconName::Pause } else { IconName::Play }, if info.playing { l.pause.clone() } else { l.play.clone() }, 14.0),
                        &cb.toggle,
                    ))
                    .when(info.can_skip, |d| d.child(wire(btn("next", IconName::SkipForward, l.next.clone(), 13.0), &cb.next)))
                    .child(wire(
                        btn("mute", if info.muted { IconName::VolumeX } else { IconName::Volume2 }, if info.muted { l.unmute.clone() } else { l.mute.clone() }, 13.0).active(info.muted),
                        &cb.mute,
                    )),
            );
        // The bottom margin lives on a wrapper: `surface_enter` animates the card's own margins.
        div().flex_none().mb(space::S1).child(surface_enter(card, ElementId::from((self.id, "enter")), self.closing, reduced))
    }
}

/// Compact version for the tab row: cover or favicon and a play / pause button.
#[derive(IntoElement)]
pub struct MediaChip {
    id: ElementId,
    info: MediaInfo,
    labels: MediaLabels,
    cb: Callbacks,
    closing: bool,
}

impl MediaChip {
    pub fn new(id: impl Into<ElementId>, info: MediaInfo, labels: MediaLabels) -> Self {
        Self { id: id.into(), info, labels, cb: Callbacks::default(), closing: false }
    }
}
callbacks!(MediaChip);

impl RenderOnce for MediaChip {
    fn render(self, _w: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.signet().clone();
        let reduced = reduced_motion(cx);
        let (info, l, cb) = (self.info, self.labels, self.cb);
        let activate = cb.activate.clone();
        let chip = div()
            .id(self.id.clone())
            .flex()
            .flex_none()
            .items_center()
            .gap(space::S1)
            .h(px(28.0))
            .pl(space::S1)
            .pr(space::HALF)
            .rounded(radius::CONTROL)
            .bg(t.surface_2)
            .cursor_pointer()
            .when_some(activate, |d, f| d.on_click(move |e, w, cx| f(e, w, cx)))
            .child(div().flex_none().size(px(20.0)).flex().items_center().justify_center().rounded(radius::ICON).overflow_hidden().child(art(&info, 20.0, 16.0)))
            .child(wire(
                IconButton::new(ElementId::from((self.id.clone(), "toggle")), if info.playing { IconName::Pause } else { IconName::Play })
                    .small()
                    .tooltip(if info.playing { l.pause } else { l.play }),
                &cb.toggle,
            ));
        surface_enter(chip, ElementId::from((self.id, "enter")), self.closing, reduced)
    }
}

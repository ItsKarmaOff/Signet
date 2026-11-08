//! Space creation / edition form (port of `settings/SpaceForm.tsx`).
//!
//! `SpaceFormView` is a plain view with no window of its own. Hosts: the overlay (the app opens the form when it
//! receives `UiEvent::EditSpace { space_id }`, from the sidebar's "new space" button and the space menu), or a small
//! window. It does not position itself (the React form was absolutely placed above the sidebar toolbar): the host
//! places it, about 300 px wide.
//!
//! ```ignore
//! let form = cx.new(|cx| SpaceFormView::for_space_id(space_id, window, cx));   // None = new space
//! cx.subscribe(&form, |host, _, _: &SpaceFormClosed, cx| { /* hide the host */ }).detach();
//! ```
//! The view saves through the API (`create_space` / `update_space` / `set_space_profile`) and emits
//! `SpaceFormClosed` when it is saved, cancelled (Cancel, Escape) or when the edited space disappears.

use gpui::AppContext as _;
use gpui::{
    Context, Entity, EventEmitter, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_component::input::InputState;
use gpui_component::slider::{SliderEvent, SliderState};
use signet_core::types::{Space, SpaceDraft, SpaceProfile, SpaceTheme};

use super::kit;
use crate::app_state::{api, snapshot, tr, ui_state};
use crate::color::{hsla, swatch_color};
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::{EmojiPicked, EmojiPicker, EmojiPickerLabels, Segmented, SliderField, Toggle, slider_state};
use signet_core::i18n::current_locale;

/// Space palette (`HUES` of the renderer).
pub const HUES: [i32; 10] = [0, 25, 45, 90, 150, 190, 215, 250, 285, 330];

/// Emitted when the form is done: the host closes it.
pub struct SpaceFormClosed;

/// Hues of a gradient started from the solid one: the color and its neighbor on the palette.
pub fn gradient_start(hue: i32) -> Vec<i32> {
    let ix = HUES.iter().position(|h| *h == hue);
    let next = match ix {
        Some(i) => HUES[(i + 3) % HUES.len()],
        None => HUES[(HUES.len() + 3 - 1) % HUES.len()],
    };
    vec![hue, next]
}

/// Gradient: a click adds the hue (the last one is replaced beyond 3), a second click removes it.
pub fn toggle_hue(hues: &[i32], h: i32) -> Vec<i32> {
    if hues.contains(&h) {
        hues.iter().copied().filter(|x| *x != h).collect()
    } else if hues.len() >= 3 {
        let mut v = hues[..2].to_vec();
        v.push(h);
        v
    } else {
        let mut v = hues.to_vec();
        v.push(h);
        v
    }
}

pub struct SpaceFormView {
    /// Space being edited (None = creation).
    space: Option<Space>,
    /// Space targeted at opening: if it disappears afterwards, the form closes instead of switching to creation.
    edit_id: Option<String>,
    name: Entity<InputState>,
    icon: String,
    hue: i32,
    gradient: bool,
    hues: Vec<i32>,
    grain: f32,
    grain_slider: Entity<SliderState>,
    picking: bool,
    picker: Option<Entity<EmojiPicker>>,
    isolated: bool,
    busy: bool,
}

impl EventEmitter<SpaceFormClosed> for SpaceFormView {}

impl SpaceFormView {
    /// `space`: the space to edit, or None to create one.
    pub fn new(space: Option<Space>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        kit::watch_locale(cx);
        let edit_id = space.as_ref().map(|s| s.id.clone());
        // The edited space disappeared: close the form.
        cx.observe(&ui_state(cx), |this, _, cx| {
            if let Some(id) = &this.edit_id
                && !snapshot(cx).spaces.iter().any(|s| &s.id == id)
            {
                cx.emit(SpaceFormClosed);
            } else {
                cx.notify();
            }
        })
        .detach();
        let name = kit::input(window, cx, tr("spaceForm.namePlaceholder"), space.as_ref().map(|s| s.name.clone()).unwrap_or_default(), false, Some(Self::submit));
        name.update(cx, |s, cx| s.focus(window, cx));
        let seed = signet_core::ids::now_ms().unsigned_abs() as usize;
        let hue = space.as_ref().map(|s| s.hue.round() as i32).unwrap_or(HUES[seed / 7 % HUES.len()]);
        let theme = space.as_ref().and_then(|s| s.theme.clone());
        let grain = theme.as_ref().map(|t| t.grain as f32).unwrap_or(0.0);
        let grain_slider = slider_state(cx, 0.0, 100.0, 5.0, (grain * 100.0).round());
        cx.subscribe(&grain_slider, |this, _, ev: &SliderEvent, cx| {
            if let SliderEvent::Change(v) = ev {
                if let gpui_component::slider::SliderValue::Single(v) = v {
                    this.grain = *v / 100.0;
                }
                cx.notify();
            }
        })
        .detach();
        let mut this = Self {
            edit_id,
            icon: space.as_ref().map(|s| s.icon.clone()).filter(|i| !i.is_empty()).unwrap_or_else(|| "\u{2728}".into()),
            hue,
            gradient: theme.is_some(),
            hues: theme.map(|t| t.hues.iter().map(|h| h.round() as i32).collect()).unwrap_or_default(),
            grain,
            grain_slider,
            picking: false,
            picker: None,
            isolated: space.as_ref().is_some_and(|s| s.profile == Some(SpaceProfile::Isolated)),
            busy: false,
            name,
            space,
        };
        this.apply_capture_mode(window, cx);
        this
    }

    /// Capture harness only: `SIGNET_SPACE_FORM=gradient|emoji` opens the form on that state.
    fn apply_capture_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match std::env::var("SIGNET_SPACE_FORM").as_deref() {
            Ok("gradient") => {
                self.gradient = true;
                self.hues = gradient_start(self.hue);
            }
            Ok("emoji") => self.toggle_picker(window, cx),
            _ => {}
        }
    }

    /// Opens the form for a space id (`UiEvent::EditSpace`): None = new space. An unknown id also gives a new space.
    pub fn for_space_id(space_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let space = space_id.and_then(|id| snapshot(cx).spaces.iter().find(|s| s.id == id).cloned());
        Self::new(space, window, cx)
    }

    fn valid(&self, cx: &gpui::App) -> bool {
        !kit::text_of(&self.name, cx).trim().is_empty() && (!self.gradient || self.hues.len() >= 2)
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(SpaceFormClosed);
    }

    fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.valid(cx) || self.busy {
            return;
        }
        self.busy = true;
        self.save(cx);
        self.busy = false;
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        // The edited space was deleted in the meantime: nothing to save.
        if self.edit_id.is_some() && !self.edit_id.as_ref().is_some_and(|id| snapshot(cx).spaces.iter().any(|s| &s.id == id)) {
            return self.close(cx);
        }
        let name: String = kit::text_of(&self.name, cx).trim().chars().take(40).collect();
        let icon = if self.icon.is_empty() { "\u{2728}".to_string() } else { self.icon.clone() };
        let draft = if self.gradient {
            SpaceDraft {
                name,
                icon,
                hue: self.hues.first().copied().unwrap_or(self.hue) as f64,
                theme: Some(SpaceTheme { hues: self.hues.iter().map(|h| *h as f64).collect(), grain: self.grain as f64 }),
            }
        } else {
            SpaceDraft { name, icon, hue: self.hue as f64, theme: None }
        };
        let handle = api(cx);
        match &self.space {
            Some(space) => {
                handle.update_space(&space.id, draft);
                if self.isolated != (space.profile == Some(SpaceProfile::Isolated)) {
                    handle.set_space_profile(&space.id, if self.isolated { SpaceProfile::Isolated } else { SpaceProfile::Shared });
                }
            }
            None => {
                handle.create_space(draft);
                // The new space becomes the active space.
                if self.isolated {
                    handle.set_space_profile(&handle.get_state().active_space_id, SpaceProfile::Isolated);
                }
            }
        }
        self.close(cx);
    }

    fn toggle_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.picking = !self.picking;
        if self.picking && self.picker.is_none() {
            let labels = EmojiPickerLabels { search: tr("emoji.search").into(), recent: tr("emoji.recent").into(), no_results: tr("emoji.noResults").into() };
            let locale = current_locale().id();
            let picker = cx.new(|cx| EmojiPicker::new(window, cx, locale, labels, Vec::new()).columns(6));
            cx.subscribe(&picker, |this, _, ev: &EmojiPicked, cx| {
                this.icon = ev.0.clone();
                this.picking = false;
                cx.notify();
            })
            .detach();
            self.picker = Some(picker);
        }
        cx.notify();
    }
}

/// Preview of a space background: the gradient of its hues, or its solid color.
fn preview(hues: &[i32], hue: i32, gradient: bool) -> gpui::AnyElement {
    let color = |h: i32| hsla(swatch_color(h as f32));
    if !gradient || hues.len() < 2 {
        return div().size_full().bg(color(hues.first().copied().filter(|_| gradient).unwrap_or(hue))).into_any_element();
    }
    let stop = |h: i32, at: f32| gpui::linear_color_stop(color(h), at);
    let segments = hues.windows(2).map(|w| {
        div().flex_1().h_full().bg(gpui::linear_gradient(90.0, stop(w[0], 0.0), stop(w[1], 1.0)))
    });
    div().flex().size_full().children(segments).into_any_element()
}

impl Render for SpaceFormView {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let gradient = self.gradient;
        let valid = self.valid(cx);

        let bs = |spread: f32, color: gpui::Hsla| gpui::BoxShadow { color, offset: gpui::point(px(0.0), px(0.0)), blur_radius: px(0.0), spread_radius: px(spread), inset: false };
        let ring = vec![bs(3.5, t.text_2), bs(2.0, t.surface)];
        let swatches = HUES.iter().enumerate().map(|(i, h)| {
            let h = *h;
            let rank = self.hues.iter().position(|x| *x == h);
            let on = if gradient { rank.is_some() } else { h == self.hue };
            div()
                .id(("swatch", i))
                .relative()
                .flex()
                .w_full()
                .items_center()
                .justify_center()
                .aspect_ratio(1.0)
                .rounded_full()
                .bg(hsla(swatch_color(h as f32)))
                .cursor_pointer()
                .when(on, |d| d.shadow(ring.clone()))
                .hover(|s| s.opacity(0.85))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.gradient {
                        this.hues = toggle_hue(&this.hues, h);
                    } else {
                        this.hue = h;
                    }
                    cx.notify();
                }))
                .when_some(rank.filter(|_| gradient), |d, r| d.child(div().text_size(px(10.0)).font_weight(weight::STRONG).text_color(gpui::white()).child((r + 1).to_string())))
        });

        kit::form_surface(cx)
            .bg(t.glass_view)
            .id("space-form")
            .w_full()
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key != "escape" {
                    return;
                }
                if this.picking {
                    this.picking = false;
                    cx.notify();
                } else {
                    this.close(cx);
                }
                let _ = window;
            }))
            .child(
                div()
                    .flex()
                    .gap(space::S2)
                    .child(
                        div()
                            .id("space-emoji")
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(space::S8)
                            .rounded(radius::CONTROL)
                            .bg(t.surface_2)
                            .text_size(px(17.0))
                            .font_family(crate::theme::emoji_font())
                            .cursor_pointer()
                            .border_2().border_color(if self.picking { t.accent } else { gpui::transparent_black() })
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_picker(window, cx)))
                            .child(self.icon.clone()),
                    )
                    .child(div().flex_1().min_w_0().child(crate::screens::overlay::forms::FormInput::new(&self.name))),
            )
            .when_some(self.picker.clone().filter(|_| self.picking), |d, p| d.child(p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .child(div().flex_1().child({
                        let this = cx.entity();
                        Segmented::new("space-bg")
                            .item(tr("spaceForm.solid"))
                            .item(tr("spaceForm.gradient"))
                            .selected(if gradient { 1 } else { 0 })
                            .on_change(move |ix, _, cx| {
                                this.update(cx, |t, cx| {
                                    if ix == 1 {
                                        t.gradient = true;
                                        if t.hues.len() < 2 {
                                            t.hues = gradient_start(t.hue);
                                        }
                                    } else {
                                        t.gradient = false;
                                    }
                                    cx.notify();
                                })
                            })
                    }))
                    .child(div().flex_none().w(px(44.0)).h(px(24.0)).rounded(radius::CONTROL).overflow_hidden().border(px(0.5)).border_color(t.ctl_ring).child(preview(&self.hues, self.hue, gradient))),
            )
            .child(div().grid().grid_cols(10).gap(space::S1).children(swatches))
            .when(gradient, |d| {
                d.when(self.hues.len() < 2, |d| d.child(kit::hint_text(cx, tr("spaceForm.gradientHint"))))
                    .child(SliderField::new(&self.grain_slider).label(tr("spaceForm.grain")))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S3)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(space::HALF)
                            .child(div().font_weight(weight::STRONG).child(tr("spaceForm.isolated")))
                            .child(div().text_size(t.type_scale.cap).text_color(t.text_2).child(tr("spaceForm.isolatedHint"))),
                    )
                    .child(Toggle::new("space-isolated", self.isolated).on_change({
                        let this = cx.entity();
                        move |on, _, cx| {
                            this.update(cx, |t, cx| {
                                t.isolated = on;
                                cx.notify();
                            })
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space::S2)
                    .child(crate::screens::overlay::forms::primary_btn("space-cancel", tr("common.cancel"), false, false, cx).on_click(cx.listener(|this, _, _, cx| this.close(cx))))
                    .child({
                        let ok = valid && !self.busy;
                        let b = crate::screens::overlay::forms::primary_btn("space-submit", if self.space.is_some() { tr("spaceForm.save") } else { tr("spaceForm.create") }, true, !ok, cx);
                        b.when(ok, |b| b.on_click(cx.listener(|this, _, w, cx| this.submit(w, cx))))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_starts_with_the_neighbor_three_steps_away() {
        assert_eq!(gradient_start(0), vec![0, 90]);
        assert_eq!(gradient_start(285), vec![285, 25]);
        // Unknown hue: indexOf -1 gives HUES[2].
        assert_eq!(gradient_start(7), vec![7, 45]);
    }

    #[test]
    fn hues_toggle_and_cap_at_three() {
        assert_eq!(toggle_hue(&[0, 90], 150), vec![0, 90, 150]);
        assert_eq!(toggle_hue(&[0, 90, 150], 250), vec![0, 90, 250]);
        assert_eq!(toggle_hue(&[0, 90, 150], 90), vec![0, 150]);
        assert_eq!(toggle_hue(&[], 0), vec![0]);
    }
}

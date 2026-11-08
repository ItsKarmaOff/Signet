//! Find in page (port of `overlay/Find.tsx`, route `#find`): a pill at the top right of the page.

use super::common::{accent_for, lucide};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::motion::{ease_out, reduced_motion};
use crate::theme::duration::FAST;
use crate::theme::{ThemeExt, radius, space};
use crate::widgets::IconButton;
use crate::IconName;
use gpui::{Animation, AnimationExt, AppContext, Context, ElementId, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, px};
use gpui_component::input::{Escape, Input, InputEvent, InputState};
use signet_core::api::{FindOptions, UiEvent};
use signet_core::types::FindState;
use std::time::{Duration, Instant};

/// Root view of the `#find` window, exactly 360 x 48 px, placed at the top right of the page.
pub struct FindBar {
    st: Option<FindState>,
    query: String,
    input: Entity<InputState>,
    typed_at: Option<Instant>,
    _subs: Vec<Subscription>,
}

impl FindBar {
    pub fn new(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let state = ui_state(cx);
        cx.new(|cx: &mut Context<Self>| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("find.placeholder")));
            let subs = vec![
                cx.subscribe_in(&state, window, |this: &mut Self, _, ev: &UiEvent, window, cx| {
                    if let UiEvent::FindState(n) = ev {
                        this.on_state(n.clone(), window, cx);
                    }
                }),
                cx.subscribe_in(&input, window, |this: &mut Self, input, ev: &InputEvent, _window, cx| match ev {
                    InputEvent::Change => {
                        let value = input.read(cx).value().to_string();
                        if value != this.query {
                            this.typed_at = Some(Instant::now());
                            this.query = value.clone();
                            api(cx).find(&value, FindOptions { forward: true, find_next: false });
                            cx.notify();
                        }
                    }
                    InputEvent::PressEnter { shift, .. } => {
                        if !this.query.is_empty() {
                            api(cx).find(&this.query, FindOptions { forward: !*shift, find_next: true });
                        }
                    }
                    _ => {}
                }),
            ];
            api(cx).find_ready();
            Self { st: None, query: String::new(), input, typed_at: None, _subs: subs }
        })
    }

    fn on_state(&mut self, n: FindState, window: &mut Window, cx: &mut Context<Self>) {
        let fresh = self.st.as_ref().is_none_or(|p| p.session != n.session);
        // Cmd+F on an already open bar returns exactly the same state: take the focus back (except mid-typing, where the
        // echo of the app must not select the text).
        let resend = self.st.as_ref().is_some_and(|p| p.session == n.session && p.query == n.query && p.active == n.active && p.matches == n.matches);
        let idle = self.typed_at.is_none_or(|t| t.elapsed() > Duration::from_millis(400));
        if fresh {
            self.query = n.query.clone();
            self.input.update(cx, |i, cx| i.set_value(n.query.clone(), window, cx));
        }
        if fresh || (resend && idle) {
            self.input.update(cx, |i, cx| {
                i.focus(window, cx);
                i.select_all(window, cx);
            });
        }
        self.st = Some(n);
        cx.notify();
    }
}

impl Render for FindBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let Some(st) = self.st.clone() else { return div().size_full().into_any_element() };
        let closing = st.closing.unwrap_or(false);
        let has = !self.query.is_empty();
        let none = has && st.matches == 0;
        let count = if !has {
            String::new()
        } else if none {
            tr("find.noResults")
        } else {
            tr_args("find.count", &[("active", &st.active.max(1).to_string()), ("total", &st.matches.to_string())])
        };
        let query = self.query.clone();
        let (q1, q2) = (query.clone(), query);
        let reduced = reduced_motion(cx);
        let accent = accent_for(cx, st.hue);
        let _ = accent;
        div()
            .id("find-pill")
            .size_full()
            .flex()
            .items_center()
            .gap(space::S1)
            .pl(space::S3)
            .pr(px(6.0))
            .rounded(radius::POP)
            .bg(t.glass_view)
            .border(px(0.5))
            .border_color(t.border)
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .on_action(cx.listener(|_, _: &Escape, _, cx| api(cx).close_find()))
            .child(div().flex_none().mr(space::S1).child(lucide("search").size(px(15.0)).text_color(t.text_2)))
            .child(div().flex_1().min_w_0().h_full().flex().items_center().child(Input::new(&self.input).appearance(false).bordered(false).focus_bordered(false).w_full().text_size(t.type_scale.md)))
            .child(div().flex_none().px(space::S1).text_size(t.type_scale.sub).text_color(if none { t.danger } else { t.text_2 }).whitespace_nowrap().child(count))
            .child(IconButton::new("find-prev", IconName::ChevronUp).small().icon_size(16.0).tooltip(tr("find.previous")).disabled(!has || none).on_click(move |_, _, cx| {
                api(cx).find(&q1, FindOptions { forward: false, find_next: true })
            }))
            .child(IconButton::new("find-next", IconName::ChevronDown).small().icon_size(16.0).tooltip(tr("find.next")).disabled(!has || none).on_click(move |_, _, cx| {
                api(cx).find(&q2, FindOptions { forward: true, find_next: true })
            }))
            .child(IconButton::new("find-close", IconName::X).small().icon_size(15.0).tooltip(tr("common.close")).on_click(|_, _, cx| api(cx).close_find()))
            .with_animation(
                ElementId::from(("find-anim", st.session * 2 + u64::from(closing))),
                Animation::new(FAST).with_easing(ease_out),
                move |el, d| {
                    let p = if closing { 1.0 - d } else { d };
                    el.opacity(p).relative().top(px(if reduced { 0.0 } else { (1.0 - p) * -4.0 }))
                },
            )
            .into_any_element()
    }
}

//! Settings > Routing (port of `RoutingSettings.tsx`): links of certain sites open in a given space.

use gpui::{Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::input::InputState;

use super::kit::{self, Btn};
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{api, snapshot, tr, tr_args};
use crate::icon::IconName;
use crate::theme::{FONT_MONO, ThemeExt, emoji_font, space};

pub struct RoutingSettings {
    store: Entity<InfoStore>,
    pattern: Entity<InputState>,
    target: Option<String>,
}

impl RoutingSettings {
    pub fn new(store: Entity<InfoStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        // The list of spaces lives in the shared snapshot: re-render when it changes.
        cx.observe(&crate::app_state::ui_state(cx), |_, _, cx| cx.notify()).detach();
        let pattern = kit::input(window, cx, tr("settings.routing.placeholder"), "", false, Some(Self::add));
        Self { store, pattern, target: None }
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let spaces = snapshot(cx).spaces.clone();
        let chosen = self.target.as_ref().and_then(|id| spaces.iter().find(|s| &s.id == id)).or_else(|| spaces.first()).map(|s| s.id.clone());
        let pattern = kit::text_of(&self.pattern, cx).trim().to_string();
        let Some(space) = chosen else { return };
        if pattern.is_empty() {
            return;
        }
        api(cx).add_route(&pattern, &space);
        kit::set_text(&self.pattern, "", window, cx);
    }
}

impl Render for RoutingSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let info = info_of(&self.store, cx);
        let snap = snapshot(cx);
        let spaces = &snap.spaces;
        let chosen = self.target.as_ref().and_then(|id| spaces.iter().find(|s| &s.id == id)).or_else(|| spaces.first()).map(|s| s.id.clone()).unwrap_or_default();
        let valid = !kit::text_of(&self.pattern, cx).trim().is_empty() && !chosen.is_empty();
        let routes = &info.settings.routes;

        let list = if routes.is_empty() {
            kit::empty(cx, IconName::Signpost, vec![kit::para(tr("settings.routing.empty"))]).into_any_element()
        } else {
            let rows = routes
                .iter()
                .enumerate()
                .map(|(i, route)| {
                    let space = spaces.iter().find(|s| s.id == route.space_id);
                    let id = route.id.clone();
                    kit::row(cx)
                        .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family(FONT_MONO).text_size(t.type_scale.sub).child(route.pattern.clone()))
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(space::S2)
                                .text_color(t.text_2)
                                .text_size(t.type_scale.sub)
                                .child("\u{2192}")
                                .child(match space {
                                    Some(s) => div()
                                        .flex()
                                        .items_center()
                                        .gap(space::S1)
                                        .child(div().text_size(gpui::px(11.5)).font_family(emoji_font()).child(s.icon.clone()))
                                        .child(s.name.clone())
                                        .into_any_element(),
                                    None => kit::hint_text(cx, tr("settings.routing.deletedSpace")).into_any_element(),
                                }),
                        )
                        .child(kit::icon_btn(("route-rm", i), IconName::Trash2, tr("settings.routing.deleteRule"), move |_, _, cx| api(cx).remove_route(&id)))
                        .into_any_element()
                })
                .collect();
            kit::group(cx, rows).into_any_element()
        };

        let options = spaces
            .iter()
            .map(|s| (s.id.clone(), format!("{} {}", s.icon, s.name)))
            .collect::<Vec<_>>();
        let this = cx.entity();
        let form = kit::group(
            cx,
            vec![kit::row(cx)
                .child(kit::field(&self.pattern))
                .child(kit::choice("route-space", &chosen, options, move |v, _, cx| {
                    let v = v.to_string();
                    this.update(cx, |t, cx| {
                        t.target = Some(v);
                        cx.notify();
                    })
                }))
                .child(Btn::new("route-add", tr("settings.routing.add")).disabled(!valid).on_click(cx.listener(|this, _, w, cx| this.add(w, cx))))
                .into_any_element()],
        );
        let _ = tr_args;
        kit::body().child(kit::note_rich(cx, &tr("settings.routing.note"))).child(list).child(form)
    }
}

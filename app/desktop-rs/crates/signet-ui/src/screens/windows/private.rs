//! Port of `windows/Private.tsx` + `styles/private.css`: route `#private`, the chrome of a private browsing window.
//!
//! Layout (`main/windows/private.ts`): `PAD = 6`, tabs on top (`sidebar_width` None): tab row 40 px high, card at
//! `x = 6, y = 44, w = W - 12, h = H - 50`; with a sidebar of width `side`: card at `x = side, y = 6, w = W - side - 6, h = H - 12`.
//! The address bar is the top 40 px of the card. The displayed tab's native web view goes under it:
//! `card.x, card.y + 40, card.w, card.h - 40`. This view leaves that rectangle empty (it only draws the empty-tab
//! explanation there, when `PrivateState.url` is empty and no web view is shown).

use super::common::{BarColors, domain_of, drag_area, favicon, pane_card, pill};
use crate::app_state::{api, tr, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::IconButton;
use gpui::{
    AppContext, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, MouseButton, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, hsla, prelude::FluentBuilder,
    px,
};
use gpui_component::{
    Icon,
    input::{Input, InputEvent, InputState, SelectAll},
};
use signet_core::api::UiEvent;
use signet_core::types::{PrivateState, PrivateTab};

const PAD: f32 = 6.0;
const TOP: f32 = 44.0;
const BAR: f32 = 40.0;

pub struct Private {
    st: Option<PrivateState>,
    editing: bool,
    input: Entity<InputState>,
    /// `(active tab, empty)` of the last state: edit mode is re-evaluated when it changes.
    last_key: Option<(Option<String>, bool)>,
    focus: FocusHandle,
}

/// Title shown for a tab (`Private.tsx`).
pub fn tab_title(tab: &PrivateTab, new_tab: &str) -> String {
    if !tab.title.is_empty() {
        tab.title.clone()
    } else if !tab.url.is_empty() {
        domain_of(&tab.url)
    } else {
        new_tab.to_string()
    }
}

/// Page title shown after the domain: empty when it only repeats the address.
pub fn page_title(active: Option<&PrivateTab>, url: &str) -> String {
    match active {
        Some(t) if !t.title.is_empty() && t.title != url => t.title.clone(),
        _ => String::new(),
    }
}

impl Private {
    pub fn new(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("toolbar.addressPlaceholder")));
            cx.subscribe_in(&ui_state(cx), window, |this, _, ev: &UiEvent, window, cx| match ev {
                UiEvent::PrivateState(s) => this.apply(s.clone(), window, cx),
                UiEvent::PrivateFocusAddress => this.edit(window, cx),
                _ => {}
            })
            .detach();
            cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    let draft = input.read(cx).value().to_string();
                    if !draft.trim().is_empty() {
                        this.editing = false;
                        api(cx).private_navigate(&draft);
                        cx.notify();
                    }
                }
                InputEvent::Focus => window.dispatch_action(Box::new(SelectAll), cx),
                InputEvent::Blur => {
                    if !this.empty() {
                        this.editing = false;
                        cx.notify();
                    }
                }
                _ => {}
            })
            .detach();
            let mut me = Self { st: None, editing: false, input, last_key: None, focus: cx.focus_handle() };
            // Starting state requested from main; a state pushed in the meantime is more recent (events are queued after this).
            if let Some(s) = api(cx).get_private_state() {
                me.apply(s, window, cx);
            }
            me
        })
    }

    fn url(&self) -> &str {
        self.st.as_ref().map(|s| s.url.as_str()).unwrap_or("")
    }

    fn empty(&self) -> bool {
        self.st.as_ref().is_some_and(|s| s.url.is_empty())
    }

    fn apply(&mut self, st: PrivateState, window: &mut Window, cx: &mut Context<Self>) {
        let key = (st.active_id.clone(), st.url.is_empty());
        self.st = Some(st);
        if self.last_key.as_ref() != Some(&key) {
            self.last_key = Some(key.clone());
            // Empty tab displayed (new window, new tab): the address field takes focus.
            if key.1 {
                self.edit(window, cx);
            } else {
                self.editing = false;
            }
        }
        cx.notify();
    }

    /// Enters edit mode: only entering copies the URL, a navigation in progress does not overwrite typing.
    fn edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.url().to_string();
        let was = self.editing;
        self.editing = true;
        let input = self.input.clone();
        input.update(cx, |i, cx| {
            if !was {
                i.set_value(url, window, cx);
            }
            i.focus(window, cx);
        });
        window.dispatch_action(Box::new(SelectAll), cx);
        cx.notify();
    }

    fn tab_el(&self, tab: &PrivateTab, vertical: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = cx.signet().clone();
        let active = self.st.as_ref().and_then(|s| s.active_id.as_deref()) == Some(tab.id.as_str());
        let a = api(cx);
        let id = tab.id.clone();
        let (a_act, a_mid, a_close) = (a.clone(), a.clone(), a.clone());
        let (id_act, id_mid, id_close) = (id.clone(), id.clone(), id.clone());
        let group: gpui::SharedString = format!("ptab-{}", tab.id).into();
        let title = tab_title(tab, &tr("private.newTab"));
        let close_shown_always = active && !vertical;
        let hover_bg = t.surface_2;
        let hover_fg = t.text;
        let close = div()
            .id(gpui::SharedString::from(format!("pclose-{}", tab.id)))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .size(px(20.0))
            .rounded(radius::CONTROL)
            .text_color(t.text_2)
            .when(!close_shown_always, |d| d.invisible().group_hover(group.clone(), |s| s.visible()))
            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, _, _| a_close.private_close_tab(&id_close))
            .child(Icon::from(IconName::X).size(px(if vertical { 14.0 } else { 12.0 })));
        let mut el = div()
            .id(gpui::SharedString::from(format!("ptab-{}", tab.id)))
            .group(group)
            .relative()
            .flex_none()
            .flex()
            .items_center()
            .gap(space::S2)
            .rounded(radius::ROW)
            .cursor_pointer()
            .text_color(if active { t.text } else { t.text_tab })
            .on_mouse_down(MouseButton::Left, move |_, _, _| a_act.private_activate(&id_act))
            .on_mouse_down(MouseButton::Middle, move |_, _, _| a_mid.private_close_tab(&id_mid))
            .when(active, |d| d.bg(t.active).shadow(t.active_shadow.clone()))
            .when(!active, |d| d.hover(move |s| s.bg(hover_bg).text_color(hover_fg)));
        el = if vertical { el.w_full().h(t.row_h).pl(space::S3).pr(space::S2) } else { el.h(px(30.0)).w(px(220.0)).min_w(px(120.0)).pl(px(10.0)).pr(space::S1) };
        el.child(favicon(tab.favicon.as_deref(), 16.0, tab.loading))
            .children(tab.audible.then(|| Icon::from(IconName::Volume2).size(px(12.0)).text_color(t.text_2)))
            .child(div().flex_1().min_w_0().truncate().child(title))
            .child(close)
            .into_any_element()
    }
}

impl Focusable for Private {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Private {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let colors = BarColors::theme(&t);
        let st = self.st.clone().unwrap_or_default();
        let known = self.st.is_some();
        let side = st.sidebar_width.map(|w| w as f32);
        let vertical = side.is_some();
        let empty = self.empty();
        let a = api(cx);
        let me = cx.entity();

        let badge = pill(&t, t.sel, t.accent).font_weight(weight::LABEL).child(Icon::from(IconName::VenetianMask).size(px(14.0))).child(tr("private.badge"));

        let tabs: Vec<gpui::AnyElement> = st.tabs.iter().map(|tab| self.tab_el(tab, vertical, cx)).collect();
        let a_new = a.clone();
        let hover_bg = t.surface_2;
        let hover_fg = t.text;

        let chrome: gpui::AnyElement = if let Some(side) = side {
            div()
                .absolute()
                .top_0()
                .left_0()
                .bottom_0()
                .w(px(side))
                .flex()
                .flex_col()
                .px(space::GAP)
                .child(drag_area(div().flex_none().h(px(44.0)).mx(-space::GAP).flex().items_center().gap(space::S2).pl(px(78.0 + 6.0)).pr(space::S2).child(div().flex_1()).child(badge)))
                .child(
                    div()
                        .id("plist")
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .gap(space::HALF)
                        .pb(space::GAP)
                        .overflow_y_scroll()
                        .children(tabs)
                        .child(
                            div()
                                .id("pnew")
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(space::S2)
                                .w_full()
                                .h(t.row_h)
                                .pl(space::S3)
                                .pr(space::S2)
                                .rounded(radius::ROW)
                                .text_color(t.text_2)
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
                                .on_click(move |_, _, _| a_new.private_new_tab())
                                .child(Icon::from(IconName::Plus).size(px(16.0)))
                                .child(div().flex_1().min_w_0().truncate().child(tr("private.newTab"))),
                        ),
                )
                .into_any_element()
        } else {
            drag_area(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(40.0))
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .pl(px(78.0))
                    .pr(space::S3)
                    .child(
                        div()
                            .id("ptabs")
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(space::S1)
                            .overflow_x_scroll()
                            .children(tabs)
                            .child(IconButton::new("pplus", IconName::Plus).tooltip(tr("private.newTab")).on_click(move |_, _, _| a_new.private_new_tab())),
                    )
                    .child(badge),
            )
            .into_any_element()
        };

        // Card (address bar + web view area).
        let card = match side {
            Some(side) => pane_card(&t).top(px(PAD)).bottom(px(PAD)).right(px(PAD)).left(px(side)),
            None => pane_card(&t).top(px(TOP)).bottom(px(PAD)).left(px(PAD)).right(px(PAD)),
        };
        let (a_back, a_fwd, a_rel) = (a.clone(), a.clone(), a.clone());
        let url = st.url.clone();
        let domain = if url.is_empty() { String::new() } else { domain_of(&url) };
        let title = page_title(st.tabs.iter().find(|tab| Some(&tab.id) == st.active_id.as_ref()), &url);
        let editing = self.editing;
        let bar_hover = colors.hover;
        let address: gpui::AnyElement = if editing {
            div()
                .id("paddress")
                .flex_1()
                .min_w_0()
                .h_full()
                .rounded(radius::CONTROL)
                .bg(t.surface_2)
                .border_1()
                .border_color(t.accent.opacity(0.55))
                .on_key_down({
                    let me = me.clone();
                    move |ev, window, cx| {
                        if ev.keystroke.key == "escape" {
                            cx.stop_propagation();
                            me.update(cx, |this, cx| {
                                if this.empty() {
                                    let input = this.input.clone();
                                    input.update(cx, |i, cx| i.set_value("", window, cx));
                                } else {
                                    this.editing = false;
                                    cx.notify();
                                }
                            });
                        }
                    }
                })
                .child(Input::new(&self.input).appearance(false).text_color(colors.fg))
                .into_any_element()
        } else {
            let me2 = me.clone();
            div()
                .id("paddress")
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .gap(space::S1)
                .px(space::S2)
                .rounded(radius::CONTROL)
                .cursor_pointer()
                .hover(move |s| s.bg(bar_hover))
                .on_click(move |_, window, cx| me2.update(cx, |this, cx| this.edit(window, cx)))
                .when(!url.is_empty(), |d| {
                    d.child(div().flex_none().max_w(gpui::relative(0.6)).truncate().text_color(colors.fg).child(domain))
                        .children((!title.is_empty()).then(|| div().min_w_0().truncate().text_color(colors.fg2).child(format!("/ {title}"))))
                })
                .when(url.is_empty(), |d| d.child(div().min_w_0().truncate().text_color(colors.fg2).child(tr("toolbar.addressPlaceholder"))))
                .into_any_element()
        };
        let bar = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(BAR))
            .flex()
            .items_center()
            .gap(space::HALF)
            .pl(space::S1)
            .pr(space::S2)
            .text_color(colors.fg)
            .child(IconButton::new("pback", IconName::ArrowLeft).icon_size(17.0).tooltip(tr("common.back")).disabled(!known || !st.can_go_back).on_click(move |_, _, _| a_back.go_back(None)))
            .child(IconButton::new("pfwd", IconName::ArrowRight).icon_size(17.0).tooltip(tr("common.forward")).disabled(!known || !st.can_go_forward).on_click(move |_, _, _| a_fwd.go_forward(None)))
            .child(
                IconButton::new("preload", if st.loading { IconName::X } else { IconName::RotateCw })
                    .icon_size(if st.loading { 16.0 } else { 15.0 })
                    .tooltip(if st.loading { tr("toolbar.stop") } else { tr("toolbar.reload") })
                    .disabled(empty)
                    .on_click(move |_, _, _| a_rel.reload(None)),
            )
            .child(div().flex_1().min_w_0().h(px(28.0)).flex().items_center().child(address));

        let empty_state = empty.then(|| {
            div()
                .absolute()
                .top(px(BAR))
                .left_0()
                .right_0()
                .bottom_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(space::S3)
                .p(space::S8)
                .text_center()
                .text_color(t.text_2)
                .child(Icon::from(IconName::VenetianMask).size(px(40.0)).text_color(t.accent))
                .child(div().text_size(t.type_scale.title).font_weight(weight::TITLE).text_color(t.text).child(tr("private.emptyTitle")))
                .child(div().max_w(px(460.0)).line_height(gpui::relative(1.5)).child(tr("private.emptyText")))
                .child(div().max_w(px(460.0)).line_height(gpui::relative(1.5)).text_size(t.type_scale.sub).text_color(t.text_3).child(tr("private.emptyNote")))
        });

        // Fixed purple tint and stronger background: the private window is recognizable at a glance.
        div()
            .id("private")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(t.bg)
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .child(div().absolute().inset_0().bg(hsla(285.0 / 360.0, 0.55, 0.5, 0.14)))
            .child(chrome)
            .child(card.child(bar).children(empty_state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(title: &str, url: &str) -> PrivateTab {
        PrivateTab { id: "1".into(), title: title.into(), url: url.into(), ..Default::default() }
    }

    #[test]
    fn tab_title_falls_back_to_domain_then_new_tab() {
        assert_eq!(tab_title(&tab("Hello", "https://a.test"), "New tab"), "Hello");
        assert_eq!(tab_title(&tab("", "https://www.a.test/x"), "New tab"), "a.test");
        assert_eq!(tab_title(&tab("", ""), "New tab"), "New tab");
    }

    #[test]
    fn page_title_hides_a_title_that_repeats_the_url() {
        let t = tab("https://a.test", "https://a.test");
        assert_eq!(page_title(Some(&t), "https://a.test"), "");
        let t = tab("Home", "https://a.test");
        assert_eq!(page_title(Some(&t), "https://a.test"), "Home");
        assert_eq!(page_title(None, "x"), "");
    }
}

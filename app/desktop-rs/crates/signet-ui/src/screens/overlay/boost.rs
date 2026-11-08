//! Boost editor (port of `overlay/Boost.tsx`, route `#boost`): editing window for a site's Boost.

use super::common::{accent_for, lucide};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::theme::{FONT_MONO, ThemeExt, radius, space, weight};
use crate::widgets::{InlineConfirm, Toggle};
use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Task, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::input::{InputEvent, Textarea, TextareaState};
use signet_core::api::UiEvent;
use signet_core::types::BoostEditorState;
use std::time::Duration;

struct Preset {
    id: &'static str,
    label: &'static str,
    css: &'static str,
}

const PRESETS: [Preset; 4] = [
    Preset {
        id: "dark",
        label: "boost.preset.dark",
        css: "html { filter: invert(1) hue-rotate(180deg); }\nimg, video, picture, canvas, iframe, svg image { filter: invert(1) hue-rotate(180deg); }",
    },
    Preset { id: "font", label: "boost.preset.font", css: "* { font-family: -apple-system, system-ui, sans-serif !important; }" },
    Preset { id: "width", label: "boost.preset.width", css: "body { max-width: 860px !important; margin-inline: auto !important; }" },
    Preset { id: "still", label: "boost.preset.still", css: "*, *::before, *::after { animation: none !important; transition: none !important; }" },
];

fn open_tag(id: &str) -> String {
    format!("/* signet:{id} */")
}
fn close_tag(id: &str) -> String {
    format!("/* /signet:{id} */")
}

/// True if the CSS holds the preset's block.
pub fn has_block(css: &str, id: &str) -> bool {
    css.contains(&open_tag(id))
}

/// Appends the preset's block, wrapped in its markers.
pub fn with_block(css: &str, id: &str, body: &str) -> String {
    let block = format!("{}\n{}\n{}", open_tag(id), body, close_tag(id));
    if css.trim().is_empty() { format!("{block}\n") } else { format!("{}\n\n{block}\n", css.trim_end()) }
}

/// Removes the preset's block (and the blank lines around it).
pub fn without_block(css: &str, id: &str) -> String {
    let open = open_tag(id);
    let close = close_tag(id);
    let Some(start) = css.find(&open) else { return css.to_string() };
    let end = match css[start..].find(&close) {
        Some(i) => start + i + close.len(),
        None => css.len(),
    };
    let joined = format!("{}\n\n{}", css[..start].trim_end(), css[end..].trim_start());
    let mut out = joined.trim_start().to_string();
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out
}

/// Root view of the `#boost` window.
pub struct BoostWindow {
    st: Option<BoostEditorState>,
    area: Entity<TextareaState>,
    css: String,
    confirming: bool,
    saved: bool,
    /// Pending save (input is never overwritten while it is set).
    save_task: Option<Task<()>>,
    saved_task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl BoostWindow {
    pub fn new(window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let state = ui_state(cx);
        cx.new(|cx: &mut Context<Self>| {
            let area = cx.new(|cx| TextareaState::new(window, cx).placeholder(String::new()));
            let subs = vec![
                cx.subscribe_in(&state, window, |this: &mut Self, _, ev: &UiEvent, window, cx| {
                    if let UiEvent::BoostState(n) = ev {
                        this.on_state((**n).clone(), window, cx);
                    }
                }),
                cx.subscribe_in(&area, window, |this: &mut Self, area, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Change => {
                        let value = area.read(cx).value().to_string();
                        if value != this.css {
                            this.edit(value, false, window, cx);
                        }
                    }
                    InputEvent::Blur => {
                        // Right after saving, the state is still the old one: do not resync from it.
                        if this.save_task.is_some() {
                            this.flush(cx);
                        } else if let Some(st) = this.st.clone() {
                            this.css = st.boost.css.clone();
                            area.update(cx, |a, cx| a.set_value(st.boost.css, window, cx));
                        }
                    }
                    _ => {}
                }),
            ];
            Self { st: None, area, css: String::new(), confirming: false, saved: false, save_task: None, saved_task: None, _subs: subs }
        })
    }

    fn on_state(&mut self, n: BoostEditorState, window: &mut Window, cx: &mut Context<Self>) {
        // Never overwrite the input: neither while the field has focus, nor when a save is pending.
        let typing = gpui::Focusable::focus_handle(self.area.read(cx), cx).is_focused(window) || self.save_task.is_some();
        if !typing {
            self.css = n.boost.css.clone();
            self.area.update(cx, |a, cx| a.set_value(n.boost.css.clone(), window, cx));
        }
        self.st = Some(n);
        cx.notify();
    }

    fn flush(&mut self, cx: &mut Context<Self>) {
        self.save_task = None;
        let Some(site) = self.st.as_ref().map(|s| s.site.clone()) else { return };
        api(cx).save_boost(&site, Some(&self.css), None);
        self.saved = true;
        let ex = cx.background_executor().clone();
        self.saved_task = Some(cx.spawn(async move |this, cx| {
            ex.timer(Duration::from_millis(1200)).await;
            this.update(cx, |this, cx| {
                this.saved = false;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn edit(&mut self, value: String, immediate: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.css = value;
        self.saved = false;
        let ex = cx.background_executor().clone();
        let wait = if immediate { Duration::ZERO } else { Duration::from_millis(400) };
        self.save_task = Some(cx.spawn_in(window, async move |this, cx| {
            ex.timer(wait).await;
            this.update(cx, |this, cx| this.flush(cx)).ok();
        }));
        cx.notify();
    }

    fn toggle_preset(&mut self, p: &Preset, window: &mut Window, cx: &mut Context<Self>) {
        let next = if has_block(&self.css, p.id) { without_block(&self.css, p.id) } else { with_block(&self.css, p.id, p.css) };
        self.area.update(cx, |a, cx| a.set_value(next.clone(), window, cx));
        self.edit(next, true, window, cx);
    }
}

impl Render for BoostWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let Some(st) = self.st.clone() else { return div().size_full().bg(t.win_bg).into_any_element() };
        let accent = accent_for(cx, st.hue);
        let me = cx.entity();
        let boost = st.boost.clone();
        let exists = boost.updated_at > 0;
        let site = st.site.clone();
        let heading = |text: String| div().flex().items_center().justify_between().text_size(t.type_scale.cap).font_weight(weight::STRONG).text_color(t.text_2).child(text.to_uppercase());

        let chips = PRESETS.iter().enumerate().map(|(i, p)| {
            let on = has_block(&self.css, p.id);
            let me = me.clone();
            div()
                .id(("boost-chip", i))
                .flex()
                .items_center()
                .gap(space::S1)
                .h(px(28.0))
                .px(space::S3)
                .rounded(px(14.0))
                .cursor_pointer()
                .border_1()
                .when(on, |d| d.bg(t.sel).text_color(accent).border_color(accent))
                .when(!on, |d| d.bg(t.surface_2).border_color(t.border))
                .on_click(move |_, window, cx| me.update(cx, |this, cx| this.toggle_preset(&PRESETS[i], window, cx)))
                .when(on, |d| d.child(lucide("check").size(px(12.0))))
                .child(tr(p.label))
        });

        let zapped = boost.zapped.clone();
        let (s_toggle, s_zap, s_del) = (site.clone(), site.clone(), site.clone());
        let me_confirm = me.clone();
        let me_cancel = me.clone();
        let me_ask = me.clone();
        let zap_off = !st.has_tab && !st.zapping;

        div()
            .id("boost")
            .size_full()
            .flex()
            .flex_col()
            .bg(t.win_bg)
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(space::S3)
                    .h(px(48.0))
                    .pr(space::S4)
                    .pl(px(if cfg!(target_os = "macos") { 78.0 } else { 16.0 }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_baseline()
                            .gap(space::S2)
                            .child(div().text_size(t.type_scale.md).font_weight(weight::TITLE).child("Boost"))
                            .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(t.text_2).child(st.site.clone())),
                    )
                    .child(Toggle::new("boost-enabled", boost.enabled).on_change(move |on, _, cx| api(cx).save_boost(&s_toggle, None, Some(on)))),
            )
            .child(
                div()
                    .id("boost-body")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .pt(space::S1)
                    .px(space::S4)
                    .pb(space::S3)
                    .overflow_y_scroll()
                    .when(!boost.enabled, |d| d.opacity(0.55))
                    .child(div().flex().flex_col().gap(space::S2).child(heading(tr("boost.presets"))).child(div().flex().flex_wrap().gap(space::S2).children(chips)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h(px(140.0))
                            .gap(space::S2)
                            .child(heading("CSS".into()).child(div().flex().items_center().gap(px(3.0)).font_weight(weight::MEDIUM).opacity(if self.saved { 1.0 } else { 0.0 }).text_color(t.text_2).child(lucide("check").size(px(11.0))).child(tr("boost.saved"))))
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(px(120.0))
                                    .p(space::S3)
                                    .rounded(radius::TILE)
                                    .bg(t.surface)
                                    .border_1()
                                    .border_color(t.border)
                                    .font_family(FONT_MONO)
                                    .text_size(t.type_scale.sub)
                                    .child(Textarea::new(&self.area).appearance(false).bordered(false).h_full()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S2)
                            .child(heading(tr("boost.hidden")))
                            .child({
                                let on = st.zapping;
                                div()
                                    .id("boost-zap")
                                    .flex()
                                    .items_center()
                                    .self_start()
                                    .gap(space::S2)
                                    .h(space::S8)
                                    .px(space::S3)
                                    .rounded(radius::CONTROL)
                                    .when(on, |d| d.bg(accent).text_color(t.on_accent))
                                    .when(!on, |d| d.bg(t.surface_2).border_1().border_color(t.border))
                                    .when(zap_off, |d| d.opacity(0.4))
                                    .when(!zap_off, |d| {
                                        d.cursor_pointer().on_click(move |_, _, cx| if on { api(cx).stop_zap() } else { api(cx).start_zap(&s_zap) })
                                    })
                                    .child(lucide("crosshair").size(px(14.0)))
                                    .child(if on { tr("boost.zapDone") } else { tr("boost.zapStart") })
                            })
                            .when(st.zapping, |d| d.child(div().text_size(t.type_scale.sub).text_color(t.text_2).child(tr("boost.zapHint"))))
                            .when(zapped.is_empty() && !st.zapping, |d| d.child(div().text_size(t.type_scale.sub).text_color(t.text_2).child(tr("boost.noneHidden"))))
                            .when(!zapped.is_empty(), |d| {
                                d.child(div().id("boost-zapped").flex().flex_col().gap(space::HALF).max_h(px(128.0)).overflow_y_scroll().children(zapped.iter().enumerate().map(|(i, sel)| {
                                    let (site, sel_v) = (site.clone(), sel.clone());
                                    div()
                                        .id(("zapped", i))
                                        .group("zapped-row")
                                        .flex()
                                        .items_center()
                                        .gap(space::S2)
                                        .py(space::HALF)
                                        .pl(space::S2)
                                        .pr(space::S1)
                                        .rounded(radius::ROW)
                                        .hover(|s| s.bg(t.surface_2))
                                        .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().font_family(FONT_MONO).text_size(t.type_scale.sub).child(SharedString::from(sel.clone())))
                                        .child(
                                            crate::widgets::IconButton::new(("unzap", i), crate::IconName::X)
                                                .small()
                                                .icon_size(13.0)
                                                .tooltip(tr("boost.unhide"))
                                                .on_click(move |_, _, cx| api(cx).unzap(&site, &sel_v)),
                                        )
                                })))
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .gap(space::S2)
                    .min_h(px(48.0))
                    .px(space::S4)
                    .py(space::S2)
                    .child(if self.confirming {
                        InlineConfirm::new("boost-confirm", tr_args("boost.confirmDelete", &[("site", st.site.as_str())]), tr("boost.delete"), tr("common.cancel"))
                            .on_cancel(move |_, cx| {
                                me_cancel.update(cx, |this, cx| {
                                    this.confirming = false;
                                    cx.notify();
                                })
                            })
                            .on_confirm(move |_, cx| {
                                me_confirm.update(cx, |this, cx| {
                                    this.confirming = false;
                                    this.save_task = None;
                                    this.css.clear();
                                    cx.notify();
                                });
                                api(cx).delete_boost(&s_del);
                            })
                            .into_any_element()
                    } else {
                        let hover = t.surface_2;
                        div()
                            .id("boost-delete")
                            .flex()
                            .items_center()
                            .gap(space::S2)
                            .h(space::S7)
                            .px(space::S3)
                            .rounded(radius::CONTROL)
                            .text_color(t.danger)
                            .when(!exists, |d| d.opacity(0.35))
                            .when(exists, |d| {
                                d.cursor_pointer().hover(move |s| s.bg(hover)).on_click(move |_, _, cx| {
                                    me_ask.update(cx, |this, cx| {
                                        this.confirming = true;
                                        cx.notify();
                                    })
                                })
                            })
                            .child(lucide("trash-2").size(px(13.0)))
                            .child(tr("boost.deleteBoost"))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_roundtrip() {
        let css = with_block("", "dark", "html { x: y; }");
        assert!(has_block(&css, "dark"));
        assert_eq!(css, "/* signet:dark */\nhtml { x: y; }\n/* /signet:dark */\n");
        assert_eq!(without_block(&css, "dark").trim(), "");
    }

    #[test]
    fn keeps_user_css_around_a_block() {
        let css = with_block("a { b: c; }", "font", "* { d: e; }");
        assert!(css.starts_with("a { b: c; }\n\n/* signet:font */"));
        assert_eq!(without_block(&css, "font").trim(), "a { b: c; }");
        let two = with_block(&with_block("a {}", "dark", "x"), "font", "y");
        let back = without_block(&two, "dark");
        assert!(!has_block(&back, "dark") && has_block(&back, "font") && back.starts_with("a {}"));
    }

    #[test]
    fn missing_block_is_a_noop() {
        assert_eq!(without_block("a {}", "dark"), "a {}");
        assert!(!has_block("a {}", "dark"));
    }
}

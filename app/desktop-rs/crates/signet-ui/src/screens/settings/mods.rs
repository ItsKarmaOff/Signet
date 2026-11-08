//! Settings > Mods (port of `ModSettings.tsx`): CSS stylesheets applied to the whole Signet interface (never to
//! web pages). Saving applies right away, in all windows, Settings included.

use gpui::{AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div, prelude::FluentBuilder, px};
use gpui_component::input::{InputState, Textarea, TextareaState};
use signet_core::api::UiEvent;
use signet_core::types::{InterfaceMod, ModsInfo};

use super::kit::{self, Btn};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::icon::IconName;
use crate::theme::{FONT_MONO, ThemeExt, radius, space};
use crate::widgets::InlineConfirm;

/// Editor draft (`id` None = new mod).
struct Draft {
    id: Option<String>,
    name: Entity<InputState>,
    css: Entity<TextareaState>,
}

/// Non-blank lines of a stylesheet.
pub fn line_count(css: &str) -> usize {
    css.split('\n').filter(|l| !l.trim().is_empty()).count()
}

pub struct ModSettings {
    data: ModsInfo,
    draft: Option<Draft>,
    confirming: Option<String>,
}

impl ModSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        kit::watch_locale(cx);
        cx.subscribe(&ui_state(cx), |this, _, ev: &UiEvent, cx| {
            if matches!(ev, UiEvent::ModsChanged) {
                this.load(cx);
            }
        })
        .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.load(cx);
            }
        })
        .detach();
        Self { data: api(cx).list_mods(), draft: None, confirming: None }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.data = api(cx).list_mods();
        cx.notify();
    }

    fn open(&mut self, mod_: Option<&InterfaceMod>, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(tr("settings.mods.namePlaceholder")).default_value(mod_.map(|m| m.name.clone()).unwrap_or_default()));
        cx.observe(&name, |_, _, cx| cx.notify()).detach();
        let css = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(12)
                .placeholder(tr("settings.mods.cssPlaceholder"))
                .default_value(mod_.map(|m| m.css.clone()).unwrap_or_default())
        });
        css.update(cx, |s, cx| s.focus(window, cx));
        self.draft = Some(Draft { id: mod_.map(|m| m.id.clone()), name, css });
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.draft.take() else { return };
        let name = kit::text_of(&draft.name, cx);
        let css = draft.css.read(cx).value().to_string();
        api(cx).save_mod(draft.id.as_deref(), &name, &css);
        self.load(cx);
    }
}

impl Render for ModSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let off = self.data.disabled || self.data.safe_mode;
        let count = self.data.mods.len();

        let rows: Vec<gpui::AnyElement> = self
            .data
            .mods
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let mut row = kit::row(cx).child(kit::label(
                    cx,
                    m.name.clone(),
                    Some(tr_args("settings.mods.lines", &[("count", &line_count(&m.css).to_string())]).into()),
                ));
                if self.confirming.as_deref() == Some(m.id.as_str()) {
                    let (id, this) = (m.id.clone(), cx.entity());
                    row = row.child(
                        InlineConfirm::new(("mod-confirm", i), tr_args("settings.mods.confirmDelete", &[("name", &m.name)]), tr("settings.mods.delete"), tr("common.cancel"))
                            .on_cancel(kit::entity_cb(cx, |this, cx| {
                                this.confirming = None;
                                cx.notify();
                            }))
                            .on_confirm(move |_, cx| {
                                this.update(cx, |t, cx| {
                                    t.confirming = None;
                                    cx.notify();
                                });
                                api(cx).delete_mod(&id);
                            }),
                    );
                } else {
                    let (up, down, export, delete, toggle) = (m.id.clone(), m.id.clone(), m.id.clone(), m.id.clone(), m.id.clone());
                    let edit = m.clone();
                    let enabled = m.enabled;
                    row = row
                        .child(kit::icon_btn(("mod-up", i), IconName::ArrowUp, tr("settings.mods.up"), move |_, _, cx| api(cx).move_mod(&up, -1)).small().disabled(i == 0))
                        .child(
                            kit::icon_btn(("mod-down", i), IconName::ArrowDown, tr("settings.mods.down"), move |_, _, cx| api(cx).move_mod(&down, 1))
                                .small()
                                .disabled(i + 1 == count),
                        )
                        .child(kit::icon_btn(("mod-export", i), IconName::Upload, tr("settings.mods.export"), move |_, _, cx| api(cx).export_mod(&export)).small())
                        .child(
                            Btn::new(("mod-edit", i), tr("settings.mods.edit"))
                                .icon(IconName::Pencil)
                                .on_click(cx.listener(move |this, _, w, cx| this.open(Some(&edit), w, cx))),
                        )
                        .child(kit::icon_btn(
                            ("mod-del", i),
                            IconName::Trash2,
                            tr("settings.mods.delete"),
                            cx.listener(move |this, _, _, cx| {
                                this.confirming = Some(delete.clone());
                                cx.notify();
                            }),
                        ))
                        .child(kit::switch(("mod-sw", i), enabled, move |on, _, cx| api(cx).set_mod_enabled(&toggle, on)));
                }
                row.into_any_element()
            })
            .collect();

        let editor = self.draft.as_ref().map(|d| {
            let valid_name = true;
            let _ = valid_name;
            div()
                .id("mod-editor")
                .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                    let k = &ev.keystroke;
                    if k.key == "escape" {
                        this.draft = None;
                        cx.notify();
                    } else if k.key == "s" && (k.modifiers.platform || k.modifiers.control) {
                        this.save(cx);
                    }
                }))
                .child(
                    kit::group(
                        cx,
                        vec![div()
                            .flex()
                            .flex_col()
                            .gap(space::S2)
                            .p(space::S3)
                            .child(kit::field(&d.name))
                            .child(
                                div()
                                    .min_h(px(220.0))
                                    .rounded(radius::TILE)
                                    .bg(t.surface)
                                    .font_family(FONT_MONO)
                                    .text_size(t.type_scale.sub)
                                    .child(Textarea::new(&d.css).h(px(220.0)).appearance(false)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(space::S2)
                                    .child(kit::hint_text(cx, tr(if off { "settings.mods.offHint" } else { "settings.mods.liveHint" })).flex_1().min_w_0())
                                    .child(Btn::new("mod-cancel", tr("common.cancel")).on_click(cx.listener(|this, _, _, cx| {
                                        this.draft = None;
                                        cx.notify();
                                    })))
                                    .child(Btn::new("mod-save", tr("settings.mods.save")).accent().on_click(cx.listener(|this, _, _, cx| this.save(cx)))),
                            )
                            .into_any_element()],
                    )
                    .mb(space::S4),
                )
        });

        let list = if self.data.mods.is_empty() && self.draft.is_none() {
            Some(kit::empty(cx, IconName::Paintbrush, vec![kit::para(tr("settings.mods.empty"))]).into_any_element())
        } else if !self.data.mods.is_empty() {
            Some(kit::group(cx, rows).into_any_element())
        } else {
            None
        };

        kit::body()
            .child(kit::group(
                cx,
                vec![kit::row(cx)
                    .child(kit::label(cx, tr("settings.mods.disableAll"), Some(tr("settings.mods.disableAllHint").into())))
                    .child(kit::switch("mod-disable-all", self.data.disabled, |on, _, cx| api(cx).set_mods_disabled(on)))
                    .into_any_element()],
            ))
            .when(self.data.safe_mode, |d| d.child(kit::note(cx, tr("settings.mods.safeMode"))))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S3)
                    .child(div().flex_1().child(kit::sub(cx, tr("settings.mods.list"))))
                    .child(Btn::new("mod-import", tr("settings.mods.import")).icon(IconName::Download).on_click(|_, _, cx| {
                        api(cx).import_mod();
                    }))
                    .child(Btn::new("mod-add", tr("settings.mods.add")).icon(IconName::Plus).on_click(cx.listener(|this, _, w, cx| this.open(None, w, cx)))),
            )
            .children(editor)
            .children(list)
            .child(kit::note(cx, tr("settings.mods.help")))
    }
}

#[cfg(test)]
mod tests {
    use super::line_count;

    #[test]
    fn counts_non_blank_lines() {
        assert_eq!(line_count(""), 0);
        assert_eq!(line_count("a {\n  color: red;\n}\n\n  \n"), 3);
    }
}

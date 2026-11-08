//! Settings > Extensions (port of `ExtensionSettings.tsx`): installed extensions, removal and access to the store.

use gpui::{Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div, prelude::FluentBuilder};
use serde_json::json;

use super::kit::{self, Btn};
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::theme::{ThemeExt, space};
use crate::widgets::InlineConfirm;
use gpui_component::Icon;

pub struct ExtensionSettings {
    store: Entity<InfoStore>,
    confirming: Option<String>,
}

impl ExtensionSettings {
    pub fn new(store: Entity<InfoStore>, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        Self { store, confirming: None }
    }
}

impl Render for ExtensionSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let info = info_of(&self.store, cx);
        let pinned = info.settings.pinned_extensions.clone().unwrap_or_default();
        let dev_mode = info.settings.dev_mode == Some(true);

        let mut store_rows = vec![
            kit::row(cx)
                .child(kit::label(cx, tr("settings.extensions.store"), Some(tr("settings.extensions.storeHint").into())))
                .child(Btn::new("ext-store", tr("settings.extensions.openStore")).on_click(|_, _, cx| api(cx).open_extension_store()))
                .into_any_element(),
        ];
        if dev_mode {
            store_rows.push(
                kit::row(cx)
                    .child(kit::label(cx, tr("settings.extensions.unpackedTitle"), Some(tr("settings.extensions.unpackedHint").into())))
                    .child(
                        Btn::new("ext-load", tr("settings.extensions.loadUnpacked")).icon(IconName::FolderOpen).on_click(|_, _, cx| api(cx).load_unpacked_extension()),
                    )
                    .into_any_element(),
            );
        }

        let list = if info.extensions.is_empty() {
            kit::empty(cx, IconName::Puzzle, vec![kit::para(tr("settings.extensions.none"))]).into_any_element()
        } else {
            let rows = info
                .extensions
                .iter()
                .enumerate()
                .map(|(i, ext)| {
                    let key = if ext.id.is_empty() { ext.unpacked.as_ref().map(|u| u.path.clone()).unwrap_or_default() } else { ext.id.clone() };
                    let mut label = div().flex_1().min_w_0().flex().flex_col().gap(space::HALF).child(div().child(ext.name.clone()));
                    if !ext.version.is_empty() {
                        label = label.child(kit::hint_text(cx, tr_args("settings.extensions.version", &[("version", &ext.version)])));
                    }
                    if let Some(u) = &ext.unpacked {
                        let mut line = tr("settings.extensions.unpackedLabel");
                        if let Some(at) = u.reloaded_at.filter(|a| *a > 0) {
                            line.push_str(&format!(" \u{b7} {}", tr_args("settings.extensions.reloadedAt", &[("time", &kit::clock_time(at))])));
                        }
                        label = label.child(kit::hint_text(cx, line)).child(kit::hint_text(cx, u.display.clone()));
                        if let Some(e) = &u.error {
                            label = label.child(kit::hint_text(cx, e.clone()).text_color(t.danger));
                        }
                    }
                    let icon = match ext.icon.as_deref().and_then(|u| kit::data_image(u, 20.0, 4.0)) {
                        Some(img) => img,
                        None => Icon::from(IconName::Puzzle).size(gpui::px(20.0)).text_color(t.text_2).into_any_element(),
                    };
                    let mut row = kit::row(cx).child(icon).child(label);
                    if let Some(u) = &ext.unpacked {
                        let path = u.path.clone();
                        row = row.child(
                            Btn::new(("ext-reload", i), tr("settings.extensions.reload")).icon(IconName::RotateCw).on_click(move |_, _, cx| api(cx).reload_unpacked_extension(&path)),
                        );
                    }
                    if ext.has_action {
                        let (id, list) = (ext.id.clone(), pinned.clone());
                        let on = pinned.contains(&ext.id);
                        row = row.child(kit::hint_text(cx, tr("settings.extensions.showInToolbar"))).child(kit::switch(("ext-pin", i), on, move |_, _, cx| {
                            let next: Vec<String> = if list.contains(&id) { list.iter().filter(|x| **x != id).cloned().collect() } else { list.iter().cloned().chain([id.clone()]).collect() };
                            kit::update(cx, "pinnedExtensions", json!(next));
                        }));
                    }
                    if self.confirming.as_deref() == Some(key.as_str()) {
                        let (unpacked, id, name) = (ext.unpacked.as_ref().map(|u| u.path.clone()), ext.id.clone(), ext.name.clone());
                        let message = tr_args(if ext.unpacked.is_some() { "settings.extensions.confirmUnpacked" } else { "settings.extensions.confirm" }, &[("name", &name)]);
                        let this = cx.entity();
                        row = row.child(
                            InlineConfirm::new(("ext-confirm", i), message, tr("settings.extensions.remove"), tr("common.cancel"))
                                .on_cancel(kit::entity_cb(cx, |this, cx| {
                                    this.confirming = None;
                                    cx.notify();
                                }))
                                .on_confirm(move |_, cx| {
                                    this.update(cx, |t, cx| {
                                        t.confirming = None;
                                        cx.notify();
                                    });
                                    match &unpacked {
                                        Some(path) => api(cx).remove_unpacked_extension(path),
                                        None => api(cx).remove_extension(&id),
                                    }
                                }),
                        );
                    } else {
                        row = row.child(Btn::new(("ext-remove", i), tr("settings.extensions.remove")).icon(IconName::Trash2).on_click(cx.listener(move |this, _, _, cx| {
                            this.confirming = Some(key.clone());
                            cx.notify();
                        })));
                    }
                    row.into_any_element()
                })
                .collect();
            kit::group(cx, rows).into_any_element()
        };

        kit::body()
            .child(kit::note(cx, tr("settings.extensions.note")))
            .child(kit::note(cx, tr("settings.extensions.orderHint")))
            .child(kit::group(cx, store_rows))
            .when(!dev_mode, |d| d.child(kit::note(cx, tr("settings.extensions.unpackedDevMode"))))
            .child(list)
    }
}

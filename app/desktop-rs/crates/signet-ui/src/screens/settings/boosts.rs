//! Settings > Boosts (port of `Boosts` of `Settings.tsx`).

use gpui::{Context, Entity, IntoElement, ParentElement, Render, Window, div};
use signet_core::i18n::IS_MAC;
use signet_core::shortcuts::shortcut_label;

use super::kit::{self, Btn};
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::widgets::InlineConfirm;

pub struct Boosts {
    store: Entity<InfoStore>,
    confirming: Option<String>,
}

impl Boosts {
    pub fn new(store: Entity<InfoStore>, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        Self { store, confirming: None }
    }
}

impl Render for Boosts {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = info_of(&self.store, cx);
        let keys = shortcut_label("customizeSite", info.settings.shortcuts.as_ref(), IS_MAC).filter(|k| !k.is_empty());
        if info.boosts.is_empty() {
            let second = match &keys {
                Some(k) => kit::rich(cx, &tr_args("settings.boosts.empty2", &[("keys", k)])).into_any_element(),
                None => div().child(tr("settings.boosts.empty2NoKeys")).into_any_element(),
            };
            return kit::empty(cx, IconName::Rocket, vec![kit::para(tr("settings.boosts.empty1")), div().child(second).into_any_element()]).into_any_element();
        }
        let rows = info
            .boosts
            .iter()
            .enumerate()
            .map(|(i, boost)| {
                let site = boost.site.clone();
                let hidden = boost.zapped.len();
                let hidden_text = if hidden == 0 {
                    tr("settings.boosts.noneHidden")
                } else {
                    tr_args("settings.boosts.hidden", &[("count", &hidden.to_string())])
                };
                let hint = format!("{} \u{b7} {}", hidden_text, kit::short_date(boost.updated_at));
                let row = kit::row(cx).child(kit::label(cx, site.clone(), Some(hint.into())));
                if self.confirming.as_deref() == Some(site.as_str()) {
                    let s1 = site.clone();
                    row.child(
                        InlineConfirm::new(("boost-confirm", i), tr_args("settings.boosts.confirm", &[("site", &site)]), tr("settings.boosts.delete"), tr("common.cancel"))
                            .on_cancel(kit::entity_cb(cx, |this, cx| {
                                this.confirming = None;
                                cx.notify();
                            }))
                            .on_confirm({
                                let this = cx.entity();
                                move |_, cx| {
                                    this.update(cx, |t, cx| {
                                        t.confirming = None;
                                        cx.notify();
                                    });
                                    api(cx).delete_boost(&s1);
                                }
                            }),
                    )
                    .into_any_element()
                } else {
                    let (s_toggle, s_edit, s_del) = (site.clone(), site.clone(), site.clone());
                    row.child(kit::switch(("boost-sw", i), boost.enabled, move |v, _, cx| api(cx).save_boost(&s_toggle, None, Some(v))))
                        .child(
                            Btn::new(("boost-edit", i), tr("settings.boosts.edit"))
                                .icon(IconName::Pencil)
                                .on_click(move |_, _, cx| api(cx).open_boost(&s_edit)),
                        )
                        .child(kit::icon_btn(
                            ("boost-del", i),
                            IconName::Trash2,
                            tr("settings.boosts.deleteBoost"),
                            cx.listener(move |this, _, _, cx| {
                                this.confirming = Some(s_del.clone());
                                cx.notify();
                            }),
                        ))
                        .into_any_element()
                }
            })
            .collect();
        kit::group(cx, rows).into_any_element()
    }
}

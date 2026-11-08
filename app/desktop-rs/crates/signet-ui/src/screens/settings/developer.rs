//! Settings > Developer, System and About (port of `Developer`, `LocalNames`, `System` and `About` of `Settings.tsx`).

use gpui::{Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div, prelude::FluentBuilder, px};
use gpui_component::input::InputState;
use serde_json::json;
use signet_core::i18n::IS_MAC;
use signet_core::devhosts::{alias_target, is_alias_name};
use signet_core::shortcuts::shortcut_label;
use signet_core::types::DevAlias;

use super::kit::{self, Btn};
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::theme::{FONT_MONO, ThemeExt, space, weight};

const REPO: &str = "https://github.com/ItsKarmaOff/Signet";

/// Development: developer mode (local pages) and reminder of the tools shortcuts.
pub struct Developer {
    store: Entity<InfoStore>,
    name: Entity<InputState>,
    target: Entity<InputState>,
    error: Option<String>,
}

impl Developer {
    pub fn new(store: Entity<InfoStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        let name = kit::input(window, cx, tr("settings.developer.aliasNamePlaceholder"), "", false, Some(Self::add));
        let target = kit::input(window, cx, tr("settings.developer.aliasTargetPlaceholder"), "", false, Some(Self::add));
        // Typing clears the error shown for the previous attempt.
        for state in [&name, &target] {
            cx.subscribe(state, |this: &mut Self, _, ev: &gpui_component::input::InputEvent, cx| {
                if matches!(ev, gpui_component::input::InputEvent::Change) && this.error.take().is_some() {
                    cx.notify();
                }
            })
            .detach();
        }
        Self { store, name, target, error: None }
    }

    fn save_aliases(cx: &gpui::App, aliases: &[DevAlias]) {
        kit::update(cx, "devAliases", serde_json::to_value(aliases).unwrap_or(json!([])));
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let aliases = info_of(&self.store, cx).settings.dev_aliases.clone().unwrap_or_default();
        let n = kit::text_of(&self.name, cx).trim().to_lowercase();
        let target_text = kit::text_of(&self.target, cx);
        if n.is_empty() || target_text.trim().is_empty() {
            return;
        }
        let url = alias_target(&target_text);
        if !is_alias_name(&n) {
            self.error = Some(tr("settings.developer.aliasBadName"));
        } else if aliases.iter().any(|a| a.name == n) {
            self.error = Some(tr_args("settings.developer.aliasTaken", &[("name", &n)]));
        } else if let Some(url) = url {
            let mut next = aliases;
            next.push(DevAlias { name: n, target: url });
            Self::save_aliases(cx, &next);
            kit::set_text(&self.name, "", window, cx);
            kit::set_text(&self.target, "", window, cx);
            self.error = None;
        } else {
            self.error = Some(tr("settings.developer.aliasBadTarget"));
        }
        cx.notify();
    }
}

impl Render for Developer {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let info = info_of(&self.store, cx);
        let s = &info.settings;
        let on = s.dev_mode == Some(true);
        let keys = |id: &str| shortcut_label(id, s.shortcuts.as_ref(), IS_MAC).filter(|k| !k.is_empty()).unwrap_or_else(|| "-".into());
        let sw = |id: &'static str, title: String, hint: String, on: bool, key: &'static str| {
            kit::row(cx)
                .child(kit::label(cx, title, Some(hint.into())))
                .child(kit::switch(id, on, move |v, _, cx| kit::update(cx, key, json!(v))))
                .into_any_element()
        };
        let mut dev_rows = vec![sw("sw-dev", tr("settings.developer.devMode"), tr("settings.developer.devModeHint"), on, "devMode")];
        if on {
            dev_rows.push(sw(
                "sw-keep-out",
                tr("settings.developer.keepOut"),
                tr("settings.developer.keepOutHint"),
                s.dev_keep_out_of_history == Some(true),
                "devKeepOutOfHistory",
            ));
        }

        let aliases = s.dev_aliases.clone().unwrap_or_default();
        let mut alias_rows: Vec<gpui::AnyElement> = aliases
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let all = aliases.clone();
                let name = a.name.clone();
                kit::row(cx)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(space::HALF)
                            .child(div().font_family(FONT_MONO).text_size(t.type_scale.sub).child(a.name.clone()))
                            .child(kit::hint_text(cx, a.target.clone())),
                    )
                    .child(
                        Btn::new(("alias-rm", i), tr("settings.developer.aliasRemove")).icon(IconName::Trash2).on_click(move |_, _, cx| {
                            let next: Vec<DevAlias> = all.iter().filter(|x| x.name != name).cloned().collect();
                            Self::save_aliases(cx, &next);
                        }),
                    )
                    .into_any_element()
            })
            .collect();
        let can_add = !kit::text_of(&self.name, cx).trim().is_empty() && !kit::text_of(&self.target, cx).trim().is_empty();
        alias_rows.push(
            kit::row(cx)
                .child(kit::field(&self.name))
                .child(div().flex_none().text_color(t.text_2).child("\u{2192}"))
                .child(kit::field(&self.target))
                .child(
                    Btn::new("alias-add", tr("settings.developer.aliasAdd")).disabled(!can_add).on_click(cx.listener(|this, _, w, cx| this.add(w, cx))),
                )
                .into_any_element(),
        );
        if let Some(e) = self.error.clone() {
            alias_rows.push(kit::note_err(cx, e).px(space::S4).py(space::S2).into_any_element());
        }

        let shortcuts_hint = tr_args(
            "settings.developer.shortcutsHint",
            &[("devtools", &keys("devTools")), ("console", &keys("jsConsole")), ("responsive", &keys("responsive"))],
        );

        kit::body()
            .child(kit::group(cx, dev_rows))
            .child(kit::group(
                cx,
                vec![sw(
                    "sw-json",
                    tr("settings.developer.jsonViewer"),
                    tr("settings.developer.jsonViewerHint"),
                    s.json_viewer != Some(false),
                    "jsonViewer",
                )],
            ))
            .child(kit::sub(cx, tr("settings.developer.aliases")))
            .child(kit::note(cx, tr("settings.developer.aliasesHint")))
            .child(kit::group(cx, alias_rows))
            .child(kit::note(cx, shortcuts_hint))
    }
}

/// System: engine settings (graphics acceleration, proxy).
pub struct System {
    store: Entity<InfoStore>,
}

impl System {
    pub fn new(store: Entity<InfoStore>, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        Self { store }
    }
}

impl Render for System {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = info_of(&self.store, cx);
        let on = info.settings.hardware_acceleration != Some(false);
        let pending = on != info.hardware_acceleration_active;
        let hint = if pending {
            tr(if info.packaged { "settings.system.gpuPending" } else { "settings.system.gpuPendingDev" })
        } else {
            tr("settings.system.gpuHint")
        };
        let mut rows = vec![
            kit::row(cx)
                .child(kit::label(cx, tr("settings.system.gpu"), Some(hint.into())))
                .when(pending && info.packaged, |r| {
                    r.child(Btn::new("relaunch", tr("settings.system.relaunch")).icon(IconName::RotateCw).on_click(|_, _, cx| api(cx).relaunch_app()))
                })
                .child(kit::switch("sw-gpu", on, |v, _, cx| kit::update(cx, "hardwareAcceleration", json!(v))))
                .into_any_element(),
        ];
        if info.proxy_settings {
            rows.push(
                kit::row(cx)
                    .child(kit::label(cx, tr("settings.system.proxy"), Some(tr("settings.system.proxyHint").into())))
                    .child(
                        Btn::new("proxy-open", tr("settings.system.proxyOpen")).icon(IconName::ExternalLink).on_click(|_, _, cx| api(cx).open_proxy_settings()),
                    )
                    .into_any_element(),
            );
        }
        kit::body().child(kit::group(cx, rows))
    }
}

/// About: version, repository, updates, introduction.
pub struct About {
    store: Entity<InfoStore>,
}

impl About {
    pub fn new(store: Entity<InfoStore>, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        Self { store }
    }
}

impl Render for About {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let info = info_of(&self.store, cx);
        let icon = info.app_icons.iter().find(|i| i.id == info.settings.app_icon).or_else(|| info.app_icons.first());
        kit::body()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(space::S2)
                    .mt(space::S8)
                    .text_center()
                    .children(icon.map(|i| kit::app_icon_image(&i.id, 96.0, 21.0)))
                    .child(div().mt(space::S2).text_size(t.type_scale.hero).line_height(gpui::relative(1.1)).font_weight(weight::HERO).child("Signet"))
                    .child(kit::hint_text(cx, tr_args("settings.about.version", &[("version", &info.version)])))
                    .child(div().mt(space::S2).child(kit::link(cx, "repo", REPO.trim_start_matches("https://").to_string(), |_, _, cx| cx.open_url(REPO))))
                    .child(div().mt(px(10.0)).child(Btn::new("check-updates", tr("settings.about.checkUpdates")).on_click(|_, _, cx| api(cx).check_for_updates())))
                    .child(div().mt(px(12.0)).child(Btn::new("intro", tr("settings.about.intro")).on_click(|_, _, cx| api(cx).open_welcome()))),
            )
            .child(kit::group(
                cx,
                vec![
                    kit::row(cx)
                        .child(kit::label(cx, tr("settings.about.autoUpdates"), Some(tr("settings.about.autoUpdatesHint").into())))
                        .child(kit::switch("sw-updates", info.settings.check_updates != Some(false), |v, _, cx| kit::update(cx, "checkUpdates", json!(v))))
                        .into_any_element(),
                ],
            ))
            .child(div().h(px(0.0)))
    }
}

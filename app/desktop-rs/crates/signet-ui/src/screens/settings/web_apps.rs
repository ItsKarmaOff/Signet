//! Settings > Apps (port of `WebAppSettings.tsx`): sites installed as apps (open, rename, show the launcher,
//! uninstall).

use gpui::{Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::{Icon, input::{InputEvent, InputState}};
use signet_core::api::UiEvent;
use signet_core::types::WebAppsInfo;

use super::kit::{self, Btn};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::icon::IconName;
use crate::theme::ThemeExt;
use crate::widgets::InlineConfirm;

/// Domain of an address (`domainOf` of the renderer).
fn domain_of(url: &str) -> String {
    // Internal address: shown in full.
    if url.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("signet:")) {
        return url.trim_end_matches('/').to_string();
    }
    match url::Url::parse(url) {
        Ok(u) => u.host_str().map(|h| h.trim_start_matches("www.").to_string()).unwrap_or_else(|| url.to_string()),
        Err(_) => url.to_string(),
    }
}

pub struct WebAppSettings {
    info: WebAppsInfo,
    editing: Option<(String, Entity<InputState>)>,
    confirming: Option<String>,
}

impl WebAppSettings {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        kit::watch_locale(cx);
        cx.subscribe(&ui_state(cx), |this, _, ev: &UiEvent, cx| {
            if matches!(ev, UiEvent::WebAppsChanged) {
                this.load(cx);
            }
        })
        .detach();
        let info = api(cx).list_web_apps();
        Self { info, editing: None, confirming: None }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.info = api(cx).list_web_apps();
        cx.notify();
    }

    fn start_rename(&mut self, id: &str, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let state = kit::input(window, cx, "", name.to_string(), false, Some(Self::save_name));
        cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::Blur) {
                this.save_name(window, cx);
            }
        })
        .detach();
        state.update(cx, |s, cx| s.focus(window, cx));
        self.editing = Some((id.to_string(), state));
        cx.notify();
    }

    fn save_name(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some((id, state)) = self.editing.take() {
            let name = kit::text_of(&state, cx);
            if !name.trim().is_empty() {
                api(cx).rename_web_app(&id, &name);
            }
            cx.notify();
        }
    }
}

impl Render for WebAppSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        if self.info.apps.is_empty() {
            return kit::empty(cx, IconName::AppWindow, vec![kit::para(tr("settings.apps.empty1")), div().child(kit::rich(cx, &tr("settings.apps.empty2"))).into_any_element()])
                .into_any_element();
        }
        let rows = self
            .info
            .apps
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let icon = match kit::data_image(&a.icon, 28.0, 6.0) {
                    Some(img) => img,
                    None => Icon::from(IconName::AppWindow).size(gpui::px(28.0)).text_color(t.text_2).into_any_element(),
                };
                let mut row = kit::row(cx).child(icon);
                let is_editing = self.editing.as_ref().is_some_and(|(id, _)| *id == a.id);
                if let (true, Some((_, state))) = (is_editing, &self.editing) {
                    row = row.child(kit::field(state));
                } else {
                    let mut hint = domain_of(&a.start_url);
                    if let Some(s) = &a.space {
                        hint.push_str(&format!(" \u{b7} {}", tr_args("settings.apps.space", &[("space", &format!("{} {}", s.icon, s.name))])));
                    }
                    if !a.launcher {
                        hint.push_str(&format!(" \u{b7} {}", tr("settings.apps.noLauncher")));
                    }
                    row = row.child(kit::label(cx, a.name.clone(), Some(hint.into())));
                }
                if self.confirming.as_deref() == Some(a.id.as_str()) {
                    let id = a.id.clone();
                    let this = cx.entity();
                    row = row.child(
                        InlineConfirm::new(("app-confirm", i), tr_args("settings.apps.confirm", &[("name", &a.name)]), tr("settings.apps.remove"), tr("common.cancel"))
                            .on_cancel(kit::entity_cb(cx, |this, cx| {
                                this.confirming = None;
                                cx.notify();
                            }))
                            .on_confirm(move |_, cx| {
                                this.update(cx, |t, cx| {
                                    t.confirming = None;
                                    cx.notify();
                                });
                                api(cx).remove_web_app(&id);
                            }),
                    );
                } else if !is_editing {
                    let (open_id, rename_id, reveal_id, name) = (a.id.clone(), a.id.clone(), a.id.clone(), a.name.clone());
                    let del_id = a.id.clone();
                    row = row
                        .child(Btn::new(("app-open", i), tr("settings.apps.open")).on_click(move |_, _, cx| api(cx).open_web_app(&open_id)))
                        .child(kit::icon_btn(
                            ("app-rename", i),
                            IconName::Pencil,
                            tr("settings.apps.rename"),
                            cx.listener(move |this, _, window, cx| this.start_rename(&rename_id, &name, window, cx)),
                        ))
                        .child(kit::icon_btn(
                            ("app-reveal", i),
                            IconName::FolderOpen,
                            tr(if a.launcher { "settings.apps.reveal" } else { "settings.apps.recreate" }),
                            move |_, _, cx| api(cx).reveal_web_app(&reveal_id),
                        ))
                        .child(kit::icon_btn(
                            ("app-del", i),
                            IconName::Trash2,
                            tr("settings.apps.uninstall"),
                            cx.listener(move |this, _, _, cx| {
                                this.confirming = Some(del_id.clone());
                                cx.notify();
                            }),
                        ));
                }
                row.into_any_element()
            })
            .collect();
        kit::body()
            .child(kit::note(cx, tr_args("settings.apps.intro", &[("folder", &self.info.folder)])))
            .child(kit::group(cx, rows))
            .child(kit::note(cx, tr("settings.apps.howTo")))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_drops_www() {
        assert_eq!(domain_of("https://www.example.com/a?b=1"), "example.com");
        assert_eq!(domain_of("not a url"), "not a url");
        assert_eq!(domain_of("signet://note/abc/"), "signet://note/abc");
    }
}

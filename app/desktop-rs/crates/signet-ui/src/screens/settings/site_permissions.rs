//! Settings > Privacy > Site permissions (port of `SitePermissionSettings.tsx`).

use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div};
use signet_core::types::{PermissionDecision, SitePermissionEntry};

use super::kit::{self, Btn};
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::theme::{ThemeExt, FONT_MONO};

pub struct SitePermissionSettings {
    entries: Vec<SitePermissionEntry>,
}

impl SitePermissionSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        kit::watch_locale(cx);
        // Re-read when returning to the window.
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.load(cx);
            }
        })
        .detach();
        let mut me = Self { entries: Vec::new() };
        me.load(cx);
        me
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.entries = api(cx).list_site_permissions();
        cx.notify();
    }

    fn reset(&mut self, origin: Option<&str>, permission: Option<&str>, cx: &mut Context<Self>) {
        api(cx).reset_site_permissions(origin, permission);
        self.load(cx);
    }
}

/// Unknown permission identifiers are shown as is.
fn permission_label(permission: &str) -> String {
    kit::tr_or(&format!("settings.privacy.permNames.{permission}"), permission)
}

impl Render for SitePermissionSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let body = if self.entries.is_empty() {
            kit::note(cx, tr("settings.privacy.permEmpty")).into_any_element()
        } else {
            let mut rows: Vec<gpui::AnyElement> = self
                .entries
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let (origin, permission) = (e.origin.clone(), e.permission.clone());
                    let label = permission_label(&e.permission);
                    let decision = tr(if e.decision == PermissionDecision::Allow { "settings.privacy.permAllowed" } else { "settings.privacy.permDenied" });
                    kit::row(cx)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(crate::theme::space::HALF)
                                .child(div().text_size(t.type_scale.sub).font_family(FONT_MONO).child(kit::host_of(&e.origin)))
                                .child(kit::hint_text(cx, format!("{label} \u{b7} {decision}"))),
                        )
                        .child(kit::icon_btn(
                            ("perm-reset", i),
                            IconName::Trash2,
                            tr("settings.privacy.permReset"),
                            cx.listener(move |this, _, _, cx| this.reset(Some(&origin), Some(&permission), cx)),
                        ))
                        .into_any_element()
                })
                .collect();
            rows.push(
                kit::row(cx)
                    .justify_end()
                    .child(
                        Btn::new("perm-reset-all", tr("settings.privacy.permResetAll"))
                            .icon(IconName::Trash2)
                            .danger()
                            .on_click(cx.listener(|this, _, _, cx| this.reset(None, None, cx))),
                    )
                    .into_any_element(),
            );
            kit::group(cx, rows).into_any_element()
        };
        let _ = tr_args;
        div().flex().flex_col().gap(crate::theme::space::S2).w_full().child(kit::sub(cx, tr("settings.privacy.sitePermissions"))).child(body)
    }
}

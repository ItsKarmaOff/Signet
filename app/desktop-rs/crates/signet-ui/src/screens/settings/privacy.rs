//! Settings > Privacy (port of `Privacy` and `SecureDns` of `Settings.tsx`).

use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div, prelude::FluentBuilder,
};
use gpui_component::input::InputState;
use serde_json::json;
use signet_core::types::{BrowsingDataKind, DataCounts, SecureDnsMode};

use super::dns::{DEFAULT_DNS_SERVER, DNS_PROVIDERS, is_doh_template};
use super::kit::{self, Btn};
use super::site_permissions::SitePermissionSettings;
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::theme::space;

const ALL_KINDS: [BrowsingDataKind; 6] = [
    BrowsingDataKind::History,
    BrowsingDataKind::Archive,
    BrowsingDataKind::Downloads,
    BrowsingDataKind::Cache,
    BrowsingDataKind::Cookies,
    BrowsingDataKind::Permissions,
];

fn kind_label(kind: BrowsingDataKind, c: &DataCounts) -> String {
    let count = |n: u32| [("count", n.to_string())];
    let with = |key: &str, n: u32| {
        let args = count(n);
        tr_args(key, &[(args[0].0, args[0].1.as_str())])
    };
    match kind {
        BrowsingDataKind::History => with("settings.privacy.history", c.history),
        BrowsingDataKind::Archive => with("settings.privacy.archive", c.archive),
        BrowsingDataKind::Downloads => with("settings.privacy.downloads", c.downloads),
        BrowsingDataKind::Cache => tr("settings.privacy.cache"),
        BrowsingDataKind::Cookies => tr("settings.privacy.cookies"),
        BrowsingDataKind::Permissions => with("settings.privacy.permissions", c.permissions),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Confirming {
    Selection,
    All,
}

pub struct Privacy {
    store: Entity<InfoStore>,
    picked: Vec<BrowsingDataKind>,
    confirming: Option<Confirming>,
    done: bool,
    dns: Entity<SecureDns>,
    permissions: Entity<SitePermissionSettings>,
}

impl Privacy {
    pub fn new(store: Entity<InfoStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        let dns = cx.new(|cx| SecureDns::new(store.clone(), window, cx));
        let permissions = cx.new(|cx| SitePermissionSettings::new(window, cx));
        Self { store, picked: Vec::new(), confirming: None, done: false, dns, permissions }
    }

    fn reset_feedback(&mut self) {
        self.confirming = None;
        self.done = false;
    }

    fn toggle_all(&mut self, cx: &mut Context<Self>) {
        self.reset_feedback();
        self.picked = if self.picked.len() == ALL_KINDS.len() { Vec::new() } else { ALL_KINDS.to_vec() };
        cx.notify();
    }

    fn toggle(&mut self, kind: BrowsingDataKind, cx: &mut Context<Self>) {
        self.reset_feedback();
        if let Some(i) = self.picked.iter().position(|k| *k == kind) {
            self.picked.remove(i);
        } else {
            self.picked.push(kind);
        }
        cx.notify();
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        let kinds: Vec<BrowsingDataKind> = if self.confirming == Some(Confirming::All) { ALL_KINDS.to_vec() } else { self.picked.clone() };
        api(cx).clear_browsing_data(&kinds);
        self.picked.clear();
        self.done = true;
        self.confirming = None;
        cx.notify();
    }
}

impl Render for Privacy {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = info_of(&self.store, cx);
        let s = &info.settings;
        let sw = |id: &'static str, title: String, hint: Option<String>, on: bool, key: &'static str| {
            kit::row(cx)
                .child(kit::label(cx, title, hint.map(Into::into)))
                .child(kit::switch(id, on, move |v, _, cx| kit::update(cx, key, json!(v))))
                .into_any_element()
        };
        let all = self.picked.len() == ALL_KINDS.len();
        let partial = !self.picked.is_empty() && !all;

        let mut clear_rows = vec![kit::check_row_w(
            cx,
            "clear-all",
            all,
            partial,
            tr("settings.privacy.selectAll"),
            None,
            true,
            {
                let this = cx.entity();
                move |_, cx| this.update(cx, |t, cx| t.toggle_all(cx))
            },
        )];
        for (i, kind) in ALL_KINDS.iter().enumerate() {
            let kind = *kind;
            let this = cx.entity();
            clear_rows.push(kit::check_row(
                cx,
                ("clear-kind", i),
                self.picked.contains(&kind),
                false,
                kind_label(kind, &info.counts),
                None,
                move |_, cx| this.update(cx, |t, cx| t.toggle(kind, cx)),
            ));
        }
        let actions = {
            let mut r = kit::row(cx).justify_end();
            if self.done {
                r = r.child(kit::ok(cx, tr("settings.privacy.cleared")));
            }
            match self.confirming {
                Some(c) => r
                    .child(kit::hint_text(cx, tr(if c == Confirming::All { "settings.privacy.confirmAll" } else { "settings.privacy.confirm" })).flex_1().min_w_0())
                    .child(Btn::new("clear-cancel", tr("common.cancel")).on_click(cx.listener(|this, _, _, cx| {
                        this.confirming = None;
                        cx.notify();
                    })))
                    .child(Btn::new("clear-confirm", tr("settings.privacy.clear")).danger().solid().on_click(cx.listener(|this, _, _, cx| this.clear(cx)))),
                None => r
                    .child(
                        Btn::new("clear-all-btn", tr("settings.privacy.clearAllEllipsis"))
                            .icon(IconName::Trash2)
                            .danger()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirming = Some(Confirming::All);
                                cx.notify();
                            })),
                    )
                    .child(
                        Btn::new("clear-sel-btn", tr("settings.privacy.clearEllipsis"))
                            .icon(IconName::Trash2)
                            .danger()
                            .disabled(self.picked.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirming = Some(Confirming::Selection);
                                cx.notify();
                            })),
                    ),
            }
            .into_any_element()
        };
        clear_rows.push(actions);

        kit::body()
            .child(kit::group(
                cx,
                vec![
                    sw("sw-adblock", tr("settings.privacy.adblock"), None, s.adblock, "adblock"),
                    sw("sw-https", tr("settings.privacy.httpsOnly"), Some(tr("settings.privacy.httpsOnlyHint")), s.https_only == Some(true), "httpsOnly"),
                    sw("sw-gpc", tr("settings.privacy.gpc"), Some(tr("settings.privacy.gpcHint")), s.global_privacy_control == Some(true), "globalPrivacyControl"),
                ],
            ))
            .child(self.dns.clone())
            .child(kit::sub(cx, tr("settings.privacy.clearData")))
            .child(kit::group(cx, clear_rows))
            .child(self.permissions.clone())
    }
}

// --------------------------------------------------------------------------- Secure DNS

const CUSTOM_DNS: &str = "custom";

/// Privacy > Secure DNS: off, with the current provider, or with a chosen DNS over HTTPS server.
pub struct SecureDns {
    store: Entity<InfoStore>,
    picked: String,
    custom: Entity<InputState>,
}

impl SecureDns {
    pub fn new(store: Entity<InfoStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        let info = info_of(&store, cx);
        let server = info.settings.secure_dns_server.clone().unwrap_or_else(|| DEFAULT_DNS_SERVER.to_string());
        let known = DNS_PROVIDERS.iter().find(|p| p.template == server);
        let picked = known.map(|p| p.id.to_string()).unwrap_or_else(|| CUSTOM_DNS.to_string());
        let custom = kit::input(window, cx, tr("settings.privacy.dnsCustomPlaceholder"), if known.is_some() { String::new() } else { server }, false, Some(Self::apply_custom));
        Self { store, picked, custom }
    }

    fn custom_text(&self, cx: &gpui::App) -> String {
        kit::text_of(&self.custom, cx).trim().to_string()
    }

    fn choose(&mut self, value: &str, cx: &mut Context<Self>) {
        if value == "off" || value == "automatic" {
            kit::update(cx, "secureDns", json!(value));
            return;
        }
        self.picked = value.to_string();
        if let Some(p) = DNS_PROVIDERS.iter().find(|p| p.id == value) {
            kit::update_many(cx, vec![("secureDns", json!("secure")), ("secureDnsServer", json!(p.template))]);
        } else if is_doh_template(&self.custom_text(cx)) {
            let custom = self.custom_text(cx);
            kit::update_many(cx, vec![("secureDns", json!("secure")), ("secureDnsServer", json!(custom))]);
        }
        cx.notify();
    }

    fn apply_custom(&mut self, _w: &mut Window, cx: &mut Context<Self>) {
        let custom = self.custom_text(cx);
        if is_doh_template(&custom) {
            kit::update_many(cx, vec![("secureDns", json!("secure")), ("secureDnsServer", json!(custom))]);
        }
    }
}

impl Render for SecureDns {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let info = info_of(&self.store, cx);
        let mode = info.settings.secure_dns.unwrap_or_default();
        let server = info.settings.secure_dns_server.clone().unwrap_or_else(|| DEFAULT_DNS_SERVER.to_string());
        let custom = self.custom_text(cx);
        let custom_ok = is_doh_template(&custom);
        let choice = match mode {
            SecureDnsMode::Off => "off".to_string(),
            SecureDnsMode::Automatic => "automatic".to_string(),
            SecureDnsMode::Secure => self.picked.clone(),
        };
        let mut options = vec![
            ("automatic".to_string(), tr("settings.privacy.dnsAutomatic")),
            ("off".to_string(), tr("settings.privacy.dnsOff")),
        ];
        options.extend(DNS_PROVIDERS.iter().map(|p| (p.id.to_string(), p.name.to_string())));
        options.push((CUSTOM_DNS.to_string(), tr("settings.privacy.dnsCustom")));
        let this = cx.entity();
        let mut rows = vec![
            kit::row(cx)
                .child(kit::label(cx, tr("settings.privacy.dns"), Some(tr("settings.privacy.dnsHint").into())))
                .child(kit::choice("dns", &choice, options, move |v, _, cx| this.update(cx, |t, cx| t.choose(v, cx))))
                .into_any_element(),
        ];
        let is_custom = choice == CUSTOM_DNS;
        if is_custom {
            rows.push(
                kit::row(cx)
                    .child(kit::field(&self.custom))
                    .child(
                        Btn::new("dns-use", tr("settings.privacy.dnsUse"))
                            .disabled(!custom_ok || (mode == SecureDnsMode::Secure && custom == server))
                            .on_click(cx.listener(|this, _, w, cx| this.apply_custom(w, cx))),
                    )
                    .into_any_element(),
            );
        }
        let group = kit::group(cx, rows);
        div()
            .flex()
            .flex_col()
            .gap(space::S1)
            .w_full()
            .child(group)
            .when(is_custom && !custom.is_empty() && !custom_ok, |d| d.child(kit::note_err(cx, tr("settings.privacy.dnsCustomInvalid")).px(space::S4).pt(space::S2)))
    }
}

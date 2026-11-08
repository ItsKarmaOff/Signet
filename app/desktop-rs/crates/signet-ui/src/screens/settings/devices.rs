//! Settings > Devices (port of `DeviceSettings.tsx`): master switch, paired phones, pairing by QR code.

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use gpui::{Context, IntoElement, ParentElement, Render, Styled, Window, div, prelude::FluentBuilder, px};
use gpui_component::Icon;
use signet_core::api::{SignetApi, UiEvent};
use signet_core::types::{LinkDeviceInfo, LinkPlatform, LinkState, LinkStatus};

use super::kit::{self, Btn};
use crate::app_state::{api, tr, tr_args, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space};
use crate::widgets::InlineConfirm;

fn platform_name(p: LinkPlatform) -> &'static str {
    match p {
        LinkPlatform::Ios => "iOS",
        LinkPlatform::Android => "Android",
        LinkPlatform::Macos => "macOS",
        LinkPlatform::Windows => "Windows",
        LinkPlatform::Linux => "Linux",
    }
}

fn status_key(s: LinkStatus) -> &'static str {
    match s {
        LinkStatus::Connected => "connected",
        LinkStatus::Connecting => "connecting",
        LinkStatus::PausedHere => "paused-here",
        LinkStatus::PausedThere => "paused-there",
        LinkStatus::Offline => "offline",
        LinkStatus::Invalid => "invalid",
    }
}

/// "m:ss" of a duration in milliseconds, rounded up.
pub fn clock(ms: i64) -> String {
    let s = (ms.max(0) + 999) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

pub struct DeviceSettings {
    link: LinkState,
    handle: Rc<dyn SignetApi>,
    confirming: Option<String>,
    /// QR code area open (current, expired or failed code).
    pairing: bool,
    starting: bool,
    failed: bool,
    notice: Option<String>,
    /// Links known when pairing started: a new link = the phone is paired.
    before: HashSet<String>,
}

impl DeviceSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        kit::watch_locale(cx);
        let _ = window;
        cx.subscribe(&ui_state(cx), |this, _, ev: &UiEvent, cx| {
            if let UiEvent::LinkState(state) = ev {
                this.link = (**state).clone();
                this.sync();
                cx.notify();
            }
        })
        .detach();
        let handle = api(cx);
        let link = handle.get_link_state();
        Self { link, handle, confirming: None, pairing: false, starting: false, failed: false, notice: None, before: HashSet::new() }
    }

    fn added(&self) -> Option<&LinkDeviceInfo> {
        self.link.devices.iter().find(|d| !self.before.contains(&d.link_id))
    }

    /// The phone paired: the area closes and the message names it.
    fn sync(&mut self) {
        if self.pairing && !self.starting && self.link.pairing.is_none() {
            if let Some(name) = self.added().map(|d| d.device.name.clone()) {
                self.pairing = false;
                self.notice = Some(tr_args("settings.devices.paired", &[("name", &name)]));
            }
        }
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.before = self.link.devices.iter().map(|d| d.link_id.clone()).collect();
        self.notice = None;
        self.failed = false;
        self.pairing = true;
        self.starting = true;
        cx.notify();
        let ok = self.handle.start_link_pairing();
        self.failed = !ok;
        self.starting = false;
        self.link = self.handle.get_link_state();
        self.sync();
        cx.notify();
        // Countdown: re-render twice a second while a code is shown.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                let alive = this
                    .update(cx, |t, cx| {
                        cx.notify();
                        t.pairing
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.pairing = false;
        self.handle.cancel_link_pairing();
        cx.notify();
    }
}

// Leaving the section (or the window) cancels the current code.
impl Drop for DeviceSettings {
    fn drop(&mut self) {
        if self.pairing {
            self.handle.cancel_link_pairing();
        }
    }
}

impl Render for DeviceSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let general = self.link.general;
        let now = signet_core::ids::now_ms();
        let remaining = self.link.pairing.as_ref().map(|p| p.expires_at - now).unwrap_or(0);
        let added = self.added().is_some();
        let expired = self.pairing && !self.starting && !self.failed && !added && (self.link.pairing.is_none() || remaining <= 0);

        let mut phone_rows: Vec<gpui::AnyElement> = self
            .link
            .devices
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let status = tr_args(&format!("settings.devices.status.{}", status_key(d.status)), &[("name", &d.device.name)]);
                let link_id = d.link_id.clone();
                let mut row = kit::row(cx)
                    .child(div().flex_none().when(!general, |d| d.opacity(0.6)).child(Icon::from(IconName::Smartphone).size(px(22.0)).text_color(t.text_2)))
                    .child(kit::label(cx, d.device.name.clone(), Some(format!("{} \u{b7} {}", platform_name(d.device.platform), status).into())).when(!general, |d| d.opacity(0.6)));
                if self.confirming.as_deref() == Some(d.link_id.as_str()) {
                    let this = cx.entity();
                    let forget = link_id.clone();
                    row = row.child(
                        InlineConfirm::new(("dv-confirm", i), tr_args("settings.devices.forgetConfirm", &[("name", &d.device.name)]), tr("settings.devices.forget"), tr("common.cancel"))
                            .on_cancel(kit::entity_cb(cx, |this, cx| {
                                this.confirming = None;
                                cx.notify();
                            }))
                            .on_confirm(move |_, cx| {
                                this.update(cx, |t, cx| {
                                    t.confirming = None;
                                    cx.notify();
                                });
                                api(cx).forget_link(&forget);
                            }),
                    );
                } else {
                    let toggle = link_id.clone();
                    let ask = link_id.clone();
                    row = row
                        .child(kit::icon_btn(
                            ("dv-forget", i),
                            IconName::Trash2,
                            tr("settings.devices.forget"),
                            cx.listener(move |this, _, _, cx| {
                                this.confirming = Some(ask.clone());
                                cx.notify();
                            }),
                        ))
                        .child(crate::widgets::Toggle::new(("dv-sw", i), d.enabled).disabled(!general).on_change(move |on, _, cx| api(cx).set_link_device(&toggle, on)));
                }
                row.into_any_element()
            })
            .collect();
        {
            let mut r = kit::row(cx);
            if self.link.devices.is_empty() {
                r = r.child(kit::label(cx, tr("settings.devices.empty"), Some(tr("settings.devices.emptyHint").into())));
            } else if !general {
                r = r.child(kit::hint_text(cx, tr("settings.devices.generalOffNote")).flex_1());
            }
            r = r.child(
                Btn::new("dv-pair", tr("settings.devices.pair"))
                    .icon(IconName::Smartphone)
                    .disabled(!general || self.pairing)
                    .on_click(cx.listener(|this, _, w, cx| this.start(w, cx))),
            );
            phone_rows.push(r.into_any_element());
        }

        let pairing_area = if self.pairing {
            let qr = match (&self.link.pairing, expired) {
                (Some(p), false) => kit::data_image(&p.qr, 232.0, 0.0),
                _ => None,
            };
            let status_line = if self.failed {
                Some(kit::err(cx, tr("settings.devices.pairFailed")).into_any_element())
            } else if expired {
                Some(kit::hint_text(cx, tr("settings.devices.pairExpired")).into_any_element())
            } else if self.link.pairing.is_some() {
                Some(kit::hint_text(cx, tr_args("settings.devices.pairExpires", &[("time", &clock(remaining))])).into_any_element())
            } else {
                None
            };
            Some(kit::group(
                cx,
                vec![
                    kit::row(cx)
                        .items_start()
                        .child(
                            div()
                                .flex_none()
                                .size(px(232.0))
                                .rounded(radius::ROW)
                                .bg(gpui::white())
                                .overflow_hidden()
                                .children(qr),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(space::HALF)
                                .child(tr("settings.devices.pairHow"))
                                .children(status_line),
                        )
                        .into_any_element(),
                    kit::row(cx)
                        .justify_end()
                        .child(Btn::new("dv-cancel", tr("common.cancel")).on_click(cx.listener(|this, _, _, cx| this.cancel(cx))))
                        .child(
                            Btn::new("dv-new", tr("settings.devices.pairNew"))
                                .icon(IconName::RefreshCw)
                                .disabled(self.starting)
                                .on_click(cx.listener(|this, _, w, cx| this.start(w, cx))),
                        )
                        .into_any_element(),
                ],
            ))
        } else {
            None
        };

        kit::body()
            .child(kit::group(
                cx,
                vec![kit::row(cx)
                    .child(kit::label(cx, tr("settings.devices.general"), Some(tr("settings.devices.generalHint").into())))
                    .child(kit::switch("dv-general", general, |on, _, cx| api(cx).set_link_general(on)))
                    .into_any_element()],
            ))
            .child(kit::sub(cx, tr("settings.devices.phones")))
            .child(kit::group(cx, phone_rows))
            .when_some(self.notice.clone().filter(|_| !self.pairing), |d, n| d.child(kit::note(cx, n).text_color(t.accent).font_weight(crate::theme::weight::MEDIUM)))
            .children(pairing_area)
    }
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn countdown_format() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(-5), "0:00");
        assert_eq!(clock(1), "0:01");
        assert_eq!(clock(61_000), "1:01");
        assert_eq!(clock(120_000), "2:00");
    }
}

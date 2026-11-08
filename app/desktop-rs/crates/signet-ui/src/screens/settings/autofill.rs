//! Settings > Autofill (port of `AutofillSettings.tsx`): the two switches, saved addresses and cards, and the
//! sites where saving is never offered.

use gpui::{AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div, px};
use gpui_component::input::{InputState, Textarea, TextareaState};
use serde_json::json;
use signet_core::types::{AddressInput, AutofillAddress, AutofillCard, AutofillInfo, CardInput, CardNetwork};

use super::kit::{self, Btn};
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::theme::{FONT_MONO, ThemeExt, radius, space};
use crate::widgets::InlineConfirm;

const MASK: &str = "\u{2022}\u{2022}\u{2022}\u{2022} \u{2022}\u{2022}\u{2022}\u{2022} \u{2022}\u{2022}\u{2022}\u{2022} ";

/// Proper names of the card networks (`NETWORK_NAMES`); empty for an unknown one.
pub fn network_name(n: CardNetwork) -> &'static str {
    match n {
        CardNetwork::Visa => "Visa",
        CardNetwork::Mastercard => "Mastercard",
        CardNetwork::Amex => "American Express",
        CardNetwork::Discover => "Discover",
        CardNetwork::Diners => "Diners Club",
        CardNetwork::Jcb => "JCB",
        CardNetwork::Unionpay => "UnionPay",
        CardNetwork::Maestro => "Maestro",
        CardNetwork::Unknown => "",
    }
}

/// Number with spaces, the way it is printed on the card (4-6-5 for Amex).
pub fn group_card_number(digits: &str, network: CardNetwork) -> String {
    let chars: Vec<char> = digits.chars().collect();
    if network == CardNetwork::Amex && chars.len() == 15 {
        let part = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
        return format!("{} {} {}", part(0, 4), part(4, 10), part(10, 15));
    }
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && i % 4 == 0 && chars[i - 1].is_ascii_digit() && c.is_ascii_digit() {
            out.push(' ');
        }
        out.push(*c);
    }
    out
}

/// First and last name joined, whichever exist.
pub fn full_name(given: &str, family: &str) -> String {
    [given, family].iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Expiry years: this year and the next 15, plus the card's own year when it is outside that range (an expired card).
pub fn year_options(current: &str, first: i32) -> Vec<String> {
    let years: Vec<String> = (0..16).map(|i| (first + i).to_string()).collect();
    if current.len() == 4 && current.chars().all(|c| c.is_ascii_digit()) && !years.iter().any(|y| y == current) {
        std::iter::once(current.to_string()).chain(years).collect()
    } else {
        years
    }
}

struct AddressDraft {
    id: Option<String>,
    given: Entity<InputState>,
    family: Entity<InputState>,
    organization: Entity<InputState>,
    street: Entity<TextareaState>,
    postal_code: Entity<InputState>,
    city: Entity<InputState>,
    region: Entity<InputState>,
    country: Entity<InputState>,
    phone: Entity<InputState>,
    email: Entity<InputState>,
}

struct CardDraft {
    id: Option<String>,
    holder: Entity<InputState>,
    number: Entity<InputState>,
    month: String,
    year: String,
    nickname: Entity<InputState>,
}

pub struct AutofillSettings {
    store: Entity<InfoStore>,
    data: Option<AutofillInfo>,
    address: Option<AddressDraft>,
    card: Option<CardDraft>,
    error: Option<String>,
    confirming: Option<String>,
    shown: std::collections::HashMap<String, String>,
    shown_gen: u64,
}

impl AutofillSettings {
    pub fn new(store: Entity<InfoStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        // Does not hide revealed numbers on focus (the window gets focus back right after the authentication
        // prompt); they go away when the window loses focus, after 30 s, or when their card is gone.
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.load(cx);
            } else if !this.shown.is_empty() {
                this.shown.clear();
                cx.notify();
            }
        })
        .detach();
        let mut me = Self { store, data: None, address: None, card: None, error: None, confirming: None, shown: Default::default(), shown_gen: 0 };
        me.load(cx);
        me
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let data = api(cx).list_autofill();
        self.shown.retain(|id, _| data.cards.iter().any(|c| &c.id == id));
        self.data = Some(data);
        cx.notify();
    }

    fn input(&self, window: &mut Window, cx: &mut Context<Self>, key: &str, value: &str, masked: bool) -> Entity<InputState> {
        let state = kit::input(window, cx, tr(key), value.to_string(), masked, None);
        state
    }

    fn open_address(&mut self, a: Option<&AutofillAddress>, window: &mut Window, cx: &mut Context<Self>) {
        let v = |f: fn(&AutofillAddress) -> &String| a.map(|a| f(a).clone()).unwrap_or_default();
        let mk = |this: &Self, window: &mut Window, cx: &mut Context<Self>, key: &str, value: String| this.input(window, cx, key, &value, false);
        let street = cx.new(|cx| TextareaState::new(window, cx).rows(3).placeholder(tr("settings.autofill.street")).default_value(v(|a| &a.street)));
        let draft = AddressDraft {
            id: a.map(|a| a.id.clone()),
            given: mk(self, window, cx, "settings.autofill.givenName", v(|a| &a.given_name)),
            family: mk(self, window, cx, "settings.autofill.familyName", v(|a| &a.family_name)),
            organization: mk(self, window, cx, "settings.autofill.organization", v(|a| &a.organization)),
            street,
            postal_code: mk(self, window, cx, "settings.autofill.postalCode", v(|a| &a.postal_code)),
            city: mk(self, window, cx, "settings.autofill.city", v(|a| &a.city)),
            region: mk(self, window, cx, "settings.autofill.region", v(|a| &a.region)),
            country: mk(self, window, cx, "settings.autofill.country", v(|a| &a.country)),
            phone: mk(self, window, cx, "settings.autofill.phone", v(|a| &a.phone)),
            email: mk(self, window, cx, "settings.autofill.email", v(|a| &a.email)),
        };
        draft.given.update(cx, |s, cx| s.focus(window, cx));
        self.error = None;
        self.card = None;
        self.address = Some(draft);
        cx.notify();
    }

    fn open_card(&mut self, c: Option<&AutofillCard>, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = if c.is_some() { "settings.autofill.numberUnchanged" } else { "settings.autofill.number" };
        let draft = CardDraft {
            id: c.map(|c| c.id.clone()),
            holder: self.input(window, cx, "settings.autofill.holder", c.map(|c| c.holder.as_str()).unwrap_or(""), false),
            number: self.input(window, cx, placeholder, "", false),
            month: c.map(|c| c.exp_month.to_string()).unwrap_or_default(),
            year: c.map(|c| c.exp_year.to_string()).unwrap_or_default(),
            nickname: self.input(window, cx, "settings.autofill.nickname", c.map(|c| c.nickname.as_str()).unwrap_or(""), false),
        };
        draft.holder.update(cx, |s, cx| s.focus(window, cx));
        self.error = None;
        self.address = None;
        self.card = Some(draft);
        cx.notify();
    }

    fn close_forms(&mut self, cx: &mut Context<Self>) {
        self.address = None;
        self.card = None;
        self.error = None;
        cx.notify();
    }

    fn submit_address(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.address else { return };
        let input = AddressInput {
            given_name: kit::text_of(&d.given, cx),
            family_name: kit::text_of(&d.family, cx),
            organization: kit::text_of(&d.organization, cx),
            street: d.street.read(cx).value().to_string(),
            city: kit::text_of(&d.city, cx),
            postal_code: kit::text_of(&d.postal_code, cx),
            region: kit::text_of(&d.region, cx),
            country: kit::text_of(&d.country, cx),
            phone: kit::text_of(&d.phone, cx),
            email: kit::text_of(&d.email, cx),
        };
        let id = d.id.clone();
        match api(cx).save_address(id.as_deref(), input) {
            Some(e) => {
                self.error = Some(e);
                cx.notify();
            }
            None => {
                self.close_forms(cx);
                self.load(cx);
            }
        }
    }

    fn submit_card(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.card else { return };
        let number = kit::text_of(&d.number, cx);
        let input = CardInput {
            holder: kit::text_of(&d.holder, cx),
            number: if number.trim().is_empty() { None } else { Some(number.trim().to_string()) },
            exp_month: d.month.parse().unwrap_or(0),
            exp_year: d.year.parse().unwrap_or(0),
            nickname: kit::text_of(&d.nickname, cx),
        };
        let id = d.id.clone();
        match api(cx).save_card(id.as_deref(), input) {
            Some(e) => {
                self.error = Some(e);
                cx.notify();
            }
            None => {
                // The shown number may be the old one.
                if let Some(id) = id {
                    self.shown.remove(&id);
                }
                self.close_forms(cx);
                self.load(cx);
            }
        }
    }

    fn reveal(&mut self, c: &AutofillCard, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown.remove(&c.id).is_some() {
            cx.notify();
            return;
        }
        if let Some(number) = api(cx).reveal_card(&c.id) {
            self.shown.insert(c.id.clone(), group_card_number(&number, c.network));
            self.shown_gen += 1;
            let generation = self.shown_gen;
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_secs(30)).await;
                let _ = this.update(cx, |t, cx| {
                    if t.shown_gen == generation {
                        t.shown.clear();
                        cx.notify();
                    }
                });
            })
            .detach();
            cx.notify();
        }
    }
}

fn two(n: u32) -> String {
    format!("{n:02}")
}

impl Render for AutofillSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let info = info_of(&self.store, cx);
        let s = &info.settings;
        let addresses = self.data.as_ref().map(|d| d.addresses.clone()).unwrap_or_default();
        let cards = self.data.as_ref().map(|d| d.cards.clone()).unwrap_or_default();
        let encryption = self.data.as_ref().is_some_and(|d| d.encryption);
        let never = s.never_save_autofill.clone().unwrap_or_default();

        let switch_row = |id: &'static str, title: String, hint: String, on: bool, key: &'static str| {
            kit::row(cx)
                .child(kit::label(cx, title, Some(hint.into())))
                .child(kit::switch(id, on, move |v, _, cx| kit::update(cx, key, json!(v))))
                .into_any_element()
        };

        let mut col = kit::body().child(kit::group(
            cx,
            vec![
                switch_row("sw-addr", tr("settings.autofill.switchAddresses"), tr("settings.autofill.switchAddressesHint"), s.autofill_addresses != Some(false), "autofillAddresses"),
                switch_row("sw-cards", tr("settings.autofill.switchCards"), tr("settings.autofill.switchCardsHint"), s.autofill_cards != Some(false), "autofillCards"),
            ],
        ));

        // ---- addresses
        col = col.child(kit::sub(cx, tr("settings.autofill.addresses"))).child(kit::group(
            cx,
            vec![kit::row(cx)
                .child(div())
                .child(Btn::new("addr-add", tr("settings.autofill.addAddress")).icon(IconName::Plus).on_click(cx.listener(|this, _, w, cx| this.open_address(None, w, cx))))
                .into_any_element()],
        ));
        if let Some(d) = &self.address {
            let mut actions = kit::row(cx).justify_end();
            if let Some(e) = self.error.clone() {
                actions = actions.child(div().flex_1().child(kit::err(cx, e)));
            }
            let rows = vec![
                kit::row(cx).child(kit::field(&d.given)).child(kit::field(&d.family)).into_any_element(),
                kit::row(cx).child(kit::field(&d.organization)).into_any_element(),
                kit::row(cx)
                    .child(div().flex_1().min_w_0().h(px(72.0)).rounded(radius::CONTROL).bg(t.ctl).child(Textarea::new(&d.street).h(px(72.0)).appearance(false)))
                    .into_any_element(),
                kit::row(cx).child(kit::field(&d.postal_code)).child(kit::field(&d.city)).into_any_element(),
                kit::row(cx).child(kit::field(&d.region)).child(kit::field(&d.country)).into_any_element(),
                kit::row(cx).child(kit::field(&d.phone)).child(kit::field(&d.email)).into_any_element(),
                actions
                    .child(Btn::new("addr-cancel", tr("common.cancel")).on_click(cx.listener(|this, _, _, cx| this.close_forms(cx))))
                    .child(Btn::new("addr-save", tr("settings.autofill.save")).on_click(cx.listener(|this, _, _, cx| this.submit_address(cx))))
                    .into_any_element(),
            ];
            col = col.child(kit::group(cx, rows).id("addr-form").on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_forms(cx);
                }
            })));
        }
        if addresses.is_empty() && self.address.is_none() {
            col = col.child(kit::empty(cx, IconName::MapPin, vec![kit::para(tr("settings.autofill.emptyAddresses"))]));
        } else if !addresses.is_empty() {
            let rows = addresses
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let name = Some(full_name(&a.given_name, &a.family_name)).filter(|n| !n.is_empty()).unwrap_or_else(|| tr("settings.autofill.noName"));
                    let first_line = a.street.split('\n').next().unwrap_or("");
                    let hint = [first_line, a.postal_code.as_str(), a.city.as_str()].iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join(", ");
                    let mut row = kit::row(cx).child(kit::label(cx, name.clone(), Some(hint.into())));
                    if self.confirming.as_deref() == Some(a.id.as_str()) {
                        let (id, this) = (a.id.clone(), cx.entity());
                        row = row.child(
                            InlineConfirm::new(("addr-confirm", i), tr_args("settings.autofill.confirmDeleteAddress", &[("name", &name)]), tr("settings.autofill.delete"), tr("common.cancel"))
                                .on_cancel(kit::entity_cb(cx, |this, cx| {
                                    this.confirming = None;
                                    cx.notify();
                                }))
                                .on_confirm(move |_, cx| {
                                    this.update(cx, |t, cx| {
                                        t.confirming = None;
                                        api(cx).delete_address(&id);
                                        t.load(cx);
                                    });
                                }),
                        );
                    } else {
                        let (edit, del) = (a.clone(), a.id.clone());
                        row = row
                            .child(kit::icon_btn(("addr-edit", i), IconName::Pencil, tr("settings.autofill.edit"), cx.listener(move |this, _, w, cx| this.open_address(Some(&edit), w, cx))))
                            .child(kit::icon_btn(
                                ("addr-del", i),
                                IconName::Trash2,
                                tr("settings.autofill.delete"),
                                cx.listener(move |this, _, _, cx| {
                                    this.confirming = Some(del.clone());
                                    cx.notify();
                                }),
                            ));
                    }
                    row.into_any_element()
                })
                .collect();
            col = col.child(kit::group(cx, rows));
        }

        // ---- cards
        col = col.child(kit::sub(cx, tr("settings.autofill.cards")));
        if self.data.is_some() && !encryption {
            col = col.child(kit::note_err(cx, tr("settings.autofill.noEncryption")));
        }
        col = col.child(kit::group(
            cx,
            vec![kit::row(cx)
                .child(div())
                .child(
                    Btn::new("card-add", tr("settings.autofill.addCard"))
                        .icon(IconName::Plus)
                        .disabled(!encryption)
                        .on_click(cx.listener(|this, _, w, cx| this.open_card(None, w, cx))),
                )
                .into_any_element()],
        ));
        if let Some(d) = &self.card {
            let months: Vec<(String, String)> = (1..=12u32).map(|m| (m.to_string(), format!("{} ({})", two(m), month_label(m)))).collect();
            let first_year = chrono::Datelike::year(&chrono::Local::now());
            let years: Vec<(String, String)> = year_options(&d.year, first_year).into_iter().map(|y| (y.clone(), y)).collect();
            let month_now = d.month.parse::<u32>().ok().filter(|m| *m > 0).map(|m| m.to_string()).unwrap_or_default();
            let this = cx.entity();
            let this2 = this.clone();
            let mut actions = kit::row(cx).justify_end();
            if let Some(e) = self.error.clone() {
                actions = actions.child(div().flex_1().child(kit::err(cx, e)));
            }
            let rows = vec![
                kit::row(cx).child(kit::field(&d.holder)).child(kit::field(&d.number)).into_any_element(),
                kit::row(cx)
                    .child(kit::choice_ph("card-month", tr("settings.autofill.expMonth").into(), &month_now, months, move |v, _, cx| {
                        let v = v.to_string();
                        this.update(cx, |t, cx| {
                            if let Some(c) = &mut t.card {
                                c.month = v;
                            }
                            cx.notify();
                        })
                    }))
                    .child(kit::choice_ph("card-year", tr("settings.autofill.expYear").into(), &d.year, years, move |v, _, cx| {
                        let v = v.to_string();
                        this2.update(cx, |t, cx| {
                            if let Some(c) = &mut t.card {
                                c.year = v;
                            }
                            cx.notify();
                        })
                    }))
                    .child(kit::field(&d.nickname))
                    .into_any_element(),
                actions
                    .child(Btn::new("card-cancel", tr("common.cancel")).on_click(cx.listener(|this, _, _, cx| this.close_forms(cx))))
                    .child(Btn::new("card-save", tr("settings.autofill.save")).on_click(cx.listener(|this, _, _, cx| this.submit_card(cx))))
                    .into_any_element(),
            ];
            col = col.child(kit::group(cx, rows).id("card-form").on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_forms(cx);
                }
            })));
        }
        if cards.is_empty() && self.card.is_none() {
            col = col.child(kit::empty(cx, IconName::CreditCard, vec![kit::para(tr("settings.autofill.emptyCards"))]));
        } else if !cards.is_empty() {
            let rows = cards
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let network = if c.nickname.is_empty() { network_name(c.network).to_string() } else { c.nickname.clone() };
                    let network = if network.is_empty() { tr("settings.autofill.cards") } else { network };
                    let name = tr_args("settings.autofill.cardLabel", &[("network", &network), ("last4", &c.last4)]);
                    let date = format!("{}/{}", two(c.exp_month), c.exp_year);
                    let expires = tr_args("settings.autofill.expires", &[("date", &date)]);
                    let hint = [c.holder.as_str(), expires.as_str()].iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join(" \u{b7} ");
                    let mut row = kit::row(cx).child(kit::label(cx, name, Some(hint.into())));
                    if self.confirming.as_deref() == Some(c.id.as_str()) {
                        let (id, this) = (c.id.clone(), cx.entity());
                        row = row.child(
                            InlineConfirm::new(("card-confirm", i), tr_args("settings.autofill.confirmDeleteCard", &[("last4", &c.last4)]), tr("settings.autofill.delete"), tr("common.cancel"))
                                .on_cancel(kit::entity_cb(cx, |this, cx| {
                                    this.confirming = None;
                                    cx.notify();
                                }))
                                .on_confirm(move |_, cx| {
                                    this.update(cx, |t, cx| {
                                        t.confirming = None;
                                        api(cx).delete_card(&id);
                                        t.load(cx);
                                    });
                                }),
                        );
                    } else {
                        let revealed = self.shown.get(&c.id).cloned();
                        let is_shown = revealed.is_some();
                        let (a, b, del) = (c.clone(), c.clone(), c.id.clone());
                        row = row
                            .child(div().flex_none().font_family(FONT_MONO).text_size(t.type_scale.sub).child(revealed.unwrap_or_else(|| format!("{MASK}{}", c.last4))))
                            .child(kit::icon_btn(
                                ("card-eye", i),
                                if is_shown { IconName::EyeOff } else { IconName::Eye },
                                tr(if is_shown { "settings.autofill.hide" } else { "settings.autofill.show" }),
                                cx.listener(move |this, _, w, cx| this.reveal(&a, w, cx)),
                            ))
                            .child(kit::icon_btn(("card-edit", i), IconName::Pencil, tr("settings.autofill.edit"), cx.listener(move |this, _, w, cx| this.open_card(Some(&b), w, cx))))
                            .child(kit::icon_btn(
                                ("card-del", i),
                                IconName::Trash2,
                                tr("settings.autofill.delete"),
                                cx.listener(move |this, _, _, cx| {
                                    this.confirming = Some(del.clone());
                                    cx.notify();
                                }),
                            ));
                    }
                    row.into_any_element()
                })
                .collect();
            col = col.child(kit::group(cx, rows));
        }
        col = col.child(kit::note(cx, tr("settings.autofill.authNote")));

        if !never.is_empty() {
            let rows = never
                .iter()
                .enumerate()
                .map(|(i, origin)| {
                    let (origin, all) = (origin.clone(), never.clone());
                    kit::row(cx)
                        .child(kit::label(cx, kit::host_of(&origin), None))
                        .child(kit::icon_btn(("af-never-rm", i), IconName::Trash2, tr("settings.autofill.neverRemove"), move |_, _, cx| {
                            let next: Vec<&String> = all.iter().filter(|o| **o != origin).collect();
                            kit::update(cx, "neverSaveAutofill", json!(next));
                        }))
                        .into_any_element()
                })
                .collect();
            col = col.child(kit::sub(cx, tr("settings.autofill.never"))).child(kit::note(cx, tr("settings.autofill.neverHint"))).child(kit::group(cx, rows));
        }
        let _ = space::S1;
        col
    }
}

fn month_label(m: u32) -> String {
    let name = kit::month_name(m);
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_card_numbers() {
        assert_eq!(group_card_number("4111111111111111", CardNetwork::Visa), "4111 1111 1111 1111");
        assert_eq!(group_card_number("378282246310005", CardNetwork::Amex), "3782 822463 10005");
        assert_eq!(group_card_number("4111", CardNetwork::Visa), "4111");
        assert_eq!(group_card_number("41111", CardNetwork::Unknown), "4111 1");
    }

    #[test]
    fn joins_names() {
        assert_eq!(full_name(" Ada ", "Lovelace"), "Ada Lovelace");
        assert_eq!(full_name("", "Lovelace"), "Lovelace");
        assert_eq!(full_name("", " "), "");
    }

    #[test]
    fn keeps_the_cards_own_expired_year() {
        let years = year_options("2019", 2026);
        assert_eq!(years.len(), 17);
        assert_eq!(years[0], "2019");
        assert_eq!(year_options("2030", 2026).len(), 16);
        assert_eq!(year_options("", 2026)[0], "2026");
    }
}

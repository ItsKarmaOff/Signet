//! Autofill (port of `main/features/autofill/*` and `shared/autofill/*`): saved addresses and payment cards, field
//! classification, save offers. Cards are encrypted with a `Cipher` (OS keychain key) and released only after the
//! app authenticated the user (`osauth::AuthGate`); the CVC is never stored. Page IPC and prompts belong to the app.
//!
//! The page scripts (autofill, passwords) are owned by `signet-scripts`; this crate has the pure Rust logic.
pub mod card;
pub mod classify;
pub mod countries;
pub mod rules;
pub mod values;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::passwords::Cipher;
use crate::util::{clip, now_ms, write_atomic};
use card::*;
use rules::*;
use values::{full_name, split_name};

const MENU_TTL_MS: i64 = 10 * 60_000;
const MENU_MAX: usize = 8;
const MAX_NICK: usize = 40;
const OFFER_RATE_MS: i64 = 3000;

pub use signet_core::types::{AddressInput, AutofillAddress, AutofillCard, AutofillInfo};

/// Field access helpers for `AddressInput` (the type lives in `signet-core`).
pub trait AddressFields {
    fn fields(&self) -> [&str; 10];
    fn fields_mut(&mut self) -> [&mut String; 10];
}

impl AddressFields for AddressInput {
    fn fields(&self) -> [&str; 10] {
        [&self.given_name, &self.family_name, &self.organization, &self.street, &self.city, &self.postal_code, &self.region, &self.country, &self.phone, &self.email]
    }
    fn fields_mut(&mut self) -> [&mut String; 10] {
        [&mut self.given_name, &mut self.family_name, &mut self.organization, &mut self.street, &mut self.city, &mut self.postal_code, &mut self.region, &mut self.country, &mut self.phone, &mut self.email]
    }
}

/// Conversions between a saved address and its editable fields (core's address is flat).
pub trait AddressExt {
    fn from_input(id: &str, a: AddressInput, now: i64) -> AutofillAddress;
    fn input(&self) -> AddressInput;
    fn set_input(&mut self, a: AddressInput);
}

impl AddressExt for AutofillAddress {
    fn from_input(id: &str, a: AddressInput, now: i64) -> AutofillAddress {
        let mut s = AutofillAddress { id: id.into(), created_at: now, updated_at: now, ..Default::default() };
        s.set_input(a);
        s
    }
    fn input(&self) -> AddressInput {
        AddressInput {
            given_name: self.given_name.clone(),
            family_name: self.family_name.clone(),
            organization: self.organization.clone(),
            street: self.street.clone(),
            city: self.city.clone(),
            postal_code: self.postal_code.clone(),
            region: self.region.clone(),
            country: self.country.clone(),
            phone: self.phone.clone(),
            email: self.email.clone(),
        }
    }
    fn set_input(&mut self, a: AddressInput) {
        self.given_name = a.given_name;
        self.family_name = a.family_name;
        self.organization = a.organization;
        self.street = a.street;
        self.city = a.city;
        self.postal_code = a.postal_code;
        self.region = a.region;
        self.country = a.country;
        self.phone = a.phone;
        self.email = a.email;
    }
}

/// Card as written on disk: the number is encrypted (base64).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredCard {
    #[serde(flatten)]
    pub card: AutofillCard,
    pub number: String,
}

#[derive(Debug, Clone)]
pub struct CardInput {
    pub holder: String,
    /// `None` = unchanged.
    pub number: Option<String>,
    pub exp_month: i64,
    pub exp_year: i64,
    pub nickname: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub matcher: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuReply {
    pub kind: AutofillKind,
    pub items: Vec<MenuItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fill {
    Address(AddressInput),
    Card { network: CardNetwork, holder: String, number: String, exp_month: u32, exp_year: i64 },
}

/// Settings the rules depend on.
#[derive(Debug, Clone, Default)]
pub struct AutofillSettings {
    pub addresses_enabled: Option<bool>,
    pub cards_enabled: Option<bool>,
    pub never: Vec<String>,
}

impl AutofillSettings {
    fn enabled(&self, kind: AutofillKind) -> bool {
        (if kind == AutofillKind::Address { self.addresses_enabled } else { self.cards_enabled }) != Some(false)
    }
}

struct MenuState {
    kind: AutofillKind,
    tab_id: String,
    origin: String,
    ids: Vec<String>,
    at: i64,
}

#[derive(Debug, Clone)]
pub struct Offer {
    pub tab_id: String,
    pub origin: String,
    pub kind: AutofillKind,
    pub summary: String,
    pub update: bool,
    pub existing_id: Option<String>,
    pub address: Option<AddressInput>,
    pub card: Option<CardCandidate>,
}

#[derive(Deserialize)]
struct FileShape {
    #[serde(default)]
    addresses: Vec<Value>,
    #[serde(default)]
    cards: Vec<Value>,
}

/// Address read from disk: files written before the name was split carry a single `name`.
pub fn migrate_address(raw: &Value) -> Option<AutofillAddress> {
    let mut v = raw.clone();
    let o = v.as_object_mut()?;
    if let Some(Value::String(name)) = o.remove("name") {
        let has = |k: &str| o.get(k).and_then(|x| x.as_str()).is_some_and(|s| !s.is_empty());
        if !has("givenName") && !has("familyName") {
            let (g, f) = split_name(&name);
            o.insert("givenName".into(), g.into());
            o.insert("familyName".into(), f.into());
        }
    }
    serde_json::from_value(v).ok()
}

pub struct Autofill {
    file: PathBuf,
    cipher: Arc<dyn Cipher>,
    addresses: Vec<AutofillAddress>,
    cards: Vec<StoredCard>,
    blocked: bool,
    menus: HashMap<String, MenuState>,
    pending: Vec<(String, Offer)>,
    last_offer: HashMap<String, i64>,
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

impl Autofill {
    pub fn new(data_dir: &Path, cipher: Arc<dyn Cipher>) -> Self {
        let mut a = Autofill { file: data_dir.join("autofill.json"), cipher, addresses: vec![], cards: vec![], blocked: false, menus: HashMap::new(), pending: vec![], last_offer: HashMap::new() };
        a.load();
        a
    }

    fn load(&mut self) {
        match fs::read_to_string(&self.file) {
            Ok(raw) => match serde_json::from_str::<FileShape>(&raw) {
                Ok(f) => {
                    self.addresses = f.addresses.iter().filter_map(migrate_address).collect();
                    self.cards = f.cards.into_iter().filter_map(|c| serde_json::from_value::<StoredCard>(c).ok()).collect();
                }
                Err(_) => {
                    if fs::copy(&self.file, format!("{}.corrupt-{}", self.file.display(), now_ms())).is_err() {
                        self.blocked = true;
                    }
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => self.blocked = true,
        }
    }

    pub fn flush(&self) {
        if self.blocked {
            return;
        }
        let body = serde_json::json!({ "version": 1, "addresses": self.addresses, "cards": self.cards });
        let _ = write_atomic(&self.file, body.to_string().as_bytes());
    }

    pub fn encryption(&self) -> bool {
        self.cipher.available()
    }

    fn encrypt(&self, number: &str) -> Option<String> {
        self.cipher.encrypt(number).map(|b| b64().encode(b))
    }

    fn decrypt(&self, c: &StoredCard) -> Option<String> {
        self.cipher.decrypt(&b64().decode(&c.number).ok()?)
    }

    // ------------------------------------------------------------- settings

    pub fn list(&self) -> AutofillInfo {
        let mut addresses = self.addresses.clone();
        addresses.sort_by_key(|a| {
            let n = full_name(&a.input());
            if n.is_empty() { a.street.clone() } else { n }.to_lowercase()
        });
        let mut cards: Vec<AutofillCard> = self.cards.iter().map(|c| c.card.clone()).collect();
        cards.sort_by_key(|c| c.created_at);
        AutofillInfo { addresses, cards, encryption: self.encryption() }
    }

    /// Adds (no id) or edits an address; returns an error key (`autofill.error.*`) or `None`.
    pub fn upsert_address(&mut self, id: Option<&str>, input: &Value) -> Option<&'static str> {
        let typed = clean_address(input);
        if is_empty_address(&typed) {
            return Some("autofill.error.empty");
        }
        let existing = match id {
            Some(i) => match self.addresses.iter().position(|a| a.id == i) {
                Some(ix) => Some(ix),
                None => return Some("autofill.error.missing"),
            },
            None => None,
        };
        if self.addresses.iter().enumerate().any(|(i, a)| Some(i) != existing && same_address(&a.input(), &typed)) {
            return Some("autofill.error.duplicateAddress");
        }
        let now = now_ms();
        match existing {
            Some(ix) => {
                self.addresses[ix].set_input(typed);
                self.addresses[ix].updated_at = now;
            }
            None => self.addresses.push(AutofillAddress::from_input(&uuid::Uuid::new_v4().to_string(), typed, now)),
        }
        self.flush();
        None
    }

    pub fn remove_address(&mut self, id: &str) {
        let n = self.addresses.len();
        self.addresses.retain(|a| a.id != id);
        if self.addresses.len() != n {
            self.flush();
        }
    }

    /// Adds or edits a card. `authenticate` is called when a new number replaces an existing one.
    pub fn upsert_card(&mut self, id: Option<&str>, input: &CardInput, authenticate: &mut dyn FnMut() -> bool) -> Option<&'static str> {
        if !self.encryption() {
            return Some("autofill.error.encryption");
        }
        let existing = match id {
            Some(i) => match self.cards.iter().position(|c| c.card.id == i) {
                Some(ix) => Some(ix),
                None => return Some("autofill.error.missing"),
            },
            None => None,
        };
        let exp_year = full_year(input.exp_year);
        if !(1..=12).contains(&input.exp_month) || exp_year.is_none() {
            return Some("autofill.error.expiry");
        }
        let (exp_month, exp_year) = (input.exp_month as u32, exp_year.unwrap_or(0));
        let holder = clip(input.holder.trim(), 200);
        let nickname = clip(input.nickname.trim(), MAX_NICK);
        let typed = input.number.as_deref().filter(|n| !n.trim().is_empty());
        let mut digits = None;
        match typed {
            Some(t) => {
                let Some(d) = card_digits(t).filter(|d| luhn(d)) else { return Some("autofill.error.number") };
                if self.cards.iter().enumerate().any(|(i, c)| Some(i) != existing && self.decrypt(c).as_deref() == Some(d.as_str())) {
                    return Some("autofill.error.duplicateCard");
                }
                if existing.is_some() && !authenticate() {
                    return Some("autofill.error.auth");
                }
                digits = Some(d);
            }
            None if existing.is_none() => return Some("autofill.error.number"),
            None => {}
        }
        let now = now_ms();
        let enc = match &digits {
            Some(d) => match self.encrypt(d) {
                Some(e) => Some(e),
                None => return Some("autofill.error.encryption"),
            },
            None => None,
        };
        match existing {
            Some(ix) => {
                let c = &mut self.cards[ix];
                c.card.holder = holder;
                c.card.nickname = nickname;
                c.card.exp_month = exp_month;
                c.card.exp_year = exp_year as u32;
                c.card.updated_at = now;
                if let (Some(d), Some(e)) = (&digits, enc) {
                    c.number = e;
                    c.card.last4 = d[d.len() - 4..].to_string();
                    c.card.network = detect_network(d);
                }
            }
            None => {
                let (d, e) = (digits.unwrap_or_default(), enc.unwrap_or_default());
                self.cards.push(StoredCard {
                    card: AutofillCard { id: uuid::Uuid::new_v4().to_string(), holder, last4: d[d.len().saturating_sub(4)..].to_string(), network: detect_network(&d), exp_month, exp_year: exp_year as u32, nickname, created_at: now, updated_at: now, use_count: 0, last_used_at: None },
                    number: e,
                });
            }
        }
        self.flush();
        None
    }

    pub fn remove_card(&mut self, id: &str) {
        let n = self.cards.len();
        self.cards.retain(|c| c.card.id != id);
        if self.cards.len() != n {
            self.flush();
        }
    }

    /// Full card number for Settings. The app authenticates the user first.
    pub fn reveal_card(&self, id: &str) -> Option<String> {
        self.decrypt(self.cards.iter().find(|c| c.card.id == id)?)
    }

    // ------------------------------------------------------------- page side

    /// Items for the dropdown under a field; `None` when nothing should show. `origin` comes from the sending frame.
    pub fn menu(&mut self, key: &str, tab_id: &str, origin: &str, kind: AutofillKind, s: &AutofillSettings, t: crate::util::Translate) -> Option<MenuReply> {
        if !s.enabled(kind) || !can_fill(kind, Some(origin)) {
            return None;
        }
        let now = now_ms();
        let (ids, items): (Vec<String>, Vec<MenuItem>) = if kind == AutofillKind::Card {
            if !self.encryption() {
                return None;
            }
            let mut cards: Vec<&StoredCard> = self.cards.iter().collect();
            cards.sort_by(|a, b| by_usage((a.card.use_count, a.card.last_used_at, a.card.created_at), (b.card.use_count, b.card.last_used_at, b.card.created_at)));
            cards.truncate(MENU_MAX);
            cards
                .iter()
                .map(|c| {
                    let name = if !c.card.nickname.is_empty() { c.card.nickname.clone() } else if !network_name(c.card.network).is_empty() { network_name(c.card.network).into() } else { t("autofill.card", &[]) };
                    let label = format!("{name} \u{2022}\u{2022}\u{2022}\u{2022} {}", c.card.last4);
                    let detail = t("autofill.menu.expires", &[("date", format_expiry(c.card.exp_month, c.card.exp_year as i64, &ExpiryHint::default()))]);
                    (c.card.id.clone(), MenuItem { id: c.card.id.clone(), matcher: format!("{label} {}", c.card.holder).to_lowercase(), label, detail })
                })
                .unzip()
        } else {
            let mut list: Vec<&AutofillAddress> = self.addresses.iter().collect();
            list.sort_by(|a, b| by_usage((a.use_count, a.last_used_at, a.created_at), (b.use_count, b.last_used_at, b.created_at)));
            list.truncate(MENU_MAX);
            list.iter()
                .map(|a| {
                    let name = full_name(&a.input());
                    let first = a.street.split('\n').next().unwrap_or("").to_string();
                    let label = if name.is_empty() { first.clone() } else { name.clone() };
                    let detail = if name.is_empty() { String::new() } else { first };
                    (a.id.clone(), MenuItem { id: a.id.clone(), matcher: format!("{name} {} {}", a.street, a.city).to_lowercase(), label, detail })
                })
                .unzip()
        };
        if items.is_empty() {
            return None;
        }
        self.menus.insert(key.into(), MenuState { kind, tab_id: tab_id.into(), origin: origin.into(), ids, at: now });
        Some(MenuReply { kind, items })
    }

    fn owned_menu(&self, key: &str, origin: &str, kind: AutofillKind, id: &str) -> bool {
        self.menus.get(key).is_some_and(|m| m.kind == kind && m.origin == origin && m.ids.iter().any(|i| i == id) && now_ms() - m.at < MENU_TTL_MS)
    }

    /// Values for a chosen item. For a card, `authenticate` is called (the app shows the system prompt) and the
    /// menu is consumed; `still_same_frame` re-checks that the page did not move on meanwhile.
    #[allow(clippy::too_many_arguments)]
    pub fn fill(&mut self, key: &str, origin: &str, kind: AutofillKind, id: &str, is_private: bool, s: &AutofillSettings, authenticate: &mut dyn FnMut() -> bool, still_same_frame: &dyn Fn() -> bool) -> Option<Fill> {
        if !s.enabled(kind) || !can_fill(kind, Some(origin)) || !self.owned_menu(key, origin, kind, id) {
            return None;
        }
        let now = now_ms();
        if kind == AutofillKind::Address {
            let a = self.addresses.iter_mut().find(|x| x.id == id)?;
            if !is_private {
                a.use_count += 1;
                a.last_used_at = Some(now);
            }
            let out = a.input();
            if !is_private {
                self.flush();
            }
            return Some(Fill::Address(out));
        }
        let ix = self.cards.iter().position(|x| x.card.id == id)?;
        if !self.encryption() {
            return None;
        }
        let ok = authenticate();
        if !ok || !still_same_frame() {
            return None;
        }
        self.menus.remove(key);
        let number = self.decrypt(&self.cards[ix])?;
        if !is_private {
            self.cards[ix].card.use_count += 1;
            self.cards[ix].card.last_used_at = Some(now);
            self.flush();
        }
        let c = &self.cards[ix].card;
        Some(Fill::Card { network: c.network, holder: c.holder.clone(), number, exp_month: c.exp_month, exp_year: c.exp_year as i64 })
    }

    // ------------------------------------------------------------- save offers

    /// Form submitted in a tab. Returns the offers queued (tokens), the first one is to be shown.
    pub fn submitted(&mut self, tab_id: &str, origin: &str, is_private: bool, payload: &Value, s: &AutofillSettings, now_date: chrono::NaiveDate) -> Vec<String> {
        let enc = self.encryption();
        let mk = |kind, enabled| OfferContext { kind, origin, is_private, enabled, never: &s.never, encryption: enc };
        let mut offers: Vec<Offer> = Vec::new();
        if let Some(addr) = payload.get("address").filter(|a| a.is_object())
            && may_offer(&mk(AutofillKind::Address, s.addresses_enabled)) {
                let typed = clean_address(addr);
                match plan_address(&self.addresses, &typed) {
                    AddressPlan::Save => offers.push(Offer { tab_id: tab_id.into(), origin: origin.into(), kind: AutofillKind::Address, summary: address_summary(&typed), update: false, existing_id: None, address: Some(typed), card: None }),
                    AddressPlan::Update { id, merged } => offers.push(Offer { tab_id: tab_id.into(), origin: origin.into(), kind: AutofillKind::Address, summary: address_summary(&merged), update: true, existing_id: Some(id), address: Some(merged), card: None }),
                    AddressPlan::None => {}
                }
            }
        if let Some(c) = payload.get("card").filter(|c| c.is_object())
            && may_offer(&mk(AutofillKind::Card, s.cards_enabled))
                && let Some(card) = parse_card_submission(c, now_date) {
                    let refs: Vec<SavedCardRef> = self.cards.iter().map(|c| SavedCardRef { id: c.card.id.clone(), number: self.decrypt(c), holder: c.card.holder.clone(), exp_month: c.card.exp_month, exp_year: c.card.exp_year as i64 }).collect();
                    let plan = plan_card(&refs, &card);
                    if plan != CardPlan::None {
                        offers.push(Offer {
                            tab_id: tab_id.into(), origin: origin.into(), kind: AutofillKind::Card, summary: card_summary(&card),
                            update: matches!(plan, CardPlan::Update { .. }),
                            existing_id: if let CardPlan::Update { id } = &plan { Some(id.clone()) } else { None },
                            address: None, card: Some(card),
                        });
                    }
                }
        if offers.is_empty() {
            return vec![];
        }
        let now = now_ms();
        let rate_key = format!("{tab_id}\n{origin}");
        if now - self.last_offer.get(&rate_key).copied().unwrap_or(0) < OFFER_RATE_MS {
            return vec![];
        }
        self.last_offer.insert(rate_key, now);
        if self.last_offer.len() > 200 {
            self.last_offer.retain(|_, at| now - *at <= OFFER_RATE_MS);
        }
        self.pending.retain(|(_, p)| p.tab_id != tab_id);
        let mut tokens = Vec::new();
        for o in offers {
            let token = uuid::Uuid::new_v4().to_string();
            tokens.push(token.clone());
            self.pending.push((token, o));
        }
        tokens
    }

    pub fn offer(&self, token: &str) -> Option<&Offer> {
        self.pending.iter().find(|(t, _)| t == token).map(|(_, o)| o)
    }

    /// Next offer waiting for a tab.
    pub fn next_offer(&self, tab_id: &str) -> Option<(&str, &Offer)> {
        self.pending.iter().find(|(_, p)| p.tab_id == tab_id).map(|(t, o)| (t.as_str(), o))
    }

    /// Answer to a prompt (`save`, `never` or dismiss); `Some(origin)` when "never" must be recorded in settings.
    pub fn answer(&mut self, token: &str, save: bool, never: bool) -> Option<Option<String>> {
        let ix = self.pending.iter().position(|(t, _)| t == token)?;
        let (_, p) = self.pending.remove(ix);
        if save {
            if p.kind == AutofillKind::Address && p.address.is_some() {
                self.store_address(&p);
            } else if p.kind == AutofillKind::Card && p.card.is_some() && self.encryption() {
                self.store_card(&p);
            }
        } else if never {
            self.pending.retain(|(_, o)| !(o.tab_id == p.tab_id && o.origin == p.origin));
            return Some(Some(p.origin));
        }
        Some(None)
    }

    fn store_address(&mut self, p: &Offer) {
        let Some(typed) = p.address.clone() else { return };
        let now = now_ms();
        let ix = p.existing_id.as_ref().and_then(|id| self.addresses.iter().position(|a| &a.id == id)).or_else(|| self.addresses.iter().position(|a| same_address(&a.input(), &typed)));
        match ix {
            Some(ix) => {
                let a = &mut self.addresses[ix];
                a.set_input(typed);
                a.updated_at = now;
                a.last_used_at = Some(now);
            }
            None => {
                let mut a = AutofillAddress::from_input(&uuid::Uuid::new_v4().to_string(), typed, now);
                a.use_count = 1;
                a.last_used_at = Some(now);
                self.addresses.push(a);
            }
        }
        self.flush();
    }

    fn store_card(&mut self, p: &Offer) {
        let Some(c) = p.card.clone() else { return };
        let now = now_ms();
        let ix = p.existing_id.as_ref().and_then(|id| self.cards.iter().position(|x| &x.card.id == id)).or_else(|| self.cards.iter().position(|x| self.decrypt(x).as_deref() == Some(c.number.as_str())));
        match ix {
            Some(ix) => {
                let e = &mut self.cards[ix].card;
                if !c.holder.is_empty() {
                    e.holder = c.holder;
                }
                e.exp_month = c.exp_month;
                e.exp_year = c.exp_year as u32;
                e.updated_at = now;
                e.last_used_at = Some(now);
            }
            None => {
                let Some(number) = self.encrypt(&c.number) else { return };
                self.cards.push(StoredCard {
                    card: AutofillCard { id: uuid::Uuid::new_v4().to_string(), holder: c.holder, last4: c.last4, network: c.network, exp_month: c.exp_month, exp_year: c.exp_year as u32, nickname: String::new(), created_at: now, updated_at: now, use_count: 1, last_used_at: Some(now) },
                    number,
                });
            }
        }
        self.flush();
    }

    /// Tab navigation: dropdowns of the tab and offers about another site are forgotten. Returns dropped tokens.
    pub fn on_navigate(&mut self, tab_id: &str, url: &str, site_of: &dyn Fn(&str) -> String) -> Vec<String> {
        let site = site_of(url);
        self.menus.retain(|_, m| m.tab_id != tab_id);
        let gone: Vec<String> = self.pending.iter().filter(|(_, p)| p.tab_id == tab_id && site_of(&p.origin) != site).map(|(t, _)| t.clone()).collect();
        self.pending.retain(|(t, _)| !gone.contains(t));
        gone
    }
}

/// One line describing an address in a prompt.
pub fn address_summary(a: &AddressInput) -> String {
    [full_name(a), a.street.split('\n').next().unwrap_or("").to_string(), if a.city.is_empty() { a.postal_code.clone() } else { a.city.clone() }]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// One line describing a card in a prompt: never more than the last four digits.
pub fn card_summary(c: &CardCandidate) -> String {
    let name = if network_name(c.network).is_empty() { "Card" } else { network_name(c.network) };
    let year = c.exp_year.to_string();
    format!("{name} \u{2022}\u{2022}\u{2022}\u{2022} {}, {:02}/{}", c.last4, c.exp_month, &year[year.len().saturating_sub(2)..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fake(bool);
    impl Cipher for Fake {
        fn available(&self) -> bool { self.0 }
        fn encrypt(&self, p: &str) -> Option<Vec<u8>> { Some(format!("enc:{p}").into_bytes()) }
        fn decrypt(&self, b: &[u8]) -> Option<String> { String::from_utf8(b.to_vec()).ok().map(|s| s.trim_start_matches("enc:").to_string()) }
    }
    fn tr(k: &str, _: &[(&str, String)]) -> String { k.to_string() }
    fn make(d: &Path) -> Autofill { Autofill::new(d, Arc::new(Fake(true))) }
    fn date() -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap() }
    fn addr() -> Value { json!({"givenName":"Jeanne","familyName":"Dupont","street":"12 rue de la Paix","city":"Paris","country":"France"}) }
    fn card_in(number: Option<&str>) -> CardInput { CardInput { holder: "J".into(), number: number.map(String::from), exp_month: 8, exp_year: 27, nickname: " Perso ".into() } }

    #[test]
    fn address_crud_and_persistence() {
        let d = tempfile::tempdir().unwrap();
        let mut a = make(d.path());
        assert_eq!(a.upsert_address(None, &json!({})), Some("autofill.error.empty"));
        assert_eq!(a.upsert_address(None, &addr()), None);
        assert_eq!(a.upsert_address(None, &addr()), Some("autofill.error.duplicateAddress"));
        assert_eq!(a.upsert_address(Some("x"), &addr()), Some("autofill.error.missing"));
        let id = a.list().addresses[0].id.clone();
        assert_eq!(a.upsert_address(Some(&id), &json!({"givenName":"Jeanne","familyName":"Dupont","street":"12 rue de la Paix","city":"Lyon"})), None);
        let raw: Value = serde_json::from_str(&fs::read_to_string(d.path().join("autofill.json")).unwrap()).unwrap();
        assert_eq!(raw["version"], 1);
        assert_eq!(raw["addresses"][0]["city"], "Lyon");
        assert_eq!(raw["addresses"][0]["givenName"], "Jeanne");
        assert!(raw["addresses"][0].get("useCount").is_some());
        assert_eq!(make(d.path()).list().addresses[0].city, "Lyon");
        a.remove_address(&id);
        assert!(a.list().addresses.is_empty());
    }

    #[test]
    fn old_files_with_single_name_are_migrated() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("autofill.json"), r#"{"version":1,"addresses":[{"id":"1","name":"Jeanne Dupont","street":"x","useCount":2}],"cards":[]}"#).unwrap();
        let a = make(d.path());
        let l = a.list().addresses;
        assert_eq!((l[0].given_name.as_str(), l[0].family_name.as_str(), l[0].use_count), ("Jeanne", "Dupont", 2));
    }

    #[test]
    fn card_crud_and_auth() {
        let d = tempfile::tempdir().unwrap();
        let mut a = make(d.path());
        let mut yes = || true;
        assert_eq!(a.upsert_card(None, &card_in(None), &mut yes), Some("autofill.error.number"));
        assert_eq!(a.upsert_card(None, &card_in(Some("4242 4242 4242 4241")), &mut yes), Some("autofill.error.number"));
        assert_eq!(a.upsert_card(None, &CardInput { exp_month: 13, ..card_in(Some("4242424242424242")) }, &mut yes), Some("autofill.error.expiry"));
        assert_eq!(a.upsert_card(None, &card_in(Some("4242424242424242")), &mut yes), None);
        assert_eq!(a.upsert_card(None, &card_in(Some("4242424242424242")), &mut yes), Some("autofill.error.duplicateCard"));
        let info = a.list();
        assert_eq!((info.cards[0].last4.as_str(), info.cards[0].nickname.as_str(), info.cards[0].exp_year), ("4242", "Perso", 2027));
        let id = info.cards[0].id.clone();
        // New number on an existing card needs authentication.
        let mut no = || false;
        assert_eq!(a.upsert_card(Some(&id), &card_in(Some("5555555555554444")), &mut no), Some("autofill.error.auth"));
        assert_eq!(a.upsert_card(Some(&id), &card_in(Some("5555555555554444")), &mut yes), None);
        assert_eq!(a.reveal_card(&id).as_deref(), Some("5555555555554444"));
        // The file never holds the clear number.
        let raw = fs::read_to_string(d.path().join("autofill.json")).unwrap();
        assert!(!raw.contains("5555555555554444") && !raw.contains("\"4242424242424242\""));
        a.remove_card(&id);
        assert!(a.list().cards.is_empty());
        let mut off = Autofill::new(tempfile::tempdir().unwrap().path(), Arc::new(Fake(false)));
        assert_eq!(off.upsert_card(None, &card_in(Some("4242424242424242")), &mut yes), Some("autofill.error.encryption"));
    }

    #[test]
    fn menu_and_fill_guards() {
        let d = tempfile::tempdir().unwrap();
        let mut a = make(d.path());
        a.upsert_address(None, &addr());
        let s = AutofillSettings::default();
        assert!(a.menu("k", "t", "ftp://x", AutofillKind::Address, &s, &tr).is_none());
        let menu = a.menu("k", "t", "https://shop.example", AutofillKind::Address, &s, &tr).unwrap();
        assert_eq!(menu.items[0].label, "Jeanne Dupont");
        assert_eq!(menu.items[0].detail, "12 rue de la Paix");
        let id = menu.items[0].id.clone();
        let mut deny = || false;
        let same = || true;
        // Not the owner of the menu (other frame key or other origin).
        assert!(a.fill("other", "https://shop.example", AutofillKind::Address, &id, false, &s, &mut deny, &same).is_none());
        assert!(a.fill("k", "https://evil.example", AutofillKind::Address, &id, false, &s, &mut deny, &same).is_none());
        assert!(a.fill("k", "https://shop.example", AutofillKind::Address, "ghost", false, &s, &mut deny, &same).is_none());
        match a.fill("k", "https://shop.example", AutofillKind::Address, &id, true, &s, &mut deny, &same) {
            Some(Fill::Address(x)) => assert_eq!(x.city, "Paris"),
            other => panic!("{other:?}"),
        }
        assert_eq!(a.list().addresses[0].use_count, 0);
        a.fill("k", "https://shop.example", AutofillKind::Address, &id, false, &s, &mut deny, &same);
        assert_eq!(a.list().addresses[0].use_count, 1);
        let off = AutofillSettings { addresses_enabled: Some(false), ..Default::default() };
        assert!(a.menu("k2", "t", "https://shop.example", AutofillKind::Address, &off, &tr).is_none());
    }

    #[test]
    fn card_fill_needs_auth_and_same_frame() {
        let d = tempfile::tempdir().unwrap();
        let mut a = make(d.path());
        let mut yes = || true;
        a.upsert_card(None, &card_in(Some("4242424242424242")), &mut yes);
        let s = AutofillSettings::default();
        assert!(a.menu("k", "t", "http://shop.example", AutofillKind::Card, &s, &tr).is_none());
        let menu = a.menu("k", "t", "https://shop.example", AutofillKind::Card, &s, &tr).unwrap();
        assert!(menu.items[0].label.contains("Perso") && menu.items[0].label.ends_with("4242"));
        let id = menu.items[0].id.clone();
        let mut no = || false;
        assert!(a.fill("k", "https://shop.example", AutofillKind::Card, &id, false, &s, &mut no, &|| true).is_none());
        assert!(a.fill("k", "https://shop.example", AutofillKind::Card, &id, false, &s, &mut yes, &|| false).is_none());
        match a.fill("k", "https://shop.example", AutofillKind::Card, &id, false, &s, &mut yes, &|| true) {
            Some(Fill::Card { number, exp_month, .. }) => assert_eq!((number.as_str(), exp_month), ("4242424242424242", 8)),
            other => panic!("{other:?}"),
        }
        // The menu is consumed.
        assert!(a.fill("k", "https://shop.example", AutofillKind::Card, &id, false, &s, &mut yes, &|| true).is_none());
    }

    #[test]
    fn save_offers_flow() {
        let d = tempfile::tempdir().unwrap();
        let mut a = make(d.path());
        let s = AutofillSettings::default();
        let payload = json!({"address": addr(), "card": {"holder":"J","number":"4242424242424242","exp":"08/27"}});
        assert!(a.submitted("t", "https://shop.example", true, &payload, &s, date()).is_empty());
        let tokens = a.submitted("t", "https://shop.example", false, &payload, &s, date());
        assert_eq!(tokens.len(), 2);
        // Rate limit.
        assert!(a.submitted("t", "https://shop.example", false, &payload, &s, date()).is_empty());
        let (first, offer) = a.next_offer("t").unwrap();
        assert_eq!(offer.kind, AutofillKind::Address);
        assert_eq!(offer.summary, "Jeanne Dupont, 12 rue de la Paix, Paris");
        let first = first.to_string();
        assert_eq!(a.answer(&first, true, false), Some(None));
        assert_eq!(a.list().addresses.len(), 1);
        let (card_token, card_offer) = a.next_offer("t").unwrap();
        assert!(card_offer.summary.contains("4242") && !card_offer.summary.contains("4242424242424242"));
        let card_token = card_token.to_string();
        a.answer(&card_token, true, false);
        assert_eq!(a.list().cards.len(), 1);
        assert_eq!(a.answer("nope", true, false), None);
    }

    #[test]
    fn never_drops_the_siblings_and_navigation_forgets() {
        let d = tempfile::tempdir().unwrap();
        let mut a = make(d.path());
        let s = AutofillSettings::default();
        let payload = json!({"address": addr(), "card": {"number":"4242424242424242","exp":"08/27"}});
        let tokens = a.submitted("t", "https://shop.example", false, &payload, &s, date());
        assert_eq!(a.answer(&tokens[0], false, true), Some(Some("https://shop.example".to_string())));
        assert!(a.next_offer("t").is_none());
        let site = |u: &str| url::Url::parse(u).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_default();
        a.last_offer.clear();
        a.submitted("t", "https://shop.example", false, &payload, &s, date());
        assert!(a.on_navigate("t", "https://shop.example/next", &site).is_empty());
        assert_eq!(a.on_navigate("t", "https://other.example", &site).len(), 2);
    }
}

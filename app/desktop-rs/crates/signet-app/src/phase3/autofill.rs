//! PHASE3: autofill. Port of `main/features/autofill/autofill.ts` (main-process side) and `main/ipc/passwords.ts`
//! (autofill part), on top of `signet_features::autofill`. The vault shares the keychain cipher, the system
//! authentication gate, the worker-thread jobs and the overlay prompt card with the passwords module.

use chrono::Local;
use serde_json::Value;
use signet_core::api::PasswordAnswer;
use signet_core::i18n::{t, t_args};
use signet_core::ids::now_ms;
use signet_core::types::{AddressInput, AutofillInfo, AutofillKind as UiKind, CardInput};
use signet_features::autofill::rules::AutofillKind as RuleKind;
use signet_features::autofill::{Autofill, AutofillSettings, CardInput as CardNumberInput, Fill};
use signet_scripts::messages::{AddressFill, AutofillKind as PageKind, AutofillMenu, CardFill, MenuItem};
use signet_scripts::{Accepted, Reply};

use super::pages::PageCtx;
use super::passwords::{authenticate, enum_name as logic_enum_name, Job, NEXT_OFFER_MS};
use crate::browser::Core;

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    vault: Option<Autofill>,
    /// Next offer of a tab to show once the overlay has left: (tab, due time).
    next_offer: Option<(String, i64)>,
}

fn rule_kind(k: PageKind) -> RuleKind {
    match k {
        PageKind::Address => RuleKind::Address,
        PageKind::Card => RuleKind::Card,
    }
}

fn ui_kind(k: RuleKind) -> UiKind {
    match k {
        RuleKind::Address => UiKind::Address,
        RuleKind::Card => UiKind::Card,
    }
}

/// Key identifying the frame that opened a dropdown (only that frame may pick from it).
fn frame_key(ctx: &PageCtx) -> String {
    format!("{}:{}", ctx.tab, ctx.frame_id)
}

fn translate(key: &str, args: &[(&str, String)]) -> String {
    let a: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, v.as_str())).collect();
    t_args(key, &a)
}

impl Core {
    pub(super) fn af_vault(&mut self) -> &mut Autofill {
        let cipher = self.phase3.passwords.cipher.clone();
        let dir = self.data_dir.clone();
        self.phase3.autofill.vault.get_or_insert_with(|| Autofill::new(&dir, cipher))
    }

    fn af_settings(&self) -> AutofillSettings {
        let s = &self.state().settings;
        AutofillSettings { addresses_enabled: s.autofill_addresses, cards_enabled: s.autofill_cards, never: s.never_save_autofill.clone().unwrap_or_default() }
    }

    /// Tab navigation: dropdowns and offers about another site are forgotten. Returns the dropped offer tokens.
    pub(super) fn autofill_navigated(&mut self, tab: &str, url: &str) -> Vec<String> {
        self.af_vault().on_navigate(tab, url, &|u| signet_core::site::site_of(u))
    }

    /// Answer to a prompt card: false when the token is not an autofill offer.
    pub(super) fn autofill_answer(&mut self, token: &str, answer: PasswordAnswer) -> bool {
        let (save, never) = (answer == PasswordAnswer::Save, answer == PasswordAnswer::Never);
        let Some(never_origin) = self.af_vault().answer(token, save, never) else { return false };
        if let Some(origin) = never_origin {
            let s = &mut self.store.data.state.settings;
            let mut list = s.never_save_autofill.take().unwrap_or_default();
            if !list.contains(&origin) {
                list.push(origin);
            }
            s.never_save_autofill = Some(list);
            self.changed();
        }
        let shown_tab = self.phase3.passwords.shown.as_ref().filter(|s| s.token == token).map(|s| s.tab.clone());
        if let Some(tab) = shown_tab {
            self.close_prompt();
            // The next offer of the tab (a card after an address) once the overlay has left.
            self.phase3.autofill.next_offer = Some((tab, now_ms() + NEXT_OFFER_MS));
        }
        true
    }

    /// Shows the next offer waiting for a tab. An offer that cannot be shown now (background tab, busy overlay) is dropped.
    fn show_next_offer(&mut self, tab: &str) {
        if self.phase3.passwords.shown.is_some() {
            return;
        }
        let Some((token, offer)) = self.af_vault().next_offer(tab).map(|(t, o)| (t.to_string(), o.clone())) else { return };
        if !self.open_prompt(tab, &token, &offer.origin, &offer.summary, offer.update, Some(ui_kind(offer.kind)), None) {
            self.af_vault().answer(&token, false, false);
        }
    }

    // ----------------------------------------------------------------------------------- Settings API

    pub fn list_autofill(&mut self) -> AutofillInfo {
        self.af_vault().list()
    }

    pub fn save_address(&mut self, id: Option<&str>, address: AddressInput) -> Option<String> {
        let value = serde_json::to_value(&address).unwrap_or(Value::Null);
        self.af_vault().upsert_address(id, &value).map(t)
    }

    pub fn delete_address(&mut self, id: &str) {
        self.af_vault().remove_address(id);
    }

    /// A new number for an existing card needs the user's authentication (blocks until answered).
    pub fn save_card(&mut self, id: Option<&str>, card: CardInput) -> Option<String> {
        let gate = self.phase3.passwords.gate.clone();
        let input = CardNumberInput { holder: card.holder, number: card.number, exp_month: i64::from(card.exp_month), exp_year: i64::from(card.exp_year), nickname: card.nickname };
        self.af_vault().upsert_card(id, &input, &mut || authenticate(&gate, &t("autofill.authEdit"))).map(t)
    }

    pub fn delete_card(&mut self, id: &str) {
        self.af_vault().remove_card(id);
    }

    /// Full card number, after the user's authentication (blocks until answered).
    pub fn reveal_card(&mut self, id: &str) -> Option<String> {
        if !self.af_vault().list().cards.iter().any(|c| c.id == id) {
            return None;
        }
        let gate = self.phase3.passwords.gate.clone();
        if !authenticate(&gate, &t("autofill.authReveal")) {
            return None;
        }
        self.af_vault().reveal_card(id)
    }

    // ----------------------------------------------------------------------------------- page channels

    /// Page channels `af:menu`, `af:fill`, `af:manage`, `af:submit` (accepted by `signet_scripts::validate`). Answer requests with `self.page_reply(ctx, ..)`.
    pub fn page_autofill(&mut self, ctx: &PageCtx, accepted: signet_scripts::Accepted) {
        match accepted {
            Accepted::AfMenu { origin, kind } => {
                let (s, key) = (self.af_settings(), frame_key(ctx));
                let rk = rule_kind(kind);
                let menu = self.af_vault().menu(&key, &ctx.tab, &origin, rk, &s, &translate).map(|m| {
                    let (title, manage) = match rk {
                        RuleKind::Card => ("autofill.menu.cardsTitle", "autofill.menu.manageCards"),
                        RuleKind::Address => ("autofill.menu.addressesTitle", "autofill.menu.manageAddresses"),
                    };
                    AutofillMenu {
                        kind,
                        title: t(title),
                        manage: t(manage),
                        items: m.items.into_iter().map(|i| MenuItem { id: i.id, label: i.label, detail: i.detail, matching: i.matcher }).collect(),
                    }
                });
                self.af_reply(ctx, &menu);
            }
            Accepted::AfFill { origin, kind, id } => match rule_kind(kind) {
                RuleKind::Address => {
                    let (s, key) = (self.af_settings(), frame_key(ctx));
                    let fill = self.af_vault().fill(&key, &origin, RuleKind::Address, &id, ctx.sender.is_private, &s, &mut || false, &|| true);
                    self.af_reply_fill(ctx, fill);
                }
                RuleKind::Card => self.af_fill_card(ctx, origin, id),
            },
            Accepted::AfManage { .. } => self.open_settings_window(Some("autofill")),
            Accepted::AfSubmit { origin, payload } => {
                let s = self.af_settings();
                let tokens = self.af_vault().submitted(&ctx.tab, &origin, ctx.sender.is_private, &Value::Object(payload), &s, Local::now().date_naive());
                if !tokens.is_empty() {
                    self.show_next_offer(&ctx.tab);
                }
            }
            _ => {}
        }
    }

    fn af_reply<T: serde::Serialize>(&mut self, ctx: &PageCtx, data: &T) {
        if let Some(rid) = ctx.rid.as_deref() {
            self.page_reply(ctx, Reply::ok(rid, data));
        }
    }

    fn af_reply_fill(&mut self, ctx: &PageCtx, fill: Option<Fill>) {
        match fill {
            Some(Fill::Address(a)) => match serde_json::to_value(&a) {
                Ok(Value::Object(address)) => self.af_reply(ctx, &Some(AddressFill { address })),
                _ => self.af_reply(ctx, &Option::<AddressFill>::None),
            },
            Some(Fill::Card { network, holder, number, exp_month, exp_year }) => {
                let card = CardFill { network: logic_enum_name(&network), holder, number, exp_month, exp_year: exp_year.max(0) as u32 };
                self.af_reply(ctx, &Some(card));
            }
            None => self.af_reply(ctx, &Option::<CardFill>::None),
        }
    }

    /// A card leaves the vault only after the user's authentication, asked on a worker thread so the interface stays
    /// alive; the page is checked again when it is done.
    fn af_fill_card(&mut self, ctx: &PageCtx, origin: String, id: String) {
        let (s, key) = (self.af_settings(), frame_key(ctx));
        if self.phase3.passwords.auth_busy {
            return self.af_reply_fill(ctx, None);
        }
        // Dry run: every check of `fill` up to the authentication, which says no.
        let mut needs_auth = false;
        self.af_vault().fill(&key, &origin, RuleKind::Card, &id, ctx.sender.is_private, &s, &mut || { needs_auth = true; false }, &|| true);
        if !needs_auth {
            return self.af_reply_fill(ctx, None);
        }
        self.phase3.passwords.auth_busy = true;
        let (gate, jobs) = (self.phase3.passwords.gate.clone(), self.phase3.passwords.jobs.clone());
        let reason = t_args("autofill.authCard", &[("site", origin.split_once("://").map_or(origin.as_str(), |x| x.1))]);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let ok = authenticate(&gate, &reason);
            let job: Job = Box::new(move |core: &mut Core| core.af_finish_card(&ctx, key, origin, id, ok));
            jobs.push(job);
        });
    }

    fn af_finish_card(&mut self, ctx: &PageCtx, key: String, origin: String, id: String, authenticated: bool) {
        self.phase3.passwords.auth_busy = false;
        if !authenticated {
            return self.af_reply_fill(ctx, None);
        }
        // The page may have moved on while the system prompt was open: same origin, same menu (navigation drops it).
        let same = ctx.sender.is_private || self.state().tabs.get(&ctx.tab).and_then(|t| signet_scripts::validate::origin_of_url(&t.url)).as_deref() == Some(origin.as_str());
        let s = self.af_settings();
        let fill = self.af_vault().fill(&key, &origin, RuleKind::Card, &id, ctx.sender.is_private, &s, &mut || true, &|| same);
        self.af_reply_fill(ctx, fill);
    }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(_core: &mut Core) {}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(_core: &mut Core, _ev: &signet_web::WebEvent) {}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    if let Some((tab, due)) = core.phase3.autofill.next_offer.clone()
        && now_ms() >= due
    {
        core.phase3.autofill.next_offer = None;
        core.show_next_offer(&tab);
    }
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    if let Some(v) = &core.phase3.autofill.vault {
        v.flush();
    }
}

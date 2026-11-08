//! Phone link and passkeys (`signet-link`): port of the main process side of `features/link/link.ts`,
//! `web/webauthn.ts` and `windows/passkey.ts`.
//!
//! - `LinkManager` (own runtime) starts at `startup`. Its events are read on the main thread by `pump`, called from
//!   `tick` and from the CEF events (there is no wake-up channel from other threads into `Core`).
//! - Private keys of the links are sealed in the OS keychain (`vault`).
//! - Passkeys: see `passkey`.

mod convert;
mod passkey;
mod vault;

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use signet_core::api::UiEvent;
use signet_core::model::OpenOptions;
use signet_core::types::LinkState;
use signet_link::{LinkConfig, LinkEvent, LinkManager};

use crate::browser::Core;
use crate::windows::{Cmd, Entry};

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    manager: Option<LinkManager>,
    events: Option<Receiver<LinkEvent>>,
    /// Last tab list given to the manager.
    tabs: Vec<(String, String)>,
    /// Last state pushed to the interface.
    last: Option<LinkState>,
    passkey: passkey::State,
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    if std::env::var_os("SIGNET_LINK_OFF").is_some() {
        core.log("link: disabled by SIGNET_LINK_OFF");
        return;
    }
    let mut config = LinkConfig::new(core.data_dir.clone());
    config.vault = Some(Box::new(vault::KeychainVault::new(Arc::new(signet_features::secrets::KeyringStore))));
    let (manager, events) = LinkManager::start(config);
    core.phase3.link.manager = Some(manager);
    core.phase3.link.events = Some(events);
    core.link_sync_tabs();
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &signet_web::WebEvent) {
    use signet_web::WebEvent::*;
    core.passkey_web_event(ev);
    match ev {
        // Titles and addresses feed the dev server list of the phone.
        TitleChanged { .. } | AddressChanged { .. } | Closed { .. } | Created { .. } => core.link_sync_tabs(),
        _ => {}
    }
    core.link_pump();
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    core.link_sync_tabs();
    core.link_pump();
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    core.passkey_shutdown();
    core.phase3.link.events = None;
    if let Some(m) = core.phase3.link.manager.take() {
        m.shutdown();
    }
}

impl Core {
    /// State for the interface (default if the link could not start).
    pub fn get_link_state(&mut self) -> LinkState {
        self.phase3.link.manager.as_ref().map(|m| convert::link_state(&m.state())).unwrap_or_default()
    }

    pub fn set_link_general(&mut self, on: bool) {
        if let Some(m) = &self.phase3.link.manager {
            m.set_general(on);
        }
        self.link_push();
    }

    pub fn set_link_device(&mut self, link_id: &str, on: bool) {
        if let Some(m) = &self.phase3.link.manager {
            m.set_device(link_id, on);
        }
        self.link_push();
    }

    pub fn forget_link(&mut self, link_id: &str) {
        if let Some(m) = &self.phase3.link.manager {
            m.forget(link_id);
        }
        self.link_push();
    }

    pub fn start_link_pairing(&mut self) -> bool {
        let ok = self.phase3.link.manager.as_ref().is_some_and(|m| m.start_pairing());
        self.link_push();
        ok
    }

    pub fn cancel_link_pairing(&mut self) {
        if let Some(m) = &self.phase3.link.manager {
            m.cancel_pairing();
        }
        self.link_push();
    }

    /// Sends the state to the interface if it differs from the last one sent.
    fn link_push(&mut self) {
        let state = self.get_link_state();
        if self.phase3.link.last.as_ref() == Some(&state) {
            return;
        }
        self.phase3.link.last = Some(state.clone());
        self.emit(UiEvent::LinkState(Box::new(state)));
    }

    /// Gives the manager the open tabs when they changed.
    fn link_sync_tabs(&mut self) {
        let list = convert::tab_list(self.state().tabs.values().map(|t| (t.url.as_str(), t.title.as_str())));
        if list == self.phase3.link.tabs {
            return;
        }
        if let Some(m) = &self.phase3.link.manager {
            m.set_tabs(convert::to_tab_likes(&list));
        }
        self.phase3.link.tabs = list;
    }

    /// Reads the events of the manager and of the passkey session (main thread).
    fn link_pump(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.phase3.link.events {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }
        for ev in events {
            match ev {
                LinkEvent::Changed(_) => self.link_push(),
                LinkEvent::OpenRequested { url, device_name, .. } => {
                    self.log(format!("link: {device_name} asks to open {url}"));
                    if convert::phone_url_ok(&url) {
                        self.send(Cmd::ShowMain);
                        self.open_tab(&url, OpenOptions { activate: true, ..Default::default() });
                    }
                }
            }
        }
        self.passkey_pump();
    }

    // ------------------------------------------------------------------ open on phone

    /// Phones a page can be sent to right now: (link id, device name).
    pub fn phone_targets(&self) -> Vec<(String, String)> {
        self.phase3.link.manager.as_ref().map(|m| m.targets()).unwrap_or_default()
    }

    fn phone_url(&self, tab: Option<&str>) -> Option<String> {
        let url = &self.state().tabs.get(tab?)?.url;
        LinkManager::url_for_phone(url)
    }

    /// A phone is active and the page of this tab can be sent to it.
    pub fn can_open_on_phone(&self, tab: Option<&str>) -> bool {
        !self.phone_targets().is_empty() && self.phone_url(tab).is_some()
    }

    /// Sends the page of a tab to a phone (`link_id`: None = the first one).
    pub fn open_on_phone(&mut self, tab: Option<&str>, link_id: Option<&str>) -> bool {
        let Some(url) = self.phone_url(tab) else { return false };
        self.phase3.link.manager.as_ref().is_some_and(|m| m.open_on_phone(&url, link_id))
    }

    /// Command bar action: one phone receives the page at once, several ask which one in a menu.
    pub fn open_on_phone_choose(&mut self, tab: Option<&str>) {
        let targets = self.phone_targets();
        match targets.len() {
            0 => {}
            1 => {
                self.open_on_phone(tab, Some(&targets[0].0));
            }
            _ => {
                let entries = targets.iter().map(|(id, name)| phone_item(name, &format!("phone:{id}"))).collect();
                let tab = tab.map(str::to_string);
                let at = (self.content_area.x + 12.0, self.content_area.y + 8.0);
                self.send(Cmd::Popup {
                    entries,
                    at,
                    cb: Box::new(move |c: &mut Core, choice: &str| {
                        c.link_menu_run(tab.as_deref(), choice);
                    }),
                });
            }
        }
    }

    /// "Open on my phone" entry for the tab menu (empty if no phone is active or the page cannot be sent).
    /// Ids: `phone` (one phone) or `phone:<linkId>` (submenu); give the chosen id to `link_menu_run`.
    pub fn link_menu_entries(&self, tab: Option<&str>) -> Vec<Entry> {
        if self.phone_url(tab).is_none() {
            return Vec::new();
        }
        phone_entries(&signet_core::i18n::t("menu.tab.openOnPhone"), &self.phone_targets())
    }

    /// Runs an id of `link_menu_entries`. False if the id is not one of ours.
    pub fn link_menu_run(&mut self, tab: Option<&str>, choice: &str) -> bool {
        match choice {
            "phone" => {
                self.open_on_phone(tab, None);
                true
            }
            other => match other.strip_prefix("phone:") {
                Some(id) => {
                    self.open_on_phone(tab, Some(id));
                    true
                }
                None => false,
            },
        }
    }
}

fn phone_item(label: &str, id: &str) -> Entry {
    Entry::Item { label: label.to_string(), id: id.to_string(), enabled: true, checked: false }
}

/// One phone: a plain item; several: a submenu with a device per item.
fn phone_entries(label: &str, targets: &[(String, String)]) -> Vec<Entry> {
    match targets {
        [] => Vec::new(),
        [_] => vec![phone_item(label, "phone")],
        many => vec![Entry::Sub { label: label.to_string(), items: many.iter().map(|(id, name)| phone_item(name, &format!("phone:{id}"))).collect() }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, name: &str) -> (String, String) {
        (id.to_string(), name.to_string())
    }

    #[test]
    fn menu_entry_by_number_of_phones() {
        assert!(phone_entries("Open", &[]).is_empty());
        match phone_entries("Open", &[t("a", "Pixel")]).as_slice() {
            [Entry::Item { id, label, .. }] => assert_eq!((id.as_str(), label.as_str()), ("phone", "Open")),
            _ => panic!("one item"),
        }
        match phone_entries("Open", &[t("a", "Pixel"), t("b", "iPhone")]).as_slice() {
            [Entry::Sub { items, .. }] => match items.as_slice() {
                [Entry::Item { id, .. }, Entry::Item { id: id2, label, .. }] => assert_eq!((id.as_str(), id2.as_str(), label.as_str()), ("phone:a", "phone:b", "iPhone")),
                _ => panic!("two items"),
            },
            _ => panic!("one submenu"),
        }
    }
}

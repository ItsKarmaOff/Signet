//! The extension methods of `SignetApi`, the Web Store fetcher and the extension entries of the page context menu.

use std::path::Path;
use std::time::Duration;

use signet_core::model::OpenOptions;
use signet_core::types::{ExtensionActionCounts, ExtensionActionStates, ExtensionInfo, Rect};
use signet_extensions::contextmenus::{MenuNode, MenuTarget, MenuType};
use signet_extensions::crx::Fetcher;
use signet_features::http::{Http, UreqHttp};
use signet_web::ContextMenuInfo;

use super::bridge::tab_number;
use super::effects::counts_to_ui;
use super::shared::Inbox;
use crate::browser::Core;
use crate::windows::Entry;

const STORE_URL: &str = "https://chromewebstore.google.com/";

/// HTTP GET for the Web Store downloads (blocking: used from worker threads).
pub struct HttpFetcher;

impl Fetcher for HttpFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let r = UreqHttp.get(url, &[("User-Agent", &signet_web::default_user_agent())], Duration::from_secs(90), 300 * 1024 * 1024)?;
        if r.ok() { Ok(r.body) } else { Err(format!("HTTP {}", r.status)) }
    }
}

fn tr(key: &str) -> String {
    signet_core::i18n::t(key)
}

fn item(label: impl Into<String>, id: impl Into<String>, checked: bool) -> Entry {
    Entry::Item { label: label.into(), id: id.into(), enabled: true, checked }
}

/// Entries of an extension's menu nodes (`contextMenus`), ids `<prefix><json item id>`.
fn nodes_to_entries(nodes: &[MenuNode], prefix: &str) -> Vec<Entry> {
    nodes
        .iter()
        .map(|n| {
            if n.item.kind == MenuType::Separator {
                return Entry::Sep;
            }
            if !n.children.is_empty() {
                return Entry::Sub { label: n.label.clone(), items: nodes_to_entries(&n.children, prefix) };
            }
            let checked = matches!(n.item.kind, MenuType::Checkbox | MenuType::Radio) && n.item.checked;
            Entry::Item { label: n.label.clone(), id: format!("{prefix}{}", n.item.id), enabled: n.item.enabled, checked }
        })
        .collect()
}

fn target_of(info: &ContextMenuInfo) -> MenuTarget {
    let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
    let (src, media) = match (&info.image_url, &info.media_url) {
        (Some(i), _) => (Some(i.clone()), Some("image".to_string())),
        (None, Some(m)) => (Some(m.clone()), Some("video".to_string())),
        _ => (None, None),
    };
    MenuTarget { page_url: some(&info.page_url), frame_url: some(&info.frame_url), link_url: info.link_url.clone(), src_url: src, selection_text: some(&info.selection_text), media_type: media, is_editable: info.editable }
}

impl Core {
    pub fn open_extension_store(&mut self) {
        self.open_tab(STORE_URL, OpenOptions { activate: true, after_tab_id: None, opener_id: None });
    }

    pub fn remove_extension(&mut self, id: &str) {
        let Some(shared) = self.ext_shared() else { return };
        let name = shared.host().infos().into_iter().find(|i| i.id == id).map(|i| i.name).unwrap_or_else(|| id.to_string());
        let (title, detail) = (signet_core::i18n::t_args("dialog.removeExtension.message", &[("name", &name)]), tr("dialog.removeExtension.detail"));
        let (ok, cancel) = (tr("dialog.removeExtension.confirm"), tr("dialog.cancel"));
        let id = id.to_string();
        std::thread::spawn(move || {
            if super::dialogs::confirm("Signet", &format!("{title}\n\n{detail}"), &ok, &cancel) {
                shared.push(Inbox::RemoveConfirmed(id));
            }
        });
    }

    pub fn load_unpacked_extension(&mut self) {
        let Some(shared) = self.ext_shared() else { return };
        let title = tr("unpacked.pickTitle");
        std::thread::spawn(move || {
            let path = super::dialogs::pick_folder(&title);
            shared.push(Inbox::Unpacked(path));
        });
    }

    /// Self-test: loads an unpacked extension without the folder picker.
    pub fn selftest_load_unpacked(&mut self, path: &str) {
        if let Some(shared) = self.ext_shared() {
            shared.push(Inbox::Unpacked(Some(std::path::PathBuf::from(path))));
        }
    }

    pub fn reload_unpacked_extension(&mut self, path: &str) {
        let Some(shared) = self.ext_shared() else { return };
        let fx = shared.host().reload_unpacked(Path::new(path));
        self.ext_apply(fx);
    }

    pub fn remove_unpacked_extension(&mut self, path: &str) {
        let Some(shared) = self.ext_shared() else { return };
        let fx = shared.host().remove_unpacked(Path::new(path));
        self.ext_apply(fx);
    }

    /// Right click menu of a toolbar button.
    pub fn show_extension_menu(&mut self, id: &str) {
        let Some(shared) = self.ext_shared() else { return };
        let (options, pinned, nodes) = {
            let host = shared.host();
            (host.registry.get(id).is_some_and(|i| i.manifest.options_page.is_some()), host.pinned().iter().any(|p| p == id), host.button_menu_items(id))
        };
        let mut entries = Vec::new();
        if options {
            entries.push(item(tr("menu.extension.options"), "options", false));
        }
        entries.extend(nodes_to_entries(&nodes, "node:"));
        entries.push(item(if pinned { tr("menu.extension.hide") } else { tr("menu.extension.pin") }, "pin", false));
        entries.push(Entry::Sep);
        entries.push(item(tr("menu.extension.manage"), "manage", false));
        entries.push(item(tr("menu.extension.remove"), "remove", false));
        let anchor = self.phase3.extensions.rt.as_ref().and_then(|r| r.anchors.get(id).copied());
        let at = anchor.map(|a| (a.x, a.y + a.height)).unwrap_or_else(|| (self.win.as_ref().map(|w| w.size.0 - 140.0).unwrap_or(600.0), 44.0));
        let ext = id.to_string();
        self.popup(entries, at, move |core, picked| {
            let Some(shared) = core.ext_shared() else { return };
            match picked {
                "options" => {
                    let fx = vec![signet_extensions::host::Effect::OpenOptions { extension_id: ext }];
                    core.ext_apply(fx);
                }
                "pin" => {
                    let on = !shared.host().pinned().iter().any(|p| *p == ext);
                    let fx = shared.host().set_pinned(&ext, on);
                    core.ext_apply(fx);
                }
                "manage" => core.open_settings_window(Some("extensions")),
                "remove" => core.remove_extension(&ext),
                node if node.starts_with("node:") => core.ext_menu_node(&ext, &node["node:".len()..], &MenuTarget::default(), None),
                _ => {}
            }
        });
    }

    /// Menu of the Extensions button. The choice is carried out here (the popup opens under the last known button
    /// position), so nothing is returned to the caller.
    pub fn show_extensions_menu(&mut self) -> Option<String> {
        let shared = self.ext_shared()?;
        let rows: Vec<(String, String, bool)> = {
            let mut host = shared.host();
            let infos = host.infos();
            let ids = host.with_button();
            ids.into_iter().map(|id| (infos.iter().find(|i| i.id == id).map(|i| i.name.clone()).unwrap_or_else(|| id.clone()), id.clone(), host.pinned().iter().any(|p| *p == id))).collect()
        };
        let mut entries = Vec::new();
        if rows.is_empty() {
            entries.push(Entry::Item { label: tr("menu.extension.none"), id: String::new(), enabled: false, checked: false });
        } else {
            for (name, id, _) in &rows {
                entries.push(item(name.clone(), format!("open:{id}"), false));
            }
            entries.push(Entry::Sep);
            entries.push(Entry::Sub { label: tr("menu.extension.pinned"), items: rows.iter().map(|(name, id, pinned)| item(name.clone(), format!("pin:{id}"), *pinned)).collect() });
        }
        entries.push(Entry::Sep);
        entries.push(item(tr("menu.extension.manage"), "manage", false));
        let at = (self.win.as_ref().map(|w| w.size.0 - 160.0).unwrap_or(600.0), 44.0);
        self.popup(entries, at, |core, picked| {
            let Some(shared) = core.ext_shared() else { return };
            if let Some(id) = picked.strip_prefix("open:") {
                let anchor = core.phase3.extensions.rt.as_ref().and_then(|r| r.anchors.get(id).copied()).unwrap_or_else(|| Rect::new(core.win.as_ref().map(|w| w.size.0 - 120.0).unwrap_or(600.0), 40.0, 28.0, 28.0));
                core.ext_activate(id, anchor);
            } else if let Some(id) = picked.strip_prefix("pin:") {
                let on = !shared.host().pinned().iter().any(|p| p == id);
                let fx = shared.host().set_pinned(id, on);
                core.ext_apply(fx);
            } else if picked == "manage" {
                core.open_settings_window(Some("extensions"));
            }
        });
        None
    }

    pub fn get_extension_action_counts(&mut self) -> ExtensionActionCounts {
        match self.ext_shared() {
            Some(s) => counts_to_ui(&s.host().action_counts()),
            None => Default::default(),
        }
    }

    pub fn get_extension_action_states(&mut self) -> ExtensionActionStates {
        self.phase3.extensions.rt.as_ref().map(|r| r.states.clone()).unwrap_or_default()
    }

    pub fn ack_extension_action(&mut self, token: &str) -> bool {
        let now = signet_core::ids::now_ms() as u64;
        self.phase3.extensions.rt.as_mut().is_some_and(|r| r.relay.ack(token, now))
    }

    /// Installed extensions for the Settings list (`ExtensionInfo` of the settings snapshot).
    pub fn extension_infos(&mut self) -> Vec<ExtensionInfo> {
        let Some(shared) = self.ext_shared() else { return vec![] };
        let infos = shared.host().infos();
        serde_json::to_value(infos).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
    }

    /// Entries that extensions add to the page context menu (ids start with `extctx:`).
    pub fn extension_context_entries(&mut self, info: &ContextMenuInfo) -> Vec<Entry> {
        let Some(shared) = self.ext_shared() else { return vec![] };
        let groups = shared.host().menu_items(&target_of(info));
        let mut out = Vec::new();
        for (ext, name, nodes) in groups {
            let prefix = format!("extctx:{ext}:");
            let entries = nodes_to_entries(&nodes, &prefix);
            if groups_len_one(&entries) {
                out.extend(entries);
            } else {
                out.push(Entry::Sub { label: name, items: entries });
            }
        }
        out
    }

    /// Runs a choice made in the page context menu. False when `id` is not an extension entry.
    pub fn extension_context_run(&mut self, id: &str, tab: &str, info: &ContextMenuInfo) -> bool {
        let Some(rest) = id.strip_prefix("extctx:") else { return false };
        let Some((ext, item_id)) = rest.split_once(':') else { return false };
        let snap = super::tabhost::snapshot(self, Default::default());
        let tab_json = snap.tabs.iter().find(|t| t.id == tab_number(tab)).map(|t| t.to_chrome_tab(true));
        self.ext_menu_node(ext, item_id, &target_of(info), tab_json);
        true
    }

    fn ext_menu_node(&mut self, ext: &str, item_id: &str, target: &MenuTarget, tab: Option<serde_json::Value>) {
        let Some(shared) = self.ext_shared() else { return };
        // Ids are JSON (a string or a number), as the extension gave them.
        let id = serde_json::from_str(item_id).unwrap_or_else(|_| serde_json::Value::String(item_id.to_string()));
        let fx = shared.host().menu_clicked(ext, &id, target, tab);
        self.ext_apply(fx);
    }
}

/// A single plain entry needs no submenu.
fn groups_len_one(entries: &[Entry]) -> bool {
    entries.len() == 1 && matches!(entries[0], Entry::Item { .. })
}

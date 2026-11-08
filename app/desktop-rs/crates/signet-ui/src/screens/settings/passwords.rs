//! Settings > Passwords (port of `PasswordSettings.tsx`): manager choice, Signet password vault, never-saved sites.

use std::collections::HashMap;
use std::time::Duration;

use gpui::{Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::input::InputState;
use serde_json::{Value, json};
use signet_core::types::{PASSWORD_EXTENSIONS, PasswordImportError, PasswordManager, PasswordsInfo, SavedLogin};

use super::kit::{self, Btn};
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::theme::{FONT_MONO, ThemeExt, space};
use crate::widgets::InlineConfirm;

const MASK: &str = "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}";

/// Settings value of a manager (`signet`, `bitwarden`, `1password`...).
pub fn manager_id(m: PasswordManager) -> String {
    match serde_json::to_value(m) {
        Ok(Value::String(s)) => s,
        _ => "signet".into(),
    }
}

struct Draft {
    id: Option<String>,
    origin: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
}

pub struct PasswordSettings {
    store: Entity<InfoStore>,
    data: Option<PasswordsInfo>,
    load_failed: bool,
    query: Entity<InputState>,
    shown: HashMap<String, String>,
    shown_gen: u64,
    copied: Option<String>,
    confirming: Option<String>,
    draft: Option<Draft>,
    error: Option<String>,
    notice: Option<(String, bool)>,
    installing: bool,
}

impl PasswordSettings {
    pub fn new(store: Entity<InfoStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        // The list is not part of `SettingsInfo` (never any passwords in the pushed state): re-read when returning
        // to the window. Revealed passwords are hidden when the window loses focus.
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.load(cx);
            } else if !this.shown.is_empty() {
                this.shown.clear();
                cx.notify();
            }
        })
        .detach();
        let query = kit::input(window, cx, tr("settings.passwords.search"), "", false, None);
        let mut me = Self {
            store,
            data: None,
            load_failed: false,
            query,
            shown: HashMap::new(),
            shown_gen: 0,
            copied: None,
            confirming: None,
            draft: None,
            error: None,
            notice: None,
            installing: false,
        };
        me.load(cx);
        me
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.shown.clear();
        self.data = Some(api(cx).list_passwords());
        self.load_failed = false;
        cx.notify();
    }

    fn reveal(&mut self, login: &SavedLogin, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown.remove(&login.id).is_some() {
            cx.notify();
            return;
        }
        if let Some(password) = api(cx).reveal_password(&login.id) {
            self.shown.insert(login.id.clone(), password);
            // Hidden again after 30 s.
            self.shown_gen += 1;
            let generation = self.shown_gen;
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(30)).await;
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

    fn copy(&mut self, login: &SavedLogin, window: &mut Window, cx: &mut Context<Self>) {
        if api(cx).copy_password(&login.id) {
            let id = login.id.clone();
            self.copied = Some(id.clone());
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(1500)).await;
                let _ = this.update(cx, |t, cx| {
                    if t.copied.as_ref() == Some(&id) {
                        t.copied = None;
                        cx.notify();
                    }
                });
            })
            .detach();
            cx.notify();
        }
    }

    fn open_draft(&mut self, login: Option<&SavedLogin>, window: &mut Window, cx: &mut Context<Self>) {
        let origin = kit::input(window, cx, tr("settings.passwords.sitePlaceholder"), login.map(|l| l.origin.clone()).unwrap_or_default(), false, Some(Self::submit));
        let username = kit::input(window, cx, tr("settings.passwords.username"), login.map(|l| l.username.clone()).unwrap_or_default(), false, Some(Self::submit));
        let placeholder = if login.is_some() { tr("settings.passwords.passwordUnchanged") } else { tr("settings.passwords.password") };
        let password = kit::input(window, cx, placeholder, "", true, Some(Self::submit));
        origin.update(cx, |s, cx| s.focus(window, cx));
        self.error = None;
        self.draft = Some(Draft { id: login.map(|l| l.id.clone()), origin, username, password });
        cx.notify();
    }

    fn close_draft(&mut self, cx: &mut Context<Self>) {
        self.draft = None;
        self.error = None;
        cx.notify();
    }

    fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &self.draft else { return };
        let (origin, username, password) = (kit::text_of(&draft.origin, cx), kit::text_of(&draft.username, cx), kit::text_of(&draft.password, cx));
        let id = draft.id.clone();
        let err = api(cx).save_password(id.as_deref(), &origin, &username, if password.is_empty() { None } else { Some(&password) });
        if err.is_some() {
            self.error = err;
            cx.notify();
            return;
        }
        self.draft = None;
        self.error = None;
        self.load(cx);
    }

    fn run_import(&mut self, cx: &mut Context<Self>) {
        let Some(res) = api(cx).import_passwords() else { return };
        let message = match res.error {
            Some(e) => {
                let key = match e {
                    PasswordImportError::Format => "format",
                    PasswordImportError::Read => "read",
                    PasswordImportError::Encryption => "encryption",
                };
                tr(&format!("settings.passwords.importError.{key}"))
            }
            None if res.existing > 0 => {
                tr_args("settings.passwords.importedSome", &[("added", &res.added.to_string()), ("existing", &res.existing.to_string())])
            }
            None if res.added > 0 => tr_args("settings.passwords.imported", &[("count", &res.added.to_string())]),
            None => tr("settings.passwords.importNothing"),
        };
        self.notice = Some((message, res.error.is_some()));
        self.load(cx);
    }

    fn run_export(&mut self, cx: &mut Context<Self>) {
        if !api(cx).export_passwords() {
            return;
        }
        self.notice = Some((tr("settings.passwords.exported"), false));
        cx.notify();
    }
}

impl Render for PasswordSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let info = info_of(&self.store, cx);
        let settings = &info.settings;
        let manager = settings.password_manager.unwrap_or_default();
        let ext = PASSWORD_EXTENSIONS.iter().find(|m| m.id == manager);
        let installed = ext.is_some_and(|e| info.extensions.iter().any(|x| x.id == e.extension_id));
        let logins: Vec<SavedLogin> = self.data.as_ref().map(|d| d.logins.clone()).unwrap_or_default();
        let encryption = self.data.as_ref().is_some_and(|d| d.encryption);
        let q = kit::text_of(&self.query, cx).trim().to_lowercase();
        let visible: Vec<&SavedLogin> =
            logins.iter().filter(|l| q.is_empty() || l.origin.to_lowercase().contains(&q) || l.username.to_lowercase().contains(&q)).collect();
        let no_user = tr("settings.passwords.noUsername");

        let mut options = vec![(manager_id(PasswordManager::Signet), tr("settings.passwords.managerSignet"))];
        options.extend(PASSWORD_EXTENSIONS.iter().map(|m| (manager_id(m.id), m.name.to_string())));
        options.push((manager_id(PasswordManager::Off), tr("settings.passwords.managerOff")));
        let hint = match manager {
            PasswordManager::Signet => tr("settings.passwords.managerSignetHint"),
            PasswordManager::Off => tr("settings.passwords.managerOffHint"),
            _ => tr_args("settings.passwords.managerExtHint", &[("name", ext.map(|e| e.name).unwrap_or(""))]),
        };
        let mut top_rows = vec![kit::row(cx)
            .child(kit::label(cx, tr("settings.passwords.manager"), Some(hint.into())))
            .child(kit::choice("pw-manager", &manager_id(manager), options, |v, _, cx| kit::update(cx, "passwordManager", json!(v))))
            .into_any_element()];
        if let Some(ext) = ext {
            let control = if installed {
                kit::ok(cx, tr("settings.passwords.installed")).into_any_element()
            } else {
                Btn::new("pw-install", if self.installing { tr("settings.passwords.installing") } else { tr("settings.passwords.install") })
                    .disabled(self.installing)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.installing = true;
                        api(cx).install_password_extension(manager);
                        this.installing = false;
                        cx.notify();
                    }))
                    .into_any_element()
            };
            top_rows.push(
                kit::row(cx)
                    .child(kit::label(
                        cx,
                        tr_args("settings.passwords.extension", &[("name", ext.name)]),
                        Some(tr(if installed { "settings.passwords.extensionInstalledHint" } else { "settings.passwords.extensionMissingHint" }).into()),
                    ))
                    .child(control)
                    .into_any_element(),
            );
        }
        if manager == PasswordManager::Signet {
            top_rows.push(
                kit::row(cx)
                    .child(kit::label(cx, tr("settings.passwords.offer"), Some(tr("settings.passwords.offerHint").into())))
                    .child(kit::switch("pw-offer", settings.offer_save_passwords != Some(false), |v, _, cx| kit::update(cx, "offerSavePasswords", json!(v))))
                    .into_any_element(),
            );
        }

        let mut col = kit::body().child(kit::group(cx, top_rows));
        if self.data.is_some() && !encryption {
            col = col.child(kit::note_err(cx, tr("settings.passwords.noEncryption")));
        }

        if manager == PasswordManager::Signet || !logins.is_empty() {
            col = col.child(kit::sub(cx, tr("settings.passwords.saved")));
            if manager != PasswordManager::Signet {
                col = col.child(kit::note(cx, tr_args("settings.passwords.leftover", &[("count", &logins.len().to_string())])));
            }
            let mut toolbar = vec![kit::row(cx)
                .child(kit::field(&self.query))
                .child(Btn::new("pw-add", tr("settings.passwords.add")).icon(IconName::Plus).disabled(!encryption).on_click(cx.listener(|this, _, w, cx| this.open_draft(None, w, cx))))
                .child(Btn::new("pw-import", tr("settings.passwords.import")).icon(IconName::Download).disabled(!encryption).on_click(cx.listener(|this, _, _, cx| this.run_import(cx))))
                .child(Btn::new("pw-export", tr("settings.passwords.export")).icon(IconName::Upload).disabled(logins.is_empty()).on_click(cx.listener(|this, _, _, cx| this.run_export(cx))))
                .into_any_element()];
            if let Some((message, failed)) = self.notice.clone() {
                toolbar.push(
                    kit::row(cx)
                        .child(div().flex_1().child(if failed { kit::err(cx, message) } else { kit::ok(cx, message) }))
                        .child(kit::icon_btn("pw-notice-close", IconName::X, tr("common.close"), cx.listener(|this, _, _, cx| {
                            this.notice = None;
                            cx.notify();
                        })).small())
                        .into_any_element(),
                );
            }
            col = col.child(kit::group(cx, toolbar));

            if let Some(draft) = &self.draft {
                let mut form = vec![kit::row(cx).child(kit::field(&draft.origin)).child(kit::field(&draft.username)).child(kit::field(&draft.password)).into_any_element()];
                let mut actions = kit::row(cx).justify_end();
                if let Some(e) = self.error.clone() {
                    actions = actions.child(div().flex_1().child(kit::err(cx, e)));
                }
                form.push(
                    actions
                        .child(Btn::new("pw-cancel", tr("common.cancel")).on_click(cx.listener(|this, _, _, cx| this.close_draft(cx))))
                        .child(Btn::new("pw-save", tr("settings.passwords.saveLogin")).on_click(cx.listener(|this, _, w, cx| this.submit(w, cx))))
                        .into_any_element(),
                );
                col = col.child(kit::group(cx, form).id("pw-form").on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.close_draft(cx);
                    }
                })));
            }

            if self.load_failed && self.data.is_none() {
                col = col.child(kit::empty(
                    cx,
                    IconName::KeyRound,
                    vec![
                        kit::para(tr("settings.passwords.loadFailed")),
                        Btn::new("pw-retry", tr("common.retry")).on_click(cx.listener(|this, _, _, cx| this.load(cx))).into_any_element(),
                    ],
                ));
            } else if logins.is_empty() {
                col = col.child(kit::empty(cx, IconName::KeyRound, vec![kit::para(tr("settings.passwords.empty"))]));
            } else if visible.is_empty() {
                col = col.child(kit::note(cx, tr("settings.passwords.noMatch")));
            } else {
                let rows = visible
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        let login = (*l).clone();
                        let user = if l.username.is_empty() { no_user.clone() } else { l.username.clone() };
                        let site = kit::host_of(&l.origin);
                        let mut row = kit::row(cx).child(kit::label(cx, site.clone(), Some(user.clone().into())));
                        if self.confirming.as_deref() == Some(l.id.as_str()) {
                            let (id, this) = (l.id.clone(), cx.entity());
                            row = row.child(
                                InlineConfirm::new(("pw-confirm", i), tr_args("settings.passwords.confirmDelete", &[("site", &site), ("user", &user)]), tr("settings.passwords.delete"), tr("common.cancel"))
                                    .on_cancel(kit::entity_cb(cx, |this, cx| {
                                        this.confirming = None;
                                        cx.notify();
                                    }))
                                    .on_confirm(move |_, cx| {
                                        this.update(cx, |t, cx| {
                                            t.confirming = None;
                                            cx.notify();
                                        });
                                        api(cx).delete_password(&id);
                                        this.update(cx, |t, cx| t.load(cx));
                                    }),
                            );
                        } else {
                            let revealed = self.shown.get(&l.id).cloned();
                            let is_shown = revealed.is_some();
                            let (l1, l2, l3) = (login.clone(), login.clone(), login.clone());
                            let delete_id = l.id.clone();
                            row = row
                                .child(div().flex_none().font_family(FONT_MONO).text_size(t.type_scale.sub).child(revealed.unwrap_or_else(|| MASK.to_string())))
                                .child(kit::icon_btn(
                                    ("pw-eye", i),
                                    if is_shown { IconName::EyeOff } else { IconName::Eye },
                                    tr(if is_shown { "settings.passwords.hide" } else { "settings.passwords.show" }),
                                    cx.listener(move |this, _, w, cx| this.reveal(&l1, w, cx)),
                                ))
                                .child(kit::icon_btn(
                                    ("pw-copy", i),
                                    if self.copied.as_deref() == Some(l.id.as_str()) { IconName::Check } else { IconName::Copy },
                                    tr("settings.passwords.copy"),
                                    cx.listener(move |this, _, w, cx| this.copy(&l2, w, cx)),
                                ))
                                .child(kit::icon_btn(("pw-edit", i), IconName::Pencil, tr("settings.passwords.edit"), cx.listener(move |this, _, w, cx| this.open_draft(Some(&l3), w, cx))))
                                .child(kit::icon_btn(
                                    ("pw-del", i),
                                    IconName::Trash2,
                                    tr("settings.passwords.delete"),
                                    cx.listener(move |this, _, _, cx| {
                                        this.confirming = Some(delete_id.clone());
                                        cx.notify();
                                    }),
                                ));
                        }
                        row.into_any_element()
                    })
                    .collect();
                col = col.child(kit::group(cx, rows));
            }
            if self.data.is_some() {
                col = col.child(kit::note(cx, tr("settings.passwords.touchIdNote")));
            }
        }

        let never = settings.never_save_passwords.clone().unwrap_or_default();
        if !never.is_empty() {
            let rows = never
                .iter()
                .enumerate()
                .map(|(i, origin)| {
                    let (origin, all) = (origin.clone(), never.clone());
                    kit::row(cx)
                        .child(kit::label(cx, kit::host_of(&origin), None))
                        .child(kit::icon_btn(("pw-never-rm", i), IconName::Trash2, tr("settings.passwords.neverRemove"), move |_, _, cx| {
                            let next: Vec<&String> = all.iter().filter(|o| **o != origin).collect();
                            kit::update(cx, "neverSavePasswords", json!(next));
                        }))
                        .into_any_element()
                })
                .collect();
            col = col.child(kit::sub(cx, tr("settings.passwords.never"))).child(kit::group(cx, rows));
        }
        let _ = space::S1;
        col
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_ids_match_the_settings_values() {
        assert_eq!(manager_id(PasswordManager::Signet), "signet");
        assert_eq!(manager_id(PasswordManager::OnePassword), "1password");
        assert_eq!(manager_id(PasswordManager::LastPass), "lastpass");
        assert_eq!(manager_id(PasswordManager::Off), "off");
    }
}

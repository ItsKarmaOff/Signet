//! Settings > Backup (port of `BackupSettings.tsx`): export, import, restore a backup.

use gpui::{Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder};
use gpui_component::input::InputState;
use signet_core::api::BackupOpen;
use signet_core::types::{BACKUP_SECTIONS, BackupInfo, BackupMode, BackupSection, BackupSummary};

use super::kit::{self, Btn};
use crate::app_state::{api, tr, tr_args};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};

/// Minimum length of the backup password (same rules as the app).
pub const MIN_PASSWORD: usize = 6;

/// Key of a section in the catalog (`backup.section.<key>`).
pub fn section_key(s: BackupSection) -> String {
    match serde_json::to_value(s) {
        Ok(serde_json::Value::String(k)) => k,
        _ => String::new(),
    }
}

/// Sections that can be picked: those with a count.
fn available(counts: &[(BackupSection, i64)]) -> Vec<BackupSection> {
    BACKUP_SECTIONS.iter().copied().filter(|s| counts.iter().any(|(k, _)| k == s)).collect()
}

fn count_list<'a>(counts: impl Iterator<Item = (&'a BackupSection, &'a i64)>) -> Vec<(BackupSection, i64)> {
    counts.map(|(k, v)| (*k, *v)).collect()
}

/// Label of a section: no count for settings, nor when the count is unknown before decryption.
pub fn section_label(s: BackupSection, n: Option<i64>) -> String {
    let name = tr(&format!("backup.section.{}", section_key(s)));
    match n {
        Some(n) if s != BackupSection::Settings && n >= 0 => tr_args("backup.withCount", &[("name", &name), ("count", &n.to_string())]),
        _ => name,
    }
}

/// Password rules of an export: too short, or the confirmation differs.
pub fn password_error(with_passwords: bool, pw: &str, pw2: &str) -> Option<&'static str> {
    if with_passwords && !pw.is_empty() && pw.chars().count() < MIN_PASSWORD {
        Some("backup.passwordShort")
    } else if with_passwords && !pw2.is_empty() && pw != pw2 {
        Some("backup.passwordMismatch")
    } else {
        None
    }
}

fn toggle(list: &mut Vec<BackupSection>, s: BackupSection) {
    if let Some(i) = list.iter().position(|x| *x == s) {
        list.remove(i);
    } else {
        list.push(s);
    }
}

pub struct BackupSettings {
    info: Option<BackupInfo>,
    // Export
    picked: Vec<BackupSection>,
    pw: Entity<InputState>,
    pw2: Entity<InputState>,
    exporting: bool,
    export_msg: Option<(bool, String)>,
    // Import
    file: Option<BackupSummary>,
    import_picked: Vec<BackupSection>,
    mode: BackupMode,
    import_pw: Entity<InputState>,
    importing: bool,
    import_err: Option<String>,
    from_safety: bool,
}

impl BackupSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        kit::watch_locale(cx);
        let pw = kit::input(window, cx, tr("backup.passwordPlaceholder"), "", true, None);
        let pw2 = kit::input(window, cx, tr("backup.passwordConfirm"), "", true, None);
        let import_pw = kit::input(window, cx, tr("backup.passwordPlaceholder"), "", true, None);
        Self {
            info: Some(api(cx).get_backup_info()),
            picked: BACKUP_SECTIONS.iter().copied().filter(|s| *s != BackupSection::Passwords).collect(),
            pw,
            pw2,
            exporting: false,
            export_msg: None,
            file: None,
            import_picked: Vec::new(),
            mode: BackupMode::Add,
            import_pw,
            importing: false,
            import_err: None,
            from_safety: false,
        }
    }

    fn do_export(&mut self, cx: &mut Context<Self>) {
        self.exporting = true;
        self.export_msg = None;
        let with_passwords = self.picked.contains(&BackupSection::Passwords);
        let pw = kit::text_of(&self.pw, cx);
        let err = api(cx).export_backup(&self.picked, if with_passwords { Some(&pw) } else { None });
        self.export_msg = Some(match err {
            Some(e) => (false, e),
            None => (true, tr("backup.exported")),
        });
        self.exporting = false;
        cx.notify();
    }

    /// File (or backup) read: all its parts checked, "add" mode by default ("replace" for a safety backup).
    fn loaded(&mut self, res: Option<Result<BackupSummary, String>>, safety: bool, cx: &mut Context<Self>) {
        match res {
            None => return,
            Some(Err(message)) => {
                self.import_err = Some(message);
                self.file = None;
            }
            Some(Ok(summary)) => {
                self.import_err = None;
                self.import_picked = available(&count_list(summary.counts.iter()));
                self.file = Some(summary);
                self.mode = if safety { BackupMode::Replace } else { BackupMode::Add };
                self.from_safety = safety;
            }
        }
        cx.notify();
    }

    fn choose_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let res = match api(cx).open_backup() {
            BackupOpen::Summary { summary } => Some(Ok(summary)),
            BackupOpen::Error { message } => Some(Err(message)),
            BackupOpen::Cancelled => None,
        };
        kit::set_text(&self.import_pw, "", window, cx);
        self.loaded(res, false, cx);
    }

    fn open_safety(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let res = api(cx).open_safety_backup(name);
        kit::set_text(&self.import_pw, "", window, cx);
        self.loaded(Some(res), true, cx);
    }

    fn do_import(&mut self, cx: &mut Context<Self>) {
        let Some(file) = self.file.clone() else { return };
        self.importing = true;
        self.import_err = None;
        let needs_pw = !file.system_sealed.unwrap_or(false) && self.import_picked.contains(&BackupSection::Passwords);
        let pw = kit::text_of(&self.import_pw, cx);
        // On success, the app restarts.
        if let Some(e) = api(cx).apply_backup(&file.token, &self.import_picked, self.mode, if needs_pw { Some(&pw) } else { None }) {
            self.import_err = Some(e);
        }
        self.importing = false;
        cx.notify();
    }

    fn section_list(&self, cx: &Context<Self>, id: &'static str, counts: &[(BackupSection, i64)], picked: &[BackupSection], on_toggle: fn(&mut Self, Option<BackupSection>, &[BackupSection], &mut Context<Self>)) -> gpui::AnyElement {
        let avail = available(counts);
        let all = !avail.is_empty() && avail.iter().all(|s| picked.contains(s));
        let partial = !picked.is_empty() && !all;
        let this = cx.entity();
        let avail_all = avail.clone();
        let mut rows = vec![kit::check_row_w(cx, (id, 0usize), all, partial, tr("backup.selectAll"), None, true, {
            let this = this.clone();
            move |_, cx| {
                let avail = avail_all.clone();
                this.update(cx, |t, cx| on_toggle(t, None, &avail, cx));
            }
        })];
        for (i, s) in avail.iter().enumerate() {
            let s = *s;
            let hint = match s {
                BackupSection::Settings => Some(tr("backup.settingsHint")),
                BackupSection::Extensions => Some(tr("backup.extensionsHint")),
                _ => None,
            };
            let this = this.clone();
            rows.push(kit::check_row(
                cx,
                (id, i + 1),
                picked.contains(&s),
                false,
                section_label(s, counts.iter().find(|(k, _)| *k == s).map(|(_, v)| *v)),
                hint.map(Into::into),
                move |_, cx| this.update(cx, |t, cx| on_toggle(t, Some(s), &[], cx)),
            ));
        }
        kit::group(cx, rows).into_any_element()
    }
}

fn export_toggle(t: &mut BackupSettings, one: Option<BackupSection>, avail: &[BackupSection], cx: &mut Context<BackupSettings>) {
    match one {
        Some(s) => toggle(&mut t.picked, s),
        None => t.picked = if !avail.is_empty() && avail.iter().all(|s| t.picked.contains(s)) { Vec::new() } else { avail.to_vec() },
    }
    t.export_msg = None;
    cx.notify();
}

fn import_toggle(t: &mut BackupSettings, one: Option<BackupSection>, avail: &[BackupSection], cx: &mut Context<BackupSettings>) {
    match one {
        Some(s) => toggle(&mut t.import_picked, s),
        None => t.import_picked = if !avail.is_empty() && avail.iter().all(|s| t.import_picked.contains(s)) { Vec::new() } else { avail.to_vec() },
    }
    cx.notify();
}

impl Render for BackupSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let with_passwords = self.picked.contains(&BackupSection::Passwords);
        let (pw, pw2) = (kit::text_of(&self.pw, cx), kit::text_of(&self.pw2, cx));
        let pw_error = password_error(with_passwords, &pw, &pw2);
        let can_export = !self.picked.is_empty() && !self.exporting && (!with_passwords || (pw.chars().count() >= MIN_PASSWORD && pw == pw2));

        let mut col = kit::body().child(kit::note(cx, tr("backup.lead"))).child(kit::sub(cx, tr("backup.exportHeading")));
        if let Some(info) = &self.info {
            col = col.child(self.section_list(cx, "exp", &count_list(info.counts.iter()), &self.picked, export_toggle));
        }
        if with_passwords {
            col = col.child(kit::group(
                cx,
                vec![kit::row_stack(cx)
                    .child(kit::label(cx, tr("backup.passwordLabel"), Some(tr("backup.passwordHint").into())))
                    .child(div().flex().gap(space::S2).child(kit::field(&self.pw)).child(kit::field(&self.pw2)))
                    .when_some(pw_error, |d, k| d.child(kit::err(cx, tr(k))))
                    .into_any_element()],
            ));
        }
        let mut actions = div().flex().items_center().justify_end().gap(space::S2);
        if let Some((ok, text)) = self.export_msg.clone() {
            actions = actions.child(div().mr_auto().child(if ok { kit::ok(cx, text) } else { kit::err(cx, text) }));
        }
        col = col.child(actions.child(
            Btn::new("backup-export", if self.exporting { tr("backup.exporting") } else { tr("backup.export") })
                .icon(IconName::HardDriveDownload)
                .disabled(!can_export)
                .on_click(cx.listener(|this, _, _, cx| this.do_export(cx))),
        ));

        // ---- import
        col = col.child(kit::sub(cx, tr("backup.importHeading")));
        match self.file.clone() {
            None => {
                col = col.child(kit::group(
                    cx,
                    vec![kit::row(cx)
                        .child(kit::label(cx, tr("backup.importLabel"), Some(tr("backup.importHint").into())))
                        .child(Btn::new("backup-choose", tr("backup.chooseFile")).icon(IconName::HardDriveUpload).on_click(cx.listener(|this, _, w, cx| this.choose_file(w, cx))))
                        .into_any_element()],
                ));
            }
            Some(file) => {
                let needs_pw = !file.system_sealed.unwrap_or(false) && self.import_picked.contains(&BackupSection::Passwords);
                let can_import = !self.import_picked.is_empty() && !self.importing && (!needs_pw || !kit::text_of(&self.import_pw, cx).is_empty());
                col = col.child(kit::group(
                    cx,
                    vec![kit::row(cx)
                        .child(kit::label(
                            cx,
                            tr_args("backup.fileFrom", &[("date", &kit::long_datetime(file.created_at))]),
                            Some(file.app_version.clone()).filter(|v| !v.is_empty()).map(|v| tr_args("backup.fileVersion", &[("version", &v)]).into()),
                        ))
                        .child(Btn::new("backup-other", tr("backup.otherFile")).on_click(cx.listener(|this, _, w, cx| this.choose_file(w, cx))))
                        .into_any_element()],
                ));
                if self.from_safety {
                    col = col.child(kit::note(cx, tr("backup.safetyLoaded")));
                }
                col = col.child(self.section_list(cx, "imp", &count_list(file.counts.iter()), &self.import_picked, import_toggle));
                let mode_row = |id: &'static str, on: bool, title: String, hint: String, mode: BackupMode, cx: &mut Context<Self>| {
                    div()
                        .id(id)
                        .flex()
                        .items_start()
                        .gap(space::S2)
                        .p(space::S2)
                        .rounded(radius::ROW)
                        .cursor_pointer()
                        .when(on, |d| d.bg(t.surface_2))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.mode = mode;
                            cx.notify();
                        }))
                        .child(div().mt(gpui::px(2.0)).child(kit::radio_dot(cx, on)))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(gpui::px(2.0))
                                .child(div().font_weight(weight::MEDIUM).child(title))
                                .child(kit::hint_text(cx, hint)),
                        )
                };
                let mut group_rows = vec![kit::row_stack(cx)
                    .child(kit::label(cx, tr("backup.modeLabel"), None))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S1)
                            .child(mode_row("mode-add", self.mode == BackupMode::Add, tr("backup.modeAdd"), tr("backup.modeAddHint"), BackupMode::Add, cx))
                            .child(mode_row("mode-replace", self.mode == BackupMode::Replace, tr("backup.modeReplace"), tr("backup.modeReplaceHint"), BackupMode::Replace, cx)),
                    )
                    .into_any_element()];
                if needs_pw {
                    group_rows.push(
                        kit::row(cx).child(kit::label(cx, tr("backup.importPasswordLabel"), None)).child(kit::field(&self.import_pw)).into_any_element(),
                    );
                }
                col = col.child(kit::group(cx, group_rows));
                let replace = self.mode == BackupMode::Replace;
                col = col.child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(space::S2)
                        .child(Btn::new("backup-cancel", tr("common.cancel")).on_click(cx.listener(|this, _, _, cx| {
                            this.file = None;
                            cx.notify();
                        })))
                        .child({
                            let b = Btn::new("backup-import", if self.importing { tr("backup.importing") } else { tr("backup.import") }).disabled(!can_import);
                            let b = if replace { b.danger().solid() } else { b };
                            b.on_click(cx.listener(|this, _, _, cx| this.do_import(cx)))
                        }),
                );
            }
        }
        if let Some(e) = self.import_err.clone() {
            col = col.child(kit::note_err(cx, e));
        }
        col = col.child(kit::note(cx, tr("backup.restartNote")));

        // ---- safety copies
        col = col.child(kit::sub(cx, tr("backup.safetyHeading")));
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        if let Some(info) = &self.info {
            if info.safety.is_empty() {
                rows.push(kit::row(cx).child(kit::hint_text(cx, tr("backup.safetyEmpty"))).into_any_element());
            }
            for (i, s) in info.safety.iter().enumerate() {
                let name = s.name.clone();
                rows.push(
                    kit::row(cx)
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .items_center()
                                .gap(space::S1)
                                .child(gpui_component::Icon::from(IconName::History).size(gpui::px(13.0)).text_color(t.text_2))
                                .child(tr_args("backup.safetyItem", &[("date", &kit::long_datetime(s.created_at))])),
                        )
                        .child(Btn::new(("safety-restore", i), tr("backup.restore")).on_click(cx.listener(move |this, _, w, cx| this.open_safety(&name, w, cx))))
                        .into_any_element(),
                );
            }
        }
        rows.push(
            kit::row(cx)
                .child(kit::hint_text(cx, tr("backup.safetyHint")).flex_1().min_w_0())
                .child(Btn::new("safety-reveal", tr("backup.revealSafety")).icon(IconName::FolderOpen).on_click(|_, _, cx| api(cx).reveal_safety_backups()))
                .into_any_element(),
        );
        col.child(kit::group(cx, rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_rules() {
        assert_eq!(password_error(true, "abc", ""), Some("backup.passwordShort"));
        assert_eq!(password_error(true, "abcdef", "abcdeg"), Some("backup.passwordMismatch"));
        assert_eq!(password_error(true, "abcdef", "abcdef"), None);
        assert_eq!(password_error(true, "", ""), None);
        assert_eq!(password_error(false, "abc", "x"), None);
    }

    #[test]
    fn section_labels() {
        assert_eq!(section_key(BackupSection::Passwords), "passwords");
        assert!(!section_label(BackupSection::History, Some(3)).is_empty());
    }

    #[test]
    fn toggling_adds_and_removes() {
        let mut v = vec![BackupSection::Settings];
        toggle(&mut v, BackupSection::Mods);
        assert_eq!(v, vec![BackupSection::Settings, BackupSection::Mods]);
        toggle(&mut v, BackupSection::Settings);
        assert_eq!(v, vec![BackupSection::Mods]);
    }
}

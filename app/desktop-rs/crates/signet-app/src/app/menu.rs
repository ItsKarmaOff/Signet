//! Port of `app/menu.ts`: the native menu bar with the same items, accelerators from the customizable shortcut table,
//! checkmarks on the View menu, and actions routed to the browser (`RunCommand`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{actions, App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType};
use signet_core::shortcuts::{parse_accel, shortcut_accels};

use crate::app::platform::IS_MAC;
use crate::browser::Browser;

/// One action carries the command id (see `Core::run_command`).
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = signet, no_json)]
pub struct RunCommand {
    pub id: String,
}

actions!(signet, [EditUndo, EditRedo, EditCut, EditCopy, EditPaste, EditSelectAll]);

fn t(key: &str) -> String {
    signet_core::i18n::t(key)
}

thread_local! {
    /// Keystrokes currently bound to a command (`None`: bound once, disabled since).
    static BOUND: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
}

/// Electron accelerator to GPUI keystroke(s).
fn keystrokes(accel: &str) -> Vec<String> {
    let Some(p) = parse_accel(accel, IS_MAC) else { return Vec::new() };
    let mut mods = String::new();
    if p.ctrl {
        mods.push_str("ctrl-");
    }
    if p.alt {
        mods.push_str("alt-");
    }
    if p.shift {
        mods.push_str("shift-");
    }
    if p.cmd {
        mods.push_str(if IS_MAC { "cmd-" } else { "super-" });
    }
    let key = p.key.to_lowercase();
    let keys: Vec<String> = match key.as_str() {
        "plus" => vec!["=".into(), "shift-=".into()],
        "return" => vec!["enter".into()],
        "esc" => vec!["escape".into()],
        other => vec![other.to_string()],
    };
    keys.into_iter()
        .map(|k| match k.strip_prefix("shift-") {
            Some(rest) if !mods.contains("shift-") => format!("shift-{mods}{rest}").replace("shift-shift-", "shift-"),
            _ => format!("{mods}{k}"),
        })
        .collect()
}

/// Makes the bindings of the app match `wanted` (keystroke to command id). GPUI cannot remove a binding: one that is
/// no longer wanted is shadowed by a later `NoAction` binding of the same keystroke.
fn sync_bindings(cx: &mut App, wanted: &HashMap<String, String>) {
    let stale: Vec<String> = BOUND.with(|b| b.borrow().iter().filter(|(k, v)| v.is_some() && !wanted.contains_key(*k)).map(|(k, _)| k.clone()).collect());
    for stroke in stale {
        if let Ok(binding) = KeyBinding::load(&stroke, Box::new(gpui::NoAction), None, false, None, cx) {
            cx.bind_keys([binding]);
        }
        BOUND.with(|b| b.borrow_mut().insert(stroke, None));
    }
    for (stroke, id) in wanted {
        if BOUND.with(|b| b.borrow().get(stroke).is_some_and(|c| c.as_deref() == Some(id.as_str()))) {
            continue;
        }
        if let Ok(binding) = KeyBinding::load(stroke, Box::new(RunCommand { id: id.clone() }), None, false, None, cx) {
            cx.bind_keys([binding]);
            BOUND.with(|b| b.borrow_mut().insert(stroke.clone(), Some(id.clone())));
        }
    }
}

/// Menu items of popup-style entries (Develop menu).
fn entry_items(entries: &[crate::windows::Entry]) -> Vec<MenuItem> {
    use crate::windows::Entry;
    entries
        .iter()
        .map(|e| match e {
            Entry::Sep => MenuItem::separator(),
            Entry::Item { label, id, enabled, checked } => MenuItem::action(label.clone(), RunCommand { id: id.clone() }).checked(*checked).disabled(!*enabled),
            Entry::Sub { label, items } => MenuItem::submenu(Menu::new(label.clone()).items(entry_items(items))),
        })
        .collect()
}

/// (Re)builds the menu bar and binds the shortcuts.
pub fn install(browser: &Rc<Browser>, cx: &mut App) {
    let (overrides, sidebar_on, compact, horizontal, bookmarks, recording, mods_on, ext_accels, develop) = {
        let c = browser.core.borrow();
        let develop = match (c.state().settings.dev_mode == Some(true), c.active_tab_id()) {
            (true, Some(tab)) => {
                let mut items = vec![
                    MenuItem::action(t("menu.view.devTools"), RunCommand { id: "devTools".into() }),
                    MenuItem::action(t("menu.view.jsConsole"), RunCommand { id: "jsConsole".into() }),
                    MenuItem::action(t("develop.responsive"), RunCommand { id: "dev:responsive".into() }).checked(c.dev_overrides(&tab).responsive.is_some()),
                    MenuItem::separator(),
                ];
                items.extend(entry_items(&c.develop_menu_entries(&tab, true)));
                Some(items)
            }
            (true, None) => Some(vec![MenuItem::action(t("menu.view.devTools"), RunCommand { id: "devTools".into() }), MenuItem::action(t("menu.view.jsConsole"), RunCommand { id: "jsConsole".into() })]),
            _ => None,
        };
        (c.state().settings.shortcuts.clone(), c.sidebar_on(), c.state().settings.compact_mode, c.horizontal(), c.bookmarks_visible(), c.recording_shortcuts, !c.mods_all_disabled(), c.ext_command_accels(), develop)
    };
    let cmd = |label: String, id: &str| MenuItem::action(label, RunCommand { id: id.to_string() });
    // Shortcuts are bound for every command (none while a shortcut is being recorded).
    let mut wanted: HashMap<String, String> = HashMap::new();
    if !recording {
        for def in signet_core::shortcuts::SHORTCUTS {
            for accel in shortcut_accels(def.id, overrides.as_ref(), IS_MAC) {
                for s in keystrokes(&accel) {
                    wanted.insert(s, def.id.to_string());
                }
            }
        }
        wanted.insert((if IS_MAC { "cmd-," } else { "ctrl-," }).into(), "settings".into());
        for n in 1..=9 {
            wanted.insert(format!("{}-{n}", if IS_MAC { "cmd" } else { "ctrl" }), format!("goToTab:{n}"));
            wanted.insert(format!("{}-{n}", if IS_MAC { "ctrl" } else { "alt" }), format!("goToSpace:{n}"));
        }
        // Ctrl+Q (Windows, Linux) goes through the hold-to-quit gauge first (`quitkeys.rs`): no binding that would quit at once.
        if IS_MAC {
            wanted.insert("cmd-q".into(), "quit".into());
        }
        wanted.insert((if IS_MAC { "ctrl-cmd-f" } else { "f11" }).into(), "fullScreen".into());
        // Extension shortcuts never take a keystroke the app already uses.
        for (accel, id) in ext_accels {
            for s in keystrokes(&accel) {
                wanted.entry(s).or_insert_with(|| id.clone());
            }
        }
    }
    sync_bindings(cx, &wanted);

    let mut app_menu = vec![cmd(t("menu.app.about"), "about"), MenuItem::separator(), cmd(t("menu.settings"), "settings"), MenuItem::separator()];
    if IS_MAC {
        app_menu.push(MenuItem::os_submenu("Services", SystemMenuType::Services));
        app_menu.push(MenuItem::separator());
        app_menu.push(cmd(t("menu.app.hide"), "hide"));
        app_menu.push(MenuItem::separator());
    }
    app_menu.push(cmd(t("menu.app.quit"), "quit"));

    let file = vec![
        cmd(t("menu.file.newTab"), "newTab"),
        cmd(t("menu.file.newLittleSignet"), "newLittle"),
        cmd(t("menu.file.newPrivateWindow"), "newPrivate"),
        cmd(t("menu.file.newNote"), "newNote"),
        cmd(t("menu.file.newEasel"), "newEasel"),
        cmd(t("menu.file.reopenTab"), "reopenTab"),
        MenuItem::separator(),
        cmd(t("menu.file.closeTab"), "closeTab"),
        cmd(t("menu.file.closeWindow"), "closeWindow"),
        MenuItem::separator(),
        cmd(t("menu.file.savePage"), "savePage"),
        cmd(t("menu.file.exportPdf"), "exportPdf"),
        cmd(t("menu.file.print"), "print"),
        cmd(t("menu.file.capture"), "capture"),
        cmd(t("menu.file.installApp"), "installApp"),
    ];
    let edit = vec![
        MenuItem::os_action(t("menu.edit.undo"), EditUndo, OsAction::Undo),
        MenuItem::os_action(t("menu.edit.redo"), EditRedo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action(t("menu.edit.cut"), EditCut, OsAction::Cut),
        MenuItem::os_action(t("menu.edit.copy"), EditCopy, OsAction::Copy),
        MenuItem::os_action(t("menu.edit.paste"), EditPaste, OsAction::Paste),
        MenuItem::os_action(t("menu.edit.selectAll"), EditSelectAll, OsAction::SelectAll),
        MenuItem::separator(),
        cmd(t("menu.edit.find"), "find"),
        cmd(t("menu.edit.findNext"), "findNext"),
        cmd(t("menu.edit.findPrevious"), "findPrev"),
    ];
    let view = vec![
        cmd(t("menu.view.toggleSidebar"), "toggleSidebar").checked(sidebar_on),
        cmd(t("menu.view.compact"), "compactMode").checked(compact),
        cmd(t("menu.view.tabsTop"), "tabLayout").checked(horizontal),
        cmd(t(if bookmarks { "menu.view.hideBookmarksBar" } else { "menu.view.showBookmarksBar" }), "bookmarksBar"),
        MenuItem::separator(),
        cmd(t("menu.view.reload"), "reload"),
        cmd(t("menu.view.hardReload"), "forceReload"),
        MenuItem::separator(),
        cmd(t("menu.view.zoomIn"), "zoomIn"),
        cmd(t("menu.view.zoomOut"), "zoomOut"),
        cmd(t("menu.view.actualSize"), "actualSize"),
        MenuItem::separator(),
        cmd(t("menu.view.mods"), "mods").checked(mods_on),
        MenuItem::separator(),
        cmd(t("menu.view.reader"), "reader"),
        cmd(t("menu.view.translate"), "translate"),
        cmd(t("menu.view.customizeSite"), "customizeSite"),
        cmd(t("menu.view.viewSource"), "viewSource"),
        MenuItem::separator(),
        cmd(t("menu.view.fullScreen"), "fullScreen"),
        cmd(t("menu.view.devTools"), "devTools"),
        cmd(t("menu.view.jsConsole"), "jsConsole"),
    ];
    let spaces: Vec<MenuItem> = (1..=9).map(|n| cmd(signet_core::i18n::t_args("menu.navigate.space", &[("number", &n.to_string())]), &format!("goToSpace:{n}"))).collect();
    let tabs: Vec<MenuItem> = (1..=9).map(|n| cmd(signet_core::i18n::t_args("menu.navigate.tab", &[("number", &n.to_string())]), &format!("goToTab:{n}"))).collect();
    let navigate = vec![
        cmd(t("menu.navigate.back"), "back"),
        cmd(t("menu.navigate.forward"), "forward"),
        cmd(t("menu.navigate.openAddress"), "openAddress"),
        cmd(t("menu.navigate.library"), "library"),
        cmd(t("menu.navigate.copyUrl"), "copyUrl"),
        MenuItem::separator(),
        cmd(t("menu.navigate.togglePin"), "pinTab"),
        cmd(t("menu.navigate.toggleBookmark"), "bookmark"),
        cmd(t("menu.navigate.splitView"), "openSplit"),
        MenuItem::separator(),
        cmd(t("menu.navigate.nextTab"), "nextTab"),
        cmd(t("menu.navigate.previousTab"), "prevTab"),
        cmd(t("menu.navigate.nextSpace"), "nextSpace"),
        cmd(t("menu.navigate.previousSpace"), "prevSpace"),
        MenuItem::submenu(Menu::new(t("menu.navigate.space").replace("{{number}}", "").trim().to_string()).items(spaces)),
        MenuItem::submenu(Menu::new(t("menu.navigate.tab").replace("{{number}}", "").trim().to_string()).items(tabs)),
    ];
    let window = vec![cmd(t("menu.window.minimize"), "minimize"), cmd(t("menu.window.zoom"), "zoomWindow")];
    let help = vec![
        cmd(t("menu.help.checkUpdates"), "checkUpdates"),
        cmd(t("menu.help.releaseNotes"), "releaseNotes"),
        cmd(t("menu.help.reportIssue"), "reportIssue"),
        cmd(t("menu.help.github"), "github"),
    ];
    let mut menus = vec![
        Menu::new("Signet").items(app_menu),
        Menu::new(t("menu.file.title")).items(file),
        Menu::new(t("menu.edit.title")).items(edit),
        Menu::new(t("menu.view.title")).items(view),
        Menu::new(t("menu.navigate.title")).items(navigate),
    ];
    if let Some(items) = develop {
        menus.push(Menu::new(t("menu.develop.title")).items(items));
    }
    menus.push(Menu::new(t("menu.window.title")).items(window));
    menus.push(Menu::new(t("menu.help.title")).items(help));
    cx.set_menus(menus);
}

/// Routes menu actions to the browser. Call once.
pub fn register_actions(browser: &Rc<Browser>, cx: &mut App) {
    let b = browser.clone();
    cx.on_action(move |a: &RunCommand, cx: &mut App| match a.id.as_str() {
        "hide" => {
            #[cfg(target_os = "macos")]
            crate::mac::hide_app();
        }
        "minimize" | "zoomWindow" => {
            let _ = b.core.borrow().cmds.unbounded_send(if a.id == "minimize" { crate::windows::Cmd::Minimize } else { crate::windows::Cmd::ZoomWindow });
        }
        "about" => b.core.borrow_mut().open_settings_window(Some("about")),
        "checkUpdates" => b.core.borrow_mut().check_for_updates(),
        "releaseNotes" => crate::app::platform::open_external(&format!("{}/releases", signet_features::updates::REPO_URL)),
        "reportIssue" => crate::app::platform::open_external(&format!("{}/issues/new", signet_features::updates::REPO_URL)),
        "github" => crate::app::platform::open_external(signet_features::updates::REPO_URL),
        id => {
            b.core.borrow_mut().run_command(id);
            let _ = cx;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_become_keystrokes() {
        let main = if IS_MAC { "cmd" } else { "ctrl" };
        assert_eq!(keystrokes("CmdOrCtrl+T"), vec![format!("{main}-t")]);
        // Modifier order is ctrl, alt, shift, then cmd (macOS), so ctrl comes before shift elsewhere.
        let shifted = if IS_MAC { "shift-cmd-t" } else { "ctrl-shift-t" };
        assert_eq!(keystrokes("Shift+CmdOrCtrl+T"), vec![shifted.to_string()]);
        assert_eq!(keystrokes("Ctrl+Tab"), vec!["ctrl-tab".to_string()]);
        assert!(keystrokes("nonsense+").is_empty());
    }
}

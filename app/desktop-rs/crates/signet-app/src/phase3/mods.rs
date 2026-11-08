//! PHASE3: UI mods (`signet_features::mods`, port of `main/features/mods/mods.ts`).
//!
//! In Electron a mod is a CSS sheet inserted in every interface view (React). The interface of this port is drawn
//! by GPUI, which has no CSS: a mod cannot restyle it. What a mod can still style are the interface pages that are
//! web content (`signet://` pages: notes, Easels, JSON viewer, internal pages). The sheet (sanitized by
//! `sanitize_mod_css`, no remote loads) is therefore inserted in those pages: at document start through a bundle
//! part, and live in the open ones when a mod changes. The mods are stored and managed exactly as in Electron
//! (`mods.json`), so the Settings section works and a mod written for the web interface pages applies to them.

// Entry points called by modules or windows that are wired by other agents (see the report).
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde_json::json;
use signet_core::api::UiEvent;
use signet_core::types::ModsInfo;
use signet_features::mods::{self as feat, Mods};

use super::hooks;
use crate::browser::Core;
use crate::windows::Cmd;

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    mods: Option<Mods>,
    /// Sheet to insert, read on CEF threads by the bundle part.
    sheet: Arc<RwLock<String>>,
}

fn is_internal(url: &str) -> bool {
    url.starts_with("signet://") || url.starts_with("signetui://")
}

/// Script that sets (or, when empty, removes) the mods sheet of the page.
pub(crate) fn mods_js(sheet: &str) -> String {
    let lit = serde_json::to_string(sheet).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(css){{var K='signet-mods';function go(){{var r=document.documentElement;if(!r)return false;\
         try{{document.adoptedStyleSheets=document.adoptedStyleSheets.filter(function(x){{return x.__sk!==K}});}}catch(e){{}}\
         var o=document.querySelector('style[data-signet-mods]');if(o)o.remove();\
         if(!css)return true;\
         try{{var s=new CSSStyleSheet();s.replaceSync(css);s.__sk=K;document.adoptedStyleSheets=document.adoptedStyleSheets.concat([s]);}}\
         catch(e){{var n=document.createElement('style');n.setAttribute('data-signet-mods','');n.textContent=css;(document.head||r).appendChild(n);}}\
         return true}}\
         if(!go()){{var m=new MutationObserver(function(){{if(go())m.disconnect()}});m.observe(document,{{childList:true}})}}}})({lit})"
    )
}

impl Core {
    fn mods_ref(&self) -> Option<&Mods> {
        self.phase3.mods.mods.as_ref()
    }

    pub fn list_mods(&mut self) -> ModsInfo {
        self.mods_ref().map(Mods::info).unwrap_or_default()
    }

    pub fn save_mod(&mut self, id: Option<&str>, name: &str, css: &str) -> String {
        let Some(m) = self.phase3.mods.mods.as_mut() else { return String::new() };
        let id = m.save(id, &json!(name), &json!(css));
        self.mods_changed();
        id
    }

    pub fn delete_mod(&mut self, id: &str) {
        if let Some(m) = self.phase3.mods.mods.as_mut() {
            m.remove(id);
            self.mods_changed();
        }
    }

    pub fn set_mod_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(m) = self.phase3.mods.mods.as_mut() {
            m.set_enabled(id, enabled);
            self.mods_changed();
        }
    }

    pub fn move_mod(&mut self, id: &str, delta: i32) {
        if let Some(m) = self.phase3.mods.mods.as_mut() {
            m.move_by(id, delta);
            self.mods_changed();
        }
    }

    /// Turns every mod off or back on (Settings, View menu): the menu is rebuilt for its check mark.
    pub fn set_mods_disabled(&mut self, disabled: bool) {
        let Some(m) = self.phase3.mods.mods.as_mut() else { return };
        if m.disabled == disabled {
            return;
        }
        m.set_disabled(disabled);
        self.mods_changed();
        self.send(Cmd::RebuildMenu);
    }

    /// True if all mods are off (setting): for the View menu check mark.
    pub fn mods_all_disabled(&self) -> bool {
        self.mods_ref().is_some_and(|m| m.disabled)
    }

    pub fn import_mod(&mut self) -> Option<String> {
        let path = dialog::open_file("CSS", "css")?;
        let css = std::fs::read_to_string(&path).ok()?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let id = self.phase3.mods.mods.as_mut()?.import_css(&stem, &css);
        self.mods_changed();
        Some(id)
    }

    pub fn export_mod(&mut self, id: &str) {
        let Some(m) = self.mods_ref().and_then(|m| m.mods.iter().find(|m| m.id == id)).cloned() else { return };
        let Some(path) = dialog::save_file(&signet_core::i18n::t("mods.exportTitle"), &feat::export_file_name(&m.name), "css") else { return };
        if let Err(e) = std::fs::write(&path, feat::export_css(&m)) {
            self.log(format!("cannot export the mod to {}: {e}", path.display()));
        }
    }

    /// Reapplies the mods to the open interface pages, tells the Settings section.
    fn mods_changed(&mut self) {
        let sheet = self.mods_ref().map(Mods::sheet).unwrap_or_default();
        if let Ok(mut w) = self.phase3.mods.sheet.write() {
            w.clone_from(&sheet);
        }
        let js = mods_js(&sheet);
        for id in self.views.ids() {
            if let Some(v) = self.views.view_of(&id).filter(|v| is_internal(&v.url())) {
                v.execute_js(&js);
            }
        }
        self.emit(UiEvent::ModsChanged);
    }
}

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    let untitled = signet_core::i18n::t("mods.untitled");
    let mods = Mods::load(&core.data_dir, Mods::safe_mode_from_env(), &untitled);
    let sheet = mods.sheet();
    if mods.safe_mode {
        core.log("safe mode: mods are not applied");
    }
    core.phase3.mods.mods = Some(mods);
    if let Ok(mut w) = core.phase3.mods.sheet.write() {
        *w = sheet;
    }
    let shared = core.phase3.mods.sheet.clone();
    hooks::add_bundle_part(60, move |f| {
        if !f.is_main_frame || !is_internal(&f.url) {
            return None;
        }
        let s = shared.read().ok()?;
        (!s.is_empty()).then(|| mods_js(&s))
    });
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(_core: &mut Core, _ev: &signet_web::WebEvent) {}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(_core: &mut Core) {}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    if let Some(m) = core.phase3.mods.mods.as_ref() {
        m.flush();
    }
}

/// Native file dialogs without extra dependencies (a modal helper process per platform).
mod dialog {
    use super::PathBuf;
    use std::process::Command;

    fn run(mut c: Command) -> Option<PathBuf> {
        let out = c.output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim_end_matches(['\r', '\n']).to_string();
        (!s.is_empty()).then(|| PathBuf::from(s))
    }

    /// Asks for an existing file with the given extension; `None` if cancelled.
    pub fn open_file(label: &str, ext: &str) -> Option<PathBuf> {
        let _ = label;
        #[cfg(target_os = "macos")]
        {
            let mut c = Command::new("osascript");
            c.args(["-e", "on run argv", "-e", "POSIX path of (choose file of type {item 1 of argv})", "-e", "end run", "--", ext]);
            run(c)
        }
        #[cfg(target_os = "windows")]
        {
            let mut c = Command::new("powershell");
            c.env("SIGNET_DLG_FILTER", format!("{label} (*.{ext})|*.{ext}"));
            c.args(["-NoProfile", "-Command", "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Filter = $env:SIGNET_DLG_FILTER; if ($d.ShowDialog() -eq 'OK') { Write-Output $d.FileName } else { exit 1 }"]);
            run(c)
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            let mut z = Command::new("zenity");
            z.args(["--file-selection", &format!("--file-filter=*.{ext}")]);
            run(z).or_else(|| {
                let mut k = Command::new("kdialog");
                k.args(["--getopenfilename", ".", &format!("*.{ext}")]);
                run(k)
            })
        }
    }

    /// Asks where to save a file; `None` if cancelled.
    pub fn save_file(title: &str, name: &str, ext: &str) -> Option<PathBuf> {
        let _ = ext;
        #[cfg(target_os = "macos")]
        {
            let mut c = Command::new("osascript");
            c.args(["-e", "on run argv", "-e", "POSIX path of (choose file name with prompt (item 1 of argv) default name (item 2 of argv))", "-e", "end run", "--", title, name]);
            run(c)
        }
        #[cfg(target_os = "windows")]
        {
            let mut c = Command::new("powershell");
            c.env("SIGNET_DLG_TITLE", title).env("SIGNET_DLG_NAME", name);
            c.args(["-NoProfile", "-Command", "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.SaveFileDialog; $d.Title = $env:SIGNET_DLG_TITLE; $d.FileName = $env:SIGNET_DLG_NAME; if ($d.ShowDialog() -eq 'OK') { Write-Output $d.FileName } else { exit 1 }"]);
            run(c)
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            let mut z = Command::new("zenity");
            z.args(["--file-selection", "--save", "--confirm-overwrite", &format!("--title={title}"), &format!("--filename={name}")]);
            run(z).or_else(|| {
                let mut k = Command::new("kdialog");
                k.args(["--getsavefilename", name]);
                run(k)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_pages_only() {
        assert!(is_internal("signet://note/abc"));
        assert!(is_internal("signetui://app/index.html"));
        assert!(!is_internal("https://signet.example/"));
        assert!(!is_internal("data:text/html,signet://"));
    }

    #[test]
    fn script_escapes_the_sheet() {
        let js = mods_js("a::before{content:\"</style>\"}");
        assert!(js.contains(r#"\"</style>\""#));
        assert!(js.contains("signet-mods"));
        assert!(mods_js("").ends_with("(\"\")"));
    }
}

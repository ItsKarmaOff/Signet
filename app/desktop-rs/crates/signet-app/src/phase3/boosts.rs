//! PHASE3: boosts and the zap ("Hide") tool (`signet_features::boost`), port of `main/features/boost/boost.ts`
//! and `features/zap.ts`.
//!
//! - CSS: the CSS of every enabled Boost is kept in a shared map (site to CSS). A bundle part inserts it at document
//!   start in the main frame of every non-private page of the site (no flash of unstyled content, survives reloads);
//!   when a Boost changes, the open tabs of the site get the new sheet through `execute_js`.
//! - Emoji (dark preset): `EMOJI_SCRIPT` runs once the document has loaded. CEF has no isolated worlds: the script
//!   runs in the page world like every other injected script of this port.
//! - Zap: the zap script returns a promise in Electron. Here it is wrapped so that its result is posted on the page
//!   channel `zap:result` (`window.__signetPost`), which this module reads in `web_event` (before `pages.rs`, so no
//!   route is needed there). The tool runs again after every pick until Esc, a navigation or `stop_zap`.
//! - Editor: the window state is `UiEvent::BoostState`. The window itself is opened by the window manager (see the
//!   report: `Win::Boost`); this module only pushes the state (several times after `open_boost`, since the window
//!   subscribes after it exists).

// Entry points called by modules or windows that are wired by other agents (see the report).
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use indexmap::IndexMap;
use signet_core::api::UiEvent;
use signet_core::site::site_of;
use signet_core::types::{Boost, BoostEditorState};
use signet_features::boost as feat;
use signet_web::{FrameTarget, ViewId, WebEvent};

use super::hooks;
use crate::browser::Core;

/// Pushes of the editor state after `open_boost` (the window subscribes a moment after it is asked for).
const RESEND: u8 = 3;

/// The tool currently waiting for a pick.
struct Zap {
    site: String,
    tab: String,
    view: ViewId,
}

/// State owned by this feature (a field of `Phase3`).
#[derive(Default)]
pub struct State {
    /// Site to the CSS to insert (enabled, non empty Boosts only). Read on CEF threads by the bundle part.
    css: Arc<RwLock<HashMap<String, String>>>,
    /// Sites whose editor was opened, with the pushes still to repeat.
    editors: HashMap<String, u8>,
    /// Last state sent per site (JSON), to send only changes.
    last: HashMap<String, String>,
    zap: Option<Zap>,
    /// Views where the emoji script runs.
    emoji: HashSet<ViewId>,
}

// ---------------------------------------------------------------------------------------------- pure logic

/// Script that sets (or, with an empty `css`, removes) the Boost sheet of the page. A constructed stylesheet is not
/// blocked by the page CSP; a `<style>` element is the fallback. Safe to run before `<html>` exists.
pub(crate) fn boost_js(css: &str) -> String {
    let lit = serde_json::to_string(css).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(css){{var K='signet-boost';function go(){{var r=document.documentElement;if(!r)return false;\
         try{{document.adoptedStyleSheets=document.adoptedStyleSheets.filter(function(x){{return x.__sk!==K}});}}catch(e){{}}\
         var o=document.querySelector('style[data-signet-boost]');if(o)o.remove();\
         if(!css)return true;\
         try{{var s=new CSSStyleSheet();s.replaceSync(css);s.__sk=K;document.adoptedStyleSheets=document.adoptedStyleSheets.concat([s]);}}\
         catch(e){{var n=document.createElement('style');n.setAttribute('data-signet-boost','');n.textContent=css;(document.head||r).appendChild(n);}}\
         return true}}\
         if(!go()){{var m=new MutationObserver(function(){{if(go())m.disconnect()}});m.observe(document,{{childList:true}})}}}})({lit})"
    )
}

/// The zap script wrapped to post its result (a selector, or null) on the `zap:result` channel.
pub(crate) fn zap_js() -> String {
    format!(
        "(function(){{var p=({});Promise.resolve(p).then(function(r){{window.__signetPost('zap:result',JSON.stringify(typeof r==='string'?r:null))}},\
         function(){{window.__signetPost('zap:result','null')}})}})()",
        feat::ZAP_SCRIPT.trim()
    )
}

/// A selector from the page is added to a stylesheet: refuse anything that could escape the rule.
pub(crate) fn valid_selector(s: &str) -> bool {
    !s.is_empty() && s.len() <= 2000 && !s.contains(['{', '}', ';', '\n', '\r', '@']) && !s.contains("/*")
}

/// Port of `edit`/`save`/`unzap`/`delete` of the TS class on the state map. Returns false when nothing happened.
pub(crate) fn edit(all: &mut IndexMap<String, Boost>, site: &str, now: i64, f: impl FnOnce(&mut Boost)) -> bool {
    if site.is_empty() {
        return false;
    }
    let mut b = all.get(site).cloned().unwrap_or_else(|| feat::draft(site));
    f(&mut b);
    b.updated_at = now;
    all.insert(site.to_string(), b);
    true
}

/// CSS to insert for each site (enabled, non empty).
pub(crate) fn css_map(all: &IndexMap<String, Boost>) -> HashMap<String, String> {
    all.iter().filter_map(|(site, b)| feat::applied_css(b).map(|c| (site.clone(), c))).collect()
}

fn is_web(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

// ---------------------------------------------------------------------------------------------- API

impl Core {
    pub fn save_boost(&mut self, site: &str, css: Option<&str>, enabled: Option<bool>) {
        let now = signet_core::ids::now_ms();
        let done = edit(&mut self.store.data.state.boosts, site, now, |b| {
            if let Some(c) = css {
                b.css = c.to_string();
            }
            if let Some(e) = enabled {
                b.enabled = e;
            }
        });
        if done {
            self.boost_changed(site);
        }
    }

    /// Opens the "Hide" tool on the best loaded tab of the site (the active one first).
    pub fn start_zap(&mut self, site: &str) {
        if site.is_empty() {
            return;
        }
        if self.phase3.boosts.zap.is_some() {
            self.stop_zap();
        }
        let tabs = self.boost_tabs(site);
        let active = self.active_tab_id();
        let tab = active.filter(|a| tabs.contains(a)).or_else(|| tabs.first().cloned());
        let Some(tab) = tab else { return };
        let Some(view) = self.views.view_of(&tab) else { return };
        self.send(crate::windows::Cmd::ShowMain);
        self.activate_tab(&tab);
        self.phase3.boosts.zap = Some(Zap { site: site.to_string(), tab, view: view.id() });
        self.boost_push(Some(site), false);
        view.focus();
        view.execute_js(&zap_js());
    }

    pub fn stop_zap(&mut self) {
        let Some(z) = self.phase3.boosts.zap.as_ref() else { return };
        if let Some(v) = self.views.view_of(&z.tab).filter(|v| v.id() == z.view) {
            v.execute_js(feat::ZAP_STOP_SCRIPT);
        }
        self.end_zap();
    }

    pub fn unzap(&mut self, site: &str, selector: &str) {
        if !self.state().boosts.contains_key(site) {
            return;
        }
        let now = signet_core::ids::now_ms();
        edit(&mut self.store.data.state.boosts, site, now, |b| b.zapped.retain(|s| s != selector));
        self.boost_changed(site);
    }

    pub fn delete_boost(&mut self, site: &str) {
        if self.store.data.state.boosts.shift_remove(site).is_none() {
            return;
        }
        if self.phase3.boosts.zap.as_ref().is_some_and(|z| z.site == site) {
            self.stop_zap();
        }
        self.boost_changed(site);
    }

    /// Opens (or focuses) the editor of a site.
    pub fn open_boost(&mut self, site: &str) {
        if site.is_empty() {
            return;
        }
        self.phase3.boosts.editors.insert(site.to_string(), RESEND);
        self.boost_push(Some(site), true);
        self.send(crate::windows::Cmd::OpenWin(crate::windows::Win::Boost, Some(site.to_string())));
    }

    /// Menu and command bar entry: the Boost of the tab's site (ignored outside http and https).
    pub fn open_boost_for_tab(&mut self, tab: Option<&str>) {
        let id = tab.map(str::to_string).or_else(|| self.active_tab_id());
        let url = id.and_then(|id| self.state().tabs.get(&id).map(|t| t.url.clone())).unwrap_or_default();
        if is_web(&url) {
            self.open_boost(&site_of(&url));
        }
    }

    /// For the window manager: the editor window of a site was closed.
    pub fn boost_window_closed(&mut self, site: &str) {
        self.phase3.boosts.editors.remove(site);
        self.phase3.boosts.last.remove(site);
        if self.phase3.boosts.zap.as_ref().is_some_and(|z| z.site == site) {
            self.stop_zap();
        }
    }

    // ------------------------------------------------------------------ internals

    /// A Boost changed: persist, reapply to the open tabs, tell the editor and the sidebar (boosted sites).
    fn boost_changed(&mut self, site: &str) {
        self.boost_sync_css();
        self.boost_refresh(site);
        self.boost_push(Some(site), false);
        self.changed();
    }

    fn boost_sync_css(&mut self) {
        let map = css_map(&self.state().boosts);
        if let Ok(mut w) = self.phase3.boosts.css.write() {
            *w = map;
        }
    }

    /// Loaded tabs currently showing the site.
    fn boost_tabs(&self, site: &str) -> Vec<String> {
        self.state()
            .tabs
            .keys()
            .filter(|id| self.views.view_of(id).is_some_and(|v| site_of(&v.url()) == site))
            .cloned()
            .collect()
    }

    fn boost_refresh(&mut self, site: &str) {
        let dark = self.state().boosts.get(site).is_some_and(|b| b.enabled && feat::has_dark_preset(&b.css));
        let css = self.phase3.boosts.css.read().ok().and_then(|m| m.get(site).cloned()).unwrap_or_default();
        for tab in self.boost_tabs(site) {
            if let Some(v) = self.views.view_of(&tab) {
                v.execute_js_in(&FrameTarget::Main, &boost_js(&css));
                self.boost_sync_emoji(v.id(), dark, &tab);
            }
        }
    }

    /// Starts or stops the script that exempts emojis from the dark mode.
    fn boost_sync_emoji(&mut self, view: ViewId, dark: bool, tab: &str) {
        let on = self.phase3.boosts.emoji.contains(&view);
        if dark == on {
            return;
        }
        if dark {
            self.phase3.boosts.emoji.insert(view);
        } else {
            self.phase3.boosts.emoji.remove(&view);
        }
        if let Some(v) = self.views.view_of(tab) {
            v.execute_js(if dark { feat::EMOJI_SCRIPT } else { feat::EMOJI_STOP_SCRIPT });
        }
    }

    fn boost_state(&self, site: &str) -> BoostEditorState {
        BoostEditorState {
            site: site.to_string(),
            boost: self.state().boosts.get(site).cloned().unwrap_or_else(|| feat::draft(site)),
            zapping: self.phase3.boosts.zap.as_ref().is_some_and(|z| z.site == site),
            has_tab: !self.boost_tabs(site).is_empty(),
            hue: self.hue(),
        }
    }

    /// Sends the state to the editor of the site (all editors if `None`); without change since the last send, nothing.
    fn boost_push(&mut self, site: Option<&str>, force: bool) {
        let sites: Vec<String> = self.phase3.boosts.editors.keys().filter(|s| site.is_none_or(|x| x == s.as_str())).cloned().collect();
        for s in sites {
            let st = self.boost_state(&s);
            let json = serde_json::to_string(&st).unwrap_or_default();
            if !force && self.phase3.boosts.last.get(&s) == Some(&json) {
                continue;
            }
            self.phase3.boosts.last.insert(s, json);
            self.emit(UiEvent::BoostState(Box::new(st)));
        }
    }

    fn end_zap(&mut self) {
        if let Some(z) = self.phase3.boosts.zap.take() {
            self.boost_push(Some(&z.site), false);
        }
    }
}

// ---------------------------------------------------------------------------------------------- hooks

/// Called once after the browser core exists and the web layer is initialized.
pub(super) fn startup(core: &mut Core) {
    core.boost_sync_css();
    let shared = core.phase3.boosts.css.clone();
    hooks::add_bundle_part(50, move |f| {
        if !f.is_main_frame || f.is_private {
            return None;
        }
        let site = site_of(&f.url);
        if site.is_empty() {
            return None;
        }
        let css = shared.read().ok()?.get(&site)?.clone();
        Some(boost_js(&css))
    });
}

/// Called for every CEF event, before the core handles it.
pub(super) fn web_event(core: &mut Core, ev: &WebEvent) {
    match ev {
        WebEvent::PageMessage { view, channel, payload, is_main_frame, top_url, .. } if channel == "zap:result" => {
            let Some(z) = core.phase3.boosts.zap.as_ref() else { return };
            if *view != z.view || !*is_main_frame || site_of(top_url) != z.site {
                return;
            }
            let (site, tab) = (z.site.clone(), z.tab.clone());
            let picked = serde_json::from_str::<Option<String>>(payload).ok().flatten().filter(|s| valid_selector(s));
            match picked {
                Some(sel) => {
                    let now = signet_core::ids::now_ms();
                    edit(&mut core.store.data.state.boosts, &site, now, |b| {
                        if !b.zapped.contains(&sel) {
                            b.zapped.push(sel.clone());
                        }
                    });
                    core.boost_changed(&site);
                    // The tool stays active for the next pick.
                    if let Some(v) = core.views.view_of(&tab).filter(|v| v.id() == *view) {
                        v.execute_js(&zap_js());
                    }
                }
                None => core.end_zap(),
            }
        }
        // A navigation destroys the tool (and the emoji wrapping): stop.
        WebEvent::LoadStart { view, is_main_frame: true, .. } => {
            if core.phase3.boosts.zap.as_ref().is_some_and(|z| z.view == *view) {
                core.end_zap();
            }
            core.phase3.boosts.emoji.remove(view);
        }
        WebEvent::LoadEnd { view, is_main_frame: true, url, .. } => {
            let Some(tab) = core.views.tab_of(*view).map(str::to_string) else { return };
            let site = site_of(url);
            let dark = core.state().boosts.get(&site).is_some_and(|b| b.enabled && feat::has_dark_preset(&b.css));
            core.boost_sync_emoji(*view, dark, &tab);
        }
        WebEvent::Closed { view } => {
            core.phase3.boosts.emoji.remove(view);
            if core.phase3.boosts.zap.as_ref().is_some_and(|z| z.view == *view) {
                core.end_zap();
            }
        }
        _ => {}
    }
}

/// Called on every core tick (about once per second or at the next store deadline).
pub(super) fn tick(core: &mut Core) {
    // The tab of the tool went to sleep or was closed: the tool is gone.
    if let Some(z) = core.phase3.boosts.zap.as_ref()
        && core.views.view_of(&z.tab).is_none_or(|v| v.id() != z.view)
    {
        core.end_zap();
    }
    if core.phase3.boosts.editors.is_empty() {
        return;
    }
    let resend: Vec<String> = core.phase3.boosts.editors.iter().filter(|(_, n)| **n > 0).map(|(s, _)| s.clone()).collect();
    for s in resend {
        if let Some(n) = core.phase3.boosts.editors.get_mut(&s) {
            *n -= 1;
        }
        core.boost_push(Some(&s), true);
    }
    core.boost_push(None, false);
}

/// Called before quitting (flush files, stop threads).
pub(super) fn quit(core: &mut Core) {
    core.phase3.boosts.zap = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_are_checked() {
        assert!(valid_selector("#main > div.ad:nth-of-type(2)"));
        assert!(!valid_selector(""));
        assert!(!valid_selector("a{}body{display:none}"));
        assert!(!valid_selector("a; b"));
        assert!(!valid_selector("a /* x */"));
        assert!(!valid_selector(&"a".repeat(2001)));
    }

    #[test]
    fn edit_rules() {
        let mut all: IndexMap<String, Boost> = IndexMap::new();
        assert!(!edit(&mut all, "", 1, |_| {}));
        assert!(edit(&mut all, "a.com", 5, |b| b.css = "x{}".into()));
        let b = &all["a.com"];
        assert!(b.enabled && b.css == "x{}" && b.updated_at == 5);
        edit(&mut all, "a.com", 6, |b| b.zapped.push(".ad".into()));
        edit(&mut all, "a.com", 7, |b| b.enabled = false);
        assert_eq!(all["a.com"].zapped, vec![".ad"]);
        assert!(css_map(&all).is_empty());
        edit(&mut all, "a.com", 8, |b| b.enabled = true);
        let map = css_map(&all);
        assert!(map["a.com"].starts_with(".ad { display: none !important; }"));
    }

    #[test]
    fn scripts_are_well_formed() {
        let js = boost_js("body{color:red}\n\"q\"");
        assert!(js.contains(r#"\"q\""#) && js.contains("adoptedStyleSheets"));
        assert!(boost_js("").ends_with("(\"\")"));
        let z = zap_js();
        assert!(z.contains("zap:result") && z.contains("__signetZapStop") && z.starts_with("(function(){var p=(new Promise"));
    }
}

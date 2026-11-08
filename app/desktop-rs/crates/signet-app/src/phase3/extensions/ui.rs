//! The toolbar buttons: `signet_ui::screens::toolbar::BrowserActions` over the extension host.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::App;
use serde_json::Value;
use signet_core::api::Subscription;
use signet_core::types::Rect;
use signet_ui::screens::toolbar::{install_browser_actions, BrowserAction, BrowserActionState, BrowserActions};
use signet_ui::widgets::FaviconSource;

use super::bridge::tab_number;
use super::shared::Shared;
use crate::browser::Browser;

pub struct ExtActions {
    browser: Rc<Browser>,
    shared: Arc<Shared>,
}

/// Installs the extension buttons in the toolbar. Call once at startup, after `signet_ui::init`.
pub fn install_ui(browser: &Rc<Browser>, cx: &mut App) {
    let Some(shared) = browser.core.borrow().ext_shared() else { return };
    install_browser_actions(Rc::new(ExtActions { browser: browser.clone(), shared }), cx);
}

fn badge_text(state: &Value) -> Option<String> {
    state.get("badgeText").and_then(|t| t.as_str()).filter(|t| !t.is_empty()).map(String::from)
}

impl BrowserActions for ExtActions {
    fn state(&self, _partition: &str) -> BrowserActionState {
        let active = self.browser.core.try_borrow().ok().and_then(|c| c.active_tab_id()).map(|t| tab_number(&t));
        let host = self.shared.host();
        let actions = host
            .with_button()
            .into_iter()
            .map(|id| {
                let st = host.button_state(&id, active);
                let icon = host.registry.get(&id).and_then(|i| {
                    let rel = i.manifest.action.as_ref().and_then(|a| a.default_icon.iter().max_by_key(|(s, _)| *s).map(|(_, p)| p.clone())).or_else(|| i.manifest.best_icon().map(String::from))?;
                    Some(FaviconSource::Path(i.dir.join(rel.trim_start_matches('/')).to_string_lossy().into_owned().into()))
                });
                BrowserAction {
                    title: st.get("title").and_then(|t| t.as_str()).map(String::from),
                    text: badge_text(&st),
                    color: st.get("badgeColor").cloned(),
                    icon,
                    tabs: HashMap::new(),
                    id,
                }
            })
            .collect();
        BrowserActionState { active_tab_id: active, actions }
    }

    fn activate(&self, _partition: &str, extension_id: &str, anchor: Rect) {
        self.browser.core.borrow_mut().ext_activate(extension_id, anchor);
    }

    fn subscribe(&self, _partition: &str, listener: Box<dyn Fn() + Send + Sync>) -> Subscription {
        self.shared.subscribe(Arc::from(listener))
    }
}

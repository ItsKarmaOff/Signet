//! The link between the screens and the app: the `SignetApi` handle and the shared `UiState`.
//!
//! Replaces the Electron preload (`window.signet`) and `useSnapshot()` (`renderer/src/lib/util.ts`).
//!
//! - The app calls [`install`] once with its `SignetApi` implementation.
//! - Screens call actions with `api(cx).close_tab(id)`.
//! - Screens read the state from [`UiState`] (`ui_state(cx).read(cx).snapshot`) and re-render on change by
//!   observing it (`cx.observe(&ui_state(cx), |_, _, cx| cx.notify())`).
//! - One-shot pushed events (rename a tab, open the space form, overlay show/hide...) are re-emitted by the
//!   `UiState` entity as `UiEvent`s: `cx.subscribe(&ui_state(cx), |this, _, event: &UiEvent, cx| ...)`.

use std::rc::Rc;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global};
use signet_core::api::{SignetApi, Subscription, UiEvent};
use signet_core::i18n::Locale;
use signet_core::types::{Appearance, Snapshot, StatePatch};

/// The app's `SignetApi`, reachable from every screen.
struct ApiGlobal {
    api: Rc<dyn SignetApi>,
    state: Entity<UiState>,
    _subscription: Subscription,
}

impl Global for ApiGlobal {}

/// The state every screen reads. Updated on the UI thread from the app's pushed events.
pub struct UiState {
    /// Latest full state (`state` + `state:patch` applied).
    pub snapshot: Rc<Snapshot>,
    /// Latest interface language.
    pub locale: Locale,
    /// Latest appearance settings.
    pub appearance: Option<Appearance>,
    /// Sidebar slide step (1 shown, 0 hidden), while a slide runs.
    pub sidebar_slide: Option<f64>,
}

impl EventEmitter<UiEvent> for UiState {}

impl UiState {
    fn apply(&mut self, event: &UiEvent, cx: &mut Context<Self>) {
        match event {
            UiEvent::State(snapshot) => {
                self.snapshot = Rc::new((**snapshot).clone());
                cx.notify();
            }
            UiEvent::StatePatch(patch) => {
                if apply_patch(Rc::make_mut(&mut self.snapshot), patch) {
                    cx.notify();
                }
            }
            UiEvent::Locale(locale) => {
                self.locale = *locale;
                signet_core::i18n::set_locale(*locale);
                cx.notify();
            }
            UiEvent::Appearance(appearance) => {
                self.appearance = Some(*appearance);
                cx.notify();
            }
            UiEvent::SidebarSlide(step) => {
                self.sidebar_slide = if *step >= 1.0 || *step <= 0.0 { None } else { Some(*step) };
                cx.notify();
            }
            _ => {}
        }
        cx.emit(event.clone());
    }
}

/// Merges the changed tab fields of a `state:patch` into the snapshot. Returns true if something changed.
fn apply_patch(snapshot: &mut Snapshot, patch: &StatePatch) -> bool {
    let mut changed = false;
    for (id, fields) in &patch.tabs {
        let Some(tab) = snapshot.tabs.get_mut(id) else { continue };
        let Ok(serde_json::Value::Object(mut current)) = serde_json::to_value(&*tab) else { continue };
        let serde_json::Value::Object(fields) = fields else { continue };
        for (key, value) in fields {
            current.insert(key.clone(), value.clone());
        }
        if let Ok(next) = serde_json::from_value(serde_json::Value::Object(current))
            && *tab != next
        {
            *tab = next;
            changed = true;
        }
    }
    changed
}

/// Installs the app's API and starts delivering its events to `UiState`. Call once, after `signet_ui::init`.
pub fn install(api: Rc<dyn SignetApi>, cx: &mut App) {
    let snapshot = Rc::new(api.get_state());
    let state = cx.new(|_| UiState {
        snapshot,
        locale: signet_core::i18n::current_locale(),
        appearance: None,
        sidebar_slide: None,
    });

    // Events may come from any thread: hand them to the UI thread through a channel.
    let (tx, mut rx) = mpsc::unbounded::<UiEvent>();
    let subscription = api.subscribe(Box::new(move |event| {
        let _ = tx.unbounded_send(event);
    }));

    let target = state.clone();
    cx.spawn(async move |cx| {
        while let Some(event) = rx.next().await {
            target.update(cx, |state, cx| state.apply(&event, cx));
        }
    })
    .detach();

    cx.set_global(ApiGlobal { api, state, _subscription: subscription });
}

/// The app's API. Panics if [`install`] was not called (a programming error, not a runtime condition).
pub fn api(cx: &App) -> Rc<dyn SignetApi> {
    cx.global::<ApiGlobal>().api.clone()
}

/// The shared interface state.
pub fn ui_state(cx: &App) -> Entity<UiState> {
    cx.global::<ApiGlobal>().state.clone()
}

/// The latest snapshot, for quick reads in `render`.
pub fn snapshot(cx: &App) -> Rc<Snapshot> {
    cx.global::<ApiGlobal>().state.read(cx).snapshot.clone()
}

/// True once [`install`] ran (lets the gallery and tests run without an app).
pub fn is_installed(cx: &App) -> bool {
    cx.has_global::<ApiGlobal>()
}

/// Translates a `ui` catalog key (shorthand for screens).
pub fn tr(key: &str) -> String {
    signet_core::i18n::t(key)
}

/// Translates a `ui` catalog key with `{{name}}` arguments.
pub fn tr_args(key: &str, args: &[(&str, &str)]) -> String {
    signet_core::i18n::t_args(key, args)
}

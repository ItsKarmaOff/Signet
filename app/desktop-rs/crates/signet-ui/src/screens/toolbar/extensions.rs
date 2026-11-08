//! Port of `renderer/src/toolbar/ExtensionButtons.tsx` and `browser-action.d.ts`: the action buttons of the pinned
//! extensions (saved order, drag and drop to reorder), then the Extensions button.
//!
//! The browser action state of Electron (`window.browserAction`, the `<browser-action-list>` element) has no
//! equivalent in `SignetApi`: the app provides it through [`BrowserActions`] and [`install_browser_actions`]. Without
//! one, no extension button is drawn.

use super::anchor::Anchor;
use super::util::{BarColors, badge_colors, count_text};
use crate::app_state::{api, tr, ui_state};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius};
use crate::widgets::{Favicon, FaviconSource};
use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    App, AppContext, Context, Entity, Global, InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_component::{Icon, tooltip::Tooltip};
use serde_json::Value;
use signet_core::api::{Subscription, UiEvent};
use signet_core::core::store::SettingsPatch;
use signet_core::types::{ExtensionActionCounts, ExtensionActionStates, Rect};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// What an extension changed on its button for one tab (`action.setTitle`, `setBadgeText`...).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActionOverride {
    pub title: Option<String>,
    pub text: Option<String>,
    pub color: Option<Value>,
}

/// One extension's toolbar button.
#[derive(Clone, Default)]
pub struct BrowserAction {
    pub id: String,
    pub title: Option<String>,
    /// Badge text.
    pub text: Option<String>,
    /// Badge color (CSS color or Chrome's `[r, g, b, a]`).
    pub color: Option<Value>,
    /// Decoded icon, if the app has one; otherwise the first letter of the title is drawn.
    pub icon: Option<FaviconSource>,
    /// Per-tab overrides, by the extension's numeric tab id.
    pub tabs: HashMap<i64, ActionOverride>,
}

/// State of a partition's extension buttons (`BrowserActionState`).
#[derive(Clone, Default)]
pub struct BrowserActionState {
    /// Tab the buttons describe (`-1`/None: unknown).
    pub active_tab_id: Option<i64>,
    pub actions: Vec<BrowserAction>,
}

/// Port of `window.browserAction`, implemented by the app.
pub trait BrowserActions {
    /// Current state of a partition (`persist:space-<id>` or `_self`).
    fn state(&self, partition: &str) -> BrowserActionState;
    /// Click on a button (or a shortcut): opens the popup under `anchor`, or fires the click event.
    fn activate(&self, partition: &str, extension_id: &str, anchor: Rect);
    /// The state of the partition changed. Events may come from any thread.
    fn subscribe(&self, partition: &str, listener: Box<dyn Fn() + Send + Sync>) -> Subscription;
}

struct ActionsGlobal(Rc<dyn BrowserActions>);
impl Global for ActionsGlobal {}

/// Installs the extension buttons' source. Call once at startup, after `signet_ui::init`.
pub fn install_browser_actions(source: Rc<dyn BrowserActions>, cx: &mut App) {
    cx.set_global(ActionsGlobal(source));
}

fn source(cx: &App) -> Option<Rc<dyn BrowserActions>> {
    cx.try_global::<ActionsGlobal>().map(|g| g.0.clone())
}

/// The extension's own choices for one tab: the tab's value, else the global one, else the defaults.
pub fn state_for(states: &ExtensionActionStates, id: &str, tab_id: i64) -> (bool, Option<Value>) {
    let Some(entry) = states.get(id) else { return (true, None) };
    let tab = entry.tabs.as_ref().and_then(|t| t.get(&tab_id.to_string()));
    (
        tab.and_then(|t| t.enabled).or(entry.state.enabled).unwrap_or(true),
        tab.and_then(|t| t.text_color.clone()).or_else(|| entry.state.text_color.clone()),
    )
}

/// Moves `id` to slot `to` among the pinned `shown`, and writes the new order back into the full order
/// (unpinned extensions keep their place).
pub fn reorder(all: &[String], pinned: &[String], from: usize, to: usize) -> Vec<String> {
    let mut next: Vec<String> = all.iter().filter(|id| pinned.contains(id)).cloned().collect();
    if from >= next.len() || to >= next.len() {
        return all.to_vec();
    }
    let item = next.remove(from);
    next.insert(to, item);
    let mut full = all.to_vec();
    let mut it = next.into_iter();
    for slot in full.iter_mut().filter(|id| pinned.contains(id)) {
        if let Some(n) = it.next() {
            *slot = n;
        }
    }
    full
}

/// Full order (unpinned included): saved order restricted to existing actions, then the new ones.
pub fn full_order(ids: &[String], saved: &[String]) -> Vec<String> {
    saved.iter().filter(|id| ids.contains(id)).cloned().chain(ids.iter().filter(|id| !saved.contains(id)).cloned()).collect()
}

/// Payload of a drag in progress.
#[derive(Clone)]
struct ExtDrag {
    id: String,
    label: SharedString,
    owner: gpui::EntityId,
}

struct DragGhost(SharedString, BarColors);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::CONTROL)
            .bg(self.1.hover)
            .text_color(self.1.fg)
            .child(self.0.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())
    }
}

pub struct ExtensionButtons {
    partition: String,
    order: Vec<String>,
    pinned: Vec<String>,
    bar: BarColors,
    state: Option<BrowserActionState>,
    counts: ExtensionActionCounts,
    states: ExtensionActionStates,
    /// Order just dropped, shown until the saved one arrives.
    local: Option<Vec<String>>,
    /// Drag in progress: id and target slot.
    drag: Option<(String, usize)>,
    slots: Rc<RefCell<HashMap<String, Anchor>>>,
    list_anchor: Anchor,
    focus_tab: Rc<dyn Fn(&mut App)>,
    subscribed: Option<(String, Subscription)>,
    refresh_tx: mpsc::UnboundedSender<()>,
    pending_run: Option<(String, String)>,
}

impl ExtensionButtons {
    pub fn new(focus_tab: Rc<dyn Fn(&mut App)>, cx: &mut App) -> Entity<Self> {
        let (tx, mut rx) = mpsc::unbounded::<()>();
        let counts = api(cx).get_extension_action_counts();
        let states = api(cx).get_extension_action_states();
        let this = cx.new(|cx| {
            cx.subscribe(&ui_state(cx), |this: &mut Self, _, event: &UiEvent, cx| match event {
                UiEvent::ExtensionActionCounts(c) => {
                    this.counts = c.clone();
                    cx.notify();
                }
                UiEvent::ExtensionActionStates(s) => {
                    this.states = s.clone();
                    cx.notify();
                }
                UiEvent::RunExtensionAction { token, extension_id } => this.run_action(token.clone(), extension_id.clone(), cx),
                _ => {}
            })
            .detach();
            Self {
                partition: "_self".into(),
                order: vec![],
                pinned: vec![],
                bar: BarColors { bg: gpui::black(), fg: gpui::black(), fg2: gpui::black(), hover: gpui::black() },
                state: None,
                counts,
                states,
                local: None,
                drag: None,
                slots: Rc::default(),
                list_anchor: Anchor::default(),
                focus_tab,
                subscribed: None,
                refresh_tx: tx,
                pending_run: None,
            }
        });
        let weak = this.downgrade();
        cx.spawn(async move |cx| {
            while rx.next().await.is_some() {
                if weak.update(cx, |t, cx| t.refresh(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        this
    }

    /// Props from the toolbar. Cheap when nothing changed.
    pub fn set(&mut self, partition: &str, order: &[String], pinned: &[String], bar: BarColors, cx: &mut Context<Self>) {
        let changed = self.partition != partition || self.order != order || self.pinned != pinned || self.bar != bar;
        if self.order != order {
            self.local = None;
        }
        if self.partition != partition {
            self.partition = partition.to_string();
            self.state = None;
            self.subscribed = None;
        }
        self.order = order.to_vec();
        self.pinned = pinned.to_vec();
        self.bar = bar;
        if self.subscribed.is_none() {
            self.subscribe(cx);
        }
        if changed {
            cx.notify();
        }
    }

    fn subscribe(&mut self, cx: &mut Context<Self>) {
        let Some(src) = source(cx) else { return };
        let tx = self.refresh_tx.clone();
        let sub = src.subscribe(&self.partition, Box::new(move || {
            let _ = tx.unbounded_send(());
        }));
        self.subscribed = Some((self.partition.clone(), sub));
        self.refresh(cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(src) = source(cx) else { return };
        self.state = Some(src.state(&self.partition));
        cx.notify();
    }

    /// Ids of the extensions with a button, in display order (all, then those pinned).
    fn all(&self) -> Vec<String> {
        let ids: Vec<String> = self.state.iter().flat_map(|s| s.actions.iter().map(|a| a.id.clone())).collect();
        full_order(&ids, self.local.as_deref().unwrap_or(&self.order))
    }

    fn launch(&self, id: &str, anchor: Rect, cx: &mut App) {
        let tab = self.state.as_ref().and_then(|s| s.active_tab_id).unwrap_or(-1);
        // A disabled button (action.disable) does nothing.
        if !state_for(&self.states, id, tab).0 {
            return;
        }
        (self.focus_tab)(cx);
        if let Some(src) = source(cx) {
            src.activate(&self.partition, id, anchor);
        }
    }

    /// Keyboard shortcut of an extension (`_execute_action`): under its button, or under the Extensions button when
    /// the extension is not pinned.
    fn run_action(&mut self, token: String, id: String, cx: &mut Context<Self>) {
        self.pending_run = Some((token, id));
        cx.notify();
    }

    fn take_run(&mut self, cx: &mut Context<Self>) {
        let Some((token, id)) = self.pending_run.take() else { return };
        let anchor = self.slots.borrow().get(&id).map(|a| a.rect()).or_else(|| (self.list_anchor.bounds().size.width > px(0.0)).then(|| self.list_anchor.rect()));
        // Without a button, main gets no answer and opens the popup on its own.
        let Some(anchor) = anchor else { return };
        // Several toolbars can be mounted: the first to be acknowledged opens it.
        if api(cx).ack_extension_action(&token) {
            self.launch(&id, anchor, cx);
        }
    }

    fn commit(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to {
            return;
        }
        let full = reorder(&self.all(), &self.pinned, from, to);
        self.local = Some(full.clone());
        let mut patch = SettingsPatch::new();
        patch.insert("extensionOrder".into(), Value::from(full));
        api(cx).update_settings(patch);
    }
}

impl Render for ExtensionButtons {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.drag = None;
        }
        self.take_run(cx);
        let Some(state) = self.state.clone().filter(|s| !s.actions.is_empty()) else {
            return div().into_any_element();
        };
        let t = cx.signet().clone();
        let bar = self.bar;
        let tab_id = state.active_tab_id.unwrap_or(-1);
        let all = self.all();
        let mut shown: Vec<String> = all.iter().filter(|id| self.pinned.contains(id)).cloned().collect();
        let from = self.drag.as_ref().and_then(|(id, _)| shown.iter().position(|s| s == id));
        if let (Some(from), Some((_, to))) = (from, self.drag.as_ref()) {
            let item = shown.remove(from);
            shown.insert((*to).min(shown.len()), item);
        }
        let me = cx.entity_id();

        let list = {
            let anchor = self.list_anchor.clone();
            let this = cx.entity();
            div()
                .id("ext-list")
                .relative()
                .flex_none()
                .size(px(28.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(radius::CONTROL)
                .text_color(bar.fg2)
                .hover(move |s| s.bg(bar.hover).text_color(bar.fg))
                .tooltip(move |w, cx| Tooltip::new(tr("toolbar.extensions")).build(w, cx))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, _, cx| {
                    let rect = anchor.rect();
                    let this = this.clone();
                    // The Extensions button opens a menu; the chosen extension then opens under this button.
                    this.update(cx, |t, cx| (t.focus_tab)(cx));
                    if let Some(id) = api(cx).show_extensions_menu() {
                        this.update(cx, |t, cx| t.launch(&id, rect, cx));
                    }
                })
                .child(Icon::from(IconName::Puzzle).size(px(15.0)))
                .child(self.list_anchor.track())
        };
        if shown.is_empty() {
            return list.into_any_element();
        }

        let mut buttons = Vec::new();
        for id in shown.iter() {
            let Some(a) = state.actions.iter().find(|a| &a.id == id) else { continue };
            let over = (tab_id > -1).then(|| a.tabs.get(&tab_id)).flatten();
            let title = over.and_then(|o| o.title.clone()).or_else(|| a.title.clone()).unwrap_or_default();
            let mut text = over.and_then(|o| o.text.clone()).or_else(|| a.text.clone()).unwrap_or_default();
            // The extension's own badge text wins; otherwise the blocked-request count of the tab shown by this bar.
            if text.is_empty() {
                text = count_text(self.counts.get(&tab_id.to_string()).and_then(|m| m.get(id)).copied());
            }
            let color = over.and_then(|o| o.color.clone()).or_else(|| a.color.clone());
            let (enabled, text_color) = state_for(&self.states, id, tab_id);
            let anchor = self.slots.borrow_mut().entry(id.clone()).or_default().clone();
            let held = self.drag.as_ref().is_some_and(|(d, _)| d == id);
            let icon: gpui::AnyElement = match &a.icon {
                Some(src) => Favicon::new(Some(src.clone())).size(16.0).into_any_element(),
                None => div()
                    .size(px(18.0))
                    .rounded(radius::ICON)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::rgb(0x757575))
                    .text_color(gpui::white())
                    .text_size(t.type_scale.cap)
                    .child(title.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())
                    .into_any_element(),
            };
            let drag = ExtDrag { id: id.clone(), label: title.clone().into(), owner: me };
            let this = cx.entity();
            let (click_id, click_anchor, menu_id) = (id.clone(), anchor.clone(), id.clone());
            let this2 = this.clone();
            let badge = (!text.is_empty()).then(|| {
                let (bg, fg) = badge_colors(color.as_ref(), text_color.as_ref()).unwrap_or((gpui::rgb(0x5f6368).into(), gpui::white()));
                div()
                    .absolute()
                    .right(px(3.0))
                    .bottom(px(4.0))
                    .h(px(8.0))
                    .px(px(1.5))
                    .rounded(px(2.0))
                    .bg(bg)
                    .text_color(fg)
                    .text_size(px(6.5))
                    .line_height(px(8.0))
                    .font_weight(gpui::FontWeight(600.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(text.clone())
            });
            buttons.push(
                div()
                    .id(SharedString::from(format!("ext-{id}")))
                    .relative()
                    .flex_none()
                    .size(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(radius::CONTROL)
                    .text_color(bar.fg)
                    .when(held, |d| d.bg(bar.hover))
                    .when(!enabled, |d| d.opacity(0.35))
                    .hover(move |s| s.bg(bar.hover))
                    .tooltip({
                        let title = title.clone();
                        move |w, cx| Tooltip::new(title.clone()).build(w, cx)
                    })
                    .on_drag(drag, move |d: &ExtDrag, _, _, cx| cx.new(|_| DragGhost(d.label.clone(), bar)))
                    .on_drag_move::<ExtDrag>(cx.listener(move |this, e: &gpui::DragMoveEvent<ExtDrag>, _, cx| {
                        let d = e.drag(cx).clone();
                        if d.owner != me {
                            return;
                        }
                        // Slot whose center is nearest to the pointer.
                        let x = e.event.position.x;
                        let slots = this.slots.borrow();
                        let mut best: Option<(usize, f32)> = None;
                        let ids: Vec<String> = this.all().into_iter().filter(|id| this.pinned.contains(id)).collect();
                        for (slot, id) in ids.iter().enumerate() {
                            if let Some(a) = slots.get(id) {
                                let b = a.bounds();
                                let center = f32::from(b.origin.x) + f32::from(b.size.width) / 2.0;
                                let dist = (center - f32::from(x)).abs();
                                if best.is_none_or(|(_, d)| dist < d) {
                                    best = Some((slot, dist));
                                }
                            }
                        }
                        drop(slots);
                        if let Some((slot, _)) = best {
                            if this.drag.as_ref().map(|(id, s)| (id.as_str(), *s)) != Some((d.id.as_str(), slot)) {
                                this.drag = Some((d.id.clone(), slot));
                                cx.notify();
                            }
                        }
                    }))
                    .on_drop(cx.listener(move |this, d: &ExtDrag, _, cx| {
                        if d.owner != me {
                            return;
                        }
                        let ids: Vec<String> = this.all().into_iter().filter(|id| this.pinned.contains(id)).collect();
                        let from = ids.iter().position(|id| *id == d.id);
                        let to = this.drag.as_ref().map(|(_, s)| *s);
                        this.drag = None;
                        if let (Some(from), Some(to)) = (from, to) {
                            this.commit(from, to, cx);
                        }
                        cx.notify();
                    }))
                    .on_click(move |_, _, cx| {
                        let rect = click_anchor.rect();
                        let id = click_id.clone();
                        this.update(cx, |t, cx| t.launch(&id, rect, cx));
                    })
                    .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                        cx.stop_propagation();
                        let id = menu_id.clone();
                        this2.update(cx, |t, cx| (t.focus_tab)(cx));
                        api(cx).show_extension_menu(&id);
                    })
                    .child(div().when(!enabled, |d| d.opacity(0.35)).child(icon))
                    .children(badge)
                    .child(anchor.track()),
            );
        }
        let _ = window;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.0))
            .child(
                div()
                    .id("ext-actions")
                    .flex()
                    .gap(px(2.0))
                    .min_w_0()
                    .overflow_hidden()
                    .mr(px(2.0))
                    .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                        if this.drag.take().is_some() {
                            cx.notify();
                        }
                    }))
                    .children(buttons),
            )
            .child(list)
            .into_any_element()
    }
}

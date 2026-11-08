//! Port of `lib/optimistic.ts`: optimistic selection. The chosen tab or space is shown as soon as the command is
//! sent, without waiting for the app's state. It drops at the first update received after the command returned, or
//! after a short delay if the app refused (no state arrives then).
//!
//! [`OptimisticState`] is the pure machine (tested); [`Optimistic`] is the GPUI model that owns the timers.

use gpui::{App, AppContext, Context, Entity, Global};
use std::time::Duration;

use crate::app_state::{api, ui_state};
use signet_core::api::UiEvent;

/// How long the optimistic value survives once the command returned.
pub const SETTLE_DELAY_MS: u64 = 400;
/// How long a space chosen elsewhere is adopted if no state arrives.
pub const ADOPT_DELAY_MS: u64 = 1000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    pub tab_id: Option<String>,
    pub space_id: Option<String>,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.tab_id.is_none() && self.space_id.is_none()
    }
}

/// The pure state machine. Methods return `true` when the pending value changed (subscribers must be notified).
#[derive(Default)]
pub struct OptimisticState {
    pending: Pending,
    seq: u64,
    landed: bool,
}

impl OptimisticState {
    pub fn pending(&self) -> &Pending {
        &self.pending
    }

    /// A command is sent: its value shows at once. Returns the command's sequence number.
    pub fn begin(&mut self, patch: Pending, keep_tab: bool) -> u64 {
        self.seq += 1;
        self.landed = false;
        let mut next = self.pending.clone();
        if patch.tab_id.is_some() || !keep_tab {
            next.tab_id = patch.tab_id;
        }
        if patch.space_id.is_some() {
            next.space_id = patch.space_id;
        }
        self.pending = next;
        self.seq
    }

    /// The command returned: the next state received (or the delay) drops the value.
    /// Returns false if a more recent command silences this one.
    pub fn landed(&mut self, mine: u64) -> bool {
        if mine != self.seq {
            return false;
        }
        self.landed = true;
        true
    }

    /// Drops the value of command `mine` (delay elapsed, or the command failed). Returns true if something changed.
    pub fn drop_seq(&mut self, mine: u64) -> bool {
        if mine != self.seq {
            return false;
        }
        self.landed = false;
        // Nothing pending: no notification, hence no useless render.
        if self.pending.is_empty() {
            return false;
        }
        self.pending = Pending::default();
        true
    }

    /// To call on each state received: the optimistic value is stale as soon as the app has responded.
    pub fn settle(&mut self) -> bool {
        if self.landed { self.drop_seq(self.seq) } else { false }
    }

    /// Hint from the app: a space is chosen outside this window (overlay popover). Drops at the next state.
    /// Returns the sequence number to use with [`OptimisticState::drop_seq`] after [`ADOPT_DELAY_MS`].
    pub fn adopt_pending_space(&mut self, space_id: &str) -> u64 {
        self.seq += 1;
        self.landed = true; // the next state received makes it stale
        self.pending = Pending { space_id: Some(space_id.to_string()), tab_id: None };
        self.seq
    }
}

/// GPUI model around [`OptimisticState`]. One per app, reachable with [`optimistic`].
pub struct Optimistic {
    state: OptimisticState,
    _subs: Vec<gpui::Subscription>,
}

struct OptimisticGlobal(Entity<Optimistic>);
impl Global for OptimisticGlobal {}

/// The shared model (created on first use). Observe it to re-render when the selection changes.
pub fn optimistic(cx: &mut App) -> Entity<Optimistic> {
    if let Some(g) = cx.try_global::<OptimisticGlobal>() {
        return g.0.clone();
    }
    let ui = ui_state(cx);
    let model = cx.new(|cx: &mut Context<Optimistic>| {
        // Every state received settles the optimistic value; a space picked elsewhere is adopted.
        let s1 = cx.observe(&ui, |m, _, cx| {
            if m.state.settle() {
                cx.notify();
            }
        });
        let s2 = cx.subscribe(&ui, |m, _, ev: &UiEvent, cx| {
            if let UiEvent::PendingSpace(id) = ev {
                let seq = m.state.adopt_pending_space(id);
                cx.notify();
                m.expire_after(seq, ADOPT_DELAY_MS, cx);
            }
        });
        Optimistic { state: OptimisticState::default(), _subs: vec![s1, s2] }
    });
    cx.set_global(OptimisticGlobal(model.clone()));
    model
}

impl Optimistic {
    pub fn pending(&self) -> &Pending {
        self.state.pending()
    }

    fn expire_after(&mut self, seq: u64, ms: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(ms)).await;
            this.update(cx, |m, cx| {
                if m.state.drop_seq(seq) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn run(&mut self, patch: Pending, keep_tab: bool, send: impl FnOnce(), cx: &mut Context<Self>) {
        let seq = self.state.begin(patch, keep_tab);
        cx.notify();
        send();
        if self.state.landed(seq) {
            self.expire_after(seq, SETTLE_DELAY_MS, cx);
        }
    }
}

/// Activates a tab, showing it as selected right away.
pub fn activate_tab(tab_id: &str, cx: &mut App) {
    let model = optimistic(cx);
    let a = api(cx);
    let id = tab_id.to_string();
    model.update(cx, |m, cx| m.run(Pending { tab_id: Some(id.clone()), space_id: None }, true, move || a.activate_tab(&id), cx));
}

/// Switches space, showing it as selected right away. The tab choice is forgotten (the space restores its own).
pub fn switch_space(space_id: &str, cx: &mut App) {
    let model = optimistic(cx);
    let a = api(cx);
    let id = space_id.to_string();
    model.update(cx, |m, cx| m.run(Pending { tab_id: None, space_id: Some(id.clone()) }, false, move || a.switch_space(&id), cx));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: &str) -> Pending {
        Pending { tab_id: Some(id.into()), space_id: None }
    }

    #[test]
    fn empty_at_the_start() {
        assert_eq!(*OptimisticState::default().pending(), Pending::default());
    }

    #[test]
    fn activate_shows_the_tab_right_away() {
        let mut s = OptimisticState::default();
        s.begin(tab("t1"), true);
        assert_eq!(s.pending().tab_id.as_deref(), Some("t1"));
    }

    #[test]
    fn drops_at_the_first_state_received_after_the_response() {
        let mut s = OptimisticState::default();
        let seq = s.begin(tab("t1"), true);
        // State received before the response: we keep it.
        assert!(!s.settle());
        assert_eq!(s.pending().tab_id.as_deref(), Some("t1"));
        assert!(s.landed(seq));
        assert!(s.settle());
        assert_eq!(*s.pending(), Pending::default());
    }

    #[test]
    fn drops_when_the_delay_elapses_if_no_state_arrives() {
        let mut s = OptimisticState::default();
        let seq = s.begin(tab("t1"), true);
        assert!(s.landed(seq));
        assert!(s.drop_seq(seq));
        assert_eq!(*s.pending(), Pending::default());
    }

    #[test]
    fn drops_right_away_if_the_command_fails() {
        let mut s = OptimisticState::default();
        let seq = s.begin(tab("t1"), true);
        assert!(s.drop_seq(seq));
        assert_eq!(*s.pending(), Pending::default());
    }

    #[test]
    fn switch_space_forgets_the_tab_choice() {
        let mut s = OptimisticState::default();
        s.begin(tab("t1"), true);
        s.begin(Pending { tab_id: None, space_id: Some("s2".into()) }, false);
        assert_eq!(*s.pending(), Pending { tab_id: None, space_id: Some("s2".into()) });
    }

    #[test]
    fn a_more_recent_command_silences_the_previous_one() {
        let mut s = OptimisticState::default();
        let first = s.begin(tab("t1"), true);
        let second = s.begin(tab("t2"), true);
        assert!(!s.landed(first));
        assert!(!s.drop_seq(first)); // the delay of t1 removed nothing
        assert_eq!(s.pending().tab_id.as_deref(), Some("t2"));
        assert!(s.landed(second));
    }

    #[test]
    fn settle_with_nothing_pending_does_not_notify() {
        let mut s = OptimisticState::default();
        assert!(!s.settle());
        let seq = s.begin(tab("t1"), true);
        s.landed(seq);
        s.settle();
        assert!(!s.settle());
    }

    #[test]
    fn adopt_pending_space_sets_the_space_and_expires_at_the_next_state() {
        let mut s = OptimisticState::default();
        s.adopt_pending_space("s3");
        assert_eq!(*s.pending(), Pending { tab_id: None, space_id: Some("s3".into()) });
        assert!(s.settle());
        assert_eq!(*s.pending(), Pending::default());
    }

    #[test]
    fn adopt_pending_space_drops_after_the_delay_if_the_app_refused() {
        let mut s = OptimisticState::default();
        let seq = s.adopt_pending_space("s3");
        assert!(s.drop_seq(seq));
        assert_eq!(*s.pending(), Pending::default());
    }
}

//! Drives the CEF message loop from the GPUI foreground executor.
//!
//! CEF is initialized with `external_message_pump`: it calls `OnScheduleMessagePumpWork(delay)` (any
//! thread) when it wants `do_message_loop_work` to run. We forward those requests to an unbounded
//! channel read by one foreground task, which sleeps until the requested deadline (or a fallback
//! interval) and then pumps CEF on the main thread and delivers queued [`WebEvent`]s.
//! The task only wakes up when CEF asks for it or every [`FALLBACK`], so it does not busy-spin.

use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use futures::future::{select, Either};

use crate::types::WebEvent;

/// Idle poll interval used when CEF has not scheduled any work.
pub const FALLBACK: Duration = Duration::from_millis(100);

/// Install the pump on the GPUI application. Call after [`crate::init`].
/// `on_event` runs on the main thread for every event, with access to the GPUI app.
/// Also registers a quit hook that closes browsers and shuts CEF down.
pub fn install(cx: &mut gpui::App, mut on_event: impl FnMut(WebEvent, &mut gpui::AsyncApp) + 'static) {
    let (tx, mut rx) = unbounded::<i64>();
    crate::set_wake_handler(move |delay| {
        let _ = tx.unbounded_send(delay);
    });
    cx.spawn(async move |cx| {
        let mut next: Option<Instant> = None;
        loop {
            let now = Instant::now();
            let wait = next.map(|t| t.saturating_duration_since(now)).unwrap_or(FALLBACK).min(FALLBACK);
            let mut due = wait.is_zero();
            if !due {
                let timer = cx.background_executor().timer(wait);
                match select(Box::pin(rx.next()), Box::pin(timer)).await {
                    Either::Left((Some(d), _)) => merge(&mut next, d),
                    Either::Left((None, _)) => return,
                    Either::Right(_) => due = true,
                }
            }
            while let Ok(d) = rx.try_recv() {
                merge(&mut next, d);
            }
            let now = Instant::now();
            if due || next.is_some_and(|t| t <= now) {
                next = None;
                crate::startup::once("pump.first_tick");
                crate::pump();
                for ev in crate::drain_events() {
                    on_event(ev, cx);
                }
            }
        }
    })
    .detach();
    cx.on_app_quit(|_| async {
        crate::shutdown();
    })
    .detach();
}

fn merge(next: &mut Option<Instant>, delay_ms: i64) {
    let t = Instant::now() + Duration::from_millis(delay_ms.max(0) as u64);
    *next = Some(next.map_or(t, |n| n.min(t)));
}

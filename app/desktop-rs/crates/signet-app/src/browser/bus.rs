//! Event bus to the interface: `SignetApi::subscribe` listeners and the `UiEvent` fan-out.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use signet_core::api::{Subscription, UiEvent};

type Listener = Arc<dyn Fn(UiEvent) + Send + Sync>;

#[derive(Clone, Default)]
pub struct Bus {
    listeners: Arc<Mutex<Vec<(u64, Listener)>>>,
    next: Arc<AtomicU64>,
}

impl Bus {
    pub fn subscribe(&self, f: Box<dyn Fn(UiEvent) + Send + Sync>) -> Subscription {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut l) = self.listeners.lock() {
            l.push((id, Arc::from(f)));
        }
        let listeners = self.listeners.clone();
        Subscription::new(move || {
            if let Ok(mut l) = listeners.lock() {
                l.retain(|(i, _)| *i != id);
            }
        })
    }

    pub fn emit(&self, event: UiEvent) {
        let list: Vec<Listener> = self.listeners.lock().map(|l| l.iter().map(|(_, f)| f.clone()).collect()).unwrap_or_default();
        for f in list {
            f(event.clone());
        }
    }
}

//! BLE scanning for the phone's caBLE advert (replaces BlueZ D-Bus on Linux, CoreBluetooth + Swift helper on macOS).
//! `btleplug` covers all three systems: BlueZ (Linux), CoreBluetooth (macOS), WinRT (Windows).
//!
//! The phone advertises service data on the 16-bit UUIDs 0xFFF9 / 0xFDE2 (see `eid::CABLE_UUIDS`). Service-data-only
//! adverts may not match a UUID filter, so the scan is unfiltered and the data is filtered here.

use std::sync::Arc;
use std::time::Duration;

use btleplug::api::{Central, CentralEvent, CentralState, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager};
use futures_util::StreamExt;
use serde::Serialize;
use tokio::runtime::Handle;
use tokio::sync::watch;
use uuid::Uuid;

use super::eid::CABLE_UUIDS;

/// State of the Bluetooth scan, as the passkey window shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanState {
    /// Scanning, nothing found yet.
    Waiting,
    /// Bluetooth is turned off (the app never turns it on).
    Off,
    NoAdapter,
    /// The user (or the system) denied Bluetooth to the app.
    Denied,
    Error,
    /// No scan on this system.
    Unsupported,
}

pub type OnData = Arc<dyn Fn(Vec<u8>) + Send + Sync>;
pub type OnState = Arc<dyn Fn(ScanState) + Send + Sync>;
pub type StopFn = Box<dyn FnOnce() + Send>;

/// Scans for adverts while a hybrid session is open.
pub trait BleScanner: Send + Sync {
    /// Starts a scan. `on_data` gets the service data of each candidate advert, `on_state` the state of the scan.
    /// Returns the function that stops it.
    fn start(&self, on_data: OnData, on_state: OnState) -> StopFn;
}

/// Always reports `Unsupported`.
pub struct NoScanner;
impl BleScanner for NoScanner {
    fn start(&self, _: OnData, on_state: OnState) -> StopFn {
        on_state(ScanState::Unsupported);
        Box::new(|| {})
    }
}

/// btleplug scanner; the work runs on the given runtime.
pub struct BtleplugScanner {
    handle: Handle,
}

impl BtleplugScanner {
    pub fn new(handle: Handle) -> Self {
        Self { handle }
    }
}

fn wanted(uuid: &Uuid) -> bool {
    let s = uuid.to_string();
    CABLE_UUIDS.contains(&s.as_str())
}

fn looks_denied(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("denied") || m.contains("permission") || m.contains("unauthorized") || m.contains("not authorized")
}

impl BleScanner for BtleplugScanner {
    fn start(&self, on_data: OnData, on_state: OnState) -> StopFn {
        let (stop_tx, stop_rx) = watch::channel(false);
        self.handle.spawn(async move {
            if let Err(state) = scan(on_data, on_state.clone(), stop_rx).await {
                on_state(state);
            }
        });
        Box::new(move || {
            let _ = stop_tx.send(true);
        })
    }
}

async fn first_adapter() -> Result<Adapter, ScanState> {
    let manager = Manager::new().await.map_err(|e| if looks_denied(&e.to_string()) { ScanState::Denied } else { ScanState::Error })?;
    let adapters = manager.adapters().await.map_err(|e| if looks_denied(&e.to_string()) { ScanState::Denied } else { ScanState::Error })?;
    adapters.into_iter().next().ok_or(ScanState::NoAdapter)
}

async fn forward_properties(adapter: &Adapter, id: &btleplug::platform::PeripheralId, on_data: &OnData) {
    if let Ok(p) = adapter.peripheral(id).await {
        if let Ok(Some(props)) = p.properties().await {
            for (uuid, data) in props.service_data {
                if wanted(&uuid) {
                    on_data(data);
                }
            }
        }
    }
}

async fn scan(on_data: OnData, on_state: OnState, mut stop: watch::Receiver<bool>) -> Result<(), ScanState> {
    let adapter = tokio::select! {
        a = first_adapter() => a?,
        _ = stop.changed() => return Ok(()),
    };
    let mut events = adapter.events().await.map_err(|_| ScanState::Error)?;
    let mut scanning = false;
    let mut reported: Option<ScanState> = None;
    let report = |s: ScanState, reported: &mut Option<ScanState>| {
        if *reported != Some(s) {
            *reported = Some(s);
            on_state(s);
        }
    };
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = tick.tick() => {
                match adapter.adapter_state().await {
                    Ok(CentralState::PoweredOff) => {
                        if scanning {
                            let _ = adapter.stop_scan().await;
                            scanning = false;
                        }
                        report(ScanState::Off, &mut reported);
                    }
                    Ok(CentralState::PoweredOn) => {
                        if !scanning {
                            match adapter.start_scan(ScanFilter::default()).await {
                                Ok(()) => {
                                    scanning = true;
                                    report(ScanState::Waiting, &mut reported);
                                }
                                Err(e) => {
                                    report(if looks_denied(&e.to_string()) { ScanState::Denied } else { ScanState::Error }, &mut reported);
                                }
                            }
                        }
                    }
                    // Not known yet (CoreBluetooth before its first update): keep waiting.
                    _ => report(ScanState::Waiting, &mut reported),
                }
            }
            ev = events.next() => match ev {
                Some(CentralEvent::ServiceDataAdvertisement { service_data, .. }) => {
                    for (uuid, data) in service_data {
                        if wanted(&uuid) {
                            on_data(data);
                        }
                    }
                }
                Some(CentralEvent::DeviceDiscovered(id)) | Some(CentralEvent::DeviceUpdated(id)) => {
                    forward_properties(&adapter, &id, &on_data).await;
                }
                Some(CentralEvent::StateUpdate(CentralState::PoweredOff)) => {
                    scanning = false;
                    report(ScanState::Off, &mut reported);
                }
                Some(_) => {}
                None => {
                    report(ScanState::Error, &mut reported);
                    break;
                }
            },
        }
    }
    if scanning {
        let _ = adapter.stop_scan().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_cable_uuids_are_wanted() {
        assert!(wanted(&Uuid::parse_str("0000fff9-0000-1000-8000-00805f9b34fb").unwrap()));
        assert!(wanted(&Uuid::parse_str("0000fde2-0000-1000-8000-00805f9b34fb").unwrap()));
        assert!(!wanted(&Uuid::parse_str("0000180f-0000-1000-8000-00805f9b34fb").unwrap()));
    }

    #[test]
    fn denied_is_recognized_in_error_text() {
        assert!(looks_denied("Permission denied"));
        assert!(looks_denied("org.bluez.Error.NotAuthorized: not authorized"));
        assert!(!looks_denied("adapter not found"));
    }

    #[test]
    fn no_scanner_reports_unsupported() {
        let seen = Arc::new(parking_lot::Mutex::new(vec![]));
        let s2 = seen.clone();
        let stop = NoScanner.start(Arc::new(|_| {}), Arc::new(move |s| s2.lock().push(s)));
        stop();
        assert_eq!(*seen.lock(), vec![ScanState::Unsupported]);
    }
}

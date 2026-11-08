//! mDNS announcement (`_signet._tcp`): the phone finds the computer on the local network.

use std::collections::HashMap;

use mdns_sd::{ServiceDaemon, ServiceInfo};

use super::protocol::{LINK_PROTOCOL_VERSION, LINK_SERVICE_TYPE};

#[derive(Default)]
pub struct Discovery {
    daemon: Option<ServiceDaemon>,
    key: String,
}

impl Discovery {
    /// Announces (or re-announces if the name or port changed). Failure is silent: the phone falls back to the QR code addresses.
    pub fn publish(&mut self, name: &str, id: &str, port: u16) {
        let key = format!("{name}\n{id}\n{port}");
        if self.daemon.is_some() && key == self.key {
            return;
        }
        self.unpublish();
        let Ok(daemon) = ServiceDaemon::new() else { return };
        let props: HashMap<String, String> = HashMap::from([("v".to_string(), LINK_PROTOCOL_VERSION.to_string()), ("id".to_string(), id.to_string())]);
        let host: String = {
            let h: String = id.chars().take(12).collect();
            format!("signet-{h}.local.")
        };
        let ty = format!("_{LINK_SERVICE_TYPE}._tcp.local.");
        let info = match ServiceInfo::new(&ty, name, &host, "", port, props) {
            Ok(i) => i.enable_addr_auto(),
            Err(_) => {
                let _ = daemon.shutdown();
                return;
            }
        };
        if daemon.register(info).is_err() {
            let _ = daemon.shutdown();
            return;
        }
        self.daemon = Some(daemon);
        self.key = key;
    }

    pub fn unpublish(&mut self) {
        self.key.clear();
        if let Some(d) = self.daemon.take() {
            let _ = d.shutdown();
        }
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.unpublish();
    }
}

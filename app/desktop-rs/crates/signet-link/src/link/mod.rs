//! Phone link: pairing, control connection and localhost tunnels (port of app/desktop/src/main/features/link).

pub mod certs;
pub mod devservers;
pub mod discovery;
pub mod identity;
pub mod loopback;
pub mod manager;
pub mod protocol;
pub mod status;
pub mod store;
pub mod service;
#[cfg(test)]
mod service_tests;

//! Passkeys with a phone: FIDO hybrid (caBLE v2) over CTAP2 (port of app/desktop/src/main/web/hybrid).
//!
//! Entry points: `session::webauthn_precheck` (validate the page's request), `session::HybridManager::start_hybrid`
//! (QR text + events), `session::WebAuthnAnswer::to_json` (answer for the page script).

pub mod ble;
pub mod cbor;
pub mod ctap;
pub mod eid;
pub mod noise;
pub mod qr;
pub mod session;
pub mod tunnel;
pub mod webauthn_request;

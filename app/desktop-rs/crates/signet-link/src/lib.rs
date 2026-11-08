//! signet-link: phone link and passkeys over the phone (hybrid / caBLE v2, CTAP2). No UI, no CEF.
//! See README.md.

pub mod b64;
pub mod hybrid;
pub mod link;
pub mod qrimage;

pub use hybrid::session::{HybridDeps, HybridEvent, HybridHandle, HybridManager, PasskeyStatus, Precheck, WebAuthnAnswer, webauthn_precheck};
pub use hybrid::webauthn_request::{WebAuthnError, WebAuthnRequest};
pub use link::devservers::TabLike;
pub use link::manager::{DeviceInfo, LinkConfig, LinkEvent, LinkManager, LinkState, PairingInfo};
pub use link::status::LinkStatus;
pub use link::store::Vault;
pub use qrimage::QrMatrix;

/// Cryptographically secure random bytes.
pub(crate) fn rand_bytes<const N: usize>() -> [u8; N] {
    use rand_core::{OsRng, RngCore};
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

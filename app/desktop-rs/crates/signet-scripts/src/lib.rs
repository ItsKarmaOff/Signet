//! signet-scripts: the page scripts injected in web tabs (port of `app/desktop/src/preload/*`, except the trusted
//! interface preload and the extension APIs) and the Rust side of their channels.
//!
//! - [`bridge`]: names of the two hidden functions shared with `signet-web`, and helpers to build the JS to run in a frame.
//! - [`scripts`]: the scripts ([`all_scripts`], [`PageScript`]) and how they are bundled for a frame.
//! - [`messages`]: typed messages of every channel, page to browser and browser to page.
//! - [`validate`]: what the browser side must check before acting on a page message (sender, origin, sizes).
//! - [`webauthn_request`]: validation of a passkey request, as a browser does it.
//! - [`notes`]: validation and cleaning of what the Note and Easel pages save.

pub mod bridge;
pub mod messages;
pub mod notes;
pub mod scripts;
pub mod validate;
pub mod webauthn_request;

pub use messages::{Incoming, PageEvent, PageMessage, ParseError, Reply};
pub use scripts::{
    AppPage, FrameContext, Frames, PageScript, Profiles, Registry, RunAt, ScriptOptions, ScriptSettings, When, all_scripts,
    webauthn_intercept_enabled,
};
pub use validate::{Accepted, Refusal, Sender, validate};

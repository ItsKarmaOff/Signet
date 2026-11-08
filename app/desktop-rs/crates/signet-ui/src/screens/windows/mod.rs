//! Separate windows and window-level screens (port of `renderer/src/windows/*`). See PORTING.md section 8.
//!
//! Root views (`new(window, cx) -> Entity<Self>`, wrap each in `gpui_component::Root`):
//!
//! | Route      | View                         | Notes |
//! | ---------- | ---------------------------- | ----- |
//! | `#library` | [`library::Library`]         | history, archive, downloads, spaces |
//! | `#little`  | [`little::Little`]           | bar above a native web view |
//! | `#private` | [`private::Private`]         | tabs, address bar, empty state |
//! | `#pip`     | [`pip::Pip`]                 | transparent controls over the video |
//! | `#webapp`  | [`webapp::WebApp`]           | 38 px title bar of an installed app |
//! | `#welcome` | [`welcome::Welcome`]         | first-run introduction |
//! | `#passkey` | [`passkey::Passkey`]         | QR code window |
//!
//! Overlay contents (built by the overlay root, which gives them the whole overlay window):
//! [`site_panel::SitePanel`] (`site` mode), [`webapp_install::WebAppInstall`] (`webapp` mode).
//!
//! Shared pieces: [`downloads`] (row, panel, progress and "unseen" helpers for the sidebar), [`recovery`] (fallback view and
//! retry rule), [`common`] (bar colors, buttons, drag area), [`qr`], [`data_url`], [`dates`], [`icon_compose`].

pub mod common;
pub mod data_url;
pub mod dates;
pub mod downloads;
pub mod icon_compose;
pub mod library;
pub mod little;
pub mod passkey;
pub mod pip;
pub mod private;
pub mod qr;
pub mod recovery;
pub mod site_panel;
pub mod webapp;
pub mod webapp_install;
pub mod welcome;

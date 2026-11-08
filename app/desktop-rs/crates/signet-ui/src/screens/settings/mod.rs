//! Settings window (`#settings`) and the space form (port of `renderer/src/settings/*`).
//!
//! - [`SettingsWindow`]: root view, `SettingsWindow::new(window, cx)`; `open_section("passwords", window, cx)` shows
//!   a section (the app calls it when another part asks to open Settings at a section).
//! - [`SpaceFormView`]: reusable space creation / edition form, hosted by the overlay or a small window
//!   (`SpaceFormView::for_space_id(space_id, window, cx)` for `UiEvent::EditSpace`; see `space_form.rs`).
//! - [`LiveFolderForm`]: reusable live folder form (`LiveFolderForm::new(folder_id, window, cx)`), same hosting rules.

mod appearance;
mod autofill;
mod backup;
mod boosts;
mod developer;
mod devices;
mod dns;
mod extensions;
mod general;
mod import;
mod kit;
mod languages;
mod live_folder;
mod mods;
mod passwords;
mod privacy;
mod routing;
mod search;
mod shortcuts;
mod site_permissions;
mod space_form;
mod web_apps;
mod window;

pub use live_folder::{LiveFolderForm, LiveFolderFormClosed};
pub use space_form::{SpaceFormClosed, SpaceFormView};
pub use window::{SectionId, SettingsWindow};

//! Overlay surfaces (port of `renderer/src/overlay/*`): the `#overlay` route (command bar, popovers, Peek, capture,
//! password prompt, zoom bubble), the Boost editor and the small `#find`, `#toast` and `#quit` views.

mod common;
pub mod inline;
pub mod keyword_mode;
pub mod palette;
pub(crate) mod forms;
mod downloads;
pub mod popover;
mod password;
mod site;
mod zoom;
mod webapp;
mod peek;
mod capture;
mod root;
pub mod boost;
pub mod find;
pub mod quit;
pub mod toast;

pub use boost::BoostWindow;
pub use find::FindBar;
pub use quit::QuitHud;
pub use root::OverlayRoot;
pub use toast::{ToastView, sidebar_on};

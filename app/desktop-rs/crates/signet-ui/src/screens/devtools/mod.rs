//! Developer tools chrome (port of `renderer/src/devtools/*`). See PORTING.md section 8.
//!
//! - [`DevToolsCard`]: docked tools card and resize handle of a pane (created by the main window per pane).
//! - [`DevToolsWindow`]: root view of the detached tools window (`#devtools`).
//! - [`ResponsiveFrame`]: responsive design mode bar and handles of a pane (created by the main window per pane).

pub mod card;
pub mod devices;
pub mod pointer;
pub mod responsive;

pub use card::{DEVTOOLS_BAR, DevToolsCard, DevToolsWindow};
pub use responsive::ResponsiveFrame;

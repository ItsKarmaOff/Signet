//! Pane bar (toolbar), bookmarks bar and extension buttons: port of `renderer/src/toolbar/*`.
//!
//! - [`Toolbar`] is a view per bar; the main window creates one per pane and updates it with [`Toolbar::set_props`].
//! - [`BookmarksBar`] is the bar under a pane's address bar.
//! - [`ExtensionButtons`] draws the pinned extensions' buttons; the app provides their state with
//!   [`install_browser_actions`].

mod anchor;
mod bookmarks;
mod extensions;
mod icons;
mod toolbar;
mod util;

pub use anchor::Anchor;
pub use bookmarks::{BookmarkDrag, BookmarksBar};
pub use extensions::{ActionOverride, BrowserAction, BrowserActionState, BrowserActions, ExtensionButtons, install_browser_actions};
pub use icons::{favicon_source, image_from_data_url};
pub use toolbar::{Toolbar, ToolbarProps, lucide};
pub use util::{BOOKMARK_BAR, BarColors, BarFade, IS_MAC, PANE_BAR, bar_colors};

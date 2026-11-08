//! Signet-styled building blocks. All widgets read the theme from the global ([`crate::ThemeExt`]) and take
//! already-translated strings: no visible English text is hard-coded here.

mod dialog;
mod emoji_picker;
mod empty_state;
mod fade;
mod favicon;
mod icon_button;
mod inline_confirm;
mod list_row;
mod media_card;
mod menu;
mod popover;
mod progress;
mod section;
mod segmented;
mod shortcut;
mod slider;
mod text_field;
mod toast;
mod toggle;

pub use dialog::{ConfirmDialog, confirm_dialog, modal_surface};
pub use emoji_picker::{EmojiPicked, EmojiPicker, EmojiPickerLabels};
pub use empty_state::EmptyState;
pub use fade::{FadeEdge, edge_fade, vertical_fades};
pub use favicon::{Favicon, FaviconSource};
pub use icon_button::{IconButton, IconButtonSize};
pub use inline_confirm::InlineConfirm;
pub use list_row::{DropEdge, ListRow};
pub use media_card::{MediaCard, MediaChip, MediaInfo, MediaLabels};
pub use menu::{MenuEntry, build_menu, context_menu, dropdown_menu};
pub use popover::{popover_content, popover_surface};
pub use progress::{ProgressBar, Spinner};
pub use section::{Separator, SectionHeader};
pub use segmented::Segmented;
pub use shortcut::{ShortcutLabel, format_accelerator};
pub use slider::{SliderField, slider_state};
pub use text_field::{TextField, text_field_state};
pub use toast::Toast;
pub use toggle::Toggle;

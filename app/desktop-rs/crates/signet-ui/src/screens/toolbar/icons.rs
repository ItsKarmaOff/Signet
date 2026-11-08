//! Favicons of bookmarks and tabs arrive as `data:` URLs or web URLs; GPUI loads only the second kind directly.
//! The decoding lives in `crate::data_url`.

pub use crate::data_url::{favicon_source, image_from_data_url};

//! signet-web: generic CEF layer (startup, child browser views, profiles, events, page scripts,
//! custom schemes). See README.md. No dependency on other signet crates.

mod app;
mod bundle;
mod cdp;
mod client;
mod extension;
mod hooks;
mod media;
#[cfg(target_os = "macos")]
pub mod mac;
mod cors_proxy;
mod platform;
mod profile;
mod runtime;
mod scheme;
pub mod startup;
mod subprocess;
mod types;
mod view;
mod view_ext;
pub mod overlay;

#[cfg(target_os = "windows")]
pub mod keys_win;
#[cfg(target_os = "linux")]
pub mod keys_linux;

#[cfg(feature = "gpui")]
pub mod gpui_pump;

pub use app::BRIDGE_FUNCTION;
pub use profile::{
    clear_profile_cache, clear_profile_data, delete_cookies, drop_profile, set_spellcheck,
};
pub use runtime::{
    default_user_agent, drain_events, init, is_initialized, pump, set_certificate_handler, set_key_filter, set_key_observer, set_page_scripts,
    set_scheme_handler, set_wake_handler, shutdown, Config,
};
pub use cors_proxy::set_cors_exemption;
pub use extension::{set_extension_handler, set_preference_json, ExtensionRequest, ExtensionResponse, EXTENSION_SCHEME};
pub use scheme::CUSTOM_SCHEMES;
pub use subprocess::run_subprocess;
pub use hooks::{
    clear_frame_bundle, clear_request_filter, clear_response_hook, set_frame_bundle, set_js_dialog_mode, set_request_filter,
    set_response_hook,
};
pub use types::*;
pub use view_ext::{
    next_zoom_percent, percent_to_zoom_level, zoom_level_to_percent, FrameDescription, ZOOM_FACTOR, ZOOM_PRESETS,
};
pub use view::{cancel_download, frontend_url, is_frontend_url, pause_download, resume_download, EditCommand, WebView};
pub use runtime::{devtools_port, pick_loopback_port};

/// Test helper: native CEF browser views currently attached to an application window, as
/// (window, class, address, hidden). macOS only; empty elsewhere.
pub fn debug_native_cef_views() -> Vec<(usize, String, usize, bool)> {
    #[cfg(target_os = "macos")]
    {
        mac::debug_subviews().into_iter().filter(|v| v.class.contains("Cef") || v.class.contains("Chrome")).map(|v| (v.window, v.class, v.ptr, v.hidden)).collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        platform::live_views()
    }
}

/// Test helper: every subview (depth 1 and 2) of the window content views as "class@ptr". macOS only.
pub fn debug_native_all_views() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        mac::debug_subviews().into_iter().map(|v| format!("{}@{:x}/w{:x}{}", v.class, v.ptr, v.window, if v.hidden { "(hidden)" } else { "" })).collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        platform::live_views().into_iter().map(|(w, class, ptr, hidden)| format!("{class}@{ptr:x}/w{w:x}{}", if hidden { "(hidden)" } else { "" })).collect()
    }
}

//! Asset source: Signet's own embedded files first (lucide icons used by the renderer, app icons, emoji data),
//! then the full lucide catalog and component icons of gpui-kit-assets (what gpui-component draws itself).

use anyhow::anyhow;
use gpui::{AssetSource, Result, SharedString};
use std::borrow::Cow;

#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
struct Own;

/// Register with `Application::with_assets(SignetAssets)`.
#[derive(Clone, Copy, Default)]
pub struct SignetAssets;

impl AssetSource for SignetAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(f) = Own::get(path) {
            return Ok(Some(f.data));
        }
        gpui_kit_assets::AllAssets.load(path).map_err(|_| anyhow!("could not find asset at path \"{path}\""))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = Own::iter().filter(|n| n.starts_with(path)).map(|n| n.to_string().into()).collect();
        out.extend(gpui_kit_assets::AllAssets.list(path)?);
        Ok(out)
    }
}

/// Embedded bytes of one of Signet's own files (`emoji/en.json`...).
pub fn own_bytes(path: &str) -> Option<Cow<'static, [u8]>> {
    Own::get(path).map(|f| f.data)
}

/// Asset path of a packaged app icon (`ocean`, `nuit`...), drawable with `gpui::img`.
pub fn app_icon_path(name: &str) -> String {
    format!("app-icons/{name}.svg")
}

/// Names of the packaged app icons.
pub const APP_ICONS: &[&str] = &["aurore", "corail", "graphite", "menthe", "nuit", "ocean", "or", "papier"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon::IconName;

    #[test]
    fn every_icon_is_embedded() {
        for i in IconName::ALL {
            assert!(SignetAssets.load(i.asset_path()).unwrap().is_some(), "{}", i.asset_path());
        }
    }

    #[test]
    fn kit_icons_fall_back() {
        assert!(SignetAssets.load("icons/check.svg").unwrap().is_some());
        assert!(SignetAssets.load("icons/does-not-exist.svg").is_err());
    }

    #[test]
    fn app_icons_and_emoji() {
        for n in APP_ICONS {
            assert!(own_bytes(&app_icon_path(n)).is_some());
        }
        assert!(own_bytes("emoji/en.json").is_some() && own_bytes("emoji/fr.json").is_some());
    }
}

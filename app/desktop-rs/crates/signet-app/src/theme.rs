//! Keeps the GPUI theme in step with the settings and the active space (port of `broadcastAppearance`).

use gpui::App;
use signet_core::types::{Density as CoreDensity, TextSize, TintStrength as CoreTint, Theme};
use signet_ui::color::TintStrength;
use signet_ui::theme::{Density, TextScale, ThemeMode, ThemeSettings};

pub fn settings_from_snapshot(snap: &signet_core::types::Snapshot) -> ThemeSettings {
    let s = &snap.settings;
    let space = snap.spaces.iter().find(|sp| sp.id == snap.active_space_id);
    let (hues, grain) = match space {
        Some(sp) => match &sp.theme {
            Some(t) if !t.hues.is_empty() => (t.hues.iter().map(|h| *h as f32).collect(), t.grain as f32),
            _ => (vec![sp.hue as f32], 0.0),
        },
        None => (vec![250.0], 0.0),
    };
    ThemeSettings {
        mode: match s.theme.unwrap_or_default() {
            Theme::System => ThemeMode::System,
            Theme::Light => ThemeMode::Light,
            Theme::Dark => ThemeMode::Dark,
        },
        hues,
        grain,
        tint: match s.tint.unwrap_or_default() {
            CoreTint::Subtle => TintStrength::Subtle,
            CoreTint::Normal => TintStrength::Normal,
            CoreTint::Strong => TintStrength::Strong,
        },
        density: match s.density.unwrap_or_default() {
            CoreDensity::Compact => Density::Compact,
            CoreDensity::Normal => Density::Normal,
            CoreDensity::Roomy => Density::Roomy,
        },
        text: match s.text_size.unwrap_or_default() {
            TextSize::Small => TextScale::Small,
            TextSize::Normal => TextScale::Normal,
            TextSize::Large => TextScale::Large,
        },
        glass: s.transparency != Some(false),
        high_contrast: false,
    }
}

/// Re-installs the theme when the snapshot's look changed.
pub fn sync(cx: &mut App) {
    let snap = signet_ui::app_state::snapshot(cx);
    let wanted = settings_from_snapshot(&snap);
    use signet_ui::ThemeExt as _;
    if cx.signet().settings != wanted {
        signet_ui::SignetTheme::update_settings(cx, |s| *s = wanted);
    }
}

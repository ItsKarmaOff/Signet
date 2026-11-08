//! The main window's background (the space gradient) and the space theme (port of `useSpaceTheme` and
//! `html.route-main` of base.css).

use crate::color::{Scheme, mix, rgba8};
use crate::theme::{SignetTheme, ThemeExt};
use gpui::{App, Background, Hsla, Rgba, linear_color_stop, linear_gradient};
use signet_core::types::Space;

/// Angle of the gradient (`155deg`).
const ANGLE: f32 = 155.0;

/// Hues and grain a space asks of the theme: a gradient of 2 or 3 hues, or its single hue.
pub fn space_hues(space: &Space) -> (Vec<f32>, f32) {
    match space.theme.as_ref().filter(|t| t.hues.len() >= 2) {
        Some(theme) => (theme.hues.iter().take(3).map(|h| *h as f32).collect(), theme.grain as f32),
        None => (vec![space.hue as f32], 0.0),
    }
}

/// Puts the space's tint into the theme (the app-wide `--h`, `--h2`, `--h3`, `--gk`, `--grain`), if it differs.
pub fn apply_space_theme(space: &Space, cx: &mut App) {
    let (hues, grain) = space_hues(space);
    let current = &cx.signet().settings;
    if current.hues == hues && (current.grain - grain).abs() < f32::EPSILON {
        return;
    }
    SignetTheme::update_settings(cx, |s| {
        s.hues = hues;
        s.grain = grain;
    });
}

/// `color-mix(in oklch, var(--bg-base), tint calc(dose * k * gk))` for each hue.
fn tint_colors(t: &SignetTheme) -> Vec<Hsla> {
    let dark = t.scheme == Scheme::Dark;
    let alpha = if t.settings.glass && !t.settings.high_contrast { 0.72 } else { 1.0 };
    let base = if dark { rgba8(44, 43, 48, alpha) } else { rgba8(236, 236, 239, alpha) };
    let dose = (if dark { 0.08 } else { 0.07 }) * t.settings.tint.k() * t.gradient_k;
    t.space_tints.iter().map(|c| mix(base, Rgba::from(*c), dose).into()).collect()
}

/// Background layers of the window, bottom first: a solid `--bg`, or the gradient (hue 1 at 0%, hue 2 at 55%,
/// hue 3 at 100%) as two stacked two-stop gradients.
pub fn layers(t: &SignetTheme) -> Vec<Background> {
    if t.space_tints.len() < 2 {
        return vec![t.bg.into()];
    }
    let c = tint_colors(t);
    let (c0, c1) = (c[0], c[1]);
    let c2 = c.get(2).copied().unwrap_or(c1);
    let clear = Hsla { a: 0.0, ..c1 };
    vec![
        linear_gradient(ANGLE, linear_color_stop(c0, 0.0), linear_color_stop(c1, 0.55)),
        linear_gradient(ANGLE, linear_color_stop(clear, 0.55), linear_color_stop(c2, 1.0)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use signet_core::types::SpaceTheme;

    fn space(hue: f64, theme: Option<SpaceTheme>) -> Space {
        Space { hue, theme, ..Default::default() }
    }

    #[test]
    fn solid_space_has_one_hue_and_no_grain() {
        assert_eq!(space_hues(&space(90.0, None)), (vec![90.0], 0.0));
        let one = SpaceTheme { hues: vec![10.0], grain: 0.5 };
        assert_eq!(space_hues(&space(90.0, Some(one))), (vec![90.0], 0.0));
    }

    #[test]
    fn gradient_space_keeps_three_hues_and_grain() {
        let t = SpaceTheme { hues: vec![10.0, 20.0, 30.0, 40.0], grain: 0.5 };
        assert_eq!(space_hues(&space(10.0, Some(t))), (vec![10.0, 20.0, 30.0], 0.5));
    }
}

//! Signet theme: port of `renderer/src/styles/tokens.css`.
//!
//! The theme is a GPUI global (`SignetTheme`). Read it with `cx.signet()` (see [`ThemeExt`]).
//! [`SignetTheme::apply`] also maps it onto gpui-component's theme so its widgets match.
//!
//! Vibrancy: GPUI offers `WindowBackgroundAppearance::Blurred` (macOS vibrancy, Wayland/KDE blur) per window,
//! but no per-element backdrop blur (CSS `backdrop-filter`). `bg` is therefore translucent (alpha 0.72) and the
//! window must be opened with [`SignetTheme::window_background`]; floating layers (`glass`) are drawn with high
//! alpha without blurring what is behind them.

use crate::color::*;
use gpui::{App, BoxShadow, Global, Hsla, Pixels, Rgba, WindowAppearance, WindowBackgroundAppearance, point, px};
use gpui_component::{Theme as ComponentTheme, ThemeMode as ComponentMode};
use std::time::Duration;

/// User choice for light / dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

/// Interface text size (Appearance setting).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextScale {
    Small,
    #[default]
    Normal,
    Large,
}

/// Row density (Appearance setting).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Density {
    Compact,
    #[default]
    Normal,
    Roomy,
}

/// Everything the user can change in Settings > Appearance, plus the current space.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeSettings {
    pub mode: ThemeMode,
    /// Hues of the current space (1 = solid, 2 or 3 = gradient), in degrees.
    pub hues: Vec<f32>,
    /// Gradient grain 0..1.
    pub grain: f32,
    pub tint: TintStrength,
    pub density: Density,
    pub text: TextScale,
    /// Translucency (sidebar vibrancy) allowed.
    pub glass: bool,
    /// Increased contrast.
    pub high_contrast: bool,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self { mode: ThemeMode::System, hues: vec![250.0], grain: 0.0, tint: TintStrength::Normal, density: Density::Normal, text: TextScale::Normal, glass: true, high_contrast: false }
    }
}

/// Spacing scale (`--s*`), 4 px grid.
pub mod space {
    use gpui::{Pixels, px};
    pub const HALF: Pixels = px(2.0);
    pub const S1: Pixels = px(4.0);
    pub const S2: Pixels = px(8.0);
    pub const S3: Pixels = px(12.0);
    pub const S4: Pixels = px(16.0);
    pub const S5: Pixels = px(20.0);
    pub const S6: Pixels = px(24.0);
    pub const S7: Pixels = px(28.0);
    pub const S8: Pixels = px(32.0);
    pub const GAP: Pixels = px(6.0);
    pub const TOOLBAR_H: Pixels = px(44.0);
}

/// Radii (`--r-*`).
pub mod radius {
    use gpui::{Pixels, px};
    pub const WINDOW: Pixels = px(16.0);
    pub const CARD: Pixels = px(12.0);
    pub const POP: Pixels = px(14.0);
    pub const GROUP: Pixels = px(12.0);
    pub const TILE: Pixels = px(10.0);
    pub const ROW: Pixels = px(8.0);
    pub const ROW_IN: Pixels = px(6.0);
    pub const CONTROL: Pixels = px(7.0);
    pub const ICON: Pixels = px(4.0);
}

/// Motion durations (`--t-*`).
pub mod duration {
    use std::time::Duration;
    pub const PRESS: Duration = Duration::from_millis(80);
    pub const FAST: Duration = Duration::from_millis(120);
    pub const HUD: Duration = Duration::from_millis(150);
    pub const NORMAL: Duration = Duration::from_millis(200);
    pub const THEME: Duration = Duration::from_millis(240);
    pub const SLOW: Duration = Duration::from_millis(320);
}

/// Font weights by role (`--fw-*`).
/// Default line height of the interface (CSS `line-height: normal` of the system font is about 1.2; GPUI's default 1.6
/// makes multi-line text too tall). Apply it on the root of each window.
pub const LINE_HEIGHT: f32 = 1.2;

pub mod weight {
    use gpui::FontWeight;
    pub const BODY: FontWeight = FontWeight(450.0);
    pub const MEDIUM: FontWeight = FontWeight(500.0);
    pub const LABEL: FontWeight = FontWeight(550.0);
    pub const STRONG: FontWeight = FontWeight(600.0);
    pub const TITLE: FontWeight = FontWeight(650.0);
    pub const HERO: FontWeight = FontWeight(700.0);
}

/// Type scale in px, already adjusted to the text size setting.
#[derive(Clone, Copy, Debug)]
pub struct TypeScale {
    pub cap: Pixels,
    pub sub: Pixels,
    pub base: Pixels,
    pub md: Pixels,
    pub lg: Pixels,
    pub title: Pixels,
    pub hero: Pixels,
}

impl TypeScale {
    pub fn for_scale(s: TextScale) -> Self {
        let v = match s {
            TextScale::Small => [10.0, 11.0, 12.0, 13.0, 15.0, 18.0, 25.0],
            TextScale::Normal => [11.0, 12.0, 13.0, 14.0, 16.0, 20.0, 28.0],
            TextScale::Large => [12.0, 13.0, 14.0, 15.0, 17.0, 22.0, 31.0],
        };
        Self { cap: px(v[0]), sub: px(v[1]), base: px(v[2]), md: px(v[3]), lg: px(v[4]), title: px(v[5]), hero: px(v[6]) }
    }
}

/// Color emoji font of the platform (`--font-emoji`).
pub fn emoji_font() -> &'static str {
    if cfg!(target_os = "macos") {
        "Apple Color Emoji"
    } else if cfg!(target_os = "windows") {
        "Segoe UI Emoji"
    } else {
        "Noto Color Emoji"
    }
}
/// Monospace stack head (`--font-mono`).
pub const FONT_MONO: &str = "Menlo";

fn shadow(y: f32, blur: f32, a: f32) -> BoxShadow {
    BoxShadow { color: rgba8(0, 0, 0, a).into(), offset: point(px(0.0), px(y)), blur_radius: px(blur), spread_radius: px(0.0), inset: false }
}

/// The resolved theme. All colors are `Hsla` ready for `.bg()`, `.text_color()`, `.border_color()`.
#[derive(Clone, Debug)]
pub struct SignetTheme {
    pub settings: ThemeSettings,
    pub scheme: Scheme,
    pub type_scale: TypeScale,
    pub row_h: Pixels,

    /// Window / sidebar background (translucent, tinted by the space).
    pub bg: Hsla,
    /// Opaque version of `bg` for when translucency is off or unsupported.
    pub bg_opaque: Hsla,
    pub surface: Hsla,
    pub surface_2: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_2: Hsla,
    pub text_3: Hsla,
    pub text_tab: Hsla,
    pub on_accent: Hsla,
    pub accent: Hsla,
    pub danger: Hsla,
    /// Active row / segmented thumb background.
    pub active: Hsla,
    pub active_shadow: Vec<BoxShadow>,
    /// Floating layers (popovers, menus).
    pub glass: Hsla,
    pub glass_view: Hsla,
    /// Fill of the floating sidebar over the page: the page is right under it and there is no backdrop blur, so it is
    /// practically opaque (two `glass_view` layers, the look before the single card fill).
    pub glass_float: Hsla,
    /// Selection background (`--sel`).
    pub sel: Hsla,
    pub scrim: Hsla,
    pub scrim_peek: Hsla,
    pub win_bg: Hsla,
    pub win_card: Hsla,
    pub ctl: Hsla,
    pub ctl_ring: Hsla,
    pub edge_light: Hsla,
    /// Hover wash on transparent controls (`--surface-2`).
    pub hover: Hsla,
    /// Gradient colors of the space background (tint of each hue); 1 entry for a solid space.
    pub space_tints: Vec<Hsla>,
    /// Gradient strength multiplier (`--gk`): 1 solid, 2.6 gradient.
    pub gradient_k: f32,

    pub shadow_card: Vec<BoxShadow>,
    pub shadow_pop: Vec<BoxShadow>,
    pub shadow_peek: Vec<BoxShadow>,
    pub shadow_knob: Vec<BoxShadow>,
    pub shadow_modal: Vec<BoxShadow>,
}

impl Global for SignetTheme {}

/// `cx.signet()` accessor.
pub trait ThemeExt {
    fn signet(&self) -> &SignetTheme;
}

impl ThemeExt for App {
    fn signet(&self) -> &SignetTheme {
        self.global::<SignetTheme>()
    }
}

impl SignetTheme {
    /// Computes the theme for a scheme (desktop `:root` + dark media query + appearance attributes).
    pub fn compute(settings: &ThemeSettings, scheme: Scheme) -> Self {
        let dark = scheme.is_dark();
        let hue = settings.hues.first().copied().unwrap_or(250.0);
        let k = settings.tint.k();
        let hc = settings.high_contrast;
        let transparent = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
        let tint = tint_color(hue, scheme);

        let bg_alpha = if settings.glass && !hc { 0.72 } else { 1.0 };
        let bg_base = if dark { rgba8(44, 43, 48, bg_alpha) } else { rgba8(236, 236, 239, bg_alpha) };
        let surface_base = if dark { rgba8(31, 31, 35, 1.0) } else { rgba8(255, 255, 255, 1.0) };
        let surface2_base = if dark { rgba8(255, 255, 255, 0.06) } else { rgba8(0, 0, 0, 0.05) };
        let border_a = match (dark, hc) {
            (true, true) => 0.2,
            (true, false) => 0.09,
            (false, true) => 0.2,
            (false, false) => 0.09,
        };
        let border_base = if dark { rgba8(255, 255, 255, border_a) } else { rgba8(0, 0, 0, border_a) };
        let dose = if dark { 0.08 } else { 0.07 };

        let bg = mix(bg_base, tint, dose * k);
        let surface = mix(surface_base, tint, 0.02 * k);
        let tint_dim = |a: f32| {
            // oklch(from tint calc(l - 0.08) c h / a): darker tint at low alpha.
            let darker = mix(tint, rgba8(0, 0, 0, 1.0), 0.18);
            with_alpha(darker, a)
        };
        let surface_2 = mix(with_alpha(surface2_base, surface2_base.a * 0.7), tint_dim(0.05 * k), 0.5);
        let border = mix(with_alpha(border_base, border_base.a * 0.8), tint_dim(0.1 * k), 0.5);
        let active_base = if dark { rgba8(0, 0, 0, 0.38) } else { rgba8(255, 255, 255, 1.0) };
        let active = mix(active_base, tint, 0.02 * k);
        let sel = with_alpha(tint, if dark { 0.18 } else { 0.14 });
        let glass_alpha = if settings.glass && !hc { if dark { 0.82 } else { 0.78 } } else { 1.0 };
        let glass_view_alpha = if settings.glass && !hc { 0.94 } else { 1.0 };
        let glass_float_alpha = 1.0 - (1.0 - glass_view_alpha) * (1.0 - glass_view_alpha);

        let (text, mut text_2, mut text_3) = if dark {
            (rgba8(236, 236, 240, 1.0), rgba8(165, 164, 173, 1.0), rgba8(144, 143, 152, 1.0))
        } else {
            (rgba8(29, 29, 31, 1.0), rgba8(92, 92, 98, 1.0), rgba8(108, 108, 115, 1.0))
        };
        let mut accent = accent_color(hue, scheme);
        if hc {
            if dark {
                text_2 = rgba8(200, 199, 207, 1.0);
                text_3 = rgba8(165, 164, 173, 1.0);
                accent = from_rgb(oklch_to_rgb(0.78, 0.14, base_hue(hue, 0.75, 0.68)));
            } else {
                text_2 = rgba8(70, 70, 76, 1.0);
                text_3 = rgba8(96, 96, 103, 1.0);
                accent = from_rgb(oklch_to_rgb(0.48, 0.16, base_hue(hue, 0.7, 0.55)));
            }
        }
        let danger = if dark { from_rgb(hsl_to_rgb(2.0, 0.8, 0.68)) } else { from_rgb(hsl_to_rgb(2.0, 0.7, 0.46)) };

        let space_tints: Vec<Hsla> = if settings.hues.len() >= 2 {
            settings.hues.iter().take(3).map(|h| hsla(tint_color(*h, scheme))).collect()
        } else {
            vec![hsla(tint)]
        };

        let row_h = match settings.density {
            Density::Compact => px(28.0),
            Density::Normal => px(32.0),
            Density::Roomy => px(36.0),
        };

        let modal_a = if dark { (0.5, 0.3) } else { (0.32, 0.18) };
        let card_a = if dark { (0.1, 0.18) } else { (0.06, 0.1) };
        let text_mixed = |a: f32| with_alpha(text, a);
        let _ = transparent;

        Self {
            settings: settings.clone(),
            scheme,
            type_scale: TypeScale::for_scale(settings.text),
            row_h,
            bg: hsla(bg),
            bg_opaque: hsla(with_alpha(bg, 1.0)),
            surface: hsla(surface),
            surface_2: hsla(surface_2),
            border: hsla(border),
            text: hsla(text),
            text_2: hsla(text_2),
            text_3: hsla(text_3),
            text_tab: hsla(text_mixed(0.85)),
            on_accent: hsla(if dark { rgba8(18, 18, 22, 1.0) } else { rgba8(255, 255, 255, 1.0) }),
            accent: hsla(accent),
            danger: hsla(danger),
            active: hsla(active),
            active_shadow: if dark {
                vec![]
            } else {
                vec![shadow(1.0, 2.0, 0.08), BoxShadow { color: rgba8(0, 0, 0, 0.07).into(), offset: point(px(0.0), px(0.0)), blur_radius: px(0.0), spread_radius: px(0.5), inset: false }]
            },
            glass: hsla(with_alpha(surface, glass_alpha)),
            glass_view: hsla(with_alpha(surface, glass_view_alpha)),
            glass_float: hsla(with_alpha(surface, glass_float_alpha)),
            sel: hsla(sel),
            scrim: hsla(rgba8(0, 0, 0, if dark { 0.36 } else { 0.18 })),
            scrim_peek: hsla(rgba8(0, 0, 0, 0.45)),
            win_bg: hsla(if dark { rgba8(35, 35, 39, 1.0) } else { rgba8(255, 255, 255, 1.0) }),
            win_card: hsla(if dark { rgba8(255, 255, 255, 0.05) } else { rgba8(0, 0, 0, 0.03) }),
            ctl: hsla(if dark { rgba8(255, 255, 255, 0.1) } else { rgba8(255, 255, 255, 1.0) }),
            ctl_ring: hsla(if dark { rgba8(255, 255, 255, 0.16) } else { rgba8(0, 0, 0, 0.14) }),
            edge_light: hsla(rgba8(255, 255, 255, if dark { 0.12 } else { 0.6 })),
            hover: hsla(surface_2),
            space_tints,
            gradient_k: if settings.hues.len() >= 2 { GRADIENT_STRENGTH } else { 1.0 },
            shadow_card: vec![shadow(1.0, 2.0, card_a.0), shadow(3.0, 8.0, card_a.1)],
            shadow_pop: vec![shadow(12.0, 32.0, 0.28), shadow(2.0, 6.0, 0.16)],
            shadow_peek: vec![shadow(24.0, 70.0, 0.5)],
            shadow_knob: vec![shadow(1.0, 3.0, 0.3)],
            shadow_modal: vec![shadow(24.0, 64.0, modal_a.0), shadow(4.0, 12.0, modal_a.1)],
        }
    }

    pub fn is_dark(&self) -> bool {
        self.scheme.is_dark()
    }

    /// Window background to request: `Blurred` when translucency is on, else opaque.
    pub fn window_background(&self) -> WindowBackgroundAppearance {
        if self.settings.glass && !self.settings.high_contrast { WindowBackgroundAppearance::Blurred } else { WindowBackgroundAppearance::Opaque }
    }

    /// Text colour for a given tint amount of the muted foreground.
    pub fn text_dim(&self, a: f32) -> Hsla {
        let mut c = self.text;
        c.a = a;
        c
    }

    /// Accent with an alpha (focus rings, soft fills).
    pub fn accent_a(&self, a: f32) -> Hsla {
        let mut c = self.accent;
        c.a = a;
        c
    }

    /// Resolves the scheme from the mode and the system appearance.
    pub fn resolve_scheme(mode: ThemeMode, system: WindowAppearance) -> Scheme {
        match mode {
            ThemeMode::Light => Scheme::Light,
            ThemeMode::Dark => Scheme::Dark,
            ThemeMode::System => match system {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Scheme::Dark,
                _ => Scheme::Light,
            },
        }
    }

    /// Installs the theme as a global and maps it onto gpui-component. Call after `gpui_component::init`.
    pub fn install(settings: ThemeSettings, cx: &mut App) {
        let scheme = Self::resolve_scheme(settings.mode, cx.window_appearance());
        cx.set_global(Self::compute(&settings, scheme));
        Self::sync_component_theme(cx);
        Self::sync_native_appearance(cx);
    }

    /// Replaces the settings (mode, space hues, tint...), recomputes everything and refreshes windows.
    pub fn update_settings(cx: &mut App, f: impl FnOnce(&mut ThemeSettings)) {
        let mut settings = cx.signet().settings.clone();
        f(&mut settings);
        Self::install(settings, cx);
    }

    /// Call when the system light/dark changes (see [`crate::watch_system_appearance`]).
    pub fn system_appearance_changed(cx: &mut App) {
        let settings = cx.signet().settings.clone();
        if settings.mode == ThemeMode::System {
            Self::install(settings, cx);
        }
    }

    fn sync_native_appearance(cx: &mut App) {
        // A forced light/dark also themes the native window chrome; System follows the OS.
        let forced = match cx.signet().settings.mode {
            ThemeMode::System => None,
            ThemeMode::Light => Some(WindowAppearance::Light),
            ThemeMode::Dark => Some(WindowAppearance::Dark),
        };
        cx.set_window_appearance(forced);
    }

    fn sync_component_theme(cx: &mut App) {
        let t = cx.signet().clone();
        let mode = if t.is_dark() { ComponentMode::Dark } else { ComponentMode::Light };
        ComponentTheme::change(mode, None, cx);
        ComponentTheme::update(cx, |c| {
            let base = if t.is_dark() { t.win_bg } else { t.surface };
            c.font_size = t.type_scale.base;
            c.radius = radius::CONTROL;
            c.radius_lg = radius::POP;
            c.shadow = false;
            let k = &mut c.colors;
            k.background = base;
            k.foreground = t.text;
            k.border = t.border;
            k.input = t.border;
            k.ring = t.accent_a(0.5);
            k.caret = t.accent;
            k.selection = t.sel;
            k.muted = t.surface_2;
            k.muted_foreground = t.text_2;
            k.accent = t.surface_2;
            k.accent_foreground = t.text;
            k.primary = t.accent;
            k.primary_hover = t.accent_a(0.9);
            k.primary_active = t.accent_a(0.8);
            k.primary_foreground = t.on_accent;
            k.secondary = t.surface_2;
            k.secondary_hover = t.surface_2;
            k.secondary_active = t.surface_2;
            k.secondary_foreground = t.text;
            k.danger = t.danger;
            k.danger_hover = t.danger;
            k.danger_active = t.danger;
            k.danger_foreground = t.on_accent;
            k.popover = t.glass_view;
            k.popover_foreground = t.text;
            k.list = gpui::transparent_black();
            k.list_hover = t.surface_2;
            k.list_active = t.sel;
            k.list_active_border = t.accent;
            k.list_even = gpui::transparent_black();
            k.list_head = gpui::transparent_black();
            k.slider_bar = t.accent;
            k.slider_thumb = if t.is_dark() { t.text } else { gpui::white() };
            k.switch = t.border;
            k.switch_thumb = gpui::white();
            k.progress_bar = t.accent;
            k.scrollbar = gpui::transparent_black();
            k.scrollbar_thumb = t.text_dim(0.25);
            k.scrollbar_thumb_hover = t.text_dim(0.4);
            k.overlay = t.scrim;
            k.title_bar = gpui::transparent_black();
            k.title_bar_border = gpui::transparent_black();
            k.sidebar = gpui::transparent_black();
            k.sidebar_foreground = t.text;
            k.sidebar_border = t.border;
            k.sidebar_accent = t.surface_2;
            k.sidebar_accent_foreground = t.text;
            k.skeleton = t.surface_2;
            k.link = t.accent;
            k.tab = gpui::transparent_black();
            k.tab_active = t.active;
            k.tab_foreground = t.text_2;
            k.tab_active_foreground = t.text;
            k.tab_bar = t.surface_2;
            k.tab_bar_segmented = t.surface_2;
            k.drop_target = t.sel;
            k.group_box = t.win_card;
        });
    }
}

/// Registers the system light/dark observer for a window; call once per window in its build closure.
pub fn watch_system_appearance(window: &mut gpui::Window, _cx: &mut App) {
    window
        .observe_window_appearance(|_window, cx| SignetTheme::system_appearance_changed(cx))
        .detach();
}

/// Duration helper: `d` scaled to zero when motion is reduced.
pub fn motion_duration(d: Duration, reduced: bool) -> Duration {
    if reduced { Duration::from_millis(1) } else { d }
}

/// Corner radius of the app window as the system draws it (macOS only), in points; unset when unknown.
#[derive(Clone, Copy, Default)]
pub struct WindowCorner(pub Option<f32>);
impl Global for WindowCorner {}

pub fn set_window_corner(cx: &mut App, r: Option<f32>) {
    cx.set_global(WindowCorner(r));
}

pub fn window_corner(cx: &App) -> Option<f32> {
    cx.try_global::<WindowCorner>().and_then(|r| r.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_is_darker() {
        let s = ThemeSettings::default();
        let l = SignetTheme::compute(&s, Scheme::Light);
        let d = SignetTheme::compute(&s, Scheme::Dark);
        assert!(l.surface.l > d.surface.l);
        assert!(l.text.l < d.text.l);
    }

    #[test]
    fn accent_follows_hue() {
        let mut s = ThemeSettings::default();
        let a = SignetTheme::compute(&s, Scheme::Light).accent;
        s.hues = vec![20.0];
        let b = SignetTheme::compute(&s, Scheme::Light).accent;
        assert!((a.h - b.h).abs() > 0.05);
    }

    #[test]
    fn glass_float_is_nearly_opaque() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            assert!(SignetTheme::compute(&ThemeSettings::default(), scheme).glass_float.a > 0.99);
        }
        let s = ThemeSettings { glass: false, ..Default::default() };
        assert_eq!(SignetTheme::compute(&s, Scheme::Dark).glass_float.a, 1.0);
        let s = ThemeSettings { high_contrast: true, ..Default::default() };
        assert_eq!(SignetTheme::compute(&s, Scheme::Light).glass_float.a, 1.0);
    }

    #[test]
    fn glass_off_is_opaque() {
        let mut s = ThemeSettings::default();
        s.glass = false;
        let t = SignetTheme::compute(&s, Scheme::Light);
        assert_eq!(t.bg.a, 1.0);
        assert_eq!(t.window_background(), WindowBackgroundAppearance::Opaque);
        assert_eq!(SignetTheme::compute(&ThemeSettings::default(), Scheme::Light).window_background(), WindowBackgroundAppearance::Blurred);
    }

    #[test]
    fn gradient_spaces() {
        let mut s = ThemeSettings::default();
        s.hues = vec![10.0, 120.0, 240.0];
        let t = SignetTheme::compute(&s, Scheme::Dark);
        assert_eq!(t.space_tints.len(), 3);
        assert_eq!(t.gradient_k, GRADIENT_STRENGTH);
    }

    #[test]
    fn scheme_resolution() {
        assert_eq!(SignetTheme::resolve_scheme(ThemeMode::System, WindowAppearance::Dark), Scheme::Dark);
        assert_eq!(SignetTheme::resolve_scheme(ThemeMode::Light, WindowAppearance::Dark), Scheme::Light);
        assert_eq!(SignetTheme::resolve_scheme(ThemeMode::Dark, WindowAppearance::Light), Scheme::Dark);
    }

    #[test]
    fn type_scale_sizes() {
        assert_eq!(TypeScale::for_scale(TextScale::Small).base, px(12.0));
        assert_eq!(TypeScale::for_scale(TextScale::Large).hero, px(31.0));
    }
}

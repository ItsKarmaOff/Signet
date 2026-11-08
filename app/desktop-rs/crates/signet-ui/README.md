# signet-ui

GPUI foundation of the Rust port: theme, motion, icons, assets and reusable widgets. Screens are built on top of it.
Depends on `gpui` (`gpui-pre` 0.3.11) and `gpui-component` 0.9. Does not depend on `signet-core` yet.

## Setup

```rust
gpui_platform::application().with_assets(signet_ui::SignetAssets).run(|cx| {
    signet_ui::init(cx, signet_ui::ThemeSettings::default());      // gpui-component init + Signet theme
    let bg = cx.signet().window_background();                       // Blurred when translucency is on
    cx.open_window(WindowOptions { window_background: bg, ..Default::default() }, |window, cx| {
        signet_ui::watch_system_appearance(window, cx);             // follow system light / dark
        let view = cx.new(|cx| MyView::new(window, cx));
        cx.new(|cx| gpui_component::Root::new(view, window, cx))   // Root hosts dialogs, menus, notifications
    });
});
```

`gpui_platform` is `gpui-pre-platform = "=0.3.11"` (the app crate adds it; here it is a dev-dependency for the gallery).

## Theme

`SignetTheme` is a GPUI global computed from `ThemeSettings { mode, hues, grain, tint, density, text, glass, high_contrast }`.
Read it with `use signet_ui::ThemeExt; let t = cx.signet();`. Change it with
`SignetTheme::update_settings(cx, |s| s.hues = vec![20.0])` (space change, Appearance settings, light / dark / system):
it recomputes every color, maps them onto gpui-component's theme and refreshes all windows.

Colors (`Hsla`, desktop token in brackets): `bg` (`--bg`, translucent), `bg_opaque`, `surface`, `surface_2`, `border`,
`text`, `text_2`, `text_3`, `text_tab`, `on_accent`, `accent`, `danger`, `active` + `active_shadow`, `glass`, `glass_view`,
`sel`, `scrim`, `scrim_peek`, `win_bg`, `win_card`, `ctl`, `ctl_ring`, `edge_light`, `hover`.
Gradient spaces: `space_tints` (one tint per hue, 1 to 3) and `gradient_k` (1 solid, 2.6 gradient); paint the window
background as a gradient of those tints mixed into `bg`. Shadows (`Vec<BoxShadow>`): `shadow_card`, `shadow_pop`,
`shadow_peek`, `shadow_knob`, `shadow_modal`. Type scale: `t.type_scale.{cap,sub,base,md,lg,title,hero}` (already
adjusted to the text size setting), `t.row_h` (density). Modules: `theme::space` (`S1`..`S8`, `GAP`), `theme::radius`
(`WINDOW`, `CARD`, `POP`, `GROUP`, `TILE`, `ROW`, `ROW_IN`, `CONTROL`, `ICON`), `theme::duration` (`PRESS`, `FAST`,
`HUD`, `NORMAL`, `THEME`, `SLOW`), `theme::weight` (`BODY`..`HERO`). `theme::emoji_font()` is the platform color emoji font.

`color` ports `packages/shared/src/mobile/color.ts` (HSL, OKLab / OKLCH, `mix`, `accent_color`, `tint_color`, `tint_boost`).

Translucency: GPUI offers `WindowBackgroundAppearance::Blurred` per window (macOS vibrancy, KDE / Wayland blur), exposed by
`t.window_background()`. There is no per-element backdrop blur, so floating layers (`glass`, `glass_view`) are high-alpha
fills without blur, and the sidebar translucency comes from the window itself.

## Motion

`motion`: `Spring` (analytic, port of `spring.ts`), `rubberband` / `unrubberband` / `project`, `cubic_bezier`, `ease_out`
(`--ease-out`), `spring_curve` (`--spring`), `surface_enter` (materialize / exit of a floating surface: opacity plus a 4 px
slide, GPUI has no element scale transform), `Presence<T>` (keeps a value mounted 120 ms while it fades out, port of
`useExit`; `Presence::set(view, |v| &mut v.field, Some(x) / None, cx)`), `set_reduced_motion` / `reduced_motion`.

Sidebar row motion lives in `screens::tabs::motion`: `Flip` (rows glide to their new place through a wrapper element that
offsets its child during prepaint), `PillSlide` (sliding active background), `Unfold` (folder and group body height,
chevron turn). `toolbar::BarFade` cross-fades the bar colors (240 ms). All of it is off with reduced motion.

## Icons and assets

`SignetAssets` serves, embedded in the binary: the 97 lucide icons used by the renderer (generated from the repo's
lucide-react, `assets/icons`), the 8 app icons (`assets/app-icons`, `assets::app_icon_path("ocean")`), the emoji data, and
as a fallback the full lucide catalog and gpui-component's own icons (gpui-kit-assets). `IconName` is an enum using the
lucide-react names (`IconName::Trash2`, `ArrowLeft`, `Globe`...; `IconName::ALL` lists them). Use
`gpui_component::Icon::from(IconName::X).size(px(16.0))`, or `signet_ui::icon::icon(name, size, color)`. Any other lucide icon:
`Icon::empty().path("icons/<kebab-name>.svg")`.

## Widgets (`signet_ui::widgets`)

All take already translated strings. Builder style, `RenderOnce` unless noted.

| Widget | Purpose |
| --- | --- |
| `IconButton` | 28 / 24 px icon button, hover / pressed / active / danger / disabled, tooltip |
| `ListRow` | tab-like row: leading, title, trailing, trailing-on-hover, active, dim, dragging, drop indicator; supports `on_drag` / `on_drop` |
| `SectionHeader`, `Separator` | caption header with trailing actions, hairline |
| `TextField` + `text_field_state` | single-line input (wraps gpui-component `Input`) |
| `Toggle` | 36x20 switch with springing knob |
| `Segmented` | segmented control, optional icons and tinted items |
| `SliderField` + `slider_state` | labelled slider (wraps gpui-component `Slider`) |
| `MenuEntry` + `build_menu`, `context_menu`, `dropdown_menu` | menus on gpui-component `PopupMenu` (icon, shortcut, check, danger, submenu) |
| `popover_surface`, `popover_content` | rounded translucent shadowed container |
| `modal_surface`, `confirm_dialog` | container for custom modals; ready confirmation dialog |
| `Toast` | message with action ("... Undo"), animated in / out |
| `Favicon`, `FaviconSource` | image from bytes / URL / path, globe or colored letter fallback, loading pulse |
| `InlineConfirm` | message + Cancel + danger button, Escape cancels |
| `MediaCard`, `MediaChip` | presentational media card and compact chip |
| `EmojiPicker` (view) | search, category tabs, virtualized grid, recents; emits `EmojiPicked`; `en` and `fr` data |
| `ShortcutLabel`, `format_accelerator` | kbd label; Electron accelerator to `⇧⌘T` / `Ctrl+Shift+T` |
| `EmptyState` | icon, title, hint, action |
| `ProgressBar`, `Spinner` | thin determinate / indeterminate bar, rotating loader |
| `edge_fade`, `vertical_fades`, `FadeEdge` | gradient overlays on the edges of scrolling areas (no mask in GPUI) |

```rust
div().child(
    ListRow::new(("tab", id))
        .leading(Favicon::new(favicon).size(16.0))
        .title(title)
        .active(is_active)
        .trailing_on_hover(IconButton::new(("close", id), IconName::X).small().tooltip(t("tab.close")))
        .on_click(cx.listener(move |this, _, _, cx| this.activate(id, cx))),
)
```

## Gallery

```sh
cd app/desktop-rs
CARGO_BUILD_JOBS=4 cargo run -p signet-ui --example gallery -- --dark    # or --light; the top switch also toggles live
```

Opens a window; close it when done.

## Tests

`cargo test -p signet-ui` covers color math, theme derivation, springs and easings, assets, shortcut formatting and emoji data.

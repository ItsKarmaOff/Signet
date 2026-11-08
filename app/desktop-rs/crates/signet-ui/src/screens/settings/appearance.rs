//! Settings > Appearance (port of the `Appearance` function of `Settings.tsx`).

use gpui::{
    Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use serde_json::json;
use signet_core::i18n::IS_MAC;
use signet_core::pagesettings::{DEFAULT_FONT_SIZE, DEFAULT_ZOOM, FONT_SIZES, MIN_FONT_SIZES, ZOOM_PERCENTS};
use signet_core::shortcuts::shortcut_label;
use signet_core::types::{Density, NewTabPosition, SidebarSide, TabLayout, TextSize, Theme, TintStrength};

use super::kit;
use super::window::{InfoStore, info_of, watch};
use crate::app_state::{tr, tr_args};
use crate::theme::{ThemeExt, radius, space, weight};

/// Labels of the default font sizes (same order as `FONT_SIZES`).
const FONT_LABELS: [&str; 5] = [
    "settings.appearance.fontVerySmall",
    "settings.appearance.fontSmall",
    "settings.appearance.fontMedium",
    "settings.appearance.fontLarge",
    "settings.appearance.fontVeryLarge",
];

pub struct Appearance {
    store: Entity<InfoStore>,
}

impl Appearance {
    pub fn new(store: Entity<InfoStore>, cx: &mut Context<Self>) -> Self {
        watch(&store, cx);
        Self { store }
    }
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items.iter().map(|(v, k)| (v.to_string(), tr(k))).collect()
}

impl Render for Appearance {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let info = info_of(&self.store, cx);
        let s = &info.settings;
        let horizontal = s.tab_layout == Some(TabLayout::Horizontal);
        let overrides = s.shortcuts.as_ref();
        let bar_keys = shortcut_label("bookmarksBar", overrides, IS_MAC);
        let layout_keys = shortcut_label("tabLayout", overrides, IS_MAC);
        let compact_keys = shortcut_label("compactMode", overrides, IS_MAC);

        let hint_keys = |keys: &Option<String>, with: &str, without: &str| match keys {
            Some(k) if !k.is_empty() => tr_args(with, &[("keys", k)]),
            _ => tr(without),
        };
        let pick = |id: &'static str, key: &'static str, current: &str, options: Vec<(String, String)>| {
            kit::choice(id, current, options, move |v, _, cx| kit::update(cx, key, json!(v)))
        };
        let sw = |id: &'static str, title: String, hint: Option<String>, on: bool, key: &'static str| {
            kit::row(cx)
                .child(kit::label(cx, title, hint.map(Into::into)))
                .child(kit::switch(id, on, move |v, _, cx| kit::update(cx, key, json!(v))))
                .into_any_element()
        };
        let row = |title: String, hint: String, control: gpui::AnyElement| {
            kit::row(cx).child(kit::label(cx, title, Some(hint.into()))).child(control).into_any_element()
        };

        let theme = match s.theme.unwrap_or_default() {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        };
        let tint = match s.tint.unwrap_or_default() {
            TintStrength::Subtle => "subtle",
            TintStrength::Normal => "normal",
            TintStrength::Strong => "strong",
        };
        let density = match s.density.unwrap_or_default() {
            Density::Compact => "compact",
            Density::Normal => "normal",
            Density::Roomy => "roomy",
        };
        let text_size = match s.text_size.unwrap_or_default() {
            TextSize::Small => "small",
            TextSize::Normal => "normal",
            TextSize::Large => "large",
        };
        let layout = if horizontal { "horizontal" } else { "vertical" };
        let side = if s.sidebar_side == Some(SidebarSide::Right) { "right" } else { "left" };
        let _ = NewTabPosition::Top;

        let mut layout_rows = vec![row(
            tr("settings.appearance.layout"),
            hint_keys(&layout_keys, "settings.appearance.layoutHint", "settings.appearance.layoutHintNoKeys"),
            pick(
                "layout",
                "tabLayout",
                layout,
                pairs(&[("vertical", "settings.appearance.layoutSide"), ("horizontal", "settings.appearance.layoutTop")]),
            )
            .into_any_element(),
        )];
        if !horizontal {
            layout_rows.push(row(
                tr("settings.appearance.side"),
                tr("settings.appearance.sideHint"),
                pick("side", "sidebarSide", side, pairs(&[("left", "settings.appearance.sideLeft"), ("right", "settings.appearance.sideRight")])).into_any_element(),
            ));
        }
        layout_rows.push(sw(
            "sw-compact",
            tr("settings.general.compact"),
            Some(hint_keys(&compact_keys, "settings.general.compactHint", "settings.general.compactHintNoKeys")),
            s.compact_mode,
            "compactMode",
        ));
        layout_rows.push(sw(
            "sw-bar",
            tr("settings.general.bookmarksBar"),
            Some(hint_keys(&bar_keys, "settings.general.bookmarksBarHint", "settings.general.bookmarksBarHintNoKeys")),
            s.show_bookmarks_bar != Some(false),
            "showBookmarksBar",
        ));

        let mut look_rows = vec![
            row(
                tr("settings.appearance.theme"),
                tr("settings.appearance.themeHint"),
                pick(
                    "theme",
                    "theme",
                    theme,
                    pairs(&[
                        ("system", "settings.appearance.themeSystem"),
                        ("light", "settings.appearance.themeLight"),
                        ("dark", "settings.appearance.themeDark"),
                    ]),
                )
                .into_any_element(),
            ),
            row(
                tr("settings.appearance.tint"),
                tr("settings.appearance.tintHint"),
                pick(
                    "tint",
                    "tint",
                    tint,
                    pairs(&[
                        ("subtle", "settings.appearance.tintSubtle"),
                        ("normal", "settings.appearance.tintNormal"),
                        ("strong", "settings.appearance.tintStrong"),
                    ]),
                )
                .into_any_element(),
            ),
            sw("sw-preview", tr("settings.appearance.tabPreview"), Some(tr("settings.appearance.tabPreviewHint")), s.tab_preview != Some(false), "tabPreview"),
        ];
        if IS_MAC {
            look_rows.push(sw(
                "sw-transparency",
                tr("settings.appearance.transparency"),
                Some(tr("settings.appearance.transparencyHint")),
                s.transparency != Some(false),
                "transparency",
            ));
        }

        let text_rows = vec![
            row(
                tr("settings.appearance.density"),
                tr("settings.appearance.densityHint"),
                pick(
                    "density",
                    "density",
                    density,
                    pairs(&[
                        ("compact", "settings.appearance.densityCompact"),
                        ("normal", "settings.appearance.densityNormal"),
                        ("roomy", "settings.appearance.densityRoomy"),
                    ]),
                )
                .into_any_element(),
            ),
            row(
                tr("settings.appearance.textSize"),
                tr("settings.appearance.textSizeHint"),
                pick(
                    "text-size",
                    "textSize",
                    text_size,
                    pairs(&[
                        ("small", "settings.appearance.textSmall"),
                        ("normal", "settings.appearance.textNormal"),
                        ("large", "settings.appearance.textLarge"),
                    ]),
                )
                .into_any_element(),
            ),
        ];

        let font = s.font_size.unwrap_or(DEFAULT_FONT_SIZE);
        let min_font = s.minimum_font_size.unwrap_or(0);
        let zoom = s.default_zoom.unwrap_or(DEFAULT_ZOOM);
        let web_rows = vec![
            row(
                tr("settings.appearance.fontSize"),
                tr("settings.appearance.fontSizeHint"),
                kit::choice(
                    "font-size",
                    &font.to_string(),
                    FONT_SIZES.iter().enumerate().map(|(i, size)| (size.to_string(), tr(FONT_LABELS[i]))).collect(),
                    |v, _, cx| kit::update(cx, "fontSize", json!(v.parse::<u32>().unwrap_or(16))),
                )
                .into_any_element(),
            ),
            row(
                tr("settings.appearance.minFontSize"),
                tr("settings.appearance.minFontSizeHint"),
                kit::choice(
                    "min-font-size",
                    &min_font.to_string(),
                    MIN_FONT_SIZES
                        .iter()
                        .map(|size| {
                            let label = if *size == 0 {
                                tr("settings.appearance.minFontNone")
                            } else {
                                tr_args("settings.appearance.minFontPx", &[("size", &size.to_string())])
                            };
                            (size.to_string(), label)
                        })
                        .collect(),
                    |v, _, cx| kit::update(cx, "minimumFontSize", json!(v.parse::<u32>().unwrap_or(0))),
                )
                .into_any_element(),
            ),
            row(
                tr("settings.appearance.defaultZoom"),
                tr("settings.appearance.defaultZoomHint"),
                kit::choice(
                    "default-zoom",
                    &zoom.to_string(),
                    ZOOM_PERCENTS
                        .iter()
                        .map(|p| (p.to_string(), tr_args("settings.appearance.defaultZoomPercent", &[("percent", &p.to_string())])))
                        .collect(),
                    |v, _, cx| kit::update(cx, "defaultZoom", json!(v.parse::<u32>().unwrap_or(100))),
                )
                .into_any_element(),
            ),
        ];

        let mut col = kit::body()
            .child(kit::sub(cx, tr("settings.appearance.groupLayout")))
            .child(kit::group(cx, layout_rows))
            .child(kit::sub(cx, tr("settings.appearance.groupLook")))
            .child(kit::group(cx, look_rows))
            .child(kit::sub(cx, tr("settings.appearance.groupText")))
            .child(kit::group(cx, text_rows))
            .child(kit::sub(cx, tr("settings.appearance.groupWeb")))
            .child(kit::group(cx, web_rows))
            .child(kit::note(cx, tr("settings.appearance.webNote")));

        if IS_MAC {
            let icons = info.app_icons.iter().enumerate().map(|(i, icon)| {
                let on = icon.id == s.app_icon;
                let id = icon.id.clone();
                div()
                    .id(("app-icon", i))
                    .w(px(68.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(space::S1)
                    .px(space::S1)
                    .pt(space::S2)
                    .pb(space::S1)
                    .rounded(radius::TILE)
                    .text_size(t.type_scale.cap)
                    .text_color(if on { t.text } else { t.text_2 })
                    .when(on, |d| d.font_weight(weight::LABEL))
                    .cursor_pointer()
                    .hover(|s| s.bg(t.surface_2))
                    .on_click(move |_, _, cx| kit::update(cx, "appIcon", json!(id)))
                    .child(
                        div()
                            .rounded(px(9.0))
                            .when(on, |d| d.border(px(2.0)).border_color(t.accent))
                            .child(kit::app_icon_image(&icon.id, 40.0, 9.0)),
                    )
                    .child(div().max_w_full().overflow_hidden().whitespace_nowrap().text_ellipsis().child(icon.name.clone()))
            });
            col = col
                .child(kit::sub(cx, tr("settings.appearance.icon")))
                .child(div().flex().flex_wrap().gap(space::S1).children(icons))
                .child(kit::note(cx, tr("settings.appearance.note")));
        }
        col
    }
}

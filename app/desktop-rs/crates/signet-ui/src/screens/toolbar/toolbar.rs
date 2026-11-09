//! Port of `renderer/src/toolbar/Toolbar.tsx`: a pane's bar, blended into the site (color of the top of the page),
//! with the actions of the pane's tab.

use super::anchor::Anchor;
use super::extensions::ExtensionButtons;
use super::util::{
    BarColors, BarFade, IS_MAC, bar_colors, count99, device_key, device_name, domain_of, host_port, is_bookmarkable, is_http, is_secure, language_name, norm,
};
use crate::app_state::{api, snapshot, tr, tr_args, ui_state};
use crate::theme::{ThemeExt, radius, space, weight};
use gpui::{
    App, AppContext, ClickEvent, Context, ElementId, Entity, Hsla, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::{Icon, tooltip::Tooltip};
use signet_core::api::UiEvent;
use signet_core::devhosts::{alias_for, is_local_dev_url};
use signet_core::pagesettings::{ZoomIndicator, format_zoom_percent, zoom_indicator};
use signet_core::shortcuts::shortcut_label;
use signet_core::site::site_of;
use signet_core::types::{Bookmark, ColorScheme, PaletteMode, PasswordKey, ReaderState, Rect, SidebarSide, Snapshot, Space, SpaceProfile, TabInfo, TranslationState};
use std::collections::HashSet;

/// What the parent decides for a bar (the props of `Toolbar.tsx`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolbarProps {
    /// Tab targeted by the bar; the active tab by default.
    pub tab_id: Option<String>,
    /// Reserves room for the traffic lights (card at the very top left, sidebar hidden).
    pub leading: bool,
    /// Shows the sidebar toggle (only one bar at a time, the top-left pane's).
    pub sidebar_toggle: bool,
    /// Narrow card: only Reload, padlock and domain.
    pub narrow: bool,
    /// Pane without focus (split view): dimmed content.
    pub dim: bool,
    /// Floating bar of compact mode (fills its view, own background).
    pub floating: bool,
    /// An overlay covers the window: the bar is no window drag area (its drag zones would swallow clicks on macOS).
    pub no_drag: bool,
}

pub struct Toolbar {
    props: ToolbarProps,
    /// Same props, readable by the extension buttons' focus callback.
    shared: std::rc::Rc<std::cell::RefCell<ToolbarProps>>,
    ext: Entity<ExtensionButtons>,
    phones: u32,
    site: Anchor,
    address: Anchor,
    key: Anchor,
    fade: BarFade,
}

/// Lucide icon by file name (all of the catalog is reachable through the assets).
pub fn lucide(name: &str, size: f32) -> Icon {
    Icon::empty().path(format!("icons/{name}.svg")).size(px(size))
}

fn active_space(s: &Snapshot) -> Option<&Space> {
    s.spaces.iter().find(|x| x.id == s.active_space_id).or_else(|| s.spaces.first())
}

/// Normalized URLs of all bookmarks (folders included).
fn bookmark_urls(items: &[Bookmark], out: &mut HashSet<String>) {
    for i in items {
        match i {
            Bookmark::Link { url, .. } => {
                out.insert(norm(url).to_string());
            }
            Bookmark::Folder { items, .. } => bookmark_urls(items, out),
        }
    }
}

/// A bar button (`button.icon`): 28 px, or 24 px in the address line.
fn bar_button(id: impl Into<ElementId>, icon: Icon, tip: impl Into<SharedString>, bar: BarColors, size: f32, color: Option<Hsla>, disabled: bool) -> gpui::Stateful<gpui::Div> {
    let tip: SharedString = tip.into();
    let rest = color.unwrap_or(bar.fg2);
    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(radius::CONTROL)
        .text_color(rest)
        .occlude()
        .when(disabled, |d| d.opacity(0.35))
        .when(!disabled, |d| {
            let hover_fg = color.unwrap_or(bar.fg);
            d.cursor_pointer().hover(move |s| s.bg(bar.hover).text_color(hover_fg))
        })
        .when(!tip.is_empty(), |d| d.tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx)))
        .child(icon)
}

fn click<F: Fn(&ClickEvent, &mut Window, &mut App) + 'static>(d: gpui::Stateful<gpui::Div>, disabled: bool, f: F) -> gpui::Stateful<gpui::Div> {
    if disabled { d } else { d.on_click(f) }
}

impl Toolbar {
    pub fn new(props: ToolbarProps, cx: &mut App) -> Entity<Self> {
        let shared = std::rc::Rc::new(std::cell::RefCell::new(props.clone()));
        let focus_props = shared.clone();
        let ext = ExtensionButtons::new(std::rc::Rc::new(move |cx: &mut App| focus_tab(&focus_props.borrow(), cx)), cx);
        let phones = api(cx).get_link_state().active;
        cx.new(|cx| {
            cx.observe(&ui_state(cx), |_, _, cx| cx.notify()).detach();
            cx.subscribe(&ui_state(cx), |this: &mut Self, _, e: &UiEvent, cx| {
                if let UiEvent::LinkState(l) = e {
                    this.phones = l.active;
                    cx.notify();
                }
            })
            .detach();
            Self { props, shared, ext, phones, site: Anchor::default(), address: Anchor::default(), key: Anchor::default(), fade: BarFade::default() }
        })
    }

    pub fn set_props(&mut self, props: ToolbarProps, cx: &mut Context<Self>) {
        if self.props != props {
            *self.shared.borrow_mut() = props.clone();
            self.props = props;
            cx.notify();
        }
    }

    /// The address field's rectangle (window coordinates), for the command bar of Cmd+L.
    pub fn address_rect(&self) -> Rect {
        self.address.rect()
    }

    pub fn is_dim_or_floating(&self) -> bool {
        self.props.dim || self.props.floating
    }
}

/// Main's windows (site panel, address) target the active tab: focus it first.
fn focus_tab(p: &ToolbarProps, cx: &App) {
    let s = snapshot(cx);
    let Some(space) = active_space(&s) else { return };
    let id = p.tab_id.clone().or_else(|| space.active_tab_id.clone());
    if let Some(id) = id.filter(|id| Some(id) != space.active_tab_id.as_ref()) {
        api(cx).activate_tab(&id);
    }
}

impl Render for Toolbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = snapshot(cx);
        let t = cx.signet().clone();
        let p = self.props.clone();
        let Some(space) = active_space(&s).cloned() else { return div().into_any_element() };
        let id = p.tab_id.clone().or_else(|| space.active_tab_id.clone());
        let tab: Option<TabInfo> = id.as_ref().and_then(|i| s.tabs.get(i)).cloned();
        let url = tab.as_ref().map(|t| t.url.clone()).unwrap_or_default();
        let bar = self.fade.step(bar_colors(tab.as_ref().and_then(|t| t.theme_color.as_deref()), &t), crate::motion::reduced_motion(cx), window);
        let locale = signet_core::i18n::current_locale();
        let lang = locale.id();
        let mac = IS_MAC;
        let shortcuts = s.settings.shortcuts.as_ref();
        let keys = |name: &str| shortcut_label(name, shortcuts, mac);
        let with_keys = |label: String, k: Option<String>| match k {
            Some(k) if !k.is_empty() => format!("{label} ({k})"),
            _ => label,
        };
        let aliases = s.settings.dev_aliases.clone().unwrap_or_default();
        let alias = alias_for(&url, &aliases);
        let domain = match alias {
            Some(a) => a.name.clone(),
            None if !url.is_empty() => domain_of(&url),
            None => String::new(),
        };
        let title = tab.as_ref().map(|t| t.title.clone()).filter(|ti| !ti.is_empty() && *ti != domain).unwrap_or_default();
        let site = if is_http(&url) { site_of(&url) } else { String::new() };
        let boosted = !site.is_empty() && s.boosted_sites.contains(&site);
        let local = s.settings.dev_mode == Some(true) && is_local_dev_url(&url);
        let port = if local && alias.is_none() { host_port(&url).map(|(_, p)| p).unwrap_or_default() } else { String::new() };
        let errors = if local { tab.as_ref().and_then(|t| t.console_errors).unwrap_or(0) } else { 0 };
        let warnings = if local { tab.as_ref().and_then(|t| t.console_warnings).unwrap_or(0) } else { 0 };
        let overrides: Vec<String> = tab
            .as_ref()
            .and_then(|t| t.dev_overrides.as_ref())
            .map(|ov| {
                let mut v = Vec::new();
                if ov.no_cache == Some(true) {
                    v.push(tr("develop.noCache"));
                }
                if ov.no_js == Some(true) {
                    v.push(tr("develop.noJs"));
                }
                if let Some(sc) = ov.scheme {
                    v.push(tr(if sc == ColorScheme::Dark { "develop.schemeDark" } else { "develop.schemeLight" }));
                }
                if ov.reduced_motion == Some(true) {
                    v.push(tr("develop.reducedMotion"));
                }
                if let Some(n) = ov.network {
                    v.push(tr(&format!("develop.network.{}", serde_json::to_value(n).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default())));
                }
                if let Some(u) = ov.user_agent {
                    v.push(tr(&format!("develop.ua.{}", serde_json::to_value(u).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default())));
                }
                if let Some(r) = &ov.responsive {
                    let device = device_name(r.device).map(String::from).unwrap_or_else(|| tr(&format!("develop.device.{}", device_key(r.device))));
                    v.push(tr_args("develop.responsive", &[("device", &device)]));
                }
                v
            })
            .unwrap_or_default();
        let zoom_kind = zoom_indicator(tab.as_ref().and_then(|t| t.zoom), s.settings.default_zoom);
        let zoom_label = tab.as_ref().and_then(|t| t.zoom).map(|z| tr_args("zoom.label", &[("value", &format_zoom_percent(z, lang))])).unwrap_or_default();
        let bookmarkable = is_bookmarkable(&url);
        let bookmarked = bookmarkable && {
            let mut set = HashSet::new();
            bookmark_urls(space.bookmarks.as_deref().unwrap_or(&[]), &mut set);
            set.contains(norm(&url))
        };
        // Extensions of an isolated space live in its partition; favorites and shared spaces, in the default session.
        let isolated = space.profile == Some(SpaceProfile::Isolated) && !id.as_ref().is_some_and(|i| s.favorites.contains(i));
        let partition = if isolated { format!("persist:space-{}", space.id) } else { "_self".to_string() };
        let order = s.settings.extension_order.clone().unwrap_or_default();
        let pinned = s.settings.pinned_extensions.clone().unwrap_or_default();
        self.ext.update(cx, |e, cx| e.set(&partition, &order, &pinned, bar, cx));
        let narrow = p.narrow;
        let tab_id = id.clone();
        let props = p.clone();

        // Left controls.
        let mut row = div()
            .id("toolbar")
            .relative()
            .flex()
            .items_center()
            .gap(space::HALF)
            .h_full()
            .when(p.floating, |d| d.size_full().rounded(radius::POP).bg(bar.bg))
            .when(!p.floating, |d| d.w_full())
            .pl(if p.leading { px(78.0) } else { space::S1 })
            .pr(space::S2)
            .text_color(bar.fg)
            .when(!p.no_drag && !p.floating, crate::title_area::title_area)
            .when(p.dim, |d| d.opacity(0.7).hover(|s| s.opacity(1.0)));

        if p.sidebar_toggle {
            let label = with_keys(tr(if s.sidebar_visible { "toolbar.hideSidebar" } else { "toolbar.showSidebar" }), keys("toggleSidebar"));
            let icon = if s.settings.sidebar_side == Some(SidebarSide::Right) { "panel-right" } else { "panel-left" };
            row = row.child(click(bar_button("sidebar-toggle", lucide(icon, 17.0), label, bar, 28.0, None, false), false, |_, _, cx| api(cx).toggle_sidebar()));
        }
        if !narrow {
            let (tid, fid) = (tab_id.clone(), tab_id.clone());
            let can_back = tab.as_ref().is_some_and(|t| t.can_go_back);
            let can_fwd = tab.as_ref().is_some_and(|t| t.can_go_forward);
            row = row
                .child(click(bar_button("back", lucide("arrow-left", 17.0), tr("common.back"), bar, 28.0, None, !can_back), !can_back, move |_, _, cx| api(cx).go_back(tid.as_deref())))
                .child(click(bar_button("forward", lucide("arrow-right", 17.0), tr("common.forward"), bar, 28.0, None, !can_fwd), !can_fwd, move |_, _, cx| api(cx).go_forward(fid.as_deref())));
        }
        let loading = tab.as_ref().is_some_and(|t| t.loading);
        {
            let rid = tab_id.clone();
            let icon = if loading { lucide("x", 16.0) } else { lucide("rotate-cw", 15.0) };
            let tip = tr(if loading { "toolbar.stop" } else { "toolbar.reload" });
            row = row.child(click(bar_button("reload", icon, tip, bar, 28.0, None, tab.is_none()), tab.is_none(), move |_, _, cx| api(cx).reload(rid.as_deref())));
        }

        // Address line.
        let mut line = div().flex().flex_1().items_center().h(space::S7).min_w(px(180.0)).overflow_hidden();
        if !site.is_empty() {
            let secure = is_secure(&url);
            let anchor = self.site.clone();
            let pr = props.clone();
            let btn = bar_button("site", lucide(if secure { "lock" } else { "info" }, if secure { 14.0 } else { 15.0 }), tr(if secure { "toolbar.secure" } else { "toolbar.insecure" }), bar, 24.0, None, false)
                .child(anchor.track());
            line = line.child(click(btn, false, move |_, _, cx| {
                focus_tab(&pr, cx);
                api(cx).open_site_panel(anchor.rect());
            }));
        }
        {
            let anchor = self.address.clone();
            let pr = props.clone();
            let page_dim = bar.fg2;
            let content = if url.is_empty() {
                div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(page_dim).child(tr("toolbar.addressPlaceholder"))
            } else {
                div()
                    .flex()
                    .items_center()
                    .gap(space::S1)
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_none()
                            .max_w(gpui::relative(0.6))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_color(bar.fg)
                            .child(domain.clone())
                            .when(!port.is_empty(), |d| d.child(div().text_color(bar.fg2).child(format!(":{port}")))),
                    )
                    .when(!title.is_empty() && !narrow, |d| {
                        d.child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(bar.fg2).child(format!("/ {title}")))
                    })
            };
            line = line.child(
                div()
                    .id("address")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .pl(if site.is_empty() { space::S2 } else { space::S1 })
                    .pr(space::S1)
                    .rounded(radius::CONTROL)
                    .text_size(t.type_scale.base)
                    .occlude()
                    .cursor_pointer()
                    .hover(move |s| s.bg(bar.hover))
                    .on_click(move |_, _, cx| {
                        focus_tab(&pr, cx);
                        // The address field serves as the anchor: the command bar grows from it.
                        api(cx).open_palette(PaletteMode::EditUrl, Some(anchor.rect()));
                    })
                    .child(content)
                    .child(self.address.track()),
            );
        }
        if errors > 0 || warnings > 0 {
            let pr = props.clone();
            let cid = tab_id.clone();
            let tip = [
                (errors > 0).then(|| tr_args("toolbar.consoleErrors", &[("count", &errors.to_string())])),
                (warnings > 0).then(|| tr_args("toolbar.consoleWarnings", &[("count", &warnings.to_string())])),
                Some(tr("toolbar.openConsole")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
            let pill = |bg: Hsla, fg: Hsla, icon: &str, n: u32| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(3.0))
                    .h(px(18.0))
                    .pl(px(5.0))
                    .pr(px(6.0))
                    .rounded(px(9.0))
                    .bg(bg)
                    .text_color(fg)
                    .text_size(t.type_scale.cap)
                    .font_weight(weight::TITLE)
                    .child(lucide(icon, 12.0))
                    .child(count99(n))
            };
            let err_bg: Hsla = gpui::hsla(2.0 / 360.0, 0.72, 0.5, 1.0);
            let warn_bg: Hsla = gpui::hsla(40.0 / 360.0, 0.96, 0.52, 1.0);
            line = line.child(
                div()
                    .id("console-badge")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(3.0))
                    .h(px(22.0))
                    .px(px(2.0))
                    .rounded(px(11.0))
                    .occlude()
                    .cursor_pointer()
                    .tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx))
                    .on_click(move |_, _, cx| {
                        focus_tab(&pr, cx);
                        if let Some(id) = &cid {
                            api(cx).show_console(id);
                        }
                    })
                    .when(errors > 0, |d| d.child(pill(err_bg, gpui::white(), "circle-x", errors)))
                    .when(warnings > 0, |d| d.child(pill(warn_bg, gpui::rgb(0x281c00).into(), "triangle-alert", warnings))),
            );
        }
        if let (false, Some(tb)) = (narrow, tab.as_ref()) {
            if tb.foreign_lang.is_some() || tb.translation.is_some() {
                let pr = props.clone();
                let tid = tab_id.clone();
                let on = tb.translation == Some(TranslationState::On);
                let working = tb.translation == Some(TranslationState::Working);
                let tip = if tb.translation.is_some() {
                    tr("toolbar.showOriginal")
                } else {
                    tr_args("toolbar.translate", &[("language", &language_name(tb.foreign_lang.as_deref().unwrap_or(""), lang))])
                };
                let mut b = bar_button("translate", lucide("languages", 15.0), tip, bar, 24.0, (on || working).then_some(t.accent), false);
                if working {
                    b = b.opacity(0.6);
                }
                line = line.child(click(b, false, move |_, _, cx| {
                    focus_tab(&pr, cx);
                    if let Some(id) = &tid {
                        api(cx).toggle_translation(id);
                    }
                }));
            }
            if let Some(reader) = tb.reader {
                let pr = props.clone();
                let tid = tab_id.clone();
                let on = reader == ReaderState::On;
                let tip = if on {
                    tr("toolbar.exitReader")
                } else {
                    match keys("reader") {
                        Some(k) if !k.is_empty() => tr_args("toolbar.reader", &[("keys", &k)]),
                        _ => tr("toolbar.readerNoKeys"),
                    }
                };
                line = line.child(click(bar_button("reader", lucide("book-open", 15.0), tip, bar, 24.0, on.then_some(t.accent), false), false, move |_, _, cx| {
                    focus_tab(&pr, cx);
                    if let Some(id) = &tid {
                        api(cx).toggle_reader(id);
                    }
                }));
            }
            if let Some(key) = tb.password_key {
                let pr = props.clone();
                let tid = tab_id.clone();
                let anchor = self.key.clone();
                let pending = key == PasswordKey::Pending;
                let b = bar_button("key", lucide("key-round", 15.0), tr(if pending { "toolbar.savePassword" } else { "toolbar.sitePasswords" }), bar, 24.0, pending.then_some(t.accent), false)
                    .child(anchor.track());
                line = line.child(click(b, false, move |_, _, cx| {
                    focus_tab(&pr, cx);
                    if let Some(id) = &tid {
                        api(cx).open_password_key(id, anchor.rect());
                    }
                }));
            }
            if bookmarkable {
                let tid = tab_id.clone();
                let tip = match keys("bookmark") {
                    Some(k) if !k.is_empty() => tr_args(if bookmarked { "toolbar.removeBookmark" } else { "toolbar.addBookmark" }, &[("keys", &k)]),
                    _ => tr(if bookmarked { "toolbar.removeBookmarkNoKeys" } else { "toolbar.addBookmarkNoKeys" }),
                };
                let star = lucide("star", 15.0);
                let star = if bookmarked { star.text_color(t.accent) } else { star };
                let _ = tb;
                line = line.child(click(bar_button("star", star, tip, bar, 24.0, bookmarked.then_some(t.accent), false), false, move |_, _, cx| api(cx).toggle_bookmark(tid.as_deref())));
            }
        }
        row = row.child(line);

        if let (false, Some(kind), Some(tid)) = (narrow, zoom_kind, tab_id.clone()) {
            let pr = props.clone();
            let icon = lucide(if kind == ZoomIndicator::In { "zoom-in" } else { "zoom-out" }, 16.0);
            row = row.child(click(bar_button("zoom", icon, zoom_label, bar, 28.0, None, false), false, move |_, _, cx| {
                focus_tab(&pr, cx);
                api(cx).open_zoom(&tid);
            }));
        }
        if !narrow {
            row = row.child(self.ext.clone());
        }
        if !overrides.is_empty() {
            let pr = props.clone();
            let tid = tab_id.clone();
            let tip = tr_args("toolbar.devOverrides", &[("list", &overrides.join(", "))]);
            row = row.child(click(bar_button("develop", lucide("flask-conical", 16.0), tip, bar, 28.0, Some(t.accent), false), false, move |_, _, cx| {
                focus_tab(&pr, cx);
                if let Some(id) = &tid {
                    api(cx).show_develop_menu(id);
                }
            }));
        }
        if let (false, Some(tb)) = (narrow, tab.as_ref()) {
            if tb.devtools.is_some() {
                let pr = props.clone();
                let tid = tab_id.clone();
                row = row.child(click(bar_button("devtools", lucide("code", 16.0), tr("toolbar.devtools"), bar, 28.0, Some(t.accent), false), false, move |_, _, cx| {
                    focus_tab(&pr, cx);
                    if let Some(id) = &tid {
                        api(cx).show_dev_tools_menu(id);
                    }
                }));
            }
        }
        if !narrow && self.phones > 0 {
            let tip = tr_args("toolbar.phoneConnected", &[("count", &self.phones.to_string())]);
            row = row.child(click(bar_button("phone", lucide("smartphone", 15.0), tip, bar, 28.0, None, false), false, |_, _, cx| api(cx).open_settings(Some("devices"))));
        }
        if !narrow {
            let site2 = site.clone();
            let no_site = site.is_empty();
            row = row.child(click(
                bar_button("boost", lucide("rocket", 16.0), tr(if boosted { "toolbar.boostOn" } else { "toolbar.boostSite" }), bar, 28.0, boosted.then_some(t.accent), no_site),
                no_site,
                move |_, _, cx| api(cx).open_boost(&site2),
            ));
        }
        // Linux and Windows: the menu bar is hidden (Alt shows it); this button opens the whole app menu.
        if !IS_MAC {
            row = row.child(click(bar_button("app-menu", lucide("ellipsis-vertical", 16.0), tr("toolbar.appMenu"), bar, 28.0, None, false), false, |_, _, cx| api(cx).show_app_menu()));
        }
        row.into_any_element()
    }
}

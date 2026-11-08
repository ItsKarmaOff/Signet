//! Route `#settings`: the settings window (port of `settings/Settings.tsx`): section sidebar and title bar.

use std::rc::Rc;

use gpui::{
    AnyView, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, ScrollHandle, StatefulInteractiveElement,
    Styled, Window, div, prelude::FluentBuilder, px,
};
use gpui_component::Icon;
use signet_core::api::UiEvent;
use signet_core::i18n::IS_MAC;
use signet_core::types::SettingsInfo;

use super::kit;
use crate::app_state::{api, tr, ui_state};
use crate::theme::{ThemeExt, radius, space, weight};

/// Latest `SettingsInfo`, shared with the section views (they observe it).
pub struct InfoStore {
    pub info: Rc<SettingsInfo>,
}

/// Reads the latest info.
pub fn info_of(store: &Entity<InfoStore>, cx: &App) -> Rc<SettingsInfo> {
    store.read(cx).info.clone()
}

/// Re-render `cx`'s view when the settings info or the interface language changes.
pub fn watch<T: 'static>(store: &Entity<InfoStore>, cx: &mut Context<T>) {
    cx.observe(store, |_, _, cx| cx.notify()).detach();
    kit::watch_locale(cx);
}

/// Sent by a section that needs a modal over the whole window (the import picker).
pub enum ModalEvent {
    Show(AnyView),
    Hide,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SectionId {
    General,
    Appearance,
    Search,
    Shortcuts,
    Boosts,
    Routing,
    Extensions,
    Apps,
    Devices,
    Privacy,
    Passwords,
    Autofill,
    Backup,
    Import,
    Mods,
    Developer,
    System,
    About,
}

struct SectionDef {
    id: SectionId,
    /// Value of `data-section` and of `openSettings(section)`.
    key: &'static str,
    group: &'static str,
    label: &'static str,
    icon: &'static str,
    mac_only: bool,
}

const fn def(id: SectionId, key: &'static str, group: &'static str, label: &'static str, icon: &'static str) -> SectionDef {
    SectionDef { id, key, group, label, icon, mac_only: false }
}

const SECTIONS: [SectionDef; 18] = [
    def(SectionId::General, "general", "basics", "settings.section.general", "icons/settings.svg"),
    def(SectionId::Appearance, "appearance", "basics", "settings.section.appearance", "icons/palette.svg"),
    def(SectionId::Search, "search", "basics", "settings.section.search", "icons/search.svg"),
    def(SectionId::Shortcuts, "shortcuts", "basics", "settings.section.shortcuts", "icons/keyboard.svg"),
    def(SectionId::Boosts, "boosts", "browsing", "settings.section.boosts", "icons/rocket.svg"),
    def(SectionId::Routing, "routing", "browsing", "settings.section.routing", "icons/signpost.svg"),
    def(SectionId::Extensions, "extensions", "browsing", "settings.section.extensions", "icons/puzzle.svg"),
    // Sites installed as apps: macOS launcher, so section reserved for Mac.
    SectionDef { mac_only: true, ..def(SectionId::Apps, "apps", "browsing", "settings.section.apps", "icons/app-window.svg") },
    def(SectionId::Devices, "devices", "browsing", "settings.section.devices", "icons/smartphone.svg"),
    def(SectionId::Privacy, "privacy", "data", "settings.section.privacy", "icons/shield.svg"),
    def(SectionId::Passwords, "passwords", "data", "settings.section.passwords", "icons/key-round.svg"),
    def(SectionId::Autofill, "autofill", "data", "settings.section.autofill", "icons/text-cursor-input.svg"),
    def(SectionId::Backup, "backup", "data", "settings.section.backup", "icons/archive-restore.svg"),
    def(SectionId::Import, "import", "data", "settings.section.import", "icons/download.svg"),
    def(SectionId::Mods, "mods", "advanced", "settings.section.mods", "icons/paintbrush.svg"),
    def(SectionId::Developer, "developer", "advanced", "settings.section.developer", "icons/square-terminal.svg"),
    def(SectionId::System, "system", "advanced", "settings.section.system", "icons/cpu.svg"),
    def(SectionId::About, "about", "advanced", "settings.section.about", "icons/info.svg"),
];

fn def_of(id: SectionId) -> &'static SectionDef {
    SECTIONS.iter().find(|s| s.id == id).unwrap_or(&SECTIONS[0])
}

/// Root view of the `#settings` window.
pub struct SettingsWindow {
    store: Entity<InfoStore>,
    section: SectionId,
    view: AnyView,
    modal: Option<AnyView>,
    scroll: ScrollHandle,
}

impl SettingsWindow {
    /// Creates the window's root. The initial state is read with `get_settings_info`; later states arrive as
    /// `UiEvent::SettingsInfo`. Wrap the result in `gpui_component::Root` (dialogs and menus need it).
    pub fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            let initial = Rc::new(api(cx).get_settings_info());
            let store = cx.new(|_| InfoStore { info: initial });
            // A pushed state is always more recent than the initial read.
            let target = store.clone();
            cx.subscribe(&ui_state(cx), move |_, _, event: &UiEvent, cx| {
                if let UiEvent::SettingsInfo(info) = event {
                    let info = Rc::new((**info).clone());
                    target.update(cx, |s, cx| {
                        s.info = info;
                        cx.notify();
                    });
                }
            })
            .detach();
            kit::watch_locale(cx);
            let view = make_section(SectionId::General, &store, window, cx);
            SettingsWindow { store, section: SectionId::General, view, modal: None, scroll: ScrollHandle::new() }
        })
    }

    /// Shows a section by its key (`general`, `passwords`...). Unknown keys are ignored.
    pub fn open_section(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(def) = SECTIONS.iter().find(|s| s.key == key)
            && (!def.mac_only || IS_MAC)
        {
            self.select(def.id, window, cx);
        }
    }

    /// Key of the shown section.
    pub fn section_key(&self) -> &'static str {
        def_of(self.section).key
    }

    fn select(&mut self, id: SectionId, window: &mut Window, cx: &mut Context<Self>) {
        // Same as React: switching remounts the section, so its local state starts fresh.
        if id == self.section {
            return;
        }
        self.section = id;
        self.modal = None;
        self.view = make_section(id, &self.store, window, cx);
        self.scroll.set_offset(gpui::point(px(0.0), px(0.0)));
        cx.notify();
    }
}

fn make_section(id: SectionId, store: &Entity<InfoStore>, window: &mut Window, cx: &mut Context<SettingsWindow>) -> AnyView {
    let store = store.clone();
    match id {
        SectionId::General => cx.new(|cx| super::general::General::new(store, window, cx)).into(),
        SectionId::Appearance => cx.new(|cx| super::appearance::Appearance::new(store, cx)).into(),
        SectionId::Search => cx.new(|cx| super::search::SearchSettings::new(store, window, cx)).into(),
        SectionId::Shortcuts => cx.new(|cx| super::shortcuts::ShortcutSettings::new(store, window, cx)).into(),
        SectionId::Boosts => cx.new(|cx| super::boosts::Boosts::new(store, cx)).into(),
        SectionId::Routing => cx.new(|cx| super::routing::RoutingSettings::new(store, window, cx)).into(),
        SectionId::Extensions => cx.new(|cx| super::extensions::ExtensionSettings::new(store, cx)).into(),
        SectionId::Apps => cx.new(|cx| super::web_apps::WebAppSettings::new(window, cx)).into(),
        SectionId::Devices => cx.new(|cx| super::devices::DeviceSettings::new(window, cx)).into(),
        SectionId::Privacy => cx.new(|cx| super::privacy::Privacy::new(store, window, cx)).into(),
        SectionId::Passwords => cx.new(|cx| super::passwords::PasswordSettings::new(store, window, cx)).into(),
        SectionId::Autofill => cx.new(|cx| super::autofill::AutofillSettings::new(store, window, cx)).into(),
        SectionId::Backup => cx.new(|cx| super::backup::BackupSettings::new(window, cx)).into(),
        SectionId::Import => {
            let import = cx.new(|cx| super::import::Import::new(store, cx));
            cx.subscribe(&import, |w, _, ev: &ModalEvent, cx| {
                w.modal = match ev {
                    ModalEvent::Show(v) => Some(v.clone()),
                    ModalEvent::Hide => None,
                };
                cx.notify();
            })
            .detach();
            import.into()
        }
        SectionId::Mods => cx.new(|cx| super::mods::ModSettings::new(window, cx)).into(),
        SectionId::Developer => cx.new(|cx| super::developer::Developer::new(store, window, cx)).into(),
        SectionId::System => cx.new(|cx| super::developer::System::new(store, cx)).into(),
        SectionId::About => cx.new(|cx| super::developer::About::new(store, cx)).into(),
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let current = def_of(self.section);
        let this = cx.entity();

        let mut nav = div()
            .id("st-nav")
            .flex_none()
            .flex()
            .flex_col()
            .w(px(200.0))
            .h_full()
            .gap(space::HALF)
            .px(space::S3)
            .pb(space::S3)
            .overflow_y_scroll()
            .text_size(t.type_scale.base)
            .text_color(t.text)
            // Room for the traffic lights.
            .child(div().flex_none().h(px(52.0)));
        for (i, item) in SECTIONS.iter().enumerate() {
            if item.mac_only && !IS_MAC {
                continue;
            }
            let first_of_group = i == 0 || SECTIONS[i - 1].group != item.group;
            if first_of_group {
                nav = nav.child(
                    div()
                        .flex_shrink(1.0)
                        .px(space::S3)
                        .pt(space::S3)
                        .pb(space::HALF)
                        .text_size(px(11.0))
                        .font_weight(weight::STRONG)
                        .text_color(t.text_2)
                        .child(tr(&format!("settings.group.{}", item.group))),
                );
            }
            let on = item.id == self.section;
            let id = item.id;
            let this = this.clone();
            nav = nav.child(
                div()
                    .id(("st-nav-item", i))
                    .flex_shrink(1.0)
                    .min_h(px(20.0))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .h(space::S7)
                    .px(space::S3)
                    .rounded(radius::CONTROL)
                    .cursor_pointer()
                    .when(on, |d| d.bg(t.active).shadow(t.active_shadow.clone()))
                    .when(!on, |d| d.hover(|s| s.bg(t.surface_2)))
                    .on_click(move |_, window, cx| this.update(cx, |w, cx| w.select(id, window, cx)))
                    .child(Icon::empty().path(item.icon).size(px(15.0)).text_color(if on { t.accent } else { t.text_2 }))
                    .child(tr(item.label)),
            );
        }

        // Dev harness: SIGNET_SETTINGS_SCROLL=<px> scrolls the content once, for screenshots of long sections.
        static FRAMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        if let Some(dy) = std::env::var("SIGNET_SETTINGS_SCROLL").ok().and_then(|v| v.parse::<f32>().ok())
            && FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 12
        {
            self.scroll.set_offset(gpui::point(px(0.0), px(-dy)));
        }

        // Title separator: only appears when the content scrolls underneath.
        let scrolled = (-f32::from(self.scroll.offset().y) / 24.0).clamp(0.0, 1.0);
        let _ = window;
        let main = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(t.win_bg)
            .border_l(px(0.5))
            .border_color(t.border)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .h(px(52.0))
                    .px(space::S7)
                    .text_size(t.type_scale.title)
                    .font_weight(weight::TITLE)
                    .text_color(t.text)
                    .border_b(px(0.5))
                    .border_color(t.border.opacity(scrolled))
                    .child(tr(current.label)),
            )
            .child(
                div()
                    .id("st-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px(space::S7)
                    .pt(space::S5)
                    .pb(space::S8)
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .text_size(t.type_scale.base)
                    .text_color(t.text)
                    // Description lines of the two sections that have no controls of their own up front.
                    .when(matches!(self.section, SectionId::Boosts | SectionId::Routing), |d| {
                        d.child(kit::note(cx, tr(&format!("settings.desc.{}", current.key))))
                    })
                    .child(self.view.clone()),
            );

        // Electron: with glass on the window is bare vibrancy (no space tint, `html.route-settings` is transparent);
        // with glass off it is `--bg`, tinted by the space hue. Glass on is drawn here as the untinted base color.
        let window_bg = if t.bg.a < 1.0 {
            let base = if matches!(t.scheme, crate::color::Scheme::Dark) { crate::color::rgba8(44, 43, 48, 0.72) } else { crate::color::rgba8(236, 236, 239, 0.72) };
            crate::color::hsla(base)
        } else {
            t.bg
        };
        div().relative().flex().size_full().bg(window_bg).line_height(gpui::relative(1.4)).child(nav).child(main).children(self.modal.clone())
    }
}

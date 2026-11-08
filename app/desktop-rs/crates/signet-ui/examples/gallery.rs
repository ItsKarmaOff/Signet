//! Visual check of every Signet widget. `cargo run -p signet-ui --example gallery [-- --dark|--light]`.
//! The scheme switch at the top flips between light and dark live (the theme is a global).

use gpui::{
    App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, WindowBounds, WindowOptions, div, prelude::FluentBuilder, px, size, Bounds,
};
use gpui_component::{
    Root,
    button::{Button, ButtonVariants},
    input::InputState,
    slider::SliderState,
};
use signet_ui::{
    IconName, SignetAssets, SignetTheme, ThemeExt, ThemeMode, ThemeSettings, init,
    motion::Presence,
    theme::{radius, space, weight},
    watch_system_appearance,
    widgets::*,
};
use std::rc::Rc;

struct Gallery {
    toggle: bool,
    seg: usize,
    seg_perm: usize,
    field: Entity<InputState>,
    slider: Entity<SliderState>,
    active_row: usize,
    toast: Presence<SharedString>,
    media_playing: bool,
    media: Presence<()>,
    picker: Entity<EmojiPicker>,
    picked: SharedString,
    drop: Option<DropEdge>,
    hue: f32,
}

impl Gallery {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = text_field_state(window, cx, "Search or enter address", "");
        let slider = slider_state(cx, 0.0, 100.0, 1.0, 40.0);
        let labels = EmojiPickerLabels { search: "Search emoji".into(), recent: "Recent".into(), no_results: "No results".into() };
        let picker = cx.new(|cx| EmojiPicker::new(window, cx, "en", labels, vec![]));
        cx.subscribe(&picker, |this, _, ev: &EmojiPicked, cx| {
            this.picked = ev.0.clone().into();
            cx.notify();
        })
        .detach();
        Self {
            toggle: true,
            seg: 0,
            seg_perm: 1,
            field,
            slider,
            active_row: 1,
            toast: Presence::new(Some("3 tabs archived".into())),
            media_playing: true,
            media: Presence::new(Some(())),
            picker,
            picked: "".into(),
            drop: Some(DropEdge::After),
            hue: 250.0,
        }
    }
}

fn card(cx: &App, title: &'static str) -> gpui::Div {
    let t = cx.signet();
    div()
        .flex()
        .flex_col()
        .gap(space::S2)
        .p(space::S3)
        .rounded(radius::GROUP)
        .bg(t.win_card)
        .child(div().text_size(t.type_scale.cap).font_weight(weight::STRONG).text_color(t.text_2).child(title))
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let me = cx.entity();
        let dark = t.is_dark();

        let top = div()
            .flex()
            .items_center()
            .gap(space::S3)
            .child(div().text_size(t.type_scale.title).font_weight(weight::TITLE).child("Signet widgets"))
            .child(
                Segmented::new("scheme").item("System").item("Light").item("Dark").selected(match t.settings.mode {
                    ThemeMode::System => 0,
                    ThemeMode::Light => 1,
                    ThemeMode::Dark => 2,
                }).on_change(|ix, _, cx| {
                    SignetTheme::update_settings(cx, |s| s.mode = [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark][ix]);
                }),
            )
            .child(
                Segmented::new("hue").item("Violet").item("Coral").item("Green").item("Gradient").selected(0).on_change(|ix, _, cx| {
                    SignetTheme::update_settings(cx, |s| s.hues = [vec![250.0], vec![20.0], vec![150.0], vec![10.0, 280.0, 200.0]][ix].clone());
                }),
            );

        let buttons = card(cx, "Icon buttons, shortcuts, spinner")
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S1)
                    .child(IconButton::new("b1", IconName::ArrowLeft).tooltip("Back"))
                    .child(IconButton::new("b2", IconName::ArrowRight).tooltip("Forward").disabled(true))
                    .child(IconButton::new("b3", IconName::RefreshCw).tooltip("Reload"))
                    .child(IconButton::new("b4", IconName::VolumeX).active(true).tooltip("Unmute"))
                    .child(IconButton::new("b5", IconName::Trash2).danger(true).small())
                    .child(Spinner::new("spin"))
                    .child(ShortcutLabel::from_accelerator("CommandOrControl+Shift+T", cfg!(target_os = "macos")))
                    .child(ShortcutLabel::new("Ctrl+K").boxed(true)),
            )
            .child(
                div()
                    .flex()
                    .gap(space::S2)
                    .child(Button::new("prim").label("Primary").primary())
                    .child(Button::new("sec").label("Secondary"))
                    .child(Button::new("dng").label("Danger").danger()),
            )
            .child(ProgressBar::new("p1").value(0.6))
            .child(ProgressBar::new("p2"));

        let rows = card(cx, "Rows (tabs)")
            .child(SectionHeader::new("Today").trailing(IconButton::new("sh", IconName::Plus).small()))
            .children((0..4).map(|i| {
                let me = me.clone();
                let titles = ["Signet - Arc-style browser", "GitHub - claude/signet", "A very long page title that has to be truncated nicely", "Unloaded tab"];
                ListRow::new(("row", i))
                    .leading(Favicon::new(None).letter(['S', 'G', 'A', 'U'][i], [t.accent, t.danger, t.text_2, t.text_3][i]))
                    .title(titles[i])
                    .active(self.active_row == i)
                    .dim(i == 3)
                    .nested(false)
                    .drop_indicator(if i == 2 { self.drop } else { None })
                    .trailing_on_hover(IconButton::new(("close", i), IconName::X).small().tooltip("Close"))
                    .on_click(move |_, _, cx| me.update(cx, |this, cx| { this.active_row = i; cx.notify(); }))
            }))
            .child(Separator::horizontal())
            .child(ListRow::new("new").leading(gpui_component::Icon::from(IconName::Plus).size(px(14.0))).title("New Tab").trailing(ShortcutLabel::new("⌘T")));

        let forms = card(cx, "Toggle, segmented, field, slider")
            .child(
                div().flex().items_center().justify_between().child("Block ads").child({
                    let me = me.clone();
                    Toggle::new("tg", self.toggle).on_change(move |v, _, cx| me.update(cx, |this, cx| { this.toggle = v; cx.notify(); }))
                }),
            )
            .child(Toggle::new("tg-off", false).disabled(true))
            .child({
                let me = me.clone();
                Segmented::new("seg").icon_item(IconName::PanelRight, "Sidebar").icon_item(IconName::PanelBottom, "Top").selected(self.seg).on_change(move |ix, _, cx| me.update(cx, |this, cx| { this.seg = ix; cx.notify(); }))
            })
            .child({
                let me = me.clone();
                Segmented::new("perm").item("Ask").tinted_item("Allow", t.accent).tinted_item("Deny", t.danger).selected(self.seg_perm).on_change(move |ix, _, cx| me.update(cx, |this, cx| { this.seg_perm = ix; cx.notify(); }))
            })
            .child(TextField::new(&self.field).cleanable(true))
            .child(SliderField::new(&self.slider).label("Grain").value_text("40%"));

        let menus = card(cx, "Menus, popover, toast, confirm")
            .child(
                div()
                    .flex()
                    .gap(space::S2)
                    .child(dropdown_menu(
                        "dd",
                        Button::new("dd-btn").label("Dropdown menu"),
                        vec![
                            MenuEntry::item("Reload", |_, _, _| {}).icon(IconName::RefreshCw).shortcut("⌘R"),
                            MenuEntry::item("Mute tab", |_, _, _| {}).checked(true),
                            MenuEntry::item("Disabled", |_, _, _| {}).disabled(true),
                            MenuEntry::separator(),
                            MenuEntry::submenu("Move to", vec![MenuEntry::item("Space 2", |_, _, _| {})]),
                            MenuEntry::item("Close tab", |_, _, _| {}).danger(true).icon(IconName::X),
                        ],
                    ))
                    .child(context_menu(
                        div().id("ctx").px(space::S3).py(space::S2).rounded(radius::CONTROL).bg(t.surface_2).child("Right-click me"),
                        vec![MenuEntry::item("Copy link", |_, _, _| {}).icon(IconName::Copy), MenuEntry::item("Delete", |_, _, _| {}).danger(true)],
                    ))
                    .child(Button::new("confirm").label("Confirm dialog...").on_click(|_, window, cx| {
                        confirm_dialog(
                            window,
                            cx,
                            ConfirmDialog { title: "Delete this folder?".into(), message: "Its 4 tabs will be closed.".into(), confirm: "Delete".into(), cancel: "Cancel".into(), danger: true, on_confirm: Rc::new(|_, _, _| {}) },
                        );
                    })),
            )
            .child(popover_content(cx).child(div().p(space::S2).child("Popover surface (glass, shadow)")))
            .children(self.toast.value().cloned().map(|msg| {
                let me = me.clone();
                Toast::new("toast", msg).closing(self.toast.closing()).action("Undo", move |_, _, cx| {
                    me.update(cx, |this, cx| {
                        Presence::set(this, |g| &mut g.toast, None, cx);
                    })
                })
            }))
            .child(
                div().flex().gap(space::S2).child(Button::new("toast-btn").label("Show toast").on_click({
                    let me = me.clone();
                    move |_, _, cx| me.update(cx, |this, cx| Presence::set(this, |g| &mut g.toast, Some("Tab archived".into()), cx))
                })),
            )
            .child(InlineConfirm::new("ic", "Delete 12 passwords?", "Delete", "Cancel").on_confirm(|_, _| {}).on_cancel(|_, _| {}));

        let media = card(cx, "Favicon, media card, empty state")
            .child(div().flex().items_center().gap(space::S3).child(Favicon::new(None)).child(Favicon::new(None).letter('x', t.accent).size(20.0)).child(Favicon::new(None).loading(true)))
            .children(self.media.value().map(|_| {
                let me = me.clone();
                let me2 = me.clone();
                let playing = self.media_playing;
                MediaCard::new(
                    "media",
                    MediaInfo { title: "Lo-fi beats to study to".into(), artist: Some("ChilledCow".into()), site: "youtube.com".into(), artwork: None, favicon: None, playing, muted: false, can_skip: true },
                    MediaLabels { previous: "Previous".into(), next: "Next".into(), play: "Play".into(), pause: "Pause".into(), mute: "Mute".into(), unmute: "Unmute".into() },
                )
                .closing(self.media.closing())
                .on_toggle(move |_, _, cx| me.update(cx, |this, cx| { this.media_playing = !this.media_playing; cx.notify(); }))
                .on_mute(move |_, _, cx| me2.update(cx, |_, _| {}))
            }))
            .child(EmptyState::new("No downloads").icon(IconName::Download).hint("Files you download appear here."));

        let picker = card(cx, "Emoji picker").child(self.picker.clone()).child(div().text_size(px(22.0)).child(self.picked.clone()));

        let icons = card(cx, "Icons").child(div().flex().flex_wrap().gap(space::S2).children(IconName::ALL.iter().map(|i| gpui_component::Icon::from(*i).size(px(16.0)).text_color(t.text_2))));

        let _ = (window.bounds(), self.hue);
        div()
            .id("gallery")
            .size_full()
            .bg(if t.settings.glass { t.bg } else { t.bg_opaque })
            .text_color(t.text)
            .text_size(t.type_scale.base)
            .when(dark, |d| d)
            .overflow_y_scroll()
            .p(space::S4)
            .flex()
            .flex_col()
            .gap(space::S3)
            .child(top)
            .child(
                div().flex().gap(space::S3).items_start().child(div().flex().flex_col().gap(space::S3).w(px(340.0)).child(buttons).child(rows).child(forms)).child(div().flex().flex_col().gap(space::S3).w(px(360.0)).child(menus).child(media).child(icons)).child(div().w(px(280.0)).child(picker)),
            )
    }
}

fn main() {
    let dark = std::env::args().any(|a| a == "--dark");
    let light = std::env::args().any(|a| a == "--light");
    gpui_platform::application().with_assets(SignetAssets).run(move |cx: &mut App| {
        let mode = if dark { ThemeMode::Dark } else if light { ThemeMode::Light } else { ThemeMode::System };
        init(cx, ThemeSettings { mode, ..Default::default() });
        let bounds = Bounds::centered(None, size(px(1060.0), px(820.0)), cx);
        let background = cx.signet().window_background();
        cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), window_background: background, ..Default::default() }, |window, cx| {
            watch_system_appearance(window, cx);
            let view = cx.new(|cx| Gallery::new(window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("open window");
        cx.activate(true);
    });
}

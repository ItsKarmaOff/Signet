//! Port of `tabs/TabCustom.tsx`: tab, folder and group icons (favicon or chosen emoji), displayed title, in-place
//! rename field and the "choose an icon" panel.

use std::sync::Arc;

use gpui::{
    AnyElement, AppContext, Context, Entity, EventEmitter, Hsla, Image, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
    div, prelude::FluentBuilder, px,
};
use gpui_component::{
    Icon, Sizable,
    input::{Input, InputEvent, InputState},
};
use signet_core::types::{FolderItem, IconTarget, Item, Snapshot, TabGroup, TabInfo};

use crate::app_state::{api, tr, tr_args, ui_state};
use crate::color::{hsl_to_rgb, oklch_hue, oklch_to_rgb};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::{EmojiPicked, EmojiPicker, EmojiPickerLabels, Favicon, text_field_state};

/// Displayed title: the name chosen by the user, otherwise the page's.
pub fn display_title(tab: &TabInfo, fallback: &str) -> String {
    if let Some(t) = tab.custom_title.as_deref().filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    if !tab.title.is_empty() {
        return tab.title.clone();
    }
    if !tab.url.is_empty() {
        return tab.url.clone();
    }
    fallback.to_string()
}

/// Emoji shown in place of an icon, in a `size` px box.
pub fn emoji_icon(emoji: &str, size: f32) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .line_height(px(size))
        .text_size(px(size - 1.0))
        .font_family(crate::theme::emoji_font())
        .child(SharedString::from(emoji.to_string()))
        .into_any_element()
}

/// Decodes an image `data:` URL into a GPUI image. None if the format is unknown.
pub fn decode_data_url(url: &str) -> Option<Arc<Image>> {
    crate::data_url::decode_image(url)
}

pub use crate::data_url::favicon_source;

/// Tab icon: the chosen emoji in place of the favicon.
pub fn tab_icon(tab: &TabInfo, size: f32) -> AnyElement {
    if let Some(e) = tab.custom_icon.as_deref().filter(|e| !e.is_empty()) {
        return emoji_icon(e, size);
    }
    Favicon::new(favicon_source(tab.favicon.as_deref())).size(size).loading(tab.loading).into_any_element()
}

/// Folder icon: live source, chosen emoji, otherwise the open or closed folder.
pub fn folder_icon(folder: &FolderItem, size: f32, color: Hsla, danger: Hsla) -> AnyElement {
    if let Some(live) = &folder.live {
        return live_icon(folder, live, size, color, danger);
    }
    if let Some(e) = folder.icon.as_deref().filter(|e| !e.is_empty()) {
        return emoji_icon(e, size);
    }
    let name = if folder.open { IconName::FolderOpen } else { IconName::Folder };
    Icon::from(name).size(px(size)).text_color(color).into_any_element()
}

/// Live folder icon: its source (feed, GitHub) or the chosen emoji; flags an update in progress or a failure.
fn live_icon(folder: &FolderItem, live: &signet_core::types::LiveFolder, size: f32, color: Hsla, danger: Hsla) -> AnyElement {
    use signet_core::types::LiveSource;
    let base: AnyElement = match folder.icon.as_deref().filter(|e| !e.is_empty()) {
        Some(e) => emoji_icon(e, size),
        None => {
            let name = if matches!(live.source, LiveSource::Github { .. }) { IconName::GitPullRequest } else { IconName::Rss };
            Icon::from(name).size(px(size)).text_color(color).into_any_element()
        }
    };
    let small = (size * 0.6).round();
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .child(base)
        .when(live.refreshing == Some(true), |d| {
            d.child(div().absolute().right(px(-2.0)).bottom(px(-2.0)).child(Icon::from(IconName::RefreshCw).size(px(small)).text_color(color)))
        })
        .when(live.refreshing != Some(true) && live.error.is_some(), |d| {
            d.child(div().absolute().right(px(-2.0)).bottom(px(-2.0)).child(Icon::from(IconName::CircleAlert).size(px(small)).text_color(danger)))
        })
        .into_any_element()
}

/// Tooltip of a live folder icon.
pub fn live_title(live: &signet_core::types::LiveFolder) -> String {
    if live.refreshing == Some(true) {
        tr("folder.liveRefreshing")
    } else if let Some(e) = &live.error {
        tr_args("folder.liveError", &[("error", e)])
    } else {
        tr("folder.live")
    }
}

/// Tab group icon: the chosen emoji, otherwise its first tab's.
pub fn group_icon(group: Option<&TabGroup>, first: Option<&TabInfo>, size: f32, color: Hsla) -> AnyElement {
    if let Some(e) = group.and_then(|g| g.icon.as_deref()).filter(|e| !e.is_empty()) {
        return emoji_icon(e, size);
    }
    match first {
        Some(t) => tab_icon(t, size),
        None => Icon::from(IconName::Folder).size(px(size)).text_color(color).into_any_element(),
    }
}

/// Colors of a tinted card (pinned folder, tab group) from its hue: background and title/icon foreground.
/// `oklch(from hsl(h 60% 50%) 0.62 0.13 h / .12)` and the matching foreground, lighter in dark mode.
#[derive(Clone, Copy)]
pub struct Tint {
    pub bg: Hsla,
    pub fg: Hsla,
    /// Hover background of a label on the card (tab row at the top).
    pub bg_hover: Hsla,
}

pub fn tint(hue: f32, dark: bool) -> Tint {
    let h = oklch_hue(hsl_to_rgb(hue, 0.6, 0.5));
    let bg = oklch_to_rgb(0.62, 0.13, h);
    let (fg_rgb, fg_h) = if dark {
        let fh = oklch_hue(hsl_to_rgb(hue, 0.7, 0.62));
        (oklch_to_rgb(0.76, 0.11, fh), fh)
    } else {
        let fh = oklch_hue(hsl_to_rgb(hue, 0.6, 0.4));
        (oklch_to_rgb(0.5, 0.11, fh), fh)
    };
    let _ = fg_h;
    let rgba = |c: [f32; 3], a: f32| gpui::Rgba { r: c[0], g: c[1], b: c[2], a };
    Tint { bg: rgba(bg, if dark { 0.16 } else { 0.12 }).into(), fg: rgba(fg_rgb, 1.0).into(), bg_hover: rgba(bg, 0.14 + if dark { 0.04 } else { 0.0 }).into() }
}

// ---------------------------------------------------------------------------------------------------------------
// In-place rename field

/// Outcome of a [`RenameField`].
pub enum RenameEvent {
    /// Enter or focus lost: the typed text (untrimmed).
    Commit(String),
    /// Escape.
    Cancel,
}

/// Input shown in place of a title (`.folder-input`): Enter or blur confirms, Escape cancels. Emits once.
pub struct RenameField {
    input: Entity<InputState>,
    done: bool,
    max_len: usize,
    _subs: Vec<gpui::Subscription>,
}

impl EventEmitter<RenameEvent> for RenameField {}

impl RenameField {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, initial: &str, placeholder: &str, max_len: usize) -> Self {
        let input = text_field_state(window, cx, placeholder.to_string(), initial.to_string());
        let sub = cx.subscribe_in(&input, window, |this, input, ev: &InputEvent, _w, cx| match ev {
            InputEvent::PressEnter { .. } | InputEvent::Blur => this.finish(false, input.read(cx).value().to_string(), cx),
            _ => {}
        });
        input.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        RenameField { input, done: false, max_len, _subs: vec![sub] }
    }

    fn finish(&mut self, cancel: bool, value: String, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.done = true;
        if cancel {
            cx.emit(RenameEvent::Cancel);
        } else {
            cx.emit(RenameEvent::Commit(value.chars().take(self.max_len).collect()));
        }
    }
}

impl Render for RenameField {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        div()
            .id("rename-field")
            .flex_1()
            .min_w_0()
            .h(px(22.0))
            .rounded(radius::ROW_IN)
            .bg(t.active)
            .border_2()
            .border_color(t.accent)
            .text_color(t.text)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _w, cx| {
                if e.keystroke.key == "escape" {
                    this.finish(true, String::new(), cx);
                    cx.stop_propagation();
                }
            }))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(Input::new(&self.input).appearance(false).small().h(px(18.0)).px(px(4.0)).text_color(t.text))
    }
}

// ---------------------------------------------------------------------------------------------------------------
// "Choose an icon" panel

/// Emitted by [`IconForm`] when it must close (picked, removed, cancelled).
pub struct IconFormClose;

struct IconInfo {
    preview: AnyElement,
    name: String,
    current: Option<String>,
}

fn find_folder_in<'a>(items: &'a [Item], id: &str) -> Option<&'a FolderItem> {
    for i in items {
        if let Item::Folder(f) = i {
            if f.id == id {
                return Some(f);
            }
            if let Some(inner) = find_folder_in(&f.items, id) {
                return Some(inner);
            }
        }
    }
    None
}

fn tab_ids_of(i: &Item) -> Vec<String> {
    match i {
        Item::Tab(t) => vec![t.tab_id.clone()],
        Item::Split(s) => s.tab_ids.clone(),
        Item::Folder(f) => f.items.iter().flat_map(tab_ids_of).collect(),
    }
}

/// What the panel shows and edits for a target: preview, name, current emoji.
fn describe(s: &Snapshot, target: &IconTarget, fallback: &str, color: Hsla, danger: Hsla) -> Option<IconInfo> {
    match target {
        IconTarget::Tab { tab_id } => {
            let tab = s.tabs.get(tab_id)?;
            Some(IconInfo { preview: tab_icon(tab, 18.0), name: display_title(tab, fallback), current: tab.custom_icon.clone() })
        }
        IconTarget::Folder { folder_id } => s.spaces.iter().find_map(|sp| find_folder_in(&sp.pinned, folder_id)).map(|f| IconInfo {
            preview: folder_icon(f, 18.0, color, danger),
            name: f.name.clone(),
            current: f.icon.clone(),
        }),
        IconTarget::Group { group_id } => s.spaces.iter().find_map(|sp| {
            let group = sp.groups.as_ref()?.get(group_id)?;
            let first_item = sp.today.iter().find(|i| i.group() == Some(group_id.as_str()));
            let first = first_item.and_then(|i| tab_ids_of(i).into_iter().next()).and_then(|id| s.tabs.get(&id));
            Some(IconInfo {
                preview: group_icon(Some(group), first, 18.0, color),
                name: if group.name.is_empty() { first.map(|f| display_title(f, fallback)).unwrap_or_default() } else { group.name.clone() },
                current: group.icon.clone(),
            })
        }),
    }
}

/// "Choose an icon" panel for a tab, folder or group, confined to the sidebar like the space form. The host places it
/// (the sidebar draws it over its bottom bar). Emits [`IconFormClose`].
pub struct IconForm {
    target: IconTarget,
    picker: Entity<EmojiPicker>,
    _subs: Vec<gpui::Subscription>,
}

impl EventEmitter<IconFormClose> for IconForm {}

impl IconForm {
    pub fn new(target: IconTarget, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let labels = EmojiPickerLabels { search: tr("emoji.search").into(), recent: tr("emoji.recent").into(), no_results: tr("emoji.noResults").into() };
        let locale = ui_state(cx).read(cx).locale.id();
        let picker = cx.new(|cx| EmojiPicker::new(window, cx, locale, labels, Vec::new()).columns(7));
        let t = target.clone();
        let sub = cx.subscribe(&picker, move |_this, _p, ev: &EmojiPicked, cx| {
            api(cx).set_icon(t.clone(), Some(&ev.0));
            cx.emit(IconFormClose);
        });
        IconForm { target, picker, _subs: vec![sub] }
    }

    pub fn target(&self) -> &IconTarget {
        &self.target
    }
}

impl Render for IconForm {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let snap = ui_state(cx).read(cx).snapshot.clone();
        let Some(info) = describe(&snap, &self.target, &tr("sidebar.newTab"), t.text_2, t.danger) else {
            return div().into_any_element();
        };
        let has_current = info.current.is_some();
        let target = self.target.clone();
        let remove_t = t.clone();
        let btn = |id: &'static str, label: String, disabled: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .h(px(28.0))
                .px(space::S3)
                .rounded(radius::CONTROL)
                .text_color(t.text)
                .when(disabled, |d| d.opacity(0.4))
                .when(!disabled, |d| {
                    let hover = t.surface_2;
                    d.cursor_pointer().hover(move |s| s.bg(hover))
                })
                .child(label)
        };
        div()
            .id("tab-icon-form")
            .flex()
            .flex_col()
            .gap(space::S3)
            .p(space::S3)
            .rounded(radius::POP)
            .bg(t.glass_view)
            .shadow(t.shadow_pop.clone())
            .border_1()
            .border_color(t.border)
            .text_size(t.type_scale.base)
            .text_color(t.text)
            .on_key_down(cx.listener(|_this, e: &KeyDownEvent, _w, cx| {
                if e.keystroke.key == "escape" {
                    cx.emit(IconFormClose);
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .min_w_0()
                    .child(info.preview)
                    .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(weight::STRONG).child(info.name)),
            )
            .child(div().h(px(220.0)).child(self.picker.clone()))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space::S2)
                    .child(btn("cancel", tr("common.cancel"), false).on_click(cx.listener(|_t, _, _w, cx| cx.emit(IconFormClose))))
                    .child(
                        btn("remove", tr("tabIcon.remove"), !has_current).on_click(cx.listener(move |_t, _, _w, cx| {
                            if has_current {
                                let _ = &remove_t;
                                api(cx).set_icon(target.clone(), None);
                                cx.emit(IconFormClose);
                            }
                        })),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_title_prefers_the_custom_title_then_title_then_url() {
        let mut t = TabInfo { title: "Page".into(), url: "https://x.y/".into(), ..Default::default() };
        assert_eq!(display_title(&t, "New"), "Page");
        t.custom_title = Some("Mine".into());
        assert_eq!(display_title(&t, "New"), "Mine");
        t.custom_title = None;
        t.title.clear();
        assert_eq!(display_title(&t, "New"), "https://x.y/");
        t.url.clear();
        assert_eq!(display_title(&t, "New"), "New");
    }

    #[test]
    fn tint_is_translucent_and_lighter_in_dark_mode() {
        let (l, d) = (tint(120.0, false), tint(120.0, true));
        assert!(l.bg.a < 0.2 && d.bg.a > l.bg.a);
        assert!(d.fg.l > l.fg.l);
    }
}

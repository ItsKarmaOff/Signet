//! Forms shown in the tab-row popovers: space form, live folder form, icon picker, rename field.

use super::common::{active_space, display_title, emoji, favicon, find_bookmark, find_folder, form_box, form_btn};
use crate::app_state::{api, snapshot, tr};
use crate::color::{hsla, swatch_color};
use crate::icon::IconName;
use crate::theme::{ThemeExt, radius, space, weight};
use crate::widgets::{EmojiPicked, EmojiPicker, EmojiPickerLabels, Segmented, Toggle, slider_state, text_field_state};
use gpui::{
    AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription, Window, div, linear_color_stop,
    linear_gradient, prelude::FluentBuilder, px,
};
use gpui_component::{
    Icon,
    input::{InputEvent, InputState},
    slider::{SliderEvent, SliderState, SliderValue},
};
use signet_core::types::{GithubQuery, IconTarget, Item, LiveSource, RenameTarget, Space, SpaceDraft, SpaceProfile, SpaceTheme};

pub const HUES: [f64; 10] = [0.0, 25.0, 45.0, 90.0, 150.0, 190.0, 215.0, 250.0, 285.0, 330.0];

fn close(cx: &gpui::App) {
    api(cx).close_overlay(false);
}

fn emoji_picker(window: &mut Window, cx: &mut gpui::App) -> Entity<EmojiPicker> {
    let labels = EmojiPickerLabels { search: tr("emoji.search").into(), recent: tr("emoji.recent").into(), no_results: tr("emoji.noResults").into() };
    let locale = signet_core::i18n::current_locale().id();
    cx.new(|cx| EmojiPicker::new(window, cx, locale, labels, vec![]))
}

fn theme_preview(hues: &[f64], solid_hue: f64) -> gpui::Div {
    let c = |h: f64| hsla(swatch_color(h as f32));
    let base = div().w(px(40.0)).h(px(20.0)).rounded(px(6.0)).overflow_hidden().flex().flex_none();
    match hues.len() {
        0 | 1 => base.bg(c(hues.first().copied().unwrap_or(solid_hue))),
        2 => base.child(div().size_full().bg(linear_gradient(90.0, linear_color_stop(c(hues[0]), 0.0), linear_color_stop(c(hues[1]), 1.0)))),
        _ => base
            .child(div().h_full().w_1_2().bg(linear_gradient(90.0, linear_color_stop(c(hues[0]), 0.0), linear_color_stop(c(hues[1]), 1.0))))
            .child(div().h_full().w_1_2().bg(linear_gradient(90.0, linear_color_stop(c(hues[1]), 0.0), linear_color_stop(c(hues[2]), 1.0)))),
    }
}


pub(crate) use super::common::form_btn as primary_btn;

/// `.space-form input`: 32 px, `surface-2` fill, no border, 2 px accent ring on focus (the ring is a transparent border
/// otherwise, so the layout never shifts).
#[derive(gpui::IntoElement)]
pub struct FormInput {
    state: Entity<InputState>,
}

impl FormInput {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self { state: state.clone() }
    }
}

impl gpui::RenderOnce for FormInput {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        use gpui::Focusable as _;
        use gpui_component::Sizable as _;
        let t = cx.signet();
        let focused = self.state.focus_handle(cx).contains_focused(window, cx);
        div()
            .w_full()
            .min_w_0()
            .h(px(32.0))
            .rounded(radius::CONTROL)
            .bg(t.surface_2)
            .border_2()
            .border_color(if focused { t.accent } else { gpui::transparent_black() })
            .flex()
            .items_center()
            .child(gpui_component::input::Input::new(&self.state).appearance(false).bordered(false).focus_bordered(false).small().w_full().px(space::S3 - px(2.0)).text_color(t.text))
    }
}

// --------------------------------------------------------------------------- space form

pub struct SpaceForm {
    edit_id: Option<String>,
    name: Entity<InputState>,
    icon: String,
    hue: f64,
    gradient: bool,
    hues: Vec<f64>,
    grain: f32,
    grain_state: Entity<SliderState>,
    isolated: bool,
    picker: Option<Entity<EmojiPicker>>,
    busy: bool,
    _subs: Vec<Subscription>,
}

impl SpaceForm {
    pub fn new(space_id: Option<&str>, window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let snap = snapshot(cx);
        let existing: Option<Space> = space_id.and_then(|id| snap.spaces.iter().find(|s| s.id == id).cloned());
        cx.new(|cx: &mut Context<Self>| {
            let name = text_field_state(window, cx, tr("spaceForm.namePlaceholder"), existing.as_ref().map(|s| s.name.clone()).unwrap_or_default());
            let theme = existing.as_ref().and_then(|s| s.theme.clone());
            let grain = theme.as_ref().map(|t| t.grain as f32).unwrap_or(0.0);
            let grain_state = slider_state(cx, 0.0, 100.0, 5.0, grain * 100.0);
            let seed = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0) as usize) % HUES.len();
            let mut subs = vec![];
            subs.push(cx.subscribe_in(&name, window, |this: &mut Self, _, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.submit(window, cx);
                }
            }));
            subs.push(cx.subscribe(&grain_state, |this: &mut Self, _, ev: &SliderEvent, cx| {
                if let SliderEvent::Change(SliderValue::Single(v)) = ev {
                    this.grain = *v / 100.0;
                    cx.notify();
                }
            }));
            name.update(cx, |i, cx| i.focus(window, cx));
            Self {
                edit_id: existing.as_ref().map(|s| s.id.clone()),
                name,
                icon: existing.as_ref().map(|s| s.icon.clone()).unwrap_or_else(|| "✨".into()),
                hue: existing.as_ref().map(|s| s.hue).unwrap_or(HUES[seed]),
                gradient: theme.is_some(),
                hues: theme.map(|t| t.hues).unwrap_or_default(),
                grain,
                grain_state,
                isolated: existing.as_ref().is_some_and(|s| s.is_isolated()),
                picker: None,
                busy: false,
                _subs: subs,
            }
        })
    }

    /// Escape closes the emoji picker first. True when consumed.
    pub fn handle_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self.picker.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    fn valid(&self, cx: &gpui::App) -> bool {
        !self.name.read(cx).value().trim().is_empty() && (!self.gradient || self.hues.len() >= 2)
    }

    fn to_gradient(&mut self) {
        self.gradient = true;
        if self.hues.len() < 2 {
            let at = HUES.iter().position(|h| *h == self.hue).unwrap_or(0);
            self.hues = vec![self.hue, HUES[(at + 3) % HUES.len()]];
        }
    }

    /// Gradient: a click adds the hue (the last one is replaced beyond 3), a second click removes it.
    fn toggle_hue(&mut self, h: f64) {
        if self.hues.contains(&h) {
            self.hues.retain(|x| *x != h);
        } else if self.hues.len() >= 3 {
            self.hues.truncate(2);
            self.hues.push(h);
        } else {
            self.hues.push(h);
        }
    }

    fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.valid(cx) || self.busy {
            return;
        }
        self.busy = true;
        let name: String = self.name.read(cx).value().trim().chars().take(40).collect();
        let icon = if self.icon.is_empty() { "✨".to_string() } else { self.icon.clone() };
        let draft = if self.gradient {
            SpaceDraft { name, icon, hue: self.hues[0], theme: Some(SpaceTheme { hues: self.hues.clone(), grain: self.grain as f64 }) }
        } else {
            SpaceDraft { name, icon, hue: self.hue, theme: None }
        };
        let api = api(cx);
        let snap = snapshot(cx);
        match &self.edit_id {
            Some(id) if snap.spaces.iter().any(|s| &s.id == id) => {
                api.update_space(id, draft);
                let was = snap.spaces.iter().find(|s| &s.id == id).is_some_and(|s| s.is_isolated());
                if was != self.isolated {
                    api.set_space_profile(id, if self.isolated { SpaceProfile::Isolated } else { SpaceProfile::Shared });
                }
            }
            Some(_) => {}
            None => {
                api.create_space(draft);
                // The new space becomes the active space.
                if self.isolated {
                    api.set_space_profile(&api.get_state().active_space_id, SpaceProfile::Isolated);
                }
            }
        }
        close(cx);
    }
}

impl Render for SpaceForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let this = cx.entity();
        let valid = self.valid(cx);
        let accent = super::common::accent_for(cx, if self.gradient { self.hues.first().copied().unwrap_or(self.hue) } else { self.hue });
        let gradient = self.gradient;
        let editing = self.edit_id.is_some();

        let swatches = div().flex().gap(space::S1).children(HUES.iter().enumerate().map(|(i, h)| {
            let h = *h;
            let rank = self.hues.iter().position(|x| *x == h);
            let on = if gradient { rank.is_some() } else { h == self.hue };
            let this = this.clone();
            div()
                .id(("swatch", i))
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.0))
                .rounded_full()
                .bg(hsla(swatch_color(h as f32)))
                .cursor_pointer()
                .text_size(t.type_scale.cap)
                .text_color(gpui::white())
                .font_weight(weight::STRONG)
                .when(on, |d| d.border_2().border_color(t.surface).shadow(vec![gpui::BoxShadow { color: t.text_2, offset: gpui::point(px(0.0), px(0.0)), blur_radius: px(0.0), spread_radius: px(1.5), inset: false }]))
                .on_click(move |_, _, cx| {
                    this.update(cx, |f, cx| {
                        if f.gradient { f.toggle_hue(h) } else { f.hue = h }
                        cx.notify();
                    })
                })
                .children(if gradient { rank.map(|r| (r + 1).to_string()) } else { None })
        }));

        let me = this.clone();
        let me2 = this.clone();
        let me3 = this.clone();
        let me4 = this.clone();
        form_box(cx)
            .id("space-form")
            .child(
                div()
                    .flex()
                    .gap(space::S2)
                    .child(
                        div()
                            .id("space-emoji")
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(space::S8)
                            .rounded(radius::CONTROL)
                            .bg(t.surface_2)
                            .cursor_pointer()
                            .when(self.picker.is_some(), |d| d.border_2().border_color(accent))
                            .child(emoji(&self.icon, 20.0))
                            .on_click(move |_, window, cx| {
                                me.update(cx, |f, cx| {
                                    if f.picker.take().is_none() {
                                        let p = emoji_picker(window, cx);
                                        cx.subscribe(&p, |f: &mut Self, _, ev: &EmojiPicked, cx| {
                                            f.icon = ev.0.clone();
                                            f.picker = None;
                                            cx.notify();
                                        })
                                        .detach();
                                        f.picker = Some(p);
                                    }
                                    cx.notify();
                                })
                            }),
                    )
                    .child(div().flex_1().min_w_0().child(FormInput::new(&self.name))),
            )
            .when_some(self.picker.clone(), |d, p| d.child(div().h(px(280.0)).child(p)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(space::S3)
                    .child(
                        div().w(px(190.0)).child(
                            Segmented::new("space-bg")
                                .item(tr("spaceForm.solid"))
                                .item(tr("spaceForm.gradient"))
                                .selected(usize::from(gradient))
                                .on_change(move |ix, _, cx| {
                                    me2.update(cx, |f, cx| {
                                        if ix == 1 { f.to_gradient() } else { f.gradient = false }
                                        cx.notify();
                                    })
                                }),
                        ),
                    )
                    .child(theme_preview(if gradient { &self.hues } else { &[] }, self.hue)),
            )
            .child(swatches)
            .when(gradient, |d| {
                d.when(self.hues.len() < 2, |d| d.child(div().text_size(t.type_scale.cap).text_color(t.text_2).child(tr("spaceForm.gradientHint"))))
                    .child(crate::widgets::SliderField::new(&self.grain_state).label(tr("spaceForm.grain")))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S3)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().font_weight(weight::STRONG).child(tr("spaceForm.isolated")))
                            .child(div().text_size(t.type_scale.cap).text_color(t.text_2).child(tr("spaceForm.isolatedHint"))),
                    )
                    .child(Toggle::new("space-isolated", self.isolated).on_change(move |on, _, cx| {
                        me3.update(cx, |f, cx| {
                            f.isolated = on;
                            cx.notify();
                        })
                    })),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space::S2)
                    .child(form_btn("space-cancel", tr("common.cancel"), false, false, cx).on_click(|_, _, cx| close(cx)))
                    .child(form_btn("space-ok", if editing { tr("spaceForm.save") } else { tr("spaceForm.create") }, true, !valid || self.busy, cx).when(valid && !self.busy, |d| {
                        d.on_click(move |_, window, cx| me4.update(cx, |f, cx| f.submit(window, cx)))
                    })),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
}

// --------------------------------------------------------------------------- live folder form

pub struct LiveFolderForm {
    folder_id: Option<String>,
    github: bool,
    url: Entity<InputState>,
    query: GithubQuery,
    token: Option<bool>,
    token_input: Entity<InputState>,
    busy: bool,
    error: Option<String>,
    token_error: Option<String>,
    _subs: Vec<Subscription>,
}

fn find_live(items: &[Item], id: &str) -> Option<LiveSource> {
    find_folder(items, id).and_then(|f| f.live.as_ref().map(|l| l.source.clone()))
}

impl LiveFolderForm {
    pub fn new(folder_id: Option<&str>, window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let snap = snapshot(cx);
        let current = folder_id.and_then(|id| snap.spaces.iter().find_map(|s| find_live(&s.pinned, id)));
        let token = Some(api(cx).has_github_token());
        cx.new(|cx: &mut Context<Self>| {
            let url_val = match &current {
                Some(LiveSource::Feed { url }) => url.clone(),
                _ => String::new(),
            };
            let url = text_field_state(window, cx, tr("liveForm.urlPlaceholder"), url_val);
            let token_input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("settings.general.githubTokenPlaceholder")).masked(true));
            let subs = vec![cx.subscribe_in(&url, window, |this: &mut Self, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            })];
            url.update(cx, |i, cx| i.focus(window, cx));
            Self {
                folder_id: folder_id.map(str::to_string),
                github: matches!(current, Some(LiveSource::Github { .. })),
                url,
                query: match &current {
                    Some(LiveSource::Github { query }) => *query,
                    _ => GithubQuery::ReviewRequested,
                },
                token,
                token_input,
                busy: false,
                error: None,
                token_error: None,
                _subs: subs,
            }
        })
    }

    fn valid(&self, cx: &gpui::App) -> bool {
        if self.github { self.token == Some(true) } else { !self.url.read(cx).value().trim().is_empty() }
    }

    fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.valid(cx) || self.busy {
            return;
        }
        self.error = None;
        let source = if self.github { LiveSource::Github { query: self.query } } else { LiveSource::Feed { url: self.url.read(cx).value().trim().to_string() } };
        match api(cx).save_live_folder(self.folder_id.as_deref(), source) {
            Some(message) => {
                self.error = Some(message);
                cx.notify();
            }
            None => close(cx),
        }
    }

    fn save_token(&mut self, cx: &mut Context<Self>) {
        let value = self.token_input.read(cx).value().trim().to_string();
        if value.is_empty() {
            return;
        }
        self.token_error = api(cx).set_github_token(Some(&value));
        if self.token_error.is_none() {
            self.token = Some(true);
        }
        cx.notify();
    }
}

impl Render for LiveFolderForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let valid = self.valid(cx);
        let this = cx.entity();
        let (me1, me2, me3, me4, me5) = (this.clone(), this.clone(), this.clone(), this.clone(), this.clone());
        let hint = |text: String| div().text_size(t.type_scale.cap).text_color(t.text_2).child(text);
        let choice = |id: &'static str, label: String, on: bool, me: Entity<Self>, q: GithubQuery| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(space::S2)
                .h(px(28.0))
                .cursor_pointer()
                .child(div().size(px(14.0)).rounded_full().border_1().border_color(t.text_2).flex().items_center().justify_center().when(on, |d| d.child(div().size(px(8.0)).rounded_full().bg(t.accent))))
                .child(label)
                .on_click(move |_, _, cx| {
                    me.update(cx, |f, cx| {
                        f.query = q;
                        cx.notify();
                    })
                })
        };
        form_box(cx)
            .id("live-form")
            .child(div().font_weight(weight::STRONG).child(if self.folder_id.is_some() { tr("liveForm.titleEdit") } else { tr("liveForm.titleNew") }))
            .child(
                Segmented::new("live-kind")
                    .icon_item(IconName::Rss, tr("liveForm.feed"))
                    .icon_item(IconName::GitPullRequest, tr("liveForm.github"))
                    .selected(usize::from(self.github))
                    .on_change(move |ix, _, cx| {
                        me1.update(cx, |f, cx| {
                            f.github = ix == 1;
                            cx.notify();
                        })
                    }),
            )
            .when(!self.github, |d| d.child(FormInput::new(&self.url)).child(hint(tr("liveForm.feedHint"))))
            .when(self.github, |d| {
                d.child(choice("live-review", tr("liveForm.reviewRequested"), self.query == GithubQuery::ReviewRequested, me2.clone(), GithubQuery::ReviewRequested))
                    .child(choice("live-mine", tr("liveForm.mine"), self.query == GithubQuery::Mine, me2.clone(), GithubQuery::Mine))
                    .when(self.token == Some(false), |d| {
                        d.child(hint(tr("liveForm.noToken")))
                            .child(FormInput::new(&self.token_input))
                            .child(
                                div()
                                    .flex()
                                    .gap(space::S3)
                                    .text_size(t.type_scale.cap)
                                    .text_color(t.accent)
                                    .child(div().id("live-token-save").cursor_pointer().child(tr("settings.general.githubTokenSave")).on_click(move |_, _, cx| me3.update(cx, |f, cx| f.save_token(cx))))
                                    .child(div().id("live-settings").cursor_pointer().child(tr("liveForm.openSettings")).on_click(|_, _, cx| api(cx).open_settings(Some("general")))),
                            )
                            .children(self.token_error.clone().map(|e| div().text_size(t.type_scale.sub).text_color(t.danger).child(e)))
                    })
            })
            .children(self.error.clone().map(|e| div().text_size(t.type_scale.sub).text_color(t.danger).child(e)))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space::S2)
                    .child(form_btn("live-cancel", tr("common.cancel"), false, false, cx).on_click(|_, _, cx| close(cx)))
                    .child(
                        form_btn("live-ok", if self.busy { tr("liveForm.checking") } else if self.folder_id.is_some() { tr("liveForm.save") } else { tr("liveForm.create") }, true, !valid || self.busy, cx)
                            .when(valid && !self.busy, |d| d.on_click(move |_, window, cx| me4.update(cx, |f, cx| f.submit(window, cx)))),
                    ),
            )
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                let _ = &me5;
                cx.stop_propagation()
            })
    }
}

// --------------------------------------------------------------------------- icon form

pub struct IconForm {
    target: IconTarget,
    picker: Entity<EmojiPicker>,
}

impl EventEmitter<()> for IconForm {}

impl IconForm {
    pub fn new(target: IconTarget, window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let picker = emoji_picker(window, cx);
        cx.new(|cx: &mut Context<Self>| {
            cx.subscribe(&picker, |this: &mut Self, _, ev: &EmojiPicked, cx| {
                api(cx).set_icon(this.target.clone(), Some(&ev.0));
                close(cx);
            })
            .detach();
            Self { target, picker }
        })
    }

    /// Preview, name and current emoji of the target.
    fn describe(&self, cx: &gpui::App) -> Option<(gpui::AnyElement, String, Option<String>)> {
        let s = snapshot(cx);
        let fallback = tr("sidebar.newTab");
        match &self.target {
            IconTarget::Tab { tab_id } => {
                let tab = s.tabs.get(tab_id)?;
                let preview = match &tab.custom_icon {
                    Some(e) => emoji(e, 18.0).into_any_element(),
                    None => favicon(tab.favicon.as_deref(), 18.0).into_any_element(),
                };
                Some((preview, display_title(tab, &fallback), tab.custom_icon.clone()))
            }
            IconTarget::Folder { folder_id } => s.spaces.iter().find_map(|sp| find_folder(&sp.pinned, folder_id)).map(|f| {
                let preview = match &f.icon {
                    Some(e) => emoji(e, 18.0).into_any_element(),
                    None => Icon::from(if f.open { IconName::FolderOpen } else { IconName::Folder }).size(px(18.0)).into_any_element(),
                };
                (preview, f.name.clone(), f.icon.clone())
            }),
            IconTarget::Group { group_id } => s.spaces.iter().find_map(|sp| sp.groups.as_ref().and_then(|g| g.get(group_id))).map(|g| {
                let preview = match &g.icon {
                    Some(e) => emoji(e, 18.0).into_any_element(),
                    None => Icon::from(IconName::Folder).size(px(18.0)).into_any_element(),
                };
                (preview, g.name.clone(), g.icon.clone())
            }),
        }
    }
}

impl Render for IconForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((preview, name, current)) = self.describe(cx) else { return div().into_any_element() };
        let target = self.target.clone();
        form_box(cx)
            .id("icon-form")
            .child(div().flex().items_center().gap(space::S2).min_w_0().child(preview).child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().font_weight(weight::STRONG).child(name)))
            .child(div().h(px(280.0)).child(self.picker.clone()))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space::S2)
                    .child(form_btn("icon-cancel", tr("common.cancel"), false, false, cx).on_click(|_, _, cx| close(cx)))
                    .child(form_btn("icon-remove", tr("tabIcon.remove"), false, current.is_none(), cx).when(current.is_some(), |d| {
                        d.on_click(move |_, _, cx| {
                            api(cx).set_icon(target.clone(), None);
                            close(cx);
                        })
                    })),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .into_any_element()
    }
}

// --------------------------------------------------------------------------- rename form

pub struct RenameForm {
    target: RenameTarget,
    input: Entity<InputState>,
    initial: String,
    page_title: Option<String>,
    _sub: Subscription,
}

impl RenameForm {
    pub fn new(target: RenameTarget, window: &mut Window, cx: &mut gpui::App) -> Entity<Self> {
        let s = snapshot(cx);
        let sp = active_space(&s);
        let tab = match &target {
            RenameTarget::Tab { tab_id } => s.tabs.get(tab_id).cloned(),
            _ => None,
        };
        let initial = match &target {
            RenameTarget::Tab { .. } => tab.as_ref().map(|t| t.custom_title.clone().unwrap_or_else(|| t.title.clone())).unwrap_or_default(),
            RenameTarget::Folder { folder_id } => sp.and_then(|sp| find_folder(&sp.pinned, folder_id)).map(|f| f.name.clone()).unwrap_or_default(),
            RenameTarget::Group { group_id } => sp.and_then(|sp| sp.groups.as_ref()?.get(group_id)).map(|g| g.name.clone()).unwrap_or_default(),
            RenameTarget::Bookmark { id } => sp
                .and_then(|sp| find_bookmark(sp.bookmarks.as_deref().unwrap_or(&[]), id))
                .map(|b| match b {
                    signet_core::types::Bookmark::Link { title, .. } => title.clone(),
                    signet_core::types::Bookmark::Folder { name, .. } => name.clone(),
                })
                .unwrap_or_default(),
        };
        cx.new(|cx: &mut Context<Self>| {
            let input = text_field_state(window, cx, tab.as_ref().map(|t| t.title.clone()).unwrap_or_default(), initial.clone());
            let sub = cx.subscribe_in(&input, window, |this: &mut Self, _, ev: &InputEvent, _window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    this.commit(cx);
                }
            });
            input.update(cx, |i, cx| {
                i.focus(window, cx);
                i.select_all(window, cx);
            });
            Self { target, input, initial, page_title: tab.map(|t| t.title), _sub: sub }
        })
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        let name: String = self.input.read(cx).value().trim().chars().take(200).collect();
        let api = api(cx);
        match &self.target {
            // Empty or identical to the page title: back to the original title.
            RenameTarget::Tab { tab_id } => api.rename_tab(tab_id, if !name.is_empty() && Some(&name) != self.page_title.as_ref() { Some(&name) } else { None }),
            RenameTarget::Bookmark { id } => {
                if !name.is_empty() && name != self.initial {
                    api.rename_bookmark(id, &name)
                }
            }
            RenameTarget::Group { group_id } => {
                if !name.is_empty() && name != self.initial {
                    api.rename_group(group_id, &name)
                }
            }
            RenameTarget::Folder { folder_id } => {
                if !name.is_empty() && name != self.initial {
                    api.rename_folder(folder_id, &name)
                }
            }
        }
        close(cx);
    }
}

impl Render for RenameForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity();
        form_box(cx)
            .id("rename-form")
            .child(FormInput::new(&self.input))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space::S2)
                    .child(form_btn("rename-cancel", tr("common.cancel"), false, false, cx).on_click(|_, _, cx| close(cx)))
                    .child(form_btn("rename-ok", tr("tabstrip.rename"), true, false, cx).on_click(move |_, _, cx| me.update(cx, |f, cx| f.commit(cx)))),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }
}

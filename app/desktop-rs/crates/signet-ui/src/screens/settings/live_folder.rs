//! Live folders: GitHub token in Settings > General (`LiveFolderSettings.tsx`) and the create / edit form
//! (`LiveFolderForm.tsx`).

use gpui::{
    Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use gpui_component::input::InputState;
use signet_core::types::{GithubQuery, Item, LiveSource, Snapshot};

use super::kit::{self, Btn};
use crate::app_state::{api, snapshot, tr};
use crate::theme::{ThemeExt, space};
use crate::widgets::Segmented;
use crate::icon::IconName;

/// GitHub page to create a classic token, with the "repo" access already checked.
const TOKEN_URL: &str = "https://github.com/settings/tokens/new?scopes=repo&description=Signet";

/// Settings > General: GitHub token of live folders (saved encrypted by the app, never read back here).
pub struct LiveFolderSettings {
    saved: bool,
    value: Entity<InputState>,
    busy: bool,
    error: Option<String>,
}

impl LiveFolderSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        kit::watch_locale(cx);
        let saved = api(cx).has_github_token();
        let value = kit::input(window, cx, tr("settings.general.githubTokenPlaceholder"), "", true, Some(Self::submit));
        Self { saved, value, busy: false, error: None }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = kit::text_of(&self.value, cx).trim().to_string();
        if !token.is_empty() && !self.busy {
            self.save(Some(token), window, cx);
        }
    }

    fn save(&mut self, token: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        let message = api(cx).set_github_token(token.as_deref());
        self.busy = false;
        if message.is_some() {
            self.error = message;
        } else {
            kit::set_text(&self.value, "", window, cx);
            self.saved = token.is_some();
        }
        cx.notify();
    }
}

impl Render for LiveFolderSettings {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_text = !kit::text_of(&self.value, cx).trim().is_empty();
        let busy = self.busy;
        let controls = if self.saved {
            div()
                .flex()
                .items_center()
                .gap(space::S2)
                .child(kit::ok(cx, tr("settings.general.githubTokenSet")))
                .child(div().flex_1())
                .child(
                    Btn::new("gh-remove", tr("settings.general.githubTokenRemove"))
                        .danger()
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, window, cx| this.save(None, window, cx))),
                )
        } else {
            div()
                .flex()
                .items_center()
                .gap(space::S2)
                .child(kit::field(&self.value))
                .child(
                    Btn::new("gh-save", tr("settings.general.githubTokenSave"))
                        .disabled(busy || !has_text)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                )
                .child(kit::link(cx, "gh-create", tr("settings.general.githubTokenCreate"), |_, _, cx| cx.open_url(TOKEN_URL)))
        };
        div()
            .flex()
            .flex_col()
            .gap(space::S2)
            .w_full()
            .child(kit::sub(cx, tr("settings.general.liveFolders")))
            .child(kit::group(
                cx,
                vec![
                    kit::row_stack(cx)
                        .child(kit::label(cx, tr("settings.general.githubToken"), Some(tr("settings.general.githubTokenHint").into())))
                        .child(controls)
                        .when_some(self.error.clone(), |d, e| d.child(kit::err(cx, e)))
                        .into_any_element(),
                ],
            ))
    }
}

// --------------------------------------------------------------------------- form

/// Emitted when the form is done (saved or cancelled): the host closes it.
pub struct LiveFolderFormClosed;

/// Source of a live folder, searched in all spaces' pinned items.
fn find_live(s: &Snapshot, folder_id: Option<&str>) -> Option<LiveSource> {
    fn walk(items: &[Item], id: &str) -> Option<LiveSource> {
        for item in items {
            if let Item::Folder(f) = item {
                if f.id == id {
                    return f.live.as_ref().map(|l| l.source.clone());
                }
                if let Some(inner) = walk(&f.items, id) {
                    return Some(inner);
                }
            }
        }
        None
    }
    let id = folder_id?;
    s.spaces.iter().find_map(|space| walk(&space.pinned, id))
}

/// Live folder form: a feed (or a site that announces one), or a GitHub query.
pub struct LiveFolderForm {
    folder_id: Option<String>,
    github: bool,
    query: GithubQuery,
    url: Entity<InputState>,
    token: bool,
    token_value: Entity<InputState>,
    token_busy: bool,
    token_error: Option<String>,
    busy: bool,
    error: Option<String>,
}

impl EventEmitter<LiveFolderFormClosed> for LiveFolderForm {}

impl LiveFolderForm {
    /// `folder_id`: folder whose source is changed, or None to create one.
    pub fn new(folder_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        kit::watch_locale(cx);
        let current = find_live(&snapshot(cx), folder_id.as_deref());
        let (github, query, url) = match &current {
            Some(LiveSource::Github { query }) => (true, *query, String::new()),
            Some(LiveSource::Feed { url }) => (false, GithubQuery::ReviewRequested, url.clone()),
            None => (false, GithubQuery::ReviewRequested, String::new()),
        };
        let url = kit::input(window, cx, tr("liveForm.urlPlaceholder"), url, false, Some(Self::submit));
        let token_value = kit::input(window, cx, tr("settings.general.githubTokenPlaceholder"), "", true, Some(Self::save_token));
        let token = api(cx).has_github_token();
        if !github {
            url.update(cx, |s, cx| s.focus(window, cx));
        }
        Self { folder_id, github, query, url, token, token_value, token_busy: false, token_error: None, busy: false, error: None }
    }

    fn valid(&self, cx: &gpui::App) -> bool {
        if self.github { self.token } else { !kit::text_of(&self.url, cx).trim().is_empty() }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(LiveFolderFormClosed);
    }

    // Token entered here: saved (and verified) by the app, as in Settings > General.
    fn save_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = kit::text_of(&self.token_value, cx).trim().to_string();
        if value.is_empty() || self.token_busy {
            return;
        }
        self.token_error = None;
        match api(cx).set_github_token(Some(&value)) {
            Some(message) => self.token_error = Some(message),
            None => {
                kit::set_text(&self.token_value, "", window, cx);
                self.token = true;
            }
        }
        cx.notify();
    }

    fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.valid(cx) || self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let source = if self.github {
            LiveSource::Github { query: self.query }
        } else {
            LiveSource::Feed { url: kit::text_of(&self.url, cx).trim().to_string() }
        };
        let message = api(cx).save_live_folder(self.folder_id.as_deref(), source);
        self.busy = false;
        match message {
            Some(m) => self.error = Some(m),
            None => self.close(cx),
        }
        cx.notify();
    }
}

impl Render for LiveFolderForm {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.signet().clone();
        let editing = self.folder_id.is_some();
        let valid = self.valid(cx);
        let github = self.github;
        let query = self.query;
        let token_text = !kit::text_of(&self.token_value, cx).trim().is_empty();
        let hint = |text: String| div().text_size(t.type_scale.sub).text_color(t.text_2).child(text);
        let choice = |id: &'static str, q: GithubQuery, text: String, cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(space::S2)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.query = q;
                    cx.notify();
                }))
                .child(kit::radio_dot(cx, query == q))
                .child(text)
        };
        kit::form_surface(cx)
            .id("live-form")
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    this.close(cx);
                }
            }))
            .child(div().font_weight(crate::theme::weight::STRONG).child(tr(if editing { "liveForm.titleEdit" } else { "liveForm.titleNew" })))
            .child(
                Segmented::new("live-kind")
                    .icon_item(IconName::Rss, tr("liveForm.feed"))
                    .icon_item(IconName::GitPullRequest, tr("liveForm.github"))
                    .selected(if github { 1 } else { 0 })
                    .on_change({
                        let this = cx.entity();
                        move |ix, _, cx| {
                            this.update(cx, |this, cx| {
                                this.github = ix == 1;
                                cx.notify();
                            })
                        }
                    }),
            )
            .child(if !github {
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(kit::field(&self.url))
                    .child(hint(tr("liveForm.feedHint")))
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(choice("q-review", GithubQuery::ReviewRequested, tr("liveForm.reviewRequested"), cx))
                    .child(choice("q-mine", GithubQuery::Mine, tr("liveForm.mine"), cx))
                    .when(!self.token, |d| {
                        d.child(hint(tr("liveForm.noToken")))
                            .child(kit::field(&self.token_value))
                            .child(
                                div()
                                    .flex()
                                    .gap(space::S3)
                                    .child(
                                        Btn::new("lf-token-save", tr("settings.general.githubTokenSave"))
                                            .disabled(self.token_busy || !token_text)
                                            .on_click(cx.listener(|this, _, window, cx| this.save_token(window, cx))),
                                    )
                                    .child(kit::link(cx, "lf-open-settings", tr("liveForm.openSettings"), |_, _, cx| api(cx).open_settings(Some("general")))),
                            )
                            .when_some(self.token_error.clone(), |d, e| d.child(kit::err(cx, e)))
                    })
                    .into_any_element()
            })
            .when_some(self.error.clone(), |d, e| d.child(kit::err(cx, e)))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space::S2)
                    .child(Btn::new("lf-cancel", tr("common.cancel")).on_click(cx.listener(|this, _, _, cx| this.close(cx))))
                    .child(
                        Btn::new(
                            "lf-submit",
                            if self.busy {
                                tr("liveForm.checking")
                            } else if editing {
                                tr("liveForm.save")
                            } else {
                                tr("liveForm.create")
                            },
                        )
                        .accent()
                        .disabled(!valid || self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}

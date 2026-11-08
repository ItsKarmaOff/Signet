//! Popovers of the tab row (port of `overlay/Popover.tsx`): menus and forms placed above the web views, under the chip
//! that opens them.

use super::common::{active_space, display_title, emoji, favicon, find_bookmark, find_folder, lucide, viewport};
use super::downloads::{count as download_count, downloads_panel};
use super::forms::{IconForm, LiveFolderForm, RenameForm, SpaceForm};
use crate::app_state::{api, snapshot, tr, ui_state};
use crate::theme::{ThemeExt, radius, space};
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Task,
    Window, div, prelude::FluentBuilder, px,
};
use signet_core::types::{Bookmark, Item, PopoverRequest, Rect, Snapshot};
use std::time::Duration;

const SUB_CLOSE: Duration = Duration::from_millis(250);
const ROW_H: f32 = 32.0;
const ROW_STEP: f32 = ROW_H + 2.0;
const SUB_W: f32 = 288.0;

/// A line of a folder or bookmark menu.
#[derive(Clone)]
enum Row {
    Folder { id: String, name: String, hue: Option<f64>, icon: FolderIcon, items: Vec<Row>, bookmark: bool },
    Tab { id: String, title: String, emoji: Option<String>, favicon: Option<String>, active: bool, unloaded: bool },
    Link { id: String, title: String, favicon: Option<String> },
}

#[derive(Clone)]
enum FolderIcon {
    Plain,
    Emoji(String),
    Live { github: bool },
}

fn tab_rows(items: &[Item], s: &Snapshot, active: Option<&str>) -> Vec<Row> {
    let mut out = vec![];
    for item in items {
        let ids: Vec<&String> = match item {
            Item::Folder(f) => {
                out.push(Row::Folder {
                    id: f.id.clone(),
                    name: f.name.clone(),
                    hue: f.hue,
                    icon: match (&f.live, &f.icon) {
                        (_, Some(e)) => FolderIcon::Emoji(e.clone()),
                        (Some(l), None) => FolderIcon::Live { github: matches!(l.source, signet_core::types::LiveSource::Github { .. }) },
                        _ => FolderIcon::Plain,
                    },
                    items: tab_rows(&f.items, s, active),
                    bookmark: false,
                });
                continue;
            }
            Item::Tab(t) => vec![&t.tab_id],
            Item::Split(sp) => sp.tab_ids.iter().collect(),
        };
        for id in ids {
            let Some(tab) = s.tabs.get(id) else { continue };
            out.push(Row::Tab {
                id: id.clone(),
                title: display_title(tab, &tr("sidebar.newTab")),
                emoji: tab.custom_icon.clone(),
                favicon: tab.favicon.clone(),
                active: Some(id.as_str()) == active,
                unloaded: !tab.loaded,
            });
        }
    }
    out
}

fn bookmark_rows(items: &[Bookmark]) -> Vec<Row> {
    items
        .iter()
        .map(|b| match b {
            Bookmark::Folder { id, name, items } => Row::Folder { id: id.clone(), name: name.clone(), hue: None, icon: FolderIcon::Plain, items: bookmark_rows(items), bookmark: true },
            Bookmark::Link { id, title, favicon, .. } => Row::Link { id: id.clone(), title: title.clone(), favicon: favicon.clone() },
        })
        .collect()
}

enum Body {
    Spaces,
    Folder(String),
    Bookmarks { folder_id: Option<String>, from: Option<usize>, tab_id: Option<String> },
    Downloads,
    SpaceForm(Entity<SpaceForm>),
    Live(Entity<LiveFolderForm>),
    Icon(Entity<IconForm>),
    Rename(Entity<RenameForm>),
}

pub struct PopoverView {
    anchor: Rect,
    body: Body,
    width: f32,
    /// Open submenu folders, one id per depth.
    open: Vec<String>,
    close_task: Option<Task<()>>,
    _obs: Subscription,
}

fn width_of(p: &PopoverRequest) -> f32 {
    match p {
        PopoverRequest::Spaces => 248.0,
        PopoverRequest::Folder { .. } => 288.0,
        PopoverRequest::SpaceForm { .. } => 304.0,
        PopoverRequest::LiveFolder { .. } => 320.0,
        PopoverRequest::Downloads => 344.0,
        PopoverRequest::Icon { .. } => 304.0,
        PopoverRequest::Rename { .. } => 264.0,
        PopoverRequest::Bookmarks { .. } => 288.0,
    }
}

impl PopoverView {
    pub fn new(popover: PopoverRequest, anchor: Rect, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let width = width_of(&popover);
        let body = match popover {
            PopoverRequest::Spaces => Body::Spaces,
            PopoverRequest::Folder { folder_id } => Body::Folder(folder_id),
            PopoverRequest::SpaceForm { space_id } => Body::SpaceForm(SpaceForm::new(space_id.as_deref(), window, cx)),
            PopoverRequest::LiveFolder { folder_id } => Body::Live(LiveFolderForm::new(folder_id.as_deref(), window, cx)),
            PopoverRequest::Downloads => Body::Downloads,
            PopoverRequest::Icon { target } => Body::Icon(IconForm::new(target, window, cx)),
            PopoverRequest::Rename { target } => Body::Rename(RenameForm::new(target, window, cx)),
            PopoverRequest::Bookmarks { folder_id, from, tab_id } => Body::Bookmarks { folder_id, from, tab_id },
        };
        let state = ui_state(cx);
        cx.new(|cx: &mut Context<Self>| Self { anchor, body, width, open: vec![], close_task: None, _obs: cx.observe(&state, |_, _, cx| cx.notify()) })
    }

    /// Escape: closes the emoji picker of the space form first. True when consumed.
    pub fn handle_escape(&mut self, cx: &mut Context<Self>) -> bool {
        match &self.body {
            Body::SpaceForm(f) => f.clone().update(cx, |f, cx| f.handle_escape(cx)),
            _ => false,
        }
    }

    fn rows(&self, s: &Snapshot) -> Vec<Row> {
        let Some(space) = active_space(s) else { return vec![] };
        match &self.body {
            Body::Folder(id) => {
                let active = space.active_tab_id.as_deref();
                find_folder(&space.pinned, id).map(|f| tab_rows(&f.items, s, active)).unwrap_or_default()
            }
            Body::Bookmarks { folder_id, from, .. } => {
                let all = space.bookmarks.as_deref().unwrap_or(&[]);
                match folder_id {
                    Some(id) => match find_bookmark(all, id) {
                        Some(Bookmark::Folder { items, .. }) => bookmark_rows(items),
                        _ => vec![],
                    },
                    None => bookmark_rows(&all[from.unwrap_or(0).min(all.len())..]),
                }
            }
            _ => vec![],
        }
    }

    fn bookmarks_tab(&self) -> Option<String> {
        match &self.body {
            Body::Bookmarks { tab_id, .. } => tab_id.clone(),
            _ => None,
        }
    }

    fn est_height(&self, s: &Snapshot, cx: &App) -> f32 {
        let menu = |n: usize| (n.max(1) as f32 * ROW_STEP + 10.0).min(440.0);
        match &self.body {
            Body::Spaces => menu(s.spaces.len() + 2) + 6.0,
            Body::Folder(_) | Body::Bookmarks { .. } => menu(self.rows(s).len()),
            Body::Downloads => (80.0 + download_count(cx).max(1) as f32 * 45.0).min(380.0),
            Body::SpaceForm(_) => 330.0,
            Body::Live(_) => 260.0,
            Body::Icon(_) => 400.0,
            Body::Rename(_) => 100.0,
        }
    }

    fn set_open(&mut self, depth: usize, id: Option<String>, cx: &mut Context<Self>) {
        self.close_task = None;
        self.open.truncate(depth);
        if let Some(id) = id {
            self.open.push(id);
        }
        cx.notify();
    }

    /// Closing is deferred and cancelled by entering the submenu: leaves time to reach it diagonally.
    fn close_soon(&mut self, depth: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.len() <= depth {
            return;
        }
        let ex = cx.background_executor().clone();
        self.close_task = Some(cx.spawn_in(window, async move |this, cx| {
            ex.timer(SUB_CLOSE).await;
            this.update(cx, |this, cx| {
                this.open.truncate(depth);
                cx.notify();
            })
            .ok();
        }));
    }

    fn item(&self, id: impl Into<SharedString>, cx: &App) -> gpui::Stateful<gpui::Div> {
        let t = cx.signet();
        let hover = t.surface_2;
        div()
            .id(id.into())
            .flex()
            .flex_none()
            .items_center()
            .gap(space::S2)
            .w_full()
            .h(px(ROW_H))
            .px(space::S2)
            .rounded(radius::ROW_IN)
            .text_color(t.text)
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
    }

    fn menu_panel(&self, cx: &App) -> gpui::Div {
        let t = cx.signet();
        div()
            .occlude()
            .flex()
            .flex_col()
            .gap(space::HALF)
            .p(px(6.0))
            .rounded(radius::POP)
            .bg(t.surface)
            .border_1()
            .border_color(t.border)
            .shadow(t.shadow_pop.clone())
            .text_color(t.text)
            .text_size(t.type_scale.base)
    }

    /// Items of one menu level; the submenus are collected into `subs` (positioned in the layer, not clipped).
    fn level(&self, rows: &[Row], depth: usize, left: f32, top: f32, viewport: (f32, f32), cx: &mut Context<Self>, subs: &mut Vec<AnyElement>) -> Vec<AnyElement> {
        let t = cx.signet().clone();
        let tab_id = self.bookmarks_tab();
        if rows.is_empty() {
            return vec![div().p(space::S2).text_size(t.type_scale.sub).text_color(t.text_3).child(tr("sidebar.emptyFolder")).into_any_element()];
        }
        let mut out = vec![];
        for (i, row) in rows.iter().enumerate() {
            match row {
                Row::Folder { id, name, hue, icon, items, bookmark } => {
                    let open = self.open.get(depth).is_some_and(|o| o == id);
                    let (hover_id, click_id) = (id.clone(), id.clone());
                    let tint = hue.map(|h| crate::color::hsla(crate::color::tint_color(h as f32, t.scheme)));
                    let tint = hue.map(|h| super::common::accent_for(cx, h)).or(tint);
                    let ico: AnyElement = match icon {
                        FolderIcon::Emoji(e) => emoji(e, 16.0).into_any_element(),
                        FolderIcon::Live { github } => lucide(if *github { "git-pull-request" } else { "rss" }).size(px(16.0)).into_any_element(),
                        FolderIcon::Plain => lucide(if open { "folder-open" } else { "folder" }).size(px(16.0)).when_some(tint, |i, c| i.text_color(c)).into_any_element(),
                    };
                    let menu_id = id.clone();
                    let el = self
                        .item(SharedString::from(format!("pop-f-{depth}-{id}")), cx)
                        .when(open, |d| d.bg(t.surface_2))
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if this.open.get(depth).map(String::as_str) != Some(hover_id.as_str()) {
                                this.set_open(depth, Some(hover_id.clone()), cx);
                            } else {
                                this.close_task = None;
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let was = this.open.get(depth).map(String::as_str) == Some(click_id.as_str());
                            this.set_open(depth, if was { None } else { Some(click_id.clone()) }, cx);
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener({
                                let tab_id = tab_id.clone();
                                let bookmark = *bookmark;
                                move |_, _, _, cx| {
                                    if bookmark {
                                        api(cx).show_bookmark_menu(&menu_id, tab_id.as_deref())
                                    } else {
                                        api(cx).show_folder_menu(&menu_id)
                                    }
                                }
                            }),
                        )
                        .child(ico)
                        .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name.clone()))
                        .child(lucide("chevron-right").size(px(14.0)).text_color(t.text_3));
                    out.push(el.into_any_element());
                    if open {
                        // Submenu to the right of the row, or to the left if it would leave the window.
                        let sub_left = if left + self.level_w(depth) + 2.0 + SUB_W + 8.0 > viewport.0 { (left - 2.0 - SUB_W).max(8.0) } else { left + self.level_w(depth) + 2.0 };
                        let est = (items.len().max(1) as f32 * ROW_STEP + 10.0).min(440.0);
                        let sub_top = (top + i as f32 * ROW_STEP).min(viewport.1 - est - 8.0).max(8.0);
                        let inner = self.level(items, depth + 1, sub_left, sub_top, viewport, cx, subs);
                        subs.push(
                            self.menu_panel(cx)
                                .id(SharedString::from(format!("pop-sub-{depth}")))
                                .absolute()
                                .left(px(sub_left))
                                .top(px(sub_top))
                                .w(px(SUB_W))
                                .max_h(px(440.0))
                                .on_hover(cx.listener(|this, over: &bool, _, _| {
                                    if *over {
                                        this.close_task = None;
                                    }
                                }))
                                .children(inner)
                                .into_any_element(),
                        );
                    }
                }
                Row::Tab { id, title, emoji: e, favicon: f, active, unloaded } => {
                    let (pick, menu, close_id) = (id.clone(), id.clone(), id.clone());
                    out.push(
                        self.item(SharedString::from(format!("pop-t-{depth}-{id}")), cx)
                            .when(*active, |d| d.bg(t.sel))
                            .on_mouse_move(cx.listener(move |this, _, window, cx| this.close_soon(depth, window, cx)))
                            .on_click(move |_, _, cx| {
                                api(cx).activate_tab(&pick);
                                api(cx).close_overlay(false);
                            })
                            .on_mouse_down(MouseButton::Right, move |_, _, cx| api(cx).show_tab_menu(&menu))
                            .on_mouse_up(MouseButton::Middle, move |_, _, cx| api(cx).close_tab(&close_id))
                            .child(match e {
                                Some(e) => emoji(e, 16.0).into_any_element(),
                                None => div().when(*unloaded, |d| d.opacity(0.7)).child(favicon(f.as_deref(), 16.0)).into_any_element(),
                            })
                            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(title.clone()))
                            .into_any_element(),
                    );
                }
                Row::Link { id, title, favicon: f, .. } => {
                    let (open_id, menu_id, mid_id) = (id.clone(), id.clone(), id.clone());
                    let (t1, t2, t3) = (tab_id.clone(), tab_id.clone(), tab_id.clone());
                    out.push(
                        self.item(SharedString::from(format!("pop-b-{depth}-{id}")), cx)
                            .on_mouse_move(cx.listener(move |this, _, window, cx| this.close_soon(depth, window, cx)))
                            .on_click(move |e: &ClickEvent, _, cx| {
                                let m = e.modifiers();
                                api(cx).open_bookmark(&open_id, t1.as_deref(), m.platform || m.control);
                                api(cx).close_overlay(false);
                            })
                            .on_mouse_down(MouseButton::Right, move |_, _, cx| api(cx).show_bookmark_menu(&menu_id, t2.as_deref()))
                            .on_mouse_up(MouseButton::Middle, move |_, _, cx| api(cx).open_bookmark(&mid_id, t3.as_deref(), true))
                            .child(favicon(f.as_deref(), 16.0))
                            .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(title.clone()))
                            .into_any_element(),
                    );
                }
            }
        }
        out
    }

    fn level_w(&self, depth: usize) -> f32 {
        if depth == 0 { self.width } else { SUB_W }
    }

    fn spaces_menu(&self, s: &Snapshot, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.signet().clone();
        let anchor = self.anchor;
        let active = s.active_space_id.clone();
        let mut rows: Vec<AnyElement> = s
            .spaces
            .iter()
            .map(|sp| {
                let (pick, menu) = (sp.id.clone(), sp.id.clone());
                self.item(SharedString::from(format!("pop-s-{}", sp.id)), cx)
                    .on_click(move |_, _, cx| {
                        api(cx).switch_space(&pick);
                        api(cx).close_overlay(false);
                    })
                    .on_mouse_down(MouseButton::Right, move |_, _, cx| api(cx).show_space_menu(&menu))
                    .child(emoji(&sp.icon, 18.0))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(sp.name.clone()))
                    .when(sp.id == active, |d| d.child(lucide("check").size(px(14.0)).text_color(t.accent)))
                    .into_any_element()
            })
            .collect();
        rows.push(div().flex_none().h(px(1.0)).mx(space::S2).my(space::S1).bg(t.border).into_any_element());
        let edit_id = active.clone();
        rows.push(
            self.item("pop-new-space", cx)
                .on_click(move |_, _, cx| api(cx).open_popover(PopoverRequest::SpaceForm { space_id: None }, anchor))
                .child(lucide("plus").size(px(15.0)))
                .child(div().flex_1().child(tr("tabstrip.newSpace")))
                .into_any_element(),
        );
        rows.push(
            self.item("pop-edit-space", cx)
                .on_click(move |_, _, cx| api(cx).open_popover(PopoverRequest::SpaceForm { space_id: Some(edit_id.clone()) }, anchor))
                .child(lucide("pencil").size(px(14.0)))
                .child(div().flex_1().child(tr("tabstrip.editSpace")))
                .into_any_element(),
        );
        self.menu_panel(cx).children(rows).into_any_element()
    }
}

impl Render for PopoverView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (w, h) = viewport(window);
        let s = snapshot(cx);
        let width = self.width;
        let a = self.anchor;
        let left = (a.x as f32).clamp(8.0, (w - width - 8.0).max(8.0));
        let below = (a.y + a.height) as f32 + 6.0;
        let est = self.est_height(&s, cx);
        let top = if below + est > h - 8.0 {
            let above = a.y as f32 - 6.0 - est;
            if above >= 8.0 { above } else { (h - 8.0 - est).max(8.0) }
        } else {
            below
        };
        let mut subs: Vec<AnyElement> = vec![];
        let body: AnyElement = match &self.body {
            Body::Spaces => self.spaces_menu(&s, cx),
            Body::Folder(_) | Body::Bookmarks { .. } => {
                let rows = self.rows(&s);
                if matches!(self.body, Body::Folder(_)) && rows.is_empty() && find_folder(&active_space(&s).map(|x| x.pinned.clone()).unwrap_or_default(), match &self.body { Body::Folder(i) => i, _ => "" }).is_none() {
                    div().into_any_element()
                } else {
                    let items = self.level(&rows, 0, left, top, (w, h), cx, &mut subs);
                    self.menu_panel(cx).max_h(px((h - 80.0).min(440.0))).children(items).into_any_element()
                }
            }
            Body::Downloads => downloads_panel(cx),
            Body::SpaceForm(f) => f.clone().into_any_element(),
            Body::Live(f) => f.clone().into_any_element(),
            Body::Icon(f) => f.clone().into_any_element(),
            Body::Rename(f) => f.clone().into_any_element(),
        };
        div()
            .absolute()
            .size_full()
            .on_mouse_down(MouseButton::Left, |_, _, cx| api(cx).close_overlay(false))
            .on_mouse_down(MouseButton::Right, |_, _, cx| api(cx).close_overlay(false))
            .child(div().absolute().left(px(left)).top(px(top)).w(px(width)).child(body))
            .children(subs)
    }
}

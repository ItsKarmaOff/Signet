//! The three lists of the sidebar (favorites, pinned, today) with folders, tab groups and the drag gap.
//! Port of `renderItems`, `FavoritesList`, `PinnedList` and `TodayList` of `Sidebar.tsx`.

use std::cell::RefCell;

use gpui::{AnimationExt as _, AnyElement, App, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px};
use signet_core::types::{FolderItem, Item, Snapshot, Space, TabGroup, Zone};

use super::dnd::{Axis, Hit, Region};
use crate::app_state::tr;
use crate::screens::tabs::custom::tint;
use crate::screens::tabs::motion::Unfold;
use crate::screens::tabs::groups::{Drop, Indicator, Run, runs_of, today_mark};
use crate::screens::tabs::row::{Header, RowEnv, favorite_tile, header_row, item_row, probe};
use crate::screens::tabs::{display_title, item_key, item_tab_ids, leaf_of};
use crate::theme::{SignetTheme, radius, space};

/// Maximum folder indentation depth.
const MAX_DEPTH: usize = 6;

pub struct ListCtx<'a> {
    pub snap: &'a Snapshot,
    pub space: &'a Space,
    pub env: &'a RowEnv,
    /// The drop target shown (None once settled or when nothing is dragged).
    pub drop: Option<&'a Drop>,
    pub gapped: bool,
    pub gap_h: f32,
    pub regions: &'a RefCell<Vec<Region>>,
    pub t: &'a SignetTheme,
}

impl ListCtx<'_> {
    fn reg(&self, hit: Hit, cell: super::dnd::Cell2) {
        self.regions.borrow_mut().push(Region { hit, cell });
    }
    fn dark(&self) -> bool {
        self.t.is_dark()
    }
}

/// Insertion marker of row `i` in a list of `len` items.
fn indicator_of(d: Option<&Drop>, zone: Zone, i: usize, len: usize, folder: Option<&str>) -> Option<Indicator> {
    let d = d?;
    if d.target.zone != zone || d.target.folder_id.as_deref() != folder || d.into.is_some() {
        return None;
    }
    if d.target.index == i {
        return Some(Indicator::Before);
    }
    if i + 1 == len && d.target.index == len { Some(Indicator::After) } else { None }
}

/// Insertion gap during a drag: the place where the held item will land.
fn gap_slot(ctx: &ListCtx, tile: bool) -> AnyElement {
    let t = ctx.t;
    let h = if tile { space::TOOLBAR_H } else { px(ctx.gap_h.max(f32::from(t.row_h))) };
    div()
        .flex_none()
        .h(h)
        .rounded(if tile { radius::TILE } else { radius::ROW })
        .bg(t.accent_a(0.10))
        .border_1()
        .border_color(t.accent_a(0.35))
        .into_any_element()
}

/// Node with its gap before or after (when gaps are on; otherwise the row's own marker is drawn).
fn with_gap(ctx: &ListCtx, node: AnyElement, ind: Option<Indicator>, tile: bool, out: &mut Vec<AnyElement>) {
    if ctx.gapped && ind == Some(Indicator::Before) {
        out.push(gap_slot(ctx, tile));
    }
    out.push(node);
    if ctx.gapped && ind == Some(Indicator::After) {
        out.push(gap_slot(ctx, tile));
    }
}

/// Row wrapped so that it glides when the list changes (no-op in the tab row).
fn flip(ctx: &ListCtx, key: &str, both_axes: bool, node: AnyElement) -> AnyElement {
    match &ctx.env.motion {
        Some(m) => m.flip.wrap(key, both_axes, node),
        None => node,
    }
}

/// Body of a folder or group (rows and their gaps) with its unfolding: while it opens or closes, the height is
/// clipped to a share of the last measured one.
fn animated_body(ctx: &ListCtx, id: &str, body: gpui::Div, anim: Option<Unfold>) -> AnyElement {
    let Some(u) = anim else { return body.into_any_element() };
    let measured = ctx.env.bounds.get(&format!("body:{id}")).map(|b| f32::from(b.size.height)).filter(|h| *h > 1.0);
    let full = measured.unwrap_or(u.height);
    div()
        .overflow_hidden()
        .child(body)
        .with_animation(
            gpui::ElementId::from(gpui::SharedString::from(format!("unfold-{id}-{}", u.opening))),
            gpui::Animation::new(crate::theme::duration::NORMAL).with_easing(crate::motion::ease_out),
            move |d, k| d.max_h(px(full * if u.opening { k } else { 1.0 - k })),
        )
        .into_any_element()
}

fn mark_of(ctx: &ListCtx, ind: Option<Indicator>) -> Option<Indicator> {
    if ctx.gapped { None } else { ind }
}

fn hint(ctx: &ListCtx, text: String) -> AnyElement {
    div().flex_none().p(space::S2).flex().justify_center().text_size(ctx.t.type_scale.cap).text_color(ctx.t.text_2).child(text).into_any_element()
}

/// Folder card or group card container.
fn card_box(ctx: &ListCtx, hue: Option<f32>, same_as_parent: bool) -> gpui::Div {
    let t = ctx.t;
    let bg = match hue {
        Some(h) => tint(h, ctx.dark()).bg,
        None => t.surface_2,
    };
    div().relative().flex().flex_col().flex_none().when(!same_as_parent, |d| d.p(space::S1).bg(bg)).rounded(radius::TILE)
}

fn children_box(depth: usize) -> gpui::Div {
    div().flex().flex_col().gap(space::HALF).pt(space::HALF).when(depth > 1 && depth <= MAX_DEPTH, |d| d.pl(space::S2))
}

fn region_cell(ctx: &ListCtx, key: &str) -> super::dnd::Cell2 {
    ctx.env.bounds.cell(key)
}

/// Rows of a list of items (pinned root, or the content of a folder).
pub fn render_items(ctx: &ListCtx, cx: &mut App, zone: Zone, items: &[Item], folder_id: Option<&str>, depth: usize, parent_hue: Option<f64>) -> Vec<AnyElement> {
    let mut out = Vec::new();
    let env = ctx.env;
    let active = env.active_id.as_deref();
    for (i, item) in items.iter().enumerate() {
        let ind = indicator_of(ctx.drop, zone, i, items.len(), folder_id);
        match item {
            Item::Folder(f) => {
                if env.src_id.as_deref() == Some(f.id.as_str()) {
                    continue;
                }
                let node = folder_card(ctx, cx, f, i, folder_id, ind, depth, parent_hue);
                let node = flip(ctx, &format!("card:{}", f.id), false, node);
                with_gap(ctx, node, ind, false, &mut out);
            }
            _ => {
                let cell = region_cell(ctx, &item_key(item));
                ctx.reg(Hit::Item { zone, index: i, folder: folder_id.map(String::from), run: None, card: None, horizontal: false }, cell);
                let node = item_row(item, ctx.snap, zone, mark_of(ctx, ind), folder_id.is_some(), None, env, cx);
                let node = flip(ctx, &item_key(item), false, node);
                with_gap(ctx, node, ind, false, &mut out);
            }
        }
    }
    let _ = active;
    out
}

#[allow(clippy::too_many_arguments)]
fn folder_card(ctx: &ListCtx, cx: &mut App, f: &FolderItem, index: usize, parent: Option<&str>, ind: Option<Indicator>, depth: usize, parent_hue: Option<f64>) -> AnyElement {
    let env = ctx.env;
    let active = env.active_id.as_deref();
    let inside = active.is_some_and(|a| f.items.iter().any(|i| item_tab_ids(i).iter().any(|t| t == a)));
    // The active tab of a closed folder stays visible under the row of the topmost closed folder.
    let shown = if !f.open && inside { active.and_then(|a| leaf_of(&f.items, a)) } else { None };
    let same = f.hue.is_some() && f.hue == parent_hue;
    let card_cell = region_cell(ctx, &format!("card:{}", f.id));
    let head_cell = region_cell(ctx, &format!("hdr:{}", f.id));
    ctx.reg(Hit::FolderHead { id: f.id.clone(), index, parent: parent.map(String::from), horizontal: false }, head_cell);
    let header = header_row(Header::Folder(f), mark_of(ctx, ind), ctx.drop.is_some_and(|d| d.into.as_deref() == Some(f.id.as_str())), f.hue.map(|h| h as f32), None, env, cx);
    let mut card = card_box(ctx, f.hue.map(|h| h as f32), same).child(probe(card_cell)).child(header);
    let anim = env.motion.as_ref().and_then(|m| m.unfold_of(&f.id));
    let closing = anim.is_some_and(|u| !u.opening);
    if f.open || closing {
        let body_cell = region_cell(ctx, &format!("body:{}", f.id));
        if f.open {
            ctx.reg(Hit::FolderBody { id: f.id.clone(), len: f.items.len() }, body_cell.clone());
        }
        let mut kids = render_items(ctx, cx, Zone::Pinned, &f.items, Some(&f.id), depth + 1, f.hue);
        if f.items.is_empty() {
            kids.push(div().flex_none().px(space::S2).py(space::S1).text_size(ctx.t.type_scale.sub).text_color(ctx.t.text_2).child(tr("sidebar.emptyFolder")).into_any_element());
        }
        card = card.child(animated_body(ctx, &f.id, children_box(depth + 1).relative().child(probe(body_cell)).children(kids), anim));
    } else if let Some(leaf) = shown {
        let cell = region_cell(ctx, &item_key(leaf));
        ctx.reg(Hit::FolderHead { id: f.id.clone(), index, parent: parent.map(String::from), horizontal: false }, cell);
        card = card.child(children_box(depth + 1).child(item_row(leaf, ctx.snap, Zone::Pinned, None, true, None, env, cx)));
    }
    card.into_any_element()
}

/// Favorites (3-column grid of tiles).
pub fn favorites_list(ctx: &ListCtx, cx: &mut App, ids: &[String], show_hint: bool) -> AnyElement {
    let t = ctx.t;
    let drop = ctx.drop.filter(|d| d.target.zone == Zone::Favorites);
    let list_cell = region_cell(ctx, "list:favorites");
    let mut children = Vec::new();
    let mut tiles: Vec<AnyElement> = Vec::new();
    let present: Vec<&String> = ids.iter().filter(|id| ctx.snap.tabs.contains_key(*id)).collect();
    for (i, id) in ids.iter().enumerate() {
        let Some(tab) = ctx.snap.tabs.get(id) else { continue };
        let ind = indicator_of(drop, Zone::Favorites, i, ids.len(), None);
        let cell = region_cell(ctx, &tab.id);
        children.push(cell.clone());
        ctx.reg(Hit::Item { zone: Zone::Favorites, index: i, folder: None, run: None, card: None, horizontal: true }, cell);
        let node = favorite_tile(tab, mark_of(ctx, ind), None, ctx.env, cx);
        let node = flip(ctx, &tab.id, true, node);
        with_gap(ctx, node, ind, true, &mut tiles);
    }
    let _ = present;
    ctx.reg(Hit::List { zone: Zone::Favorites, len: ids.len(), children, axis: Axis::Grid }, list_cell.clone());
    let empty = ids.is_empty();
    div()
        .relative()
        .flex_none()
        .pb(space::S3)
        .when(empty, |d| d.mb(space::S3).pb(px(0.0)).py(space::S4).border_2().border_dashed().border_color(t.border).rounded(radius::TILE).flex().justify_center())
        .child(probe(list_cell))
        .when(!empty, |d| d.child(div().grid().grid_cols(3).gap(space::S2).children(tiles)))
        .when(empty && show_hint, |d| d.child(div().text_size(t.type_scale.cap).text_color(t.text_2).child(tr("sidebar.dropFavorites"))))
        .into_any_element()
}

/// Pinned tabs and folders.
pub fn pinned_list(ctx: &ListCtx, cx: &mut App, items: &[Item], dragging: bool) -> AnyElement {
    let list_cell = region_cell(ctx, "list:pinned");
    let over = ctx.drop.is_some_and(|d| d.target.zone == Zone::Pinned);
    let children = render_items(ctx, cx, Zone::Pinned, items, None, 0, None);
    // Root-level entries, for the insertion rank when the pointer is in a gap.
    let root_cells: Vec<_> = items
        .iter()
        .map(|i| match i {
            Item::Folder(f) => region_cell(ctx, &format!("card:{}", f.id)),
            other => region_cell(ctx, &item_key(other)),
        })
        .collect();
    ctx.reg(Hit::List { zone: Zone::Pinned, len: items.len(), children: root_cells, axis: Axis::Vertical }, list_cell.clone());
    let show_hint = items.is_empty() && dragging;
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_none()
        .gap(space::S2)
        .rounded(radius::TILE)
        .when(items.is_empty() && !dragging, |d| d.min_h(px(0.0)))
        .when(!(items.is_empty() && !dragging), |d| d.min_h(px(6.0)))
        .when(over, |d| d.bg(ctx.t.surface_2))
        .child(probe(list_cell))
        .children(children)
        .when(show_hint, |d| d.child(hint(ctx, tr("sidebar.dropPin"))))
        .into_any_element()
}

/// Today's tabs: each group is a card with its header; ranks stay those of the flat list.
pub fn today_list(ctx: &ListCtx, cx: &mut App, items: &[Item]) -> AnyElement {
    let env = ctx.env;
    let active = env.active_id.as_deref();
    let tm = today_mark(items, ctx.drop);
    let list_cell = region_cell(ctx, "list:today");
    let mut root_cells = Vec::new();
    let mut out: Vec<AnyElement> = Vec::new();

    let mark_at = |i: usize| -> Option<Indicator> {
        match &tm {
            Some(m) if m.index == i as i64 && !m.card => Some(m.side),
            _ => None,
        }
    };
    for run in runs_of(items) {
        let row = |i: usize, run: Option<&Run>, ind: Option<Indicator>, card: Option<super::dnd::Cell2>, cx: &mut App| -> Vec<AnyElement> {
            let item = &items[i];
            let cell = region_cell(ctx, &item_key(item));
            ctx.reg(Hit::Item { zone: Zone::Today, index: i, folder: None, run: run.cloned(), card, horizontal: false }, cell);
            let node = item_row(item, ctx.snap, Zone::Today, mark_of(ctx, ind), false, None, env, cx);
            let node = flip(ctx, &item_key(item), false, node);
            let mut v = Vec::new();
            with_gap(ctx, node, ind, false, &mut v);
            v
        };
        let Some(group) = run.group.clone() else {
            root_cells.push(region_cell(ctx, &item_key(&items[run.start])));
            out.extend(row(run.start, None, mark_at(run.start), None, cx));
            continue;
        };
        let default = TabGroup { name: String::new(), icon: None, hue: 215.0, open: true };
        let header = ctx.space.groups.as_ref().and_then(|g| g.get(&group)).unwrap_or(&default);
        let card_cell = region_cell(ctx, &format!("card:{group}"));
        root_cells.push(card_cell.clone());
        // Insertion right before the group: the marker goes on the header, not on the first tab.
        let before_card = tm.as_ref().is_some_and(|m| m.side == Indicator::Before && m.index == run.start as i64) && ctx.drop.and_then(|d| d.target.group_id.as_deref()) != Some(group.as_str());
        // Insertion right after the card (end of list): the marker goes after it, not inside the group.
        let after_card = tm.as_ref().is_some_and(|m| m.card && m.index == run.end as i64 - 1);
        let members = &items[run.start..run.end];
        let shown = if !header.open { active.and_then(|a| members.iter().position(|m| item_tab_ids(m).iter().any(|t| t == a))) } else { None };
        let first_tab = item_tab_ids(&members[0]).first().and_then(|id| ctx.snap.tabs.get(id));
        let name = if header.name.is_empty() { first_tab.map(|f| display_title(f, &tr("sidebar.newTab"))).unwrap_or_default() } else { header.name.clone() };
        ctx.reg(Hit::GroupCard { run: run.clone(), horizontal: false }, card_cell.clone());
        ctx.reg(Hit::GroupHead { run: run.clone(), card: card_cell.clone(), horizontal: false }, region_cell(ctx, &format!("hdr:{group}")));

        let ind_outer = if before_card {
            Some(Indicator::Before)
        } else if after_card {
            Some(Indicator::After)
        } else {
            None
        };
        if env.src_id.as_deref() == Some(format!("g:{group}").as_str()) {
            with_gap(ctx, div().into_any_element(), ind_outer, false, &mut out);
            continue;
        }
        let head = header_row(
            Header::Group { id: &group, group: header, first: first_tab, name },
            mark_of(ctx, if before_card { Some(Indicator::Before) } else { None }),
            ctx.drop.is_some_and(|d| d.into.as_deref() == Some(group.as_str())),
            Some(header.hue as f32),
            None,
            env,
            cx,
        );
        let mut body = Vec::new();
        let anim = env.motion.as_ref().and_then(|m| m.unfold_of(&group));
        if header.open || anim.is_some_and(|u| !u.opening) {
            for i in run.start..run.end {
                body.extend(row(i, Some(&run), if i == run.start && before_card { None } else { mark_at(i) }, Some(card_cell.clone()), cx));
            }
        } else if let Some(s) = shown {
            body.extend(row(run.start + s, Some(&run), None, Some(card_cell.clone()), cx));
        }
        let body_box = children_box(0).relative().child(probe(region_cell(ctx, &format!("body:{group}")))).children(body);
        let card = card_box(ctx, Some(header.hue as f32), false).child(probe(card_cell)).child(head).child(animated_body(ctx, &group, body_box, anim));
        let mut node = vec![flip(ctx, &format!("card:{group}"), false, card.into_any_element())];
        if after_card && !ctx.gapped {
            node.push(div().relative().h(px(0.0)).child(div().absolute().left(space::S2).right(space::S2).top(px(0.0)).h(px(2.0)).rounded(px(1.0)).bg(ctx.t.accent)).into_any_element());
        }
        let mut wrapped = Vec::new();
        if ctx.gapped && ind_outer == Some(Indicator::Before) {
            wrapped.push(gap_slot(ctx, false));
        }
        wrapped.extend(node);
        if ctx.gapped && ind_outer == Some(Indicator::After) {
            wrapped.push(gap_slot(ctx, false));
        }
        out.extend(wrapped);
    }
    ctx.reg(Hit::List { zone: Zone::Today, len: items.len(), children: root_cells, axis: Axis::Vertical }, list_cell.clone());
    div().relative().flex().flex_col().flex_none().gap(space::HALF).min_h(px(40.0)).rounded(radius::TILE).child(probe(list_cell)).children(out).into_any_element()
}

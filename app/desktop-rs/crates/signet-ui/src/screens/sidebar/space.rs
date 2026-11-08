//! Space pages: the read-only preview of a neighbor space (visible while swiping) and the strip of space icons.
//! Ports of `SpacePreview.tsx` and `SpaceStrip.tsx`.

use gpui::{AnyElement, App, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder, px};
use signet_core::types::{Item, Snapshot, Space, TabGroup};

use crate::app_state::tr;
use crate::icon::IconName;
use crate::screens::tabs::custom::{folder_icon, group_icon, tab_icon, tint};
use crate::screens::tabs::groups::runs_of;
use crate::screens::tabs::{display_title, item_tab_ids};
use crate::theme::{SignetTheme, ThemeExt, radius, space};
use gpui_component::Icon;

/// Width of a space icon in the bottom bar.
pub const SLOT: f32 = 28.0;

/// At most 5 icons per page, 4 when space is tight.
pub fn per_page_for(width: f32) -> usize {
    if width >= 5.0 * SLOT { 5 } else { 4 }
}

fn preview_row(t: &SignetTheme, icon: AnyElement, title: String, dim: bool) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(space::S2)
        .h(t.row_h)
        .pl(space::S3)
        .pr(space::S2)
        .rounded(radius::ROW)
        .text_size(t.type_scale.base)
        .text_color(t.text_tab)
        .when(dim, |d| d.opacity(0.7))
        .child(div().flex_none().child(icon))
        .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(title))
        .into_any_element()
}

fn rows(snap: &Snapshot, t: &SignetTheme, items: &[Item], depth: usize, cx: &App) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Item::Folder(f) => {
                let head = preview_row(t, folder_icon(f, 16.0, t.text_2, t.danger), f.name.clone(), false);
                let mut card = div().flex().flex_col().flex_none().p(space::S1).rounded(radius::TILE).bg(match f.hue {
                    Some(h) => tint(h as f32, cx.signet().is_dark()).bg,
                    None => t.surface_2,
                });
                card = card.child(head);
                if f.open && depth < 3 {
                    card = card.child(div().flex().flex_col().gap(space::HALF).pt(space::HALF).children(rows(snap, t, &f.items, depth + 1, cx)));
                }
                out.push(card.into_any_element());
            }
            other => {
                for id in item_tab_ids(other) {
                    if let Some(tab) = snap.tabs.get(&id) {
                        out.push(preview_row(t, tab_icon(tab, 16.0), display_title(tab, &tr("sidebar.newTab")), !tab.loaded));
                    }
                }
            }
        }
    }
    out
}

/// Read-only preview of a neighboring space.
pub fn space_preview(sp: &Space, snap: &Snapshot, cx: &App) -> AnyElement {
    let t = cx.signet().clone();
    let default = TabGroup { name: String::new(), icon: None, hue: 215.0, open: true };
    let mut today: Vec<AnyElement> = Vec::new();
    for run in runs_of(&sp.today) {
        let slice = &sp.today[run.start..run.end];
        let Some(g) = run.group.as_deref() else {
            today.extend(rows(snap, &t, slice, 0, cx));
            continue;
        };
        let header = sp.groups.as_ref().and_then(|m| m.get(g)).unwrap_or(&default);
        let first = item_tab_ids(&slice[0]).first().and_then(|id| snap.tabs.get(id));
        let name = if header.name.is_empty() { first.map(|f| display_title(f, &tr("sidebar.newTab"))).unwrap_or_default() } else { header.name.clone() };
        let tn = tint(header.hue as f32, t.is_dark());
        let head = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(space::S2)
            .h(t.row_h)
            .px(space::S2)
            .text_size(t.type_scale.base)
            .text_color(tn.fg)
            .child(div().flex_none().child(group_icon(Some(header), first, 16.0, tn.fg)))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(name));
        let mut card = div().flex().flex_col().flex_none().p(space::S1).rounded(radius::TILE).bg(tn.bg).child(head);
        if header.open {
            card = card.child(div().flex().flex_col().gap(space::HALF).pt(space::HALF).children(rows(snap, &t, slice, 0, cx)));
        }
        today.push(card.into_any_element());
    }
    div()
        .flex()
        .flex_col()
        .gap(space::HALF)
        .child(div().flex().flex_col().gap(space::S2).children(rows(snap, &t, &sp.pinned, 0, cx)))
        .child(div().flex_none().h(space::S3).my(space::S1).px(space::S3).flex().items_center().child(div().flex_1().h(px(1.0)).bg(t.border)))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(space::S2)
                .h(t.row_h)
                .pl(space::S3)
                .text_size(t.type_scale.base)
                .text_color(t.text_2)
                .child(Icon::from(IconName::Plus).size(px(16.0)))
                .child(tr("sidebar.newTab")),
        )
        .child(div().flex().flex_col().gap(space::HALF).children(today))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_icons_per_page_when_wide_enough_otherwise_four() {
        assert_eq!(per_page_for(140.0), 5);
        assert_eq!(per_page_for(200.0), 5);
        assert_eq!(per_page_for(139.0), 4);
    }
}

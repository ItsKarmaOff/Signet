//! Drag and drop of the sidebar: hit regions registered while rendering, and the target computation of
//! `Sidebar.tsx` (`overItem`, `overFolder`, `overGroupHead`...). Hit testing is central: one `on_drag_move` on the
//! sidebar finds the deepest region under the pointer, which replaces the nested `stopPropagation` handlers of the
//! DOM version.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{Bounds, Pixels, Point};
use signet_core::types::{DragSource, Zone};

use crate::screens::tabs::groups::{Drop, Run, around_run, is_block};

/// Dead band (px) around the middle of a row: as long as the pointer stays in it, the current target is kept.
pub const DEAD_BAND: f32 = 4.0;

pub type Cell2 = Rc<Cell<Bounds<Pixels>>>;

/// What a registered region is.
/// How the entries of a list are laid out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Vertical,
    Horizontal,
    Grid,
}

#[derive(Clone)]
pub enum Hit {
    /// A tab row, split row or favorite tile.
    Item { zone: Zone, index: usize, folder: Option<String>, run: Option<Run>, card: Option<Cell2>, horizontal: bool },
    FolderHead { id: String, index: usize, parent: Option<String>, horizontal: bool },
    GroupHead { run: Run, card: Cell2, horizontal: bool },
    /// Margins of a group card.
    GroupCard { run: Run, horizontal: bool },
    /// Open folder: empty area appends at its end.
    FolderBody { id: String, len: usize },
    /// A whole list; `children` are its root-level entries in order (for the insertion rank from the pointer).
    List { zone: Zone, len: usize, children: Vec<Cell2>, axis: Axis },
}

#[derive(Clone)]
pub struct Region {
    pub hit: Hit,
    pub cell: Cell2,
}

impl Region {
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.cell.get()
    }
}

/// Is the pointer past the middle? In the dead band, keeps the side of the current target if it touches this row.
pub fn side_of(pos: f32, mid: f32, current: Option<usize>, index: usize) -> bool {
    if let Some(c) = current
        && (c == index || c == index + 1)
        && (pos - mid).abs() <= DEAD_BAND
    {
        return c == index + 1;
    }
    pos > mid
}

/// Insertion rank from the pointer in the gaps of a list: first entry whose middle is after it, otherwise the end.
pub fn index_at(pos: Point<Pixels>, axis: Axis, children: &[Bounds<Pixels>], length: usize) -> usize {
    if children.len() != length {
        return length;
    }
    let (px, py) = (f32::from(pos.x), f32::from(pos.y));
    for (i, r) in children.iter().enumerate() {
        let (left, top, w, h) = (f32::from(r.origin.x), f32::from(r.origin.y), f32::from(r.size.width), f32::from(r.size.height));
        let before = match axis {
            Axis::Grid => py < top || (py <= top + h && px < left + w / 2.0),
            Axis::Horizontal => px < left + w / 2.0,
            Axis::Vertical => py < top + h / 2.0,
        };
        if before {
            return i;
        }
    }
    length
}

/// Everything `Sidebar::drag_over` needs to know about the current drag.
pub struct DragCtx<'a> {
    pub src: &'a DragSource,
    pub space_id: &'a str,
    pub current: Option<&'a Drop>,
    pub forbidden: &'a std::collections::HashSet<String>,
    /// Open folders: id -> number of items.
    pub folder_len: &'a dyn Fn(&str) -> Option<usize>,
}

fn accepts(src: &DragSource, zone: Zone) -> bool {
    // A group dropped into the pinned tabs becomes a folder; a folder dropped into today's tabs, a group.
    matches!(src, DragSource::Tab { .. }) || zone != Zone::Favorites
}

/// Target for a pointer position over a region. `None`: nothing to show (the caller keeps or clears the target).
pub fn target_for(region: &Region, pos: Point<Pixels>, ctx: &DragCtx, group_card: Option<Bounds<Pixels>>) -> Option<Drop> {
    let (px, py) = (f32::from(pos.x), f32::from(pos.y));
    let b = region.bounds();
    let (left, top, w, h) = (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height));
    let cur = ctx.current;
    let sp = ctx.space_id;
    match &region.hit {
        Hit::Item { zone, index, folder, run, card, horizontal } => {
            if !accepts(ctx.src, *zone) {
                return None;
            }
            if let (Some(run), Some(card)) = (run, card)
                && run.group.is_some()
                && is_block(Some(ctx.src))
            {
                let c = card.get();
                let idx = if *horizontal {
                    around_run(f32::from(c.origin.x), f32::from(c.size.width), run, px, cur.map(|d| d.target.index))
                } else {
                    around_run(f32::from(c.origin.y), f32::from(c.size.height), run, py, cur.map(|d| d.target.index))
                };
                return Some(Drop::new(*zone, sp, idx));
            }
            let current = cur
                .filter(|p| {
                    p.target.zone == *zone
                        && p.target.folder_id == *folder
                        && p.target.group_id.as_deref() == run.as_ref().and_then(|r| r.group.as_deref())
                        && p.into.is_none()
                        && p.target.space_id == sp
                })
                .map(|p| p.target.index);
            let after = if *horizontal { side_of(px, left + w / 2.0, current, *index) } else { side_of(py, top + h / 2.0, current, *index) };
            Some(Drop::new(*zone, sp, index + after as usize).folder(folder.clone()).group(run.as_ref().and_then(|r| r.group.clone())))
        }
        Hit::FolderHead { id, index, parent, horizontal } => {
            if !accepts(ctx.src, Zone::Pinned) {
                return None;
            }
            let y = if *horizontal { (px - left) / w.max(1.0) } else { (py - top) / h.max(1.0) };
            let len = (ctx.folder_len)(id)?;
            if y > 0.25 && y < 0.75 && !ctx.forbidden.contains(id) {
                Some(Drop::new(Zone::Pinned, sp, len).folder(Some(id.clone())).into(id.clone()))
            } else {
                Some(Drop::new(Zone::Pinned, sp, index + (y >= 0.5) as usize).folder(parent.clone()))
            }
        }
        Hit::GroupHead { run, card, horizontal } => {
            if !accepts(ctx.src, Zone::Today) {
                return None;
            }
            let group = run.group.clone()?;
            if is_block(Some(ctx.src)) {
                let c = card.get();
                let idx = if *horizontal {
                    around_run(f32::from(c.origin.x), f32::from(c.size.width), run, px, cur.map(|d| d.target.index))
                } else {
                    around_run(f32::from(c.origin.y), f32::from(c.size.height), run, py, cur.map(|d| d.target.index))
                };
                return Some(Drop::new(Zone::Today, sp, idx));
            }
            let (py, top, h) = if *horizontal { (px, left, w) } else { (py, top, h) };
            let mid = top + h / 2.0;
            let was_into = cur.is_some_and(|p| p.target.zone == Zone::Today && p.into.as_deref() == Some(group.as_str()));
            // Dead band: we keep "before" or "inside" as long as the pointer stays near the middle.
            let band = if *horizontal { 0.0 } else { DEAD_BAND };
            let before = if (py - mid).abs() <= band && cur.is_some_and(|p| p.target.zone == Zone::Today && p.target.index == run.start) { !was_into } else { py < mid };
            if before {
                Some(Drop::new(Zone::Today, sp, run.start))
            } else {
                Some(Drop::new(Zone::Today, sp, run.start).group(Some(group.clone())).into(group))
            }
        }
        Hit::GroupCard { run, horizontal } => {
            if !accepts(ctx.src, Zone::Today) {
                return None;
            }
            let card = group_card.unwrap_or(b);
            if is_block(Some(ctx.src)) {
                let (ct, ch, p) = if *horizontal { (f32::from(card.origin.x), f32::from(card.size.width), px) } else { (f32::from(card.origin.y), f32::from(card.size.height), py) };
                Some(Drop::new(Zone::Today, sp, around_run(ct, ch, run, p, cur.map(|d| d.target.index))))
            } else {
                Some(Drop::new(Zone::Today, sp, run.end).group(run.group.clone()))
            }
        }
        Hit::FolderBody { id, len } => {
            if !accepts(ctx.src, Zone::Pinned) {
                return None;
            }
            Some(Drop::new(Zone::Pinned, sp, *len).folder(Some(id.clone())))
        }
        Hit::List { zone, len, children, axis } => {
            if !accepts(ctx.src, *zone) {
                return None;
            }
            let rects: Vec<Bounds<Pixels>> = children.iter().map(|c| c.get()).collect();
            let index = if *zone == Zone::Today && *axis == Axis::Vertical { *len } else { index_at(pos, *axis, &rects, *len) };
            Some(Drop::new(*zone, sp, index))
        }
    }
}

/// Specificity of a region: the deepest wins (items and headers over folder bodies over group margins over lists).
pub fn rank(hit: &Hit) -> u8 {
    match hit {
        Hit::Item { .. } | Hit::FolderHead { .. } | Hit::GroupHead { .. } => 4,
        Hit::FolderBody { .. } => 3,
        Hit::GroupCard { .. } => 2,
        Hit::List { .. } => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds { origin: point(px(x), px(y)), size: size(px(w), px(h)) }
    }
    fn cell(x: f32, y: f32, w: f32, h: f32) -> Cell2 {
        Rc::new(Cell::new(b(x, y, w, h)))
    }
    fn tab_src() -> DragSource {
        DragSource::Tab { tab_id: "t".into() }
    }
    fn ctx<'a>(src: &'a DragSource, current: Option<&'a Drop>, forbidden: &'a std::collections::HashSet<String>, len: &'a dyn Fn(&str) -> Option<usize>) -> DragCtx<'a> {
        DragCtx { src, space_id: "s", current, forbidden, folder_len: len }
    }

    #[test]
    fn side_of_dead_band_keeps_the_current_side() {
        assert!(side_of(110.0, 100.0, None, 2));
        assert!(!side_of(90.0, 100.0, None, 2));
        // Near the middle, the current target (touching this row) wins.
        assert!(side_of(98.0, 100.0, Some(3), 2));
        assert!(!side_of(102.0, 100.0, Some(2), 2));
        // An unrelated target does not.
        assert!(!side_of(98.0, 100.0, Some(7), 2));
    }

    #[test]
    fn index_at_vertical_and_grid() {
        let rows = [b(0.0, 0.0, 100.0, 20.0), b(0.0, 20.0, 100.0, 20.0)];
        assert_eq!(index_at(point(px(5.0), px(5.0)), Axis::Vertical, &rows, 2), 0);
        assert_eq!(index_at(point(px(5.0), px(25.0)), Axis::Vertical, &rows, 2), 1);
        assert_eq!(index_at(point(px(5.0), px(60.0)), Axis::Vertical, &rows, 2), 2);
        // Wrong count of children: the end.
        assert_eq!(index_at(point(px(5.0), px(5.0)), Axis::Vertical, &rows, 3), 3);
        let tiles = [b(0.0, 0.0, 30.0, 44.0), b(40.0, 0.0, 30.0, 44.0)];
        // A row of chips only looks at x.
        assert_eq!(index_at(point(px(60.0), px(500.0)), Axis::Horizontal, &tiles, 2), 2);
        assert_eq!(index_at(point(px(10.0), px(500.0)), Axis::Horizontal, &tiles, 2), 0);
        assert_eq!(index_at(point(px(60.0), px(10.0)), Axis::Grid, &tiles, 2), 2);
        assert_eq!(index_at(point(px(41.0), px(10.0)), Axis::Grid, &tiles, 2), 1);
    }

    #[test]
    fn item_hover_picks_the_half() {
        let src = tab_src();
        let none = Default::default();
        let len = |_: &str| None;
        let c = ctx(&src, None, &none, &len);
        let r = Region { hit: Hit::Item { zone: Zone::Today, index: 1, folder: None, run: None, card: None, horizontal: false }, cell: cell(0.0, 100.0, 100.0, 30.0) };
        assert_eq!(target_for(&r, point(px(5.0), px(105.0)), &c, None).unwrap().target.index, 1);
        assert_eq!(target_for(&r, point(px(5.0), px(125.0)), &c, None).unwrap().target.index, 2);
    }

    #[test]
    fn favorites_refuse_folders_and_groups() {
        let src = DragSource::Folder { id: "f".into() };
        let none = Default::default();
        let len = |_: &str| None;
        let c = ctx(&src, None, &none, &len);
        let r = Region { hit: Hit::Item { zone: Zone::Favorites, index: 0, folder: None, run: None, card: None, horizontal: true }, cell: cell(0.0, 0.0, 30.0, 44.0) };
        assert!(target_for(&r, point(px(5.0), px(5.0)), &c, None).is_none());
    }

    #[test]
    fn folder_head_quarters() {
        let src = tab_src();
        let none = Default::default();
        let len = |id: &str| (id == "f").then_some(3);
        let c = ctx(&src, None, &none, &len);
        let r = Region { hit: Hit::FolderHead { id: "f".into(), index: 2, parent: None, horizontal: false }, cell: cell(0.0, 100.0, 100.0, 40.0) };
        // Middle: inside, at the end of the folder.
        let d = target_for(&r, point(px(5.0), px(120.0)), &c, None).unwrap();
        assert_eq!((d.target.folder_id.as_deref(), d.target.index, d.into.as_deref()), (Some("f"), 3, Some("f")));
        // Top quarter: before the folder; bottom quarter: after it.
        assert_eq!(target_for(&r, point(px(5.0), px(102.0)), &c, None).unwrap().target.index, 2);
        assert_eq!(target_for(&r, point(px(5.0), px(138.0)), &c, None).unwrap().target.index, 3);
        // A dragged folder cannot go into itself.
        let mut forbidden = std::collections::HashSet::new();
        forbidden.insert("f".to_string());
        let c2 = ctx(&src, None, &forbidden, &len);
        assert!(target_for(&r, point(px(5.0), px(120.0)), &c2, None).unwrap().into.is_none());
    }

    #[test]
    fn group_head_before_or_inside() {
        let src = tab_src();
        let none = Default::default();
        let len = |_: &str| None;
        let c = ctx(&src, None, &none, &len);
        let run = Run { group: Some("g".into()), start: 2, end: 5 };
        let r = Region { hit: Hit::GroupHead { run: run.clone(), card: cell(0.0, 100.0, 100.0, 120.0), horizontal: false }, cell: cell(0.0, 100.0, 100.0, 30.0) };
        let before = target_for(&r, point(px(5.0), px(105.0)), &c, None).unwrap();
        assert_eq!((before.target.index, before.into), (2, None));
        let inside = target_for(&r, point(px(5.0), px(125.0)), &c, None).unwrap();
        assert_eq!((inside.target.index, inside.target.group_id.as_deref(), inside.into.as_deref()), (2, Some("g"), Some("g")));
    }

    #[test]
    fn a_block_over_a_group_lands_before_or_after_the_card() {
        let src = DragSource::Group { id: "h".into() };
        let none = Default::default();
        let len = |_: &str| None;
        let c = ctx(&src, None, &none, &len);
        let run = Run { group: Some("g".into()), start: 2, end: 5 };
        let r = Region { hit: Hit::Item { zone: Zone::Today, index: 3, folder: None, run: Some(run), card: Some(cell(0.0, 100.0, 100.0, 100.0)), horizontal: false }, cell: cell(0.0, 130.0, 100.0, 30.0) };
        assert_eq!(target_for(&r, point(px(5.0), px(110.0)), &c, None).unwrap().target.index, 2);
        assert_eq!(target_for(&r, point(px(5.0), px(190.0)), &c, None).unwrap().target.index, 5);
    }

    #[test]
    fn list_fallback_appends() {
        let src = tab_src();
        let none = Default::default();
        let len = |_: &str| None;
        let c = ctx(&src, None, &none, &len);
        let r = Region { hit: Hit::List { zone: Zone::Today, len: 4, children: vec![], axis: Axis::Vertical }, cell: cell(0.0, 0.0, 100.0, 300.0) };
        assert_eq!(target_for(&r, point(px(5.0), px(250.0)), &c, None).unwrap().target.index, 4);
    }
}

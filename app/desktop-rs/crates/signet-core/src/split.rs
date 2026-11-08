//! Ports `packages/shared/src/split.ts`: advanced split view geometry (rectangles, dividers) and insertion.

use crate::types::{Divider, Orientation, Rect, SplitItem, SplitLayout};

pub const GAP: f64 = 8.0;
/// Minimum size of a pane (width or height depending on the axis), in pixels.
pub const MIN_PANE: f64 = 200.0;
pub const MAX_PANES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Part of a split view needed for the computation (subset of `SplitItem`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SplitShape {
    pub tab_ids: Vec<String>,
    pub layout: Option<SplitLayout>,
    pub sizes: Option<Vec<f64>>,
}

impl From<&SplitItem> for SplitShape {
    fn from(s: &SplitItem) -> Self {
        SplitShape { tab_ids: s.tab_ids.clone(), layout: s.layout, sizes: s.sizes.clone() }
    }
}

/// A divider before it knows which split view it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct RawDivider {
    pub index: usize,
    pub orientation: Orientation,
    pub rect: Rect,
}

impl RawDivider {
    pub fn into_divider(self, split_id: &str) -> Divider {
        Divider { split_id: split_id.to_string(), index: self.index, orientation: self.orientation, rect: self.rect }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SplitGeometry {
    /// One rectangle per tab, in `tab_ids` order.
    pub rects: Vec<Rect>,
    pub dividers: Vec<RawDivider>,
    /// Layout actually applied (`grid` becomes `columns` below 3 panes).
    pub layout: SplitLayout,
}

/// Effective layout: the grid only exists from 3 panes.
pub fn effective_layout(layout: Option<SplitLayout>, n: usize) -> SplitLayout {
    match layout {
        Some(SplitLayout::Rows) => SplitLayout::Rows,
        Some(SplitLayout::Grid) if n >= 3 => SplitLayout::Grid,
        _ => SplitLayout::Columns,
    }
}

/// Number of panes in the top row of a grid.
pub fn top_count(n: usize) -> usize {
    n.div_ceil(2)
}

/// Expected number of parts per layout.
pub fn part_count(layout: SplitLayout, n: usize) -> usize {
    if layout == SplitLayout::Grid { 2 } else { n }
}

/// Valid parts (positive, sum 1) or equal parts.
pub fn normalize_parts(sizes: Option<&[f64]>, count: usize) -> Vec<f64> {
    let equal = vec![1.0 / count as f64; count];
    let Some(sizes) = sizes else { return equal };
    if sizes.len() != count || sizes.iter().any(|s| !s.is_finite() || *s <= 0.0) {
        return equal;
    }
    let sum: f64 = sizes.iter().sum();
    if sum > 0.0 { sizes.iter().map(|s| s / sum).collect() } else { equal }
}

/// Lengths (sum = `total`) matching the parts, each at least `min`.
pub fn clamp_lengths(parts: &[f64], total: f64, min: f64) -> Vec<f64> {
    let n = parts.len();
    if n == 0 {
        return Vec::new();
    }
    if total < n as f64 * min {
        return vec![total / n as f64; n];
    }
    let mut fixed = vec![false; n];
    let mut out: Vec<f64> = parts.iter().map(|p| p * total).collect();
    for _ in 0..n {
        let mut changed = false;
        for i in 0..n {
            if !fixed[i] && out[i] < min {
                out[i] = min;
                fixed[i] = true;
                changed = true;
            }
        }
        let free_sum: f64 = parts.iter().zip(&fixed).filter(|(_, f)| !**f).map(|(p, _)| p).sum();
        let rest = total - fixed.iter().filter(|f| **f).count() as f64 * min;
        let free_count = fixed.iter().filter(|f| !**f).count() as f64;
        for i in 0..n {
            if !fixed[i] {
                out[i] = if free_sum > 0.0 { parts[i] / free_sum * rest } else { rest / free_count };
            }
        }
        if !changed {
            break;
        }
    }
    out
}

/// JavaScript `Math.round` (halves go up).
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Splits `total` (excluding gaps) into rounded pieces whose sum is exact.
fn segments(start: f64, lengths: &[f64]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut acc = 0.0;
    let mut pos = start;
    for (i, l) in lengths.iter().enumerate() {
        acc += l;
        let end = js_round(start + acc + GAP * i as f64);
        out.push((js_round(pos), end - js_round(pos)));
        pos = end + GAP;
    }
    out
}

/// Rectangles and dividers of the split view within `area`.
pub fn compute_split(area: Rect, split: &SplitShape) -> SplitGeometry {
    let n = split.tab_ids.len();
    let layout = effective_layout(split.layout, n);
    if n <= 1 {
        return SplitGeometry { rects: if n == 1 { vec![area] } else { vec![] }, dividers: vec![], layout: SplitLayout::Columns };
    }
    let parts = normalize_parts(split.sizes.as_deref(), part_count(layout, n));
    let mut dividers = Vec::new();

    if layout == SplitLayout::Columns {
        let segs = segments(area.x, &clamp_lengths(&parts, area.width - GAP * (n - 1) as f64, MIN_PANE));
        let rects: Vec<Rect> = segs.iter().map(|(p, l)| Rect::new(*p, area.y, *l, area.height)).collect();
        for (i, r) in rects.iter().take(n - 1).enumerate() {
            dividers.push(RawDivider { index: i, orientation: Orientation::Vertical, rect: Rect::new(r.x + r.width, area.y, GAP, area.height) });
        }
        return SplitGeometry { rects, dividers, layout };
    }

    if layout == SplitLayout::Rows {
        let segs = segments(area.y, &clamp_lengths(&parts, area.height - GAP * (n - 1) as f64, MIN_PANE));
        let rects: Vec<Rect> = segs.iter().map(|(p, l)| Rect::new(area.x, *p, area.width, *l)).collect();
        for (i, r) in rects.iter().take(n - 1).enumerate() {
            dividers.push(RawDivider { index: i, orientation: Orientation::Horizontal, rect: Rect::new(area.x, r.y + r.height, area.width, GAP) });
        }
        return SplitGeometry { rects, dividers, layout };
    }

    // Grid: two rows, equal columns in each row.
    let rows = segments(area.y, &clamp_lengths(&parts, area.height - GAP, MIN_PANE));
    let (top, bottom) = (rows[0], rows[1]);
    let n_top = top_count(n);
    let mut rects = Vec::new();
    let mut row = |count: usize, y: f64, height: f64| {
        let each = (area.width - GAP * (count - 1) as f64) / count as f64;
        for (p, l) in segments(area.x, &vec![each; count]) {
            rects.push(Rect::new(p, y, l, height));
        }
    };
    row(n_top, top.0, top.1);
    row(n - n_top, bottom.0, bottom.1);
    dividers.push(RawDivider { index: 0, orientation: Orientation::Horizontal, rect: Rect::new(area.x, top.0 + top.1, area.width, GAP) });
    SplitGeometry { rects, dividers, layout }
}

/// Parts from measured lengths (any sum).
pub fn parts_from_lengths(lengths: &[f64]) -> Vec<f64> {
    let sum: f64 = lengths.iter().sum();
    if sum > 0.0 { lengths.iter().map(|l| l / sum).collect() } else { vec![1.0 / lengths.len() as f64; lengths.len()] }
}

/// Adds `tab_id` to the split view, before (left/top) or after (right/bottom) `focused_id`. False if it is full.
pub fn insert_into_split(split: &mut SplitShape, tab_id: &str, edge: DropEdge, focused_id: Option<&str>) -> bool {
    if split.tab_ids.len() >= MAX_PANES || split.tab_ids.iter().any(|t| t == tab_id) {
        return false;
    }
    let at = focused_id.and_then(|f| split.tab_ids.iter().position(|t| t == f));
    let base = at.unwrap_or(split.tab_ids.len() - 1);
    let pos = if matches!(edge, DropEdge::Left | DropEdge::Top) { base } else { base + 1 };
    split.tab_ids.insert(pos, tab_id.to_string());

    let n = split.tab_ids.len();
    let was = effective_layout(split.layout, n - 1);
    let horizontal = matches!(edge, DropEdge::Left | DropEdge::Right);
    let next = if was == SplitLayout::Grid {
        SplitLayout::Grid
    } else if horizontal {
        if was == SplitLayout::Rows && n >= 3 { SplitLayout::Grid } else { SplitLayout::Columns }
    } else if was == SplitLayout::Columns && n >= 3 {
        SplitLayout::Grid
    } else {
        SplitLayout::Rows
    };
    split.layout = Some(next);
    split.sizes = None;
    true
}

/// Same as `insert_into_split` on a persisted split item.
pub fn insert_into_split_item(split: &mut SplitItem, tab_id: &str, edge: DropEdge, focused_id: Option<&str>) -> bool {
    let mut shape = SplitShape::from(&*split);
    let ok = insert_into_split(&mut shape, tab_id, edge, focused_id);
    split.tab_ids = shape.tab_ids;
    split.layout = shape.layout;
    split.sizes = shape.sizes;
    ok
}

/// Puts the split view back in a consistent state after a removal or a read from disk.
pub fn sanitize_split(split: &mut SplitShape) {
    let layout = effective_layout(split.layout, split.tab_ids.len());
    split.layout = if layout == SplitLayout::Columns { None } else { Some(layout) };
    let count = part_count(layout, split.tab_ids.len());
    if let Some(s) = &split.sizes
        && (s.len() != count || s.iter().any(|x| !x.is_finite() || *x <= 0.0))
    {
        split.sizes = None;
    }
}

/// Same as `sanitize_split` on a persisted split item.
pub fn sanitize_split_item(split: &mut SplitItem) {
    let mut shape = SplitShape::from(&*split);
    sanitize_split(&mut shape);
    split.layout = shape.layout;
    split.sizes = shape.sizes;
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(10.0, 20.0, 1000.0, 600.0);
    fn shape(ids: &[&str]) -> SplitShape {
        SplitShape { tab_ids: ids.iter().map(|s| s.to_string()).collect(), ..Default::default() }
    }
    fn mk(ids: &[&str], layout: Option<SplitLayout>) -> SplitShape {
        SplitShape { layout, sizes: Some(vec![1.0, 1.0]), ..shape(ids) }
    }

    #[test]
    fn effective() {
        assert_eq!(effective_layout(Some(SplitLayout::Grid), 2), SplitLayout::Columns);
        assert_eq!(effective_layout(Some(SplitLayout::Grid), 3), SplitLayout::Grid);
        assert_eq!(effective_layout(Some(SplitLayout::Rows), 2), SplitLayout::Rows);
        assert_eq!(effective_layout(None, 4), SplitLayout::Columns);
    }

    #[test]
    fn parts_and_lengths() {
        assert_eq!(normalize_parts(Some(&[1.0, 3.0]), 2), vec![0.25, 0.75]);
        assert_eq!(normalize_parts(Some(&[1.0, 3.0]), 3), vec![1.0 / 3.0; 3]);
        assert_eq!(normalize_parts(Some(&[1.0, -1.0]), 2), vec![0.5, 0.5]);
        assert_eq!(normalize_parts(Some(&[1.0, f64::NAN]), 2), vec![0.5, 0.5]);
        assert_eq!(normalize_parts(None, 2), vec![0.5, 0.5]);
        let out = clamp_lengths(&[0.05, 0.95], 1000.0, 200.0);
        assert_eq!(out[0], 200.0);
        assert!((out[0] + out[1] - 1000.0).abs() < 1e-6);
        assert_eq!(clamp_lengths(&[0.9, 0.1], 300.0, 200.0), vec![150.0, 150.0]);
        assert!(clamp_lengths(&[], 100.0, 200.0).is_empty());
        assert_eq!(parts_from_lengths(&[100.0, 300.0]), vec![0.25, 0.75]);
        assert_eq!(parts_from_lengths(&[0.0, 0.0]), vec![0.5, 0.5]);
    }

    #[test]
    fn compute_zero_and_one() {
        let g = compute_split(AREA, &shape(&[]));
        assert!(g.rects.is_empty() && g.dividers.is_empty() && g.layout == SplitLayout::Columns);
        assert_eq!(compute_split(AREA, &shape(&["a"])).rects, vec![AREA]);
    }

    #[test]
    fn compute_columns() {
        let g = compute_split(AREA, &shape(&["a", "b"]));
        assert_eq!(g.layout, SplitLayout::Columns);
        assert_eq!(g.rects[0].x, 10.0);
        assert_eq!(g.rects[0].width + GAP + g.rects[1].width, 1000.0);
        assert_eq!(g.rects[1].x, g.rects[0].x + g.rects[0].width + GAP);
        assert_eq!(g.dividers, vec![RawDivider { index: 0, orientation: Orientation::Vertical, rect: Rect::new(g.rects[0].x + g.rects[0].width, 20.0, GAP, 600.0) }]);
        let mut s = shape(&["a", "b"]);
        s.sizes = Some(vec![0.25, 0.75]);
        assert_eq!(compute_split(AREA, &s).rects[0].width, js_round(0.25 * (1000.0 - GAP)));
    }

    #[test]
    fn compute_rows_and_grid() {
        let mut s = shape(&["a", "b", "c"]);
        s.layout = Some(SplitLayout::Rows);
        let g = compute_split(AREA, &s);
        assert_eq!(g.layout, SplitLayout::Rows);
        assert!(g.rects.iter().all(|r| r.x == 10.0 && r.width == 1000.0));
        assert_eq!(g.rects.iter().map(|r| r.height).sum::<f64>() + 2.0 * GAP, 600.0);
        assert!(g.dividers.iter().all(|d| d.orientation == Orientation::Horizontal));
        s.layout = Some(SplitLayout::Grid);
        let g = compute_split(AREA, &s);
        assert_eq!(g.layout, SplitLayout::Grid);
        assert_eq!(g.rects.len(), 3);
        assert_eq!(g.rects[0].y, g.rects[1].y);
        assert_eq!(g.rects[2].width, 1000.0);
        assert!(g.rects[2].y > g.rects[0].y);
        assert_eq!(g.dividers.len(), 1);
        let mut two = shape(&["a", "b"]);
        two.layout = Some(SplitLayout::Grid);
        assert_eq!(compute_split(AREA, &two).layout, SplitLayout::Columns);
    }

    #[test]
    fn insert_refusals_and_positions() {
        assert!(!insert_into_split(&mut mk(&["a", "b"], None), "a", DropEdge::Right, Some("a")));
        assert!(!insert_into_split(&mut mk(&["a", "b", "c", "d"], None), "e", DropEdge::Right, Some("a")));
        let mut s = mk(&["a", "b"], None);
        insert_into_split(&mut s, "x", DropEdge::Left, Some("b"));
        assert_eq!(s.tab_ids, ["a", "x", "b"]);
        let mut t = mk(&["a", "b"], None);
        insert_into_split(&mut t, "x", DropEdge::Right, Some("a"));
        assert_eq!(t.tab_ids, ["a", "x", "b"]);
        let mut s = mk(&["a", "b"], None);
        insert_into_split(&mut s, "x", DropEdge::Right, None);
        assert_eq!(s.tab_ids, ["a", "b", "x"]);
        let mut t = mk(&["a", "b"], None);
        insert_into_split(&mut t, "x", DropEdge::Left, Some("inconnu"));
        assert_eq!(t.tab_ids, ["a", "x", "b"]);
    }

    #[test]
    fn insert_layouts() {
        let mut s = mk(&["a", "b"], None);
        insert_into_split(&mut s, "x", DropEdge::Bottom, Some("a"));
        assert_eq!(s.layout, Some(SplitLayout::Grid));
        assert!(s.sizes.is_none());
        let mut t = mk(&["a", "b"], Some(SplitLayout::Rows));
        insert_into_split(&mut t, "x", DropEdge::Right, Some("a"));
        assert_eq!(t.layout, Some(SplitLayout::Grid));
        let mut u = mk(&["a", "b"], None);
        insert_into_split(&mut u, "x", DropEdge::Top, Some("a"));
        assert_eq!(u.layout, Some(SplitLayout::Grid));
        let mut w = mk(&["a", "b"], Some(SplitLayout::Rows));
        insert_into_split(&mut w, "x", DropEdge::Bottom, Some("a"));
        assert_eq!(w.layout, Some(SplitLayout::Rows));
        let mut v = mk(&["a", "b", "c"], None);
        insert_into_split(&mut v, "x", DropEdge::Bottom, Some("a"));
        assert_eq!(v.layout, Some(SplitLayout::Grid));
    }

    #[test]
    fn sanitize() {
        let mut s = SplitShape { layout: Some(SplitLayout::Columns), sizes: Some(vec![1.0]), ..shape(&["a", "b"]) };
        sanitize_split(&mut s);
        assert!(s.layout.is_none() && s.sizes.is_none());
        let mut s = SplitShape { layout: Some(SplitLayout::Grid), ..shape(&["a", "b"]) };
        sanitize_split(&mut s);
        assert!(s.layout.is_none());
        let mut g = SplitShape { layout: Some(SplitLayout::Grid), sizes: Some(vec![0.4, 0.6]), ..shape(&["a", "b", "c"]) };
        let before = g.clone();
        sanitize_split(&mut g);
        assert_eq!(g, before);
    }
}

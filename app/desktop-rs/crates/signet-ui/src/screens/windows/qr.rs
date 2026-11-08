//! QR code drawing. The API hands the code over as an SVG data URL (`PasskeyPromptState.qr`, `LinkPairing.qr`) made by
//! `signet_link::qrimage::QrMatrix::to_svg`: `<svg viewBox="0 0 N N"><rect fill="#fff"/><path d="M{x} {y}h1v1h-1z..."/></svg>`.
//! The modules are read back from the path and painted as squares (crisp at any size, no SVG rendering).

use super::data_url::decode_data_url;
use gpui::{Bounds, IntoElement, Pixels, Styled, canvas, fill, hsla, point, px, size};

/// Modules of a QR code including its quiet zone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QrGrid {
    /// Modules per side, quiet zone included.
    pub total: usize,
    /// Dark modules `(x, y)` in the same coordinates.
    pub dark: Vec<(usize, usize)>,
}

/// Reads a QR SVG data URL (or the SVG text itself). `None` if it is not the expected shape.
pub fn parse_qr_svg(source: &str) -> Option<QrGrid> {
    let svg = if source.starts_with("data:") { String::from_utf8(decode_data_url(source)?.1).ok()? } else { source.to_string() };
    let view_box = svg.split("viewBox=\"").nth(1)?.split('"').next()?;
    let total: usize = view_box.split_whitespace().nth(2)?.parse().ok()?;
    if total == 0 || total > 512 {
        return None;
    }
    let path = svg.split(" d=\"").nth(1)?.split('"').next()?;
    let mut dark = Vec::new();
    for part in path.split('M').filter(|p| !p.trim().is_empty()) {
        // `{x} {y}h1v1h-1z`
        let head = part.split('h').next()?;
        let mut nums = head.split_whitespace();
        let x: usize = nums.next()?.parse().ok()?;
        let y: usize = nums.next()?.parse().ok()?;
        if x >= total || y >= total {
            return None;
        }
        dark.push((x, y));
    }
    Some(QrGrid { total, dark })
}

/// Square element of `side` px drawing the code, black on white whatever the theme.
pub fn qr_element(grid: QrGrid, side: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            window.paint_quad(fill(bounds, hsla(0.0, 0.0, 1.0, 1.0)));
            // Whole pixels per module, centered: no seams between neighbours.
            let cell = (f32::from(bounds.size.width.min(bounds.size.height)) / grid.total as f32).floor().max(1.0);
            let used = cell * grid.total as f32;
            let ox = f32::from(bounds.origin.x) + (f32::from(bounds.size.width) - used) / 2.0;
            let oy = f32::from(bounds.origin.y) + (f32::from(bounds.size.height) - used) / 2.0;
            for (x, y) in &grid.dark {
                let b = Bounds::new(point(px(ox + *x as f32 * cell), px(oy + *y as f32 * cell)), size(px(cell), px(cell)));
                window.paint_quad(fill(b, hsla(0.0, 0.0, 0.0, 1.0)));
            }
        },
    )
    .size(px(side))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::windows::data_url::base64_encode;

    const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 5 5\" shape-rendering=\"crispEdges\"><rect width=\"5\" height=\"5\" fill=\"#fff\"/><path d=\"M2 2h1v1h-1zM3 2h1v1h-1zM2 3h1v1h-1z\" fill=\"#000\"/></svg>";

    #[test]
    fn reads_modules_from_svg_text_and_data_urls() {
        let expected = QrGrid { total: 5, dark: vec![(2, 2), (3, 2), (2, 3)] };
        assert_eq!(parse_qr_svg(SVG), Some(expected.clone()));
        assert_eq!(parse_qr_svg(&format!("data:image/svg+xml;base64,{}", base64_encode(SVG.as_bytes()))), Some(expected));
    }

    #[test]
    fn rejects_other_input() {
        assert!(parse_qr_svg("data:image/png;base64,AAAA").is_none());
        assert!(parse_qr_svg("<svg></svg>").is_none());
        assert!(parse_qr_svg("<svg viewBox=\"0 0 3 3\"><path d=\"M9 9h1v1h-1z\"/></svg>").is_none());
    }
}

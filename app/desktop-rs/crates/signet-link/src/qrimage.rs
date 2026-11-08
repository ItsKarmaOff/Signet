//! QR code as a module matrix the UI can draw, plus an SVG rendering.

use qrcode::{Color, EcLevel, QrCode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrMatrix {
    /// Modules per side (without margin).
    pub size: usize,
    /// Row-major, true = dark.
    pub dark: Vec<bool>,
}

impl QrMatrix {
    /// Error correction level L, like the Electron app.
    pub fn new(text: &str) -> Option<Self> {
        let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::L).ok()?;
        let size = code.width();
        Some(Self { size, dark: code.to_colors().into_iter().map(|c| c == Color::Dark).collect() })
    }

    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.dark[y * self.size + x]
    }

    /// SVG with a quiet zone of `margin` modules (2 in the Electron app). Black on white.
    pub fn to_svg(&self, margin: usize) -> String {
        let total = self.size + margin * 2;
        let mut path = String::new();
        for y in 0..self.size {
            for x in 0..self.size {
                if self.is_dark(x, y) {
                    path.push_str(&format!("M{} {}h1v1h-1z", x + margin, y + margin));
                }
            }
        }
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {total} {total}\" shape-rendering=\"crispEdges\"><rect width=\"{total}\" height=\"{total}\" fill=\"#fff\"/><path d=\"{path}\" fill=\"#000\"/></svg>"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_matrix_and_an_svg() {
        let m = QrMatrix::new("FIDO:/123").unwrap();
        assert_eq!(m.dark.len(), m.size * m.size);
        // Finder pattern: top-left corner module is dark.
        assert!(m.is_dark(0, 0));
        let svg = m.to_svg(2);
        assert!(svg.starts_with("<svg") && svg.contains(&format!("viewBox=\"0 0 {} {}\"", m.size + 4, m.size + 4)));
    }
}

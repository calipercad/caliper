//! Single-stroke text for engraving, pen-plotting, and laser scoring.
//!
//! [`stroke_text`] lays out a string in the built-in single-stroke [`font`] and
//! returns a [`Shape`] whose contours are **open** polylines (one per pen
//! stroke) — exactly what a drag-engraver or plotter wants, and what the SVG/DXF
//! exporters emit as `<path>` / `LWPOLYLINE` with no fill. The result is for
//! render/export only: its open, sub-triangle contours are not meaningful to
//! the filled-region ops (`area`/`regions`/`extrude`), which correctly ignore or
//! reject it.

pub mod font;

use crate::geom2d::{Contour, Shape};
use crate::math::Vec2;

/// Horizontal alignment of the laid-out text relative to the origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// Pen starts at `x = 0` (text extends to +X).
    Left,
    /// Text is centered about `x = 0`.
    Center,
    /// Text ends at `x = 0` (text extends to −X).
    Right,
}

/// Advance used for an unsupported character (so unknown glyphs leave a blank
/// space rather than drawing a tofu box or panicking).
const MISSING_ADVANCE: f32 = 8.0;

/// Lay out `text` at the given cap `height`, returning a [`Shape`] of open
/// stroke contours. The baseline sits at `y = 0`; glyphs rise to `height`
/// (descenders dip below the baseline). Alignment shifts the laid-out *ink* so
/// it starts at (Left), is centered about (Center), or ends at (Right) `x = 0`.
/// An empty string (or one of only unsupported characters / spaces) yields a
/// shape with no contours.
pub fn stroke_text(text: &str, height: f64, align: Align) -> Shape {
    let scale = (height as f32) / font::CAP;

    // First pass: lay out left-to-right with the pen starting at 0.
    let mut pen = 0.0f32;
    let mut contours: Vec<Contour> = Vec::new();
    for c in text.chars() {
        let glyph = match font::glyph(c) {
            Some(g) => g,
            None => {
                pen += MISSING_ADVANCE * scale;
                continue;
            }
        };
        for stroke in glyph.strokes {
            if stroke.len() < 2 {
                continue;
            }
            let pts: Vec<Vec2> = stroke
                .iter()
                .map(|&(x, y)| {
                    Vec2::new((pen + x as f32 * scale) as f64, (y as f32 * scale) as f64)
                })
                .collect();
            contours.push(Contour::new(pts, false));
        }
        pen += glyph.advance * scale;
    }

    // Second pass: shift the ink horizontally per the requested alignment.
    let (mut minx, mut maxx) = (f64::MAX, f64::MIN);
    for c in &contours {
        for p in &c.points {
            minx = minx.min(p.x);
            maxx = maxx.max(p.x);
        }
    }
    if minx <= maxx {
        let dx = match align {
            Align::Left => -minx,
            Align::Center => -(minx + maxx) / 2.0,
            Align::Right => -maxx,
        };
        if dx != 0.0 {
            for c in &mut contours {
                for p in &mut c.points {
                    p.x += dx;
                }
            }
        }
    }
    Shape::new(contours)
}

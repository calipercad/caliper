// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Minimal PDF 1.4 vector writer. Emits a single-page document whose content
//! stream draws each contour as a stroked path, so the drawing opens 1:1 in any
//! PDF viewer and prints at true scale. The page is sized from the geometry
//! bbox (plus a small margin) in PostScript **points** (`pt = mm * 72/25.4`);
//! PDF's origin is bottom-left with +Y up, which already matches CAD, so no
//! flip is needed (unlike the SVG writer).
//!
//! Paths are stroked with the conventional color for their entity `role`
//! (cut=black, score=blue, engrave=red), the same convention the SVG/DXF
//! writers use, so a single PDF can proof a multi-operation cutter job.
//!
//! Hand-rolled: PDF is a plain byte format, so the file is assembled as ASCII
//! text with a correct `xref` table of exact byte offsets — no dependencies.

use crate::bbox::Aabb2;
use crate::export::{role_color, Item};

/// Millimetres → PostScript points (1 pt = 1/72 inch).
const MM_TO_PT: f64 = 72.0 / 25.4;

pub fn to_pdf(items: &[Item], mm: f64) -> String {
    // Combined bounds (in scene units), with a small margin — same framing as
    // the SVG writer.
    let mut b = Aabb2::empty();
    for it in items {
        let sb = it.shape.bbox();
        if !sb.is_empty() {
            b.expand(sb.min);
            b.expand(sb.max);
        }
    }
    if b.is_empty() {
        b = Aabb2 {
            min: crate::math::Vec2::ZERO,
            max: crate::math::Vec2::new(1.0, 1.0),
        };
    }
    let margin = 5.0 / mm; // ~5mm margin in scene units
    let minx = b.min.x - margin;
    let miny = b.min.y - margin;
    let w = b.width() + 2.0 * margin;
    let h = b.height() + 2.0 * margin;

    // Page size in points.
    let wpt = w * mm * MM_TO_PT;
    let hpt = h * mm * MM_TO_PT;

    // Scene point → page point. Origin bottom-left, +Y up (matches CAD).
    let map = |p: crate::math::Vec2| -> (f64, f64) {
        let x = (p.x - minx) * mm * MM_TO_PT;
        let y = (p.y - miny) * mm * MM_TO_PT;
        (x, y)
    };

    // --- Content stream: one stroked path per contour -----------------------
    let mut content = String::new();
    content.push_str(&format!("{:.3} w\n", 0.2 * MM_TO_PT)); // ~0.2mm hairline
    for it in items {
        let (r, g, bl) = role_rgb(it.role);
        content.push_str(&format!("{r:.3} {g:.3} {bl:.3} RG\n"));
        for c in &it.shape.contours {
            if c.points.is_empty() {
                continue;
            }
            // Every contour (circle/arc/polyline) is already tessellated to
            // `points`; draw it as a moveto + lineto path. PDF has no native
            // circle, so the tessellation is the path.
            for (i, &p) in c.points.iter().enumerate() {
                let (x, y) = map(p);
                content.push_str(&format!(
                    "{x:.3} {y:.3} {}\n",
                    if i == 0 { "m" } else { "l" }
                ));
            }
            if c.closed {
                content.push_str("h\n"); // closepath
            }
            content.push_str("S\n"); // stroke
        }
    }

    // --- Assemble objects, tracking exact byte offsets for the xref ---------
    // All bytes written are ASCII, so `String::len()` is the byte offset.
    let mut out = String::new();
    out.push_str("%PDF-1.4\n");

    let mut offsets: Vec<usize> = Vec::new();

    offsets.push(out.len());
    out.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets.push(out.len());
    out.push_str("2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

    offsets.push(out.len());
    out.push_str(&format!(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {wpt:.3} {hpt:.3}] \
         /Contents 4 0 R /Resources << >> >>\nendobj\n"
    ));

    offsets.push(out.len());
    out.push_str(&format!(
        "4 0 obj\n<< /Length {} >>\nstream\n{content}endstream\nendobj\n",
        content.len()
    ));

    // --- xref table: free object 0, then one `n` entry per object -----------
    let xref_off = out.len();
    let size = offsets.len() + 1; // objects + the free object 0
    out.push_str(&format!("xref\n0 {size}\n"));
    out.push_str("0000000000 65535 f \n");
    for off in &offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }

    // --- trailer ------------------------------------------------------------
    out.push_str(&format!(
        "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_off}\n%%EOF\n"
    ));
    out
}

/// Role stroke color as normalized RGB, derived from the shared [`role_color`]
/// hex so PDF stays consistent with the SVG/DXF writers.
fn role_rgb(role: Option<&str>) -> (f64, f64, f64) {
    let (hex, _) = role_color(role);
    let h = hex.trim_start_matches('#');
    let comp = |a: usize, b: usize| u8::from_str_radix(&h[a..b], 16).unwrap_or(0) as f64 / 255.0;
    (comp(0, 2), comp(2, 4), comp(4, 6))
}

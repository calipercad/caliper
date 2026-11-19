// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 21 integration tests (roadmap v1.11): PDF vector export. Driven
//! through the interpreter, mirroring the SVG/DXF export tests.

use cad_core::Interpreter;

/// mm → PostScript points, matching the writer.
const MM_TO_PT: f64 = 72.0 / 25.4;

/// Pull the two `/MediaBox [0 0 W H]` numbers out of a PDF.
fn media_box(pdf: &str) -> (f64, f64) {
    let start = pdf.find("/MediaBox [0 0 ").expect("MediaBox present") + "/MediaBox [0 0 ".len();
    let rest = &pdf[start..];
    let end = rest.find(']').expect("MediaBox closes");
    let nums: Vec<f64> = rest[..end]
        .split_whitespace()
        .map(|t| t.parse().expect("MediaBox number"))
        .collect();
    (nums[0], nums[1])
}

#[test]
fn pdf_structure_and_mediabox() {
    let mut it = Interpreter::new();
    // Default units are mm, so scene units map 1:1 to mm.
    it.exec_line("rect 60 40 name=plate").unwrap();

    let path = std::env::temp_dir().join("cad_phase21_plate.pdf");
    it.exec_line(&format!("export pdf {}", path.display()))
        .unwrap();
    let pdf = std::fs::read_to_string(&path).unwrap();

    // Valid shell: header, EOF, xref + trailer.
    assert!(pdf.starts_with("%PDF-"), "starts with the PDF header");
    assert!(pdf.contains("%%EOF"), "ends with the EOF marker");
    assert!(pdf.contains("\nxref\n"), "has an xref table");
    assert!(pdf.contains("/Root 1 0 R"), "trailer points at the catalog");
    assert!(pdf.contains("startxref"), "has a startxref pointer");

    // MediaBox = bbox (60×40mm) + 5mm margin each side = 70×50mm, in points.
    let (wpt, hpt) = media_box(&pdf);
    let eps = 1e-3;
    assert!(
        (wpt - 70.0 * MM_TO_PT).abs() < eps,
        "page width {wpt} ≈ 70mm in points"
    );
    assert!(
        (hpt - 50.0 * MM_TO_PT).abs() < eps,
        "page height {hpt} ≈ 50mm in points"
    );

    // Vector path operators are present.
    assert!(pdf.contains(" m\n"), "has a moveto");
    assert!(pdf.contains(" l\n"), "has a lineto");
    assert!(pdf.contains("S\n"), "strokes the path");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn pdf_rejects_3d_mesh() {
    let mut it = Interpreter::new();
    it.exec_line("box 30 30 30 name=bx").unwrap();

    let path = std::env::temp_dir().join("cad_phase21_mesh.pdf");
    let r = it.exec_line(&format!("export pdf {} name=bx", path.display()));
    assert!(r.is_err(), "PDF export of a 3D mesh must error");
    let _ = std::fs::remove_file(&path);
}

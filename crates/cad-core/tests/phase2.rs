// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 2 integration tests (blueprint/roadmap v0.3): single-stroke text and
//! layered, role-colored SVG/DXF export. Driven through the public API.

use cad_core::export::{dxf, svg, Item};
use cad_core::geom2d::text::{stroke_text, Align};
use cad_core::geom2d::Shape;
use cad_core::scene::Geometry;
use cad_core::Interpreter;

// ---- stroke text -----------------------------------------------------------

#[test]
fn stroke_text_basic() {
    let s = stroke_text("HELLO", 10.0, Align::Left);
    assert!(!s.contours.is_empty(), "text produced strokes");
    // Stroke text is open polylines (no fills).
    assert!(s.contours.iter().all(|c| !c.closed), "all contours open");

    let b = s.bbox();
    // Cap-height uppercase spans ~0..height.
    assert!(
        (b.height() - 10.0).abs() < 0.6,
        "cap height ≈ 10, got {}",
        b.height()
    );
    assert!(b.width() > 10.0, "has horizontal extent, got {}", b.width());
}

#[test]
fn stroke_text_align_center() {
    let s = stroke_text("HI", 10.0, Align::Center);
    let b = s.bbox();
    // Centered text straddles x = 0 symmetrically.
    assert!(
        (b.min.x + b.max.x).abs() < 1e-6,
        "centered about 0: minx {} maxx {}",
        b.min.x,
        b.max.x
    );
}

#[test]
fn text_command_sets_role() {
    let mut it = Interpreter::new();
    it.exec_line("text \"HI\" h=12 name=t role=engrave")
        .unwrap();
    let e = it.scene.get("t").expect("entity t");
    assert!(matches!(e.geom, Geometry::Shape(_)), "text is a 2D shape");
    assert_eq!(e.role.as_deref(), Some("engrave"));
}

// ---- layer / role attributes + save round-trip -----------------------------

#[test]
fn layer_role_roundtrip() {
    let mut it = Interpreter::new();
    it.exec_line("rect 10 10 name=r").unwrap();
    it.exec_line("layer cuts target=r").unwrap();
    it.exec_line("role score target=r").unwrap();

    let e = it.scene.get("r").unwrap();
    assert_eq!(e.layer.as_deref(), Some("cuts"));
    assert_eq!(e.role.as_deref(), Some("score"));

    // save replays recorded commands verbatim, so the attributes persist.
    let path = std::env::temp_dir().join("cad_phase2_roundtrip.cad");
    it.exec_line(&format!("save {}", path.display())).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("layer cuts target=r"), "layer line saved");
    assert!(saved.contains("role score target=r"), "role line saved");
    let _ = std::fs::remove_file(&path);
}

// ---- layered export --------------------------------------------------------

#[test]
fn svg_layered() {
    let badge = Shape::rect(80.0, 24.0, 0.0, 0);
    let label = stroke_text("MAKER", 12.0, Align::Left);
    let items = vec![
        Item {
            name: "badge",
            shape: &badge,
            layer: None,
            role: Some("cut"),
        },
        Item {
            name: "label",
            shape: &label,
            layer: None,
            role: Some("engrave"),
        },
    ];
    let out = svg::to_svg(&items, 1.0);

    // Two Inkscape layers, keyed by role, with distinct stroke colors.
    assert!(out.contains("inkscape:label=\"cut\""), "cut layer");
    assert!(out.contains("inkscape:label=\"engrave\""), "engrave layer");
    assert!(out.contains("stroke=\"#000000\""), "cut is black");
    assert!(out.contains("stroke=\"#ff0000\""), "engrave is red");
    assert!(out.contains("<path"), "text/outline emitted as paths");
}

#[test]
fn dxf_layers() {
    let badge = Shape::rect(80.0, 24.0, 0.0, 0);
    let label = stroke_text("MAKER", 12.0, Align::Left);
    let items = vec![
        Item {
            name: "badge",
            shape: &badge,
            layer: None,
            role: Some("cut"),
        },
        Item {
            name: "label",
            shape: &label,
            layer: None,
            role: Some("engrave"),
        },
    ];
    let out = dxf::to_dxf(&items, 1.0);

    // A LAYER table declares both named layers...
    assert!(out.contains("\nLAYER\n"), "LAYER table present");
    assert!(out.contains("\ncut\n"), "cut layer declared");
    assert!(out.contains("\nengrave\n"), "engrave layer declared");
    // ...and entities reference them (group code 8). Engrave color = ACI 1.
    assert!(out.contains("\n62\n1\n"), "engrave color ACI 1");
    assert!(out.contains("LWPOLYLINE"), "geometry emitted");
}

#[test]
fn default_layer_when_unset() {
    // No layer/role ⇒ everything lands on the default layer "0".
    let sq = Shape::rect(10.0, 10.0, 0.0, 0);
    let out = dxf::to_dxf(&[Item::new("s", &sq)], 1.0);
    assert!(out.contains("\n0\n"), "default layer 0 referenced");
}

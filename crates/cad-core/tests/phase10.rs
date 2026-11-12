// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 10 integration tests (roadmap v1.2): 2D offset / inset driven through
//! the interpreter. The offset algorithm's own unit tests live in
//! `src/geom2d/offset.rs`; these exercise the `offset`/`inset` commands,
//! target resolution, `name=` (new entity vs in-place), and holes.

use cad_core::scene::Geometry;
use cad_core::Interpreter;

fn shape_area(it: &Interpreter, name: &str) -> f64 {
    match &it.scene.get(name).unwrap().geom {
        Geometry::Shape(s) => s.area(),
        _ => panic!("{name} is not a shape"),
    }
}

fn shape_bbox(it: &Interpreter, name: &str) -> (f64, f64) {
    match &it.scene.get(name).unwrap().geom {
        Geometry::Shape(s) => {
            let b = s.bbox();
            (b.width(), b.height())
        }
        _ => panic!("{name} is not a shape"),
    }
}

#[test]
fn offset_grows_in_place() {
    // CHECKPOINT: a 40mm square grown by 5mm → 50×50 bbox (rounded corners).
    let mut it = Interpreter::new();
    it.exec_line("rect 40 40 name=plate").unwrap();
    it.exec_line("offset 5 target=plate").unwrap();
    let (w, h) = shape_bbox(&it, "plate");
    assert!((w - 50.0).abs() < 0.2 && (h - 50.0).abs() < 0.2, "{w}×{h}");
}

#[test]
fn inset_shrinks_in_place() {
    let mut it = Interpreter::new();
    it.exec_line("rect 40 40 name=plate").unwrap();
    it.exec_line("inset 5").unwrap(); // acts on current selection
    let (w, h) = shape_bbox(&it, "plate");
    assert!((w - 30.0).abs() < 0.2 && (h - 30.0).abs() < 0.2, "{w}×{h}");
}

#[test]
fn offset_with_name_keeps_original() {
    // name= adds a separate entity (e.g. a kerf/tool-path copy).
    let mut it = Interpreter::new();
    it.exec_line("rect 40 40 name=plate").unwrap();
    it.exec_line("offset 2 target=plate name=kerf").unwrap();
    assert!(
        (shape_bbox(&it, "plate").0 - 40.0).abs() < 1e-6,
        "original intact"
    );
    assert!((shape_bbox(&it, "kerf").0 - 44.0).abs() < 0.2, "kerf grown");
}

#[test]
fn offset_shape_with_hole() {
    // Outward offset grows the outer boundary and shrinks the hole.
    let mut it = Interpreter::new();
    it.exec_line("rect 40 40 name=plate").unwrap();
    it.exec_line("circle 8 name=bore").unwrap();
    it.exec_line("translate 20 20 target=bore").unwrap();
    it.exec_line("subtract plate bore name=part").unwrap();
    let before = shape_area(&it, "part");
    it.exec_line("offset 3 target=part").unwrap();
    let after = shape_area(&it, "part");
    // Outer grows (+area) and hole shrinks (+area), so net area increases.
    assert!(after > before, "area grew {before} → {after}");
    // Still has a hole (two contours after normalize).
    match &it.scene.get("part").unwrap().geom {
        Geometry::Shape(s) => assert!(s.contours.len() >= 2, "hole preserved"),
        _ => panic!(),
    }
}

#[test]
fn inset_too_large_errors() {
    let mut it = Interpreter::new();
    it.exec_line("rect 10 10 name=s").unwrap();
    assert!(
        it.exec_line("inset 6").is_err(),
        "removing all material errors"
    );
}

#[test]
fn offset_rejects_mesh() {
    let mut it = Interpreter::new();
    it.exec_line("box 10 10 10 name=b").unwrap();
    assert!(it.exec_line("offset 1 target=b").is_err());
}

#[test]
fn offset_roundtrips_through_save() {
    // The command records to history, so it survives save/replay.
    let mut it = Interpreter::new();
    it.exec_line("rect 40 40 name=plate").unwrap();
    it.exec_line("offset 5 join=bevel target=plate").unwrap();
    let area = shape_area(&it, "plate");

    let p = std::env::temp_dir().join("cad_p10_offset.cad");
    it.exec_line(&format!("save {}", p.display())).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("offset 5 join=bevel"), "recorded:\n{text}");

    let mut it2 = Interpreter::new();
    it2.exec_line(&format!("load {}", p.display())).unwrap();
    assert!(
        (shape_area(&it2, "plate") - area).abs() < 1e-6,
        "replay matches"
    );
}

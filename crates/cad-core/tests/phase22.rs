// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 22 integration tests: the joinery & fixture skills (`shelf_pin`,
//! `drawer`, `tslot_joint`). Each is driven end-to-end through the interpreter
//! and checked for non-empty, sensibly-dimensioned, closed-contour geometry.
//! (The broad `all_skills_run_at_defaults` check in `phase5.rs` also covers
//! them automatically once they are registered.)

use cad_core::scene::Geometry;
use cad_core::Interpreter;

/// Fetch a shape's contour count for the named entity.
fn contours(it: &Interpreter, name: &str) -> usize {
    match &it
        .scene
        .get(name)
        .unwrap_or_else(|| panic!("no entity {name}"))
        .geom
    {
        Geometry::Shape(s) => s.contours.len(),
        _ => panic!("{name} is not a 2D shape"),
    }
}

/// Fetch a shape's bbox (width, height) for the named entity.
fn bbox(it: &Interpreter, name: &str) -> (f64, f64) {
    match &it.scene.get(name).unwrap().geom {
        Geometry::Shape(s) => {
            let b = s.bbox();
            (b.width(), b.height())
        }
        _ => panic!("{name} is not a 2D shape"),
    }
}

#[test]
fn shelf_pin_strip_has_outline_plus_one_hole_per_pin() {
    // Default: one strip outline + 6 hole contours = 7.
    let mut it = Interpreter::new();
    it.exec_line("skill shelf_pin name=jig").unwrap();
    assert_eq!(contours(&it, "jig"), 7, "outline + 6 holes");

    // Strip spans 2*margin + (count-1)*spacing = 24 + 5*32 = 184mm, tall 24mm.
    let (w, h) = bbox(&it, "jig");
    assert!((w - 184.0).abs() < 1e-6, "strip length {w}");
    assert!((h - 24.0).abs() < 1e-6, "strip height {h}");
}

#[test]
fn shelf_pin_hole_count_follows_count_param() {
    let holes = |n: usize| -> usize {
        let mut it = Interpreter::new();
        it.exec_line(&format!("skill shelf_pin count={n} name=j"))
            .unwrap();
        contours(&it, "j") - 1 // minus the outline
    };
    assert_eq!(holes(10), 10);
    assert!(holes(12) > holes(4), "more pins → more holes");
}

#[test]
fn drawer_emits_five_cut_panels_and_an_engrave_label() {
    let mut it = Interpreter::new();
    it.exec_line("skill drawer width=180 depth=120 height=60 name=d")
        .unwrap();

    // Five butt-jointed panel outlines on the cut layer.
    assert_eq!(contours(&it, "d_cut"), 5, "front/back/left/right/base");

    // The label entity carries the engrave role and some stroke geometry.
    let label = it.scene.get("d_label").expect("label entity");
    assert_eq!(label.role.as_deref(), Some("engrave"));
    match &label.geom {
        Geometry::Shape(s) => assert!(!s.contours.is_empty(), "labels present"),
        _ => panic!("labels should be a 2D shape"),
    }
}

#[test]
fn tslot_joint_builds_two_mating_panels() {
    let mut it = Interpreter::new();
    it.exec_line("skill tslot_joint name=t").unwrap();

    // Slot panel: rectangle + T-shaped captive-nut cutout.
    assert_eq!(contours(&it, "t_slot"), 2, "outline + T slot");
    // Tab panel: tabbed rectangle + bolt clearance hole.
    assert_eq!(contours(&it, "t_tab"), 2, "outline + bolt hole");

    // Both panels have real area and are placed side by side (positive extent).
    let (sw, sh) = bbox(&it, "t_slot");
    let (tw, th) = bbox(&it, "t_tab");
    assert!(sw > 0.0 && sh > 0.0 && tw > 0.0 && th > 0.0);
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 5 integration tests (roadmap v0.6): the parametric skill library.
//! The headline is `all_skills_run_at_defaults` — every built-in must produce
//! geometry from a bare `skill <name>` invocation.

use cad_core::scene::Geometry;
use cad_core::Interpreter;

/// CHECKPOINT: ≥20 built-in skills, and every one builds non-empty geometry at
/// its defaults (bare `skill <name>`, no arguments).
#[test]
fn all_skills_run_at_defaults() {
    let names: Vec<String> = Interpreter::new()
        .skills
        .iter()
        .map(|s| s.name().to_string())
        .collect();
    assert!(
        names.len() >= 20,
        "expected >=20 built-in skills (5 original + 15 new), got {}: {names:?}",
        names.len()
    );

    for n in &names {
        let mut it = Interpreter::new();
        let before = it.scene.entities.len();
        let r = it.exec_line(&format!("skill {n}"));
        assert!(r.is_ok(), "skill '{n}' failed at defaults: {r:?}");
        assert!(
            it.scene.entities.len() > before,
            "skill '{n}' added no entity"
        );
    }
}

/// CHECKPOINT demo geometry: a gridfinity bin is a closed manifold solid.
#[test]
fn gridfinity_is_manifold() {
    let mut it = Interpreter::new();
    it.exec_line("skill gridfinity units_x=3 units_y=2 name=bin")
        .unwrap();
    match &it.scene.get("bin").expect("bin entity").geom {
        Geometry::Mesh(m) => assert!(m.is_closed_manifold(), "gridfinity bin is manifold"),
        _ => panic!("gridfinity should produce a mesh"),
    }
}

// ---- targeted shape checks -------------------------------------------------

#[test]
fn flange_has_bore_and_bolt_holes() {
    // default: 1 outer disc + central bore + 4 bolt holes = 6 contours.
    let mut it = Interpreter::new();
    it.exec_line("skill flange name=f").unwrap();
    match &it.scene.get("f").unwrap().geom {
        Geometry::Shape(s) => assert_eq!(s.contours.len(), 6, "disc + bore + 4 bolts"),
        _ => panic!("flange with no thickness is a 2D shape"),
    }
}

#[test]
fn card_emits_a_score_layer() {
    let mut it = Interpreter::new();
    it.exec_line("skill card").unwrap();
    assert!(
        it.scene
            .entities
            .iter()
            .any(|e| e.role.as_deref() == Some("score")),
        "card emits a score-role fold line"
    );
}

#[test]
fn living_hinge_slots_scale_with_width() {
    let slots = |w: f64| -> usize {
        let mut it = Interpreter::new();
        it.exec_line(&format!("skill living_hinge width={w} name=h"))
            .unwrap();
        match &it.scene.get("h").unwrap().geom {
            Geometry::Shape(s) => s.contours.len(),
            _ => 0,
        }
    };
    assert!(
        slots(120.0) > slots(40.0),
        "a wider panel has more flex slots"
    );
}

#[test]
fn mechanical_solids_extrude() {
    // With a thickness, a mechanical profile becomes a 3D mesh.
    let mut it = Interpreter::new();
    it.exec_line("skill flange thickness=4 name=fsolid")
        .unwrap();
    assert!(matches!(
        it.scene.get("fsolid").unwrap().geom,
        Geometry::Mesh(_)
    ));
}

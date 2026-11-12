// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 7 integration tests (roadmap v0.8): import round-trips, undo/redo,
//! binary STL, and G2/G3 arc G-code. Driven through the interpreter + temp files.

use cad_core::scene::Geometry;
use cad_core::Interpreter;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cad_p7_{name}"))
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

// ---- import round-trips ----------------------------------------------------

#[test]
fn svg_roundtrip() {
    // CHECKPOINT: export an SVG, import it, geometry (bbox size) is preserved.
    let p = tmp("rt.svg");
    let mut it = Interpreter::new();
    it.exec_line("rect 30 20 name=r").unwrap(); // a <path> → 30×20 bbox
    it.exec_line(&format!("export svg {} name=r", p.display()))
        .unwrap();
    it.exec_line("clear").unwrap();
    it.exec_line(&format!("import {}", p.display())).unwrap();

    let name = p.file_stem().unwrap().to_str().unwrap();
    let (w, h) = shape_bbox(&it, name);
    assert!(
        (w - 30.0).abs() < 0.5 && (h - 20.0).abs() < 0.5,
        "svg bbox {w}×{h} ≈ 30×20"
    );
    let _ = std::fs::remove_file(&p);
}

#[test]
fn dxf_roundtrip_keeps_holes() {
    // DXF is the exact-coordinate path: a plate with a hole re-imports as a
    // shape with both contours and the same bbox size.
    let p = tmp("rt.dxf");
    let mut it = Interpreter::new();
    it.exec_line("rect 40 40 name=plate").unwrap();
    it.exec_line("circle 8 name=bore").unwrap();
    it.exec_line("translate 20 20 target=bore").unwrap(); // center the hole
    it.exec_line("subtract plate bore name=part").unwrap();
    it.exec_line(&format!("export dxf {} name=part", p.display()))
        .unwrap();
    it.exec_line("clear").unwrap();
    it.exec_line(&format!("import {}", p.display())).unwrap();

    let name = p.file_stem().unwrap().to_str().unwrap();
    match &it.scene.get(name).unwrap().geom {
        Geometry::Shape(s) => assert!(s.contours.len() >= 2, "outer + hole preserved"),
        _ => panic!("dxf imports as a shape"),
    }
    let (w, h) = shape_bbox(&it, name);
    assert!((w - 40.0).abs() < 0.5 && (h - 40.0).abs() < 0.5);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn stl_roundtrip_ascii_and_binary() {
    for (ext, flag) in [("ascii.stl", ""), ("bin.stl", "binary")] {
        let p = tmp(ext);
        let mut it = Interpreter::new();
        it.exec_line("box 20 20 20 name=b").unwrap();
        it.exec_line(&format!("export stl {} name=b {flag}", p.display()))
            .unwrap();
        it.exec_line("clear").unwrap();
        it.exec_line(&format!("import {}", p.display())).unwrap();
        match &it
            .scene
            .get(p.file_stem().unwrap().to_str().unwrap())
            .unwrap()
            .geom
        {
            Geometry::Mesh(m) => {
                assert_eq!(m.tris.len(), 12, "cube has 12 triangles ({ext})");
                assert!(
                    m.is_closed_manifold(),
                    "re-imported cube is manifold ({ext})"
                );
            }
            _ => panic!("stl imports as a mesh"),
        }
        let _ = std::fs::remove_file(&p);
    }
}

#[test]
fn binary_stl_has_correct_size() {
    let p = tmp("sized.stl");
    let mut it = Interpreter::new();
    it.exec_line("box 10 10 10 name=b").unwrap();
    it.exec_line(&format!("export stl {} name=b binary", p.display()))
        .unwrap();
    let bytes = std::fs::read(&p).unwrap();
    assert_eq!(
        bytes.len(),
        84 + 50 * 12,
        "80 header + u32 + 50·12 triangles"
    );
    let _ = std::fs::remove_file(&p);
}

// ---- undo / redo -----------------------------------------------------------

#[test]
fn undo_redo_restores_scene() {
    // CHECKPOINT: undo returns the scene to its previous state; redo reapplies.
    let mut it = Interpreter::new();
    it.exec_line("rect 10 10").unwrap();
    it.exec_line("circle 5").unwrap();
    assert_eq!(it.scene.entities.len(), 2);

    it.exec_line("undo").unwrap();
    assert_eq!(it.scene.entities.len(), 1, "undo removed the circle");
    it.exec_line("redo").unwrap();
    assert_eq!(it.scene.entities.len(), 2, "redo restored it");

    assert!(it.exec_line("undo").is_ok());
    assert!(it.exec_line("undo").is_ok());
    assert!(it.exec_line("undo").is_err(), "nothing left to undo");
}

// ---- G-code arcs -----------------------------------------------------------

#[test]
fn gcode_emits_arcs_for_circles() {
    let p = tmp("arc.gcode");
    let mut it = Interpreter::new();
    it.exec_line("circle 10 name=c").unwrap();
    it.exec_line(&format!("export gcode {} name=c", p.display()))
        .unwrap();
    let g = std::fs::read_to_string(&p).unwrap();
    assert!(
        g.contains("G2") || g.contains("G3"),
        "a circle becomes an arc move"
    );
    let _ = std::fs::remove_file(&p);
}

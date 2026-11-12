// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 4 integration tests (roadmap v0.5): layout ops (array/mirror/group),
//! view/HLR/shade rendering, and dimensioning. Driven through the interpreter.

use cad_core::scene::Geometry;
use cad_core::Interpreter;

/// Total lit braille dots in a render string (0x2800 base + popcount of bits).
fn braille_dots(s: &str) -> usize {
    s.chars()
        .filter_map(|c| {
            let u = c as u32;
            (0x2800..=0x28FF)
                .contains(&u)
                .then(|| (u - 0x2800).count_ones() as usize)
        })
        .sum()
}

// ---- layout ops ------------------------------------------------------------

#[test]
fn array_polar_gear() {
    // CHECKPOINT: a single tooth offset to the rim, arrayed polar ×12, closes a
    // full ring centered on the origin.
    let mut it = Interpreter::new();
    it.exec_line("polygon 3 5 name=tooth").unwrap();
    it.exec_line("translate 20 0 target=tooth").unwrap();
    it.exec_line("array 12 kind=polar name=gear").unwrap();

    let members = it
        .scene
        .entities
        .iter()
        .filter(|e| e.group.as_deref() == Some("gear"))
        .count();
    assert_eq!(members, 12, "12 teeth in the gear group");

    let b = it.scene.bbox2();
    assert!(b.min.x + b.max.x < 1.0, "ring centered in x");
    assert!(b.min.y + b.max.y < 1.0, "ring centered in y");
}

#[test]
fn array_linear_and_grid() {
    let mut it = Interpreter::new();
    it.exec_line("rect 10 10 name=r").unwrap();
    it.exec_line("array 4 dx=20 target=r").unwrap();
    assert_eq!(it.scene.entities.len(), 4, "original + 3 copies");
    assert!(
        it.scene.bbox2().max.x > 60.0,
        "last copy offset to x≈60..70"
    );

    let mut it2 = Interpreter::new();
    it2.exec_line("rect 10 10 name=r").unwrap();
    it2.exec_line("array kind=grid nx=3 ny=2 dx=20 dy=20 target=r")
        .unwrap();
    assert_eq!(it2.scene.entities.len(), 6, "3×2 grid");
}

#[test]
fn mirror_symmetry() {
    let mut it = Interpreter::new();
    it.exec_line("rect 10 10 name=r").unwrap(); // x in [0,10]
    it.exec_line("mirror axis=x about=origin target=r").unwrap();
    let b = it.scene.bbox2();
    assert!(b.max.x <= 1e-6, "reflected to x ≤ 0 (got max {})", b.max.x);
    assert!((b.min.x + 10.0).abs() < 1e-6, "min x ≈ -10");
}

#[test]
fn group_moves_together() {
    let mut it = Interpreter::new();
    it.exec_line("rect 10 10 name=a").unwrap();
    it.exec_line("rect 10 10 name=b").unwrap();
    it.exec_line("translate 50 0 target=b").unwrap();
    it.exec_line("group a b name=g").unwrap();
    it.exec_line("translate 0 100 target=g").unwrap();
    for n in ["a", "b"] {
        if let Geometry::Shape(s) = &it.scene.get(n).unwrap().geom {
            assert!((s.bbox().min.y - 100.0).abs() < 1e-6, "{n} moved up 100");
        }
    }
}

// ---- rendering -------------------------------------------------------------

#[test]
fn hlr_culls_hidden_edges() {
    let mut it = Interpreter::new();
    it.exec_line("box 30 30 30 name=bx").unwrap();
    let wire = it.exec_line("render bx mode=wire axes=off").unwrap();
    let hlr = it.exec_line("render bx mode=hlr axes=off").unwrap();
    assert!(braille_dots(&hlr) > 0, "hlr renders something");
    assert!(
        braille_dots(&hlr) <= braille_dots(&wire),
        "hlr ({}) culls edges vs wire ({})",
        braille_dots(&hlr),
        braille_dots(&wire)
    );
}

#[test]
fn shade_emits_half_blocks() {
    let mut it = Interpreter::new();
    it.exec_line("box 30 30 30 name=bx").unwrap();
    let s = it.exec_line("render bx mode=shade").unwrap();
    assert!(s.contains('\u{2580}'), "half-block glyph present");
    assert!(s.contains("\x1b["), "ANSI color present");
}

#[test]
fn layout_views_tiles_four() {
    let mut it = Interpreter::new();
    it.exec_line("box 30 30 30 name=bx").unwrap();
    let v = it.exec_line("render bx layout=views").unwrap();
    for label in ["top", "front", "right", "iso"] {
        assert!(v.contains(label), "viewport '{label}' present");
    }
    assert!(braille_dots(&v) > 0);
}

// ---- dimensioning ----------------------------------------------------------

#[test]
fn dim_creates_annotation_sheet() {
    let mut it = Interpreter::new();
    it.exec_line("rect 60 40 name=plate").unwrap();
    it.exec_line("dim plate").unwrap();

    let e = it.scene.get("plate_dim").expect("dim entity");
    assert_eq!(e.role.as_deref(), Some("dim"));
    assert!(matches!(e.geom, Geometry::Shape(_)));

    let path = std::env::temp_dir().join("cad_phase4_sheet.svg");
    it.exec_line(&format!("export svg {}", path.display()))
        .unwrap();
    let svg = std::fs::read_to_string(&path).unwrap();
    assert!(svg.contains("inkscape:label=\"dim\""), "dim layer in SVG");
    assert!(svg.contains("<path"), "dimension strokes exported");
    let _ = std::fs::remove_file(&path);
}

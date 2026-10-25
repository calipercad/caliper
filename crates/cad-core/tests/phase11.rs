//! Phase 11 integration tests (roadmap v1.2): measurement & analysis —
//! `measure` (mass properties), `interfere` (clash detection), and `section`
//! (mesh cross-section). Library helpers (`Shape::centroid`/`perimeter`,
//! `Mesh::surface_area`/`centroid`, `geom3d::section`) are exercised here
//! end-to-end through the interpreter.

use cad_core::geom2d::Shape;
use cad_core::geom3d::section::{section, Axis};
use cad_core::geom3d::Mesh;
use cad_core::math::Vec2;
use cad_core::scene::Geometry;
use cad_core::Interpreter;

// ---- library helpers -------------------------------------------------------

#[test]
fn shape_centroid_and_perimeter() {
    // A 10×10 square centered via its corners at (0,0)-(10,10).
    let s = Shape::polyline(
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
        ],
        true,
    );
    assert!((s.area() - 100.0).abs() < 1e-9);
    assert!((s.perimeter() - 40.0).abs() < 1e-9);
    let c = s.centroid();
    assert!(
        (c.x - 5.0).abs() < 1e-9 && (c.y - 5.0).abs() < 1e-9,
        "{c:?}"
    );
}

#[test]
fn centroid_ignores_symmetric_hole() {
    // Square with a centered circular hole → centroid stays at the center.
    let mut it = Interpreter::new();
    it.exec_line("rect 40 40 name=plate").unwrap();
    it.exec_line("circle 8 name=bore").unwrap();
    it.exec_line("translate 20 20 target=bore").unwrap();
    it.exec_line("subtract plate bore name=part").unwrap();
    if let Geometry::Shape(s) = &it.scene.get("part").unwrap().geom {
        let c = s.centroid();
        assert!(
            (c.x - 20.0).abs() < 0.1 && (c.y - 20.0).abs() < 0.1,
            "{c:?}"
        );
    } else {
        panic!();
    }
}

#[test]
fn mesh_volume_surface_centroid() {
    // A 20×20×20 cube centered at the origin.
    let mut m = Mesh::cuboid(20.0, 20.0, 20.0);
    m.center_origin();
    assert!(
        (m.signed_volume() - 8000.0).abs() < 1.0,
        "vol {}",
        m.signed_volume()
    );
    assert!(
        (m.surface_area() - 2400.0).abs() < 1.0,
        "area {}",
        m.surface_area()
    );
    let c = m.centroid();
    assert!(
        c.x.abs() < 1e-6 && c.y.abs() < 1e-6 && c.z.abs() < 1e-6,
        "{c:?}"
    );
}

// ---- commands --------------------------------------------------------------

#[test]
fn measure_reports_2d_and_3d() {
    let mut it = Interpreter::new();
    it.exec_line("rect 40 20 name=r").unwrap();
    let m2 = it.exec_line("measure r").unwrap();
    assert!(m2.contains("area") && m2.contains("perimeter"), "{m2}");

    it.exec_line("box 10 10 10 name=b").unwrap();
    let m3 = it.exec_line("measure b").unwrap();
    assert!(m3.contains("volume") && m3.contains("surface"), "{m3}");
}

#[test]
fn interfere_detects_and_clears() {
    let mut it = Interpreter::new();
    it.exec_line("rect 20 20 name=a").unwrap();
    it.exec_line("rect 20 20 name=b").unwrap();
    it.exec_line("translate 10 0 target=b").unwrap(); // overlaps a by 10×20
    let clash = it.exec_line("interfere a b").unwrap();
    assert!(clash.contains("overlap"), "{clash}");

    it.exec_line("translate 100 0 target=b").unwrap(); // now far away
    let clear = it.exec_line("interfere a b").unwrap();
    assert!(clear.contains("clear"), "{clear}");
}

#[test]
fn interfere_scan_all_pairs() {
    let mut it = Interpreter::new();
    it.exec_line("rect 20 20 name=a").unwrap();
    it.exec_line("rect 20 20 name=b").unwrap();
    it.exec_line("translate 5 5 target=b").unwrap();
    let out = it.exec_line("interfere").unwrap();
    assert!(out.contains("clash"), "{out}");
}

#[test]
fn section_of_cuboid_is_a_rectangle() {
    // Slice a 20×30×40 box at mid-height → a 20×30 rectangle outline.
    let mut m = Mesh::cuboid(20.0, 30.0, 40.0);
    m.center_origin(); // spans ±10, ±15, ±20
    let s = section(&m, Axis::Z, 0.0).unwrap();
    let b = s.bbox();
    assert!(
        (b.width() - 20.0).abs() < 0.1 && (b.height() - 30.0).abs() < 0.1,
        "{}×{}",
        b.width(),
        b.height()
    );
    assert!((s.area() - 600.0).abs() < 1.0, "area {}", s.area());
}

#[test]
fn section_command_creates_entity() {
    let mut it = Interpreter::new();
    it.exec_line("box 20 20 20 name=blk").unwrap();
    let msg = it.exec_line("section blk axis=z at=10 name=slice").unwrap();
    assert!(msg.contains("section"), "{msg}");
    match &it.scene.get("slice").unwrap().geom {
        Geometry::Shape(s) => assert!((s.area() - 400.0).abs() < 1.0, "area {}", s.area()),
        _ => panic!("slice should be a 2D shape"),
    }
}

#[test]
fn section_miss_errors() {
    let mut it = Interpreter::new();
    it.exec_line("box 20 20 20 name=blk").unwrap(); // spans z 0..20
    assert!(
        it.exec_line("section blk axis=z at=99").is_err(),
        "plane misses mesh"
    );
}

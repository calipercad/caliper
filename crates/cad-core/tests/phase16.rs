//! Phase 16 integration tests (roadmap v1.6): sketch circles, arcs, and curve
//! constraints (radius / tangent / concentric / point-on / equal-radius),
//! driven end-to-end through the interpreter. `circle`/`arc` route to the active
//! sketch; with no sketch they still create scene primitives.

use cad_core::scene::Geometry;
use cad_core::Interpreter;

fn shape_area(it: &Interpreter, name: &str) -> f64 {
    match &it.scene.get(name).unwrap().geom {
        Geometry::Shape(s) => s.area(),
        _ => panic!("{name} is not a shape"),
    }
}

#[test]
fn sketch_circle_emits_disc() {
    let mut it = Interpreter::new();
    it.exec_line("sketch disc").unwrap();
    it.exec_line("point c 0 0").unwrap();
    it.exec_line("circle k c 3").unwrap(); // inside a sketch → sketch circle
    it.exec_line("constrain fix c").unwrap();
    it.exec_line("constrain radius k 10").unwrap();
    assert!(it.exec_line("solve").unwrap().contains("solved"));
    it.exec_line("sketch close name=disc").unwrap();
    let a = shape_area(&it, "disc");
    assert!((a - std::f64::consts::PI * 100.0).abs() < 1.0, "area {a}");
}

#[test]
fn circle_outside_sketch_is_scene_primitive() {
    // With no active sketch, `circle <r>` still makes a scene entity.
    let mut it = Interpreter::new();
    let msg = it.exec_line("circle 5 name=hole").unwrap();
    assert!(msg.contains("circle 'hole'"), "{msg}");
    assert!(matches!(
        it.scene.get("hole").unwrap().geom,
        Geometry::Shape(_)
    ));
}

#[test]
fn plate_with_constrained_hole() {
    // A rectangle outline with a circular hole constrained concentric-ish:
    // hole center fixed at the plate center, radius 6 → a washer-like shape.
    let mut it = Interpreter::new();
    it.exec_line("sketch plate").unwrap();
    it.exec_line("point bl 0 0").unwrap();
    it.exec_line("point br 40 0").unwrap();
    it.exec_line("point tr 40 30").unwrap();
    it.exec_line("point tl 0 30").unwrap();
    it.exec_line("point c 20 15").unwrap();
    it.exec_line("segment s1 bl br").unwrap();
    it.exec_line("segment s2 br tr").unwrap();
    it.exec_line("segment s3 tr tl").unwrap();
    it.exec_line("segment s4 tl bl").unwrap();
    it.exec_line("circle hole c 6").unwrap();
    it.exec_line("constrain fix bl").unwrap();
    it.exec_line("constrain fix c").unwrap();
    it.exec_line("constrain radius hole 6").unwrap();
    it.exec_line("solve").unwrap();
    it.exec_line("sketch close name=plate").unwrap();
    // Net area = 40*30 − π·6² ≈ 1200 − 113 = 1087.
    let a = shape_area(&it, "plate");
    assert!(
        (a - (1200.0 - std::f64::consts::PI * 36.0)).abs() < 2.0,
        "area {a}"
    );
    // Two contours: outer boundary + the hole.
    match &it.scene.get("plate").unwrap().geom {
        Geometry::Shape(s) => assert_eq!(s.regions()[0].holes.len(), 1, "one hole"),
        _ => panic!(),
    }
}

#[test]
fn tangent_circle_to_segment() {
    let mut it = Interpreter::new();
    it.exec_line("sketch s").unwrap();
    it.exec_line("point a 0 0").unwrap();
    it.exec_line("point b 20 0").unwrap();
    it.exec_line("point c 10 9").unwrap();
    it.exec_line("segment base a b").unwrap();
    it.exec_line("circle k c 5").unwrap();
    it.exec_line("constrain fix a").unwrap();
    it.exec_line("constrain fix b").unwrap();
    it.exec_line("constrain radius k 5").unwrap();
    it.exec_line("constrain tangent base k").unwrap();
    assert!(it.exec_line("solve").unwrap().contains("solved"));
    let (ctr, r) = it.sketch.as_ref().unwrap().circle_pos("k").unwrap();
    assert!(
        (ctr.y.abs() - r).abs() < 1e-3,
        "tangent dist {} r {r}",
        ctr.y.abs()
    );
}

#[test]
fn concentric_and_equal() {
    let mut it = Interpreter::new();
    it.exec_line("sketch s").unwrap();
    it.exec_line("point c1 0 0").unwrap();
    it.exec_line("point c2 5 5").unwrap();
    it.exec_line("circle a c1 10").unwrap();
    it.exec_line("circle b c2 4").unwrap();
    it.exec_line("constrain fix c1").unwrap();
    it.exec_line("constrain concentric a b").unwrap();
    it.exec_line("constrain radius a 8").unwrap();
    it.exec_line("constrain equal a b").unwrap();
    it.exec_line("solve").unwrap();
    let sk = it.sketch.as_ref().unwrap();
    let (ca, ra) = sk.circle_pos("a").unwrap();
    let (cb, rb) = sk.circle_pos("b").unwrap();
    assert!((ca - cb).len() < 1e-4, "concentric");
    assert!(
        (ra - 8.0).abs() < 1e-4 && (rb - 8.0).abs() < 1e-4,
        "equal radii 8"
    );
}

#[test]
fn arc_in_boundary_half_disk() {
    // A half-disk: diameter segment + a semicircle arc.
    let mut it = Interpreter::new();
    it.exec_line("sketch hd").unwrap();
    it.exec_line("point o 0 0").unwrap();
    it.exec_line("point a -10 0").unwrap();
    it.exec_line("point b 10 0").unwrap();
    it.exec_line("segment dia a b").unwrap();
    it.exec_line("arc cap o b a").unwrap();
    it.exec_line("solve").unwrap();
    it.exec_line("sketch close name=hd").unwrap();
    let a = shape_area(&it, "hd");
    assert!(a > 120.0, "half-disk area {a} (~157)");
}

#[test]
fn save_roundtrip_with_circle() {
    let mut it = Interpreter::new();
    it.exec_line("sketch disc").unwrap();
    it.exec_line("point c 0 0").unwrap();
    it.exec_line("circle k c 3").unwrap();
    it.exec_line("constrain fix c").unwrap();
    it.exec_line("constrain radius k 9").unwrap();
    it.exec_line("solve").unwrap();
    it.exec_line("sketch close name=disc").unwrap();
    let area = shape_area(&it, "disc");

    let p = std::env::temp_dir().join("cad_p16.cad");
    it.exec_line(&format!("save {}", p.display())).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("circle k c 3"), "recorded:\n{text}");
    assert!(text.contains("constrain radius k 9"));

    let mut it2 = Interpreter::new();
    it2.exec_line(&format!("load {}", p.display())).unwrap();
    assert!(
        (shape_area(&it2, "disc") - area).abs() < 1e-9,
        "deterministic replay"
    );
}

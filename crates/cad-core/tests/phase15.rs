//! Phase 15 integration tests (roadmap v1.5): the 2D constraint sketch solver,
//! driven end-to-end through the interpreter — build points + segments, add
//! geometric constraints, `solve`, and emit a `Shape`. Mirrors the variables
//! tests (phase9) for undo/redo and deterministic save round-trip.

use cad_core::math::Vec2;
use cad_core::scene::Geometry;
use cad_core::Interpreter;

fn shape_bbox(it: &Interpreter, name: &str) -> (f64, f64) {
    match &it.scene.get(name).unwrap().geom {
        Geometry::Shape(s) => {
            let b = s.bbox();
            (b.width(), b.height())
        }
        _ => panic!("{name} is not a shape"),
    }
}
fn shape_area(it: &Interpreter, name: &str) -> f64 {
    match &it.scene.get(name).unwrap().geom {
        Geometry::Shape(s) => s.area(),
        _ => panic!("{name} is not a shape"),
    }
}

/// Build a constrained 40×25 rectangle: corner bl fixed at origin, sides
/// horizontal/vertical, bottom & left edges given exact lengths.
fn build_rectangle(it: &mut Interpreter) {
    it.exec_line("sketch plate").unwrap();
    it.exec_line("point bl 0 0").unwrap();
    it.exec_line("point br 30 2").unwrap(); // deliberately off; solver fixes it
    it.exec_line("point tr 33 20").unwrap();
    it.exec_line("point tl 1 22").unwrap();
    it.exec_line("segment bottom bl br").unwrap();
    it.exec_line("segment right br tr").unwrap();
    it.exec_line("segment top tr tl").unwrap();
    it.exec_line("segment left tl bl").unwrap();
    it.exec_line("constrain fix bl").unwrap();
    it.exec_line("constrain horizontal bottom").unwrap();
    it.exec_line("constrain horizontal top").unwrap();
    it.exec_line("constrain vertical left").unwrap();
    it.exec_line("constrain vertical right").unwrap();
    it.exec_line("constrain length bottom 40").unwrap();
    it.exec_line("constrain length left 25").unwrap();
}

#[test]
fn constrained_rectangle_solves_exactly() {
    let mut it = Interpreter::new();
    build_rectangle(&mut it);
    let rep = it.exec_line("solve").unwrap();
    assert!(rep.contains("solved"), "{rep}");
    it.exec_line("sketch close name=plate").unwrap();
    let (w, h) = shape_bbox(&it, "plate");
    assert!(
        (w - 40.0).abs() < 1e-3 && (h - 25.0).abs() < 1e-3,
        "{w}×{h}"
    );
    assert!(
        (shape_area(&it, "plate") - 1000.0).abs() < 1e-1,
        "area {}",
        shape_area(&it, "plate")
    );
}

#[test]
fn perpendicular_and_equal_constraints() {
    let mut it = Interpreter::new();
    it.exec_line("sketch ell").unwrap();
    it.exec_line("point o 0 0").unwrap();
    it.exec_line("point a 10 0").unwrap();
    it.exec_line("point b 2 9").unwrap();
    it.exec_line("segment oa o a").unwrap();
    it.exec_line("segment ob o b").unwrap();
    it.exec_line("constrain fix o").unwrap();
    it.exec_line("constrain fix a").unwrap();
    it.exec_line("constrain perpendicular oa ob").unwrap();
    it.exec_line("constrain equal oa ob").unwrap();
    let rep = it.exec_line("solve").unwrap();
    assert!(rep.contains("solved"), "{rep}");
    // After solving, ob ⟂ oa and |ob| == |oa| == 10.
    let sk = it.sketch.as_ref().unwrap();
    let o = sk.point_pos("o").unwrap();
    let a = sk.point_pos("a").unwrap();
    let b = sk.point_pos("b").unwrap();
    let d1 = (a - o).normalized();
    let d2 = (b - o).normalized();
    assert!(d1.dot(d2).abs() < 1e-4, "not perpendicular: {}", d1.dot(d2));
    assert!(
        ((b - o).len() - 10.0).abs() < 1e-3,
        "|ob| {}",
        (b - o).len()
    );
}

#[test]
fn distance_and_angle_targets() {
    let mut it = Interpreter::new();
    it.exec_line("sketch s").unwrap();
    it.exec_line("point o 0 0").unwrap();
    it.exec_line("point a 10 0").unwrap();
    it.exec_line("point b 5 1").unwrap();
    it.exec_line("segment oa o a").unwrap();
    it.exec_line("segment ob o b").unwrap();
    it.exec_line("constrain fix o").unwrap();
    it.exec_line("constrain fix a").unwrap();
    it.exec_line("constrain distance o b 12").unwrap();
    it.exec_line("constrain angle oa ob 60").unwrap();
    assert!(it.exec_line("solve").unwrap().contains("solved"));
    let sk = it.sketch.as_ref().unwrap();
    let o = sk.point_pos("o").unwrap();
    let b = sk.point_pos("b").unwrap();
    assert!(
        ((b - o).len() - 12.0).abs() < 1e-3,
        "distance {}",
        (b - o).len()
    );
    // angle of ob should be 60° (oa is along +x).
    let ang = (b - o).angle().to_degrees();
    assert!((ang - 60.0).abs() < 1e-2, "angle {ang}");
}

#[test]
fn coincident_pulls_points_together() {
    let mut it = Interpreter::new();
    it.exec_line("sketch s").unwrap();
    it.exec_line("point a 0 0").unwrap();
    it.exec_line("point b 5 5").unwrap();
    it.exec_line("constrain fix a").unwrap();
    it.exec_line("constrain coincident a b").unwrap();
    it.exec_line("solve").unwrap();
    let sk = it.sketch.as_ref().unwrap();
    assert!((sk.point_pos("b").unwrap() - Vec2::new(0.0, 0.0)).len() < 1e-4);
}

#[test]
fn undo_restores_pre_solve_coords() {
    let mut it = Interpreter::new();
    it.exec_line("sketch s").unwrap();
    it.exec_line("point a 0 0").unwrap();
    it.exec_line("point b 5 1").unwrap();
    it.exec_line("constrain fix a").unwrap();
    it.exec_line("constrain distance a b 10").unwrap();
    let before = it.sketch.as_ref().unwrap().point_pos("b").unwrap();
    it.exec_line("solve").unwrap();
    let solved = it.sketch.as_ref().unwrap().point_pos("b").unwrap();
    assert!((solved - before).len() > 1.0, "solve moved b");
    it.exec_line("undo").unwrap();
    let restored = it.sketch.as_ref().unwrap().point_pos("b").unwrap();
    assert!(
        (restored - before).len() < 1e-9,
        "undo restored pre-solve coords"
    );
    it.exec_line("redo").unwrap();
    assert!((it.sketch.as_ref().unwrap().point_pos("b").unwrap() - solved).len() < 1e-9);
}

#[test]
fn save_roundtrip_is_deterministic() {
    let mut it = Interpreter::new();
    build_rectangle(&mut it);
    it.exec_line("solve").unwrap();
    it.exec_line("sketch close name=plate").unwrap();
    let area = shape_area(&it, "plate");

    let p = std::env::temp_dir().join("cad_p15.cad");
    it.exec_line(&format!("save {}", p.display())).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(
        text.contains("constrain length bottom 40"),
        "recorded:\n{text}"
    );
    assert!(text.contains("solve"), "solve recorded");
    assert!(text.contains("sketch close"), "close recorded");

    let mut it2 = Interpreter::new();
    it2.exec_line(&format!("load {}", p.display())).unwrap();
    assert!(
        (shape_area(&it2, "plate") - area).abs() < 1e-9,
        "deterministic replay"
    );
}

#[test]
fn errors_without_active_sketch() {
    let mut it = Interpreter::new();
    assert!(it.exec_line("point a 0 0").is_err(), "point needs a sketch");
    assert!(it.exec_line("solve").is_err(), "solve needs a sketch");
    assert!(
        it.exec_line("sketch close").is_err(),
        "close needs a sketch"
    );
}

#[test]
fn point_coords_accept_expressions() {
    let mut it = Interpreter::new();
    it.exec_line("set w 40").unwrap();
    it.exec_line("sketch s").unwrap();
    it.exec_line("point a 0 0").unwrap();
    it.exec_line("point b $w $w/2").unwrap();
    let b = it.sketch.as_ref().unwrap().point_pos("b").unwrap();
    assert!(
        (b - Vec2::new(40.0, 20.0)).len() < 1e-9,
        "expr coords: {b:?}"
    );
}

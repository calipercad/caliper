//! Phase 1 integration tests (blueprint §10): robust 2D booleans, holed
//! extrusion + manifold verification, fillet/chamfer, and exporter fidelity.
//!
//! Driven entirely through the public API so it doubles as a contract check.

use cad_core::export::{dxf, svg};
use cad_core::geom2d::boolean::{difference, intersection, union, xor};
use cad_core::geom2d::{Contour, Shape};
use cad_core::geom3d::{self, Mesh};
use cad_core::math::{Mat4, Vec2, Vec3, PI};
use cad_core::Interpreter;

// ---- helpers ---------------------------------------------------------------

/// A CCW axis-aligned square `[x0,x1] × [y0,y1]` as a closed shape.
fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
    Shape::polyline(
        vec![
            Vec2::new(x0, y0),
            Vec2::new(x1, y0),
            Vec2::new(x1, y1),
            Vec2::new(x0, y1),
        ],
        true,
    )
}

/// Signed volume via the divergence/tetra sum. `> 0` ⇒ all faces wound outward.
fn signed_volume(m: &Mesh) -> f64 {
    let mut v = 0.0;
    for t in &m.tris {
        let a = m.verts[t[0]];
        let b = m.verts[t[1]];
        let c = m.verts[t[2]];
        v += a.dot(b.cross(c));
    }
    v / 6.0
}

// ---- boolean core ----------------------------------------------------------

#[test]
fn boolean_truth_table() {
    let a = square(0.0, 0.0, 2.0, 2.0);
    let b = square(1.0, 1.0, 3.0, 3.0);
    assert!((union(&a, &b).unwrap().area() - 7.0).abs() < 1e-6);
    assert!((intersection(&a, &b).unwrap().area() - 1.0).abs() < 1e-6);
    assert!((difference(&a, &b).unwrap().area() - 3.0).abs() < 1e-6);
    assert!((xor(&a, &b).unwrap().area() - 6.0).abs() < 1e-6);
}

#[test]
fn subtract_circle_from_rect() {
    let mut rect = Shape::rect(40.0, 40.0, 0.0, 0);
    rect.center_origin();
    let circle = Shape::circle(8.0, 64);
    let result = rect.difference(&circle).unwrap();

    let regions = result.regions();
    assert_eq!(regions.len(), 1, "one shell");
    assert_eq!(regions[0].holes.len(), 1, "exactly one bore");

    let expected = 1600.0 - circle.area();
    assert!(
        (result.area() - expected).abs() < 1e-3,
        "area {} vs expected {}",
        result.area(),
        expected
    );
}

#[test]
fn coincident_and_shared_edge() {
    // Two identical squares union to that one square.
    let a = square(0.0, 0.0, 2.0, 2.0);
    let b = square(0.0, 0.0, 2.0, 2.0);
    let u = union(&a, &b).unwrap();
    assert_eq!(u.contours.len(), 1);
    assert!((u.area() - 4.0).abs() < 1e-6);

    // Two squares sharing one full edge merge into a single rectangle.
    let a2 = square(0.0, 0.0, 2.0, 2.0);
    let b2 = square(2.0, 0.0, 4.0, 2.0);
    let u2 = union(&a2, &b2).unwrap();
    assert_eq!(u2.contours.len(), 1, "shared edge dissolves");
    assert!((u2.area() - 8.0).abs() < 1e-6);
}

#[test]
fn disjoint_split() {
    // A bar spanning the full width cuts the rect into two disjoint shells.
    let rect = square(0.0, 0.0, 10.0, 10.0);
    let bar = square(-1.0, 4.0, 11.0, 6.0);
    let d = difference(&rect, &bar).unwrap();
    assert_eq!(d.regions().len(), 2, "two disjoint pieces");
    assert!((d.area() - (100.0 - 20.0)).abs() < 1e-6);
}

#[test]
fn empty_result_graceful() {
    // Disjoint intersection and self-difference are both empty.
    let a = square(0.0, 0.0, 2.0, 2.0);
    let b = square(5.0, 5.0, 7.0, 7.0);
    assert!(intersection(&a, &b).unwrap().is_empty());
    assert!(difference(&a, &a).unwrap().is_empty());

    // The command layer turns an empty result into a clean error (no panic).
    let mut interp = Interpreter::new();
    interp.exec_line("rect 2 2 name=a").unwrap();
    interp.exec_line("rect 2 2 name=b").unwrap();
    interp.exec_line("translate 5 5 target=b").unwrap();
    assert!(interp.exec_line("intersect a b").is_err());
    assert!(interp.exec_line("subtract a a").is_err());
}

#[test]
fn determinism() {
    let a = square(0.0, 0.0, 2.0, 2.0);
    let b = square(1.0, 1.0, 3.0, 3.0);
    let u1 = union(&a, &b).unwrap();
    let u2 = union(&a, &b).unwrap();
    assert_eq!(u1.contours.len(), u2.contours.len());
    for (c1, c2) in u1.contours.iter().zip(&u2.contours) {
        assert_eq!(c1.points, c2.points, "byte-stable point lists");
    }
}

#[test]
fn inclusion_exclusion() {
    // |A∪B| + |A∩B| == |A| + |B| over a handful of overlapping shapes.
    let mut circ = Shape::circle(4.0, 64);
    circ.transform(&Mat4::translation(Vec3::new(5.0, 5.0, 0.0)));
    let cases: Vec<(Shape, Shape)> = vec![
        (square(0.0, 0.0, 4.0, 4.0), square(2.0, 2.0, 6.0, 6.0)),
        (square(0.0, 0.0, 4.0, 4.0), square(1.0, 1.0, 3.0, 3.0)), // b ⊂ a
        (square(0.0, 0.0, 10.0, 10.0), circ),
    ];
    for (a, b) in cases {
        let lhs = union(&a, &b).unwrap().area() + intersection(&a, &b).unwrap().area();
        let rhs = a.area() + b.area();
        assert!((lhs - rhs).abs() < 1e-2, "lhs {lhs} rhs {rhs}");
    }
}

// ---- holed extrusion + manifold verification -------------------------------

#[test]
fn extrude_weld_regression() {
    // A plain rectangle must extrude to a closed manifold (guards the fixed
    // cap/wall weld bug).
    let rect = square(0.0, 0.0, 10.0, 5.0);
    let mesh = geom3d::extrude(&rect, 3.0).unwrap();
    assert!(mesh.is_closed_manifold(), "plain box is a closed manifold");
    assert!(signed_volume(&mesh) > 0.0, "outward normals");
}

#[test]
fn hole_is_manifold() {
    // CHECKPOINT: rect 40 − circle 8, extruded, is a genus-1 closed manifold
    // with outward normals.
    let mut rect = Shape::rect(40.0, 40.0, 0.0, 0);
    rect.center_origin();
    let circle = Shape::circle(8.0, 64);
    let shape = rect.difference(&circle).unwrap();

    let mesh = geom3d::extrude(&shape, 5.0).unwrap();
    assert!(mesh.is_closed_manifold(), "bored solid is a closed manifold");
    assert_eq!(mesh.euler_characteristic(), 0, "genus 1 ⇒ χ = 0");
    assert!(signed_volume(&mesh) > 0.0, "outward normals");
}

#[test]
fn multi_hole_genus() {
    // Rect with three circular bores ⇒ genus 3 ⇒ χ = 2 - 2·3 = -4.
    let mut rect = Shape::rect(60.0, 20.0, 0.0, 0);
    rect.center_origin();
    let mut shape = rect;
    for cx in [-20.0, 0.0, 20.0] {
        let mut c = Shape::circle(4.0, 48);
        c.transform(&Mat4::translation(Vec3::new(cx, 0.0, 0.0)));
        shape = shape.difference(&c).unwrap();
    }
    let mesh = geom3d::extrude(&shape, 5.0).unwrap();
    assert!(mesh.is_closed_manifold());
    assert_eq!(mesh.euler_characteristic(), -4, "three bores ⇒ genus 3");
}

// ---- fillet / chamfer ------------------------------------------------------

#[test]
fn fillet_square() {
    let sq = Contour::new(
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(40.0, 40.0),
            Vec2::new(0.0, 40.0),
        ],
        true,
    );
    let seg = 8;
    let f = sq.fillet(5.0, seg);
    assert_eq!(f.points.len(), 4 * (seg + 1), "4 rounded corners");

    let expected = 1600.0 - 4.0 * (1.0 - PI / 4.0) * 25.0;
    assert!(
        (f.signed_area() - expected).abs() < 3.0,
        "area {} vs ~{}",
        f.signed_area(),
        expected
    );
}

#[test]
fn chamfer_square() {
    let sq = Contour::new(
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(40.0, 40.0),
            Vec2::new(0.0, 40.0),
        ],
        true,
    );
    let c = sq.chamfer(5.0);
    assert_eq!(c.points.len(), 8, "each corner becomes a straight bevel");
    // Bevel legs of 5 remove 4 triangles of area 5²/2 = 12.5.
    assert!((c.signed_area() - (1600.0 - 4.0 * 12.5)).abs() < 1e-6);
}

#[test]
fn fillet_clamp() {
    // A radius larger than half the shortest edge clamps the setback; the
    // result must stay a simple, positively-wound contour.
    let sq = Contour::new(
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(40.0, 40.0),
            Vec2::new(0.0, 40.0),
        ],
        true,
    );
    let f = sq.fillet(100.0, 8);
    assert!(f.signed_area() > 0.0, "still CCW / simple");
    assert!(f.signed_area() < 1600.0, "corners were rounded in");
}

#[test]
fn fillet_skips_smooth() {
    // A finely tessellated circle is left (near-)unchanged by a small fillet.
    let circle = Shape::circle(10.0, 64);
    let c0 = &circle.contours[0];
    let orig = c0.area();
    let f = c0.fillet(0.5, 8);
    assert!(f.signed_area() > 0.0);
    assert!(
        (f.area() - orig).abs() / orig < 0.05,
        "area changed by {}%",
        100.0 * (f.area() - orig).abs() / orig
    );
}

// ---- exporter fidelity -----------------------------------------------------

#[test]
fn exporter_primitives() {
    // Circle primitive ⇒ native CIRCLE / <circle>.
    let circle = Shape::circle(8.0, 64);
    let dxf_c = dxf::to_dxf(&[("c", &circle)], 1.0);
    assert!(dxf_c.contains("CIRCLE"), "DXF CIRCLE entity");
    assert!(dxf_c.contains("\n40\n"), "DXF radius group");
    let svg_c = svg::to_svg(&[("c", &circle)], 1.0);
    assert!(svg_c.contains("<circle"), "SVG <circle>");

    // Arc primitive ⇒ native ARC with start/end groups.
    let arc = Shape::arc(5.0, 0.0, 90.0, 48);
    let dxf_a = dxf::to_dxf(&[("a", &arc)], 1.0);
    assert!(dxf_a.contains("ARC"), "DXF ARC entity");
    assert!(dxf_a.contains("\n50\n") && dxf_a.contains("\n51\n"), "ARC angles");

    // A boolean result is a Polyline ⇒ falls back to LWPOLYLINE / <path>.
    let a = square(0.0, 0.0, 2.0, 2.0);
    let b = square(1.0, 1.0, 3.0, 3.0);
    let u = union(&a, &b).unwrap();
    let dxf_u = dxf::to_dxf(&[("u", &u)], 1.0);
    assert!(dxf_u.contains("LWPOLYLINE"), "boolean result is a polyline");
    let svg_u = svg::to_svg(&[("u", &u)], 1.0);
    assert!(svg_u.contains("<path"), "SVG path for polyline");
}

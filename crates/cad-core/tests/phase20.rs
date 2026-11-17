// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 20 integration tests (roadmap v1.10): non-convex fillet & chamfer.
//! Convex solids keep the exact global-polytope path; non-convex solids
//! (L-brackets, notched plates) instead get a per-edge swept tool applied via
//! the CSG kernel — convex edges subtracted, concave edges unioned. We assert
//! the hard invariants (closed 2-manifold, genus 0) and that the convex path is
//! unchanged.

use cad_core::geom2d::Shape;
use cad_core::geom3d::{self, bevel, Mesh};
use cad_core::math::Vec2;

/// An L-shaped prism: one concave (reflex) vertical edge at (15,15), the rest
/// convex, plus the top/bottom rim edges.
fn l_prism(h: f64) -> Mesh {
    let pts = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(40.0, 0.0),
        Vec2::new(40.0, 15.0),
        Vec2::new(15.0, 15.0),
        Vec2::new(15.0, 40.0),
        Vec2::new(0.0, 40.0),
    ];
    geom3d::extrude(&Shape::polyline(pts, true), h).unwrap()
}

#[test]
fn l_prism_is_a_valid_solid() {
    let m = l_prism(20.0);
    assert!(m.is_closed_manifold());
    assert_eq!(m.euler_characteristic(), 2);
}

#[test]
fn fillet_non_convex_l_prism() {
    let base = l_prism(20.0);
    let f = bevel::fillet(&base, 4.0, 3, 30.0).unwrap();
    assert!(f.is_closed_manifold(), "non-convex fillet is closed");
    assert_eq!(f.euler_characteristic(), 2, "genus 0");
    assert!(f.signed_volume() > 0.0, "outward normals");
    assert!(
        (f.signed_volume() - base.signed_volume()).abs() > 1e-6,
        "geometry actually changed"
    );
}

#[test]
fn chamfer_non_convex_l_prism() {
    let base = l_prism(20.0);
    let c = bevel::chamfer(&base, 3.0, 30.0).unwrap();
    assert!(c.is_closed_manifold(), "non-convex chamfer is closed");
    assert_eq!(c.euler_characteristic(), 2, "genus 0");
    assert!(c.signed_volume() > 0.0);
}

#[test]
fn fillet_notched_plate() {
    // A U-slot plate: two concave (inside) corners at the base of the slot.
    let pts = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(40.0, 0.0),
        Vec2::new(40.0, 30.0),
        Vec2::new(26.0, 30.0),
        Vec2::new(26.0, 12.0),
        Vec2::new(14.0, 12.0),
        Vec2::new(14.0, 30.0),
        Vec2::new(0.0, 30.0),
    ];
    let base = geom3d::extrude(&Shape::polyline(pts, true), 10.0).unwrap();
    let f = bevel::fillet(&base, 3.0, 2, 30.0).unwrap();
    assert!(f.is_closed_manifold(), "notched-plate fillet is closed");
    assert_eq!(f.euler_characteristic(), 2, "genus 0");
}

#[test]
fn convex_box_still_uses_exact_path() {
    // Regression: a convex cuboid still fillets/chamfers to a closed genus-0 solid.
    let b = Mesh::cuboid(40.0, 40.0, 20.0);
    let f = bevel::fillet(&b, 5.0, 3, 20.0).unwrap();
    assert!(f.is_closed_manifold());
    assert_eq!(f.euler_characteristic(), 2);
    assert!(
        f.signed_volume() < b.signed_volume(),
        "convex fillet removes material"
    );
}

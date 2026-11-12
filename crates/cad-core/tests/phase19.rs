// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 19 integration tests (roadmap v1.9): loft — skin a watertight solid
//! through a stack of 2D profiles. We assert the hard invariants (closed
//! 2-manifold, Euler characteristic 2, sensible volume bounds), resampling
//! across mismatched vertex counts, and clean errors.

use cad_core::geom2d::Shape;
use cad_core::geom3d::{self};
use cad_core::math::Vec2;
use cad_core::scene::Geometry;
use cad_core::Interpreter;

fn pts(s: &Shape) -> Vec<Vec2> {
    s.contours[0].points.clone()
}

#[test]
fn loft_identical_squares_is_a_prism() {
    let sq = pts(&Shape::rect(40.0, 40.0, 0.0, 0));
    let m = geom3d::loft(&[&sq, &sq], &[0.0, 10.0], 0).unwrap();
    assert!(m.is_closed_manifold(), "loft of equal squares is closed");
    assert_eq!(m.euler_characteristic(), 2);
    let v = m.signed_volume();
    assert!((v - 16000.0).abs() < 1e-6, "volume = area*h, got {v}");
}

#[test]
fn loft_frustum_volume_between_bounds() {
    let big = pts(&Shape::rect(40.0, 40.0, 0.0, 0));
    let small = pts(&Shape::rect(20.0, 20.0, 0.0, 0));
    let m = geom3d::loft(&[&big, &small], &[0.0, 10.0], 0).unwrap();
    assert!(m.is_closed_manifold());
    assert_eq!(m.euler_characteristic(), 2);
    let v = m.signed_volume();
    // Between the small prism (400*10) and the large prism (1600*10).
    assert!(v > 4000.0 && v < 16000.0, "frustum between prisms, got {v}");
}

#[test]
fn loft_resamples_mismatched_counts() {
    let tri = pts(&Shape::polygon(3, 20.0));
    let sq = pts(&Shape::rect(30.0, 30.0, 0.0, 0));
    assert_eq!(tri.len(), 3);
    assert_eq!(sq.len(), 4);
    // Common vertex count is the larger input (4); the triangle is resampled.
    let m = geom3d::loft(&[&tri, &sq], &[0.0, 12.0], 0).unwrap();
    assert!(
        m.is_closed_manifold(),
        "resampled triangle→square is closed"
    );
    assert_eq!(m.euler_characteristic(), 2);
    assert!(m.signed_volume() > 0.0, "outward normals");
}

#[test]
fn loft_three_sections_stacks() {
    let a = pts(&Shape::rect(40.0, 40.0, 0.0, 0));
    let b = pts(&Shape::rect(20.0, 20.0, 0.0, 0));
    let c = pts(&Shape::rect(30.0, 30.0, 0.0, 0));
    let m = geom3d::loft(&[&a, &b, &c], &[0.0, 5.0, 10.0], 0).unwrap();
    assert!(m.is_closed_manifold());
    assert_eq!(m.euler_characteristic(), 2);
    assert!(m.signed_volume() > 0.0);
}

#[test]
fn loft_needs_two_profiles() {
    let sq = pts(&Shape::rect(10.0, 10.0, 0.0, 0));
    assert!(geom3d::loft(&[&sq], &[0.0], 0).is_err());
}

#[test]
fn script_loft_builds_mesh_and_rejects_mesh_profile() {
    let mut it = Interpreter::new();
    it.exec_line("rect 40 40 name=a").unwrap();
    it.exec_line("rect 20 20 name=b").unwrap();
    let out = it.exec_line("loft a b height=10 name=cone").unwrap();
    assert!(out.contains("lofted"), "got: {out}");
    match &it.scene.get("cone").unwrap().geom {
        Geometry::Mesh(m) => {
            assert!(m.is_closed_manifold(), "scripted loft is closed");
            assert!(m.signed_volume() > 0.0);
        }
        _ => panic!("loft should produce a mesh"),
    }
    // A single profile errors.
    assert!(it.exec_line("loft a height=10").is_err());
    // A mesh profile errors.
    it.exec_line("box 10 10 10 name=blk").unwrap();
    assert!(it.exec_line("loft a blk").is_err());
}

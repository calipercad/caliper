// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 23 integration tests (roadmap v1.12): sweep a 2D cross-section along a
//! 2D path into a solid. A straight path reduces to a prism (the volume anchor);
//! a bent path stays a closed genus-0 manifold.

use cad_core::geom2d::Shape;
use cad_core::geom3d::{self};
use cad_core::math::Vec2;
use cad_core::scene::Geometry;
use cad_core::Interpreter;

fn square(s: f64) -> Vec<Vec2> {
    let h = s / 2.0;
    vec![
        Vec2::new(-h, -h),
        Vec2::new(h, -h),
        Vec2::new(h, h),
        Vec2::new(-h, h),
    ]
}

fn pts(s: &Shape) -> Vec<Vec2> {
    s.contours[0].points.clone()
}

#[test]
fn straight_sweep_is_a_prism() {
    let (s, len) = (6.0, 40.0);
    let path = vec![Vec2::new(0.0, 0.0), Vec2::new(len, 0.0)];
    let m = geom3d::sweep(&square(s), &path).unwrap();
    assert!(m.is_closed_manifold(), "straight sweep is closed");
    assert_eq!(m.euler_characteristic(), 2);
    let v = m.signed_volume();
    assert!(
        (v - s * s * len).abs() < 1e-6,
        "prism volume s^2*len, got {v}"
    );
}

#[test]
fn bent_sweep_is_closed() {
    let prof = pts(&Shape::circle(4.0, 24));
    let path = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(30.0, 0.0),
        Vec2::new(30.0, 30.0),
    ];
    let m = geom3d::sweep(&prof, &path).unwrap();
    assert!(m.is_closed_manifold(), "bent pipe is closed");
    assert_eq!(m.euler_characteristic(), 2, "genus 0");
    assert!(m.signed_volume() > 0.0, "outward normals");
}

#[test]
fn errors_on_degenerate() {
    // path with < 2 points
    assert!(geom3d::sweep(&square(5.0), &[Vec2::new(0.0, 0.0)]).is_err());
    // profile with < 3 points
    let seg = vec![Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0)];
    assert!(geom3d::sweep(&seg, &[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)]).is_err());
}

#[test]
fn script_sweep_builds_mesh_and_rejects_mesh() {
    let mut it = Interpreter::new();
    it.exec_line("circle 4 name=prof").unwrap();
    it.exec_line("path 0 0 30 0 30 30 name=track").unwrap();
    let out = it.exec_line("sweep prof track name=pipe").unwrap();
    assert!(out.contains("swept"), "got: {out}");
    match &it.scene.get("pipe").unwrap().geom {
        Geometry::Mesh(m) => {
            assert!(m.is_closed_manifold(), "scripted sweep is closed");
            assert!(m.signed_volume() > 0.0);
        }
        _ => panic!("sweep should produce a mesh"),
    }
    // a mesh target errors
    it.exec_line("box 5 5 5 name=blk").unwrap();
    assert!(it.exec_line("sweep blk track").is_err());
}

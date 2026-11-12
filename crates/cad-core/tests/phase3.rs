// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 3 integration tests (roadmap v0.4): BSP mesh CSG, shell, and the
//! fillet-profile-then-extrude workflow for rounded 3D edges.

use cad_core::geom2d::Shape;
use cad_core::geom3d::{self, csg, Mesh};
use cad_core::math::{Mat4, Vec3};
use cad_core::scene::Geometry;
use cad_core::Interpreter;

/// Signed volume (Σ a·(b×c)/6). `> 0` ⇒ consistent outward winding.
fn volume(m: &Mesh) -> f64 {
    let mut v = 0.0;
    for t in &m.tris {
        let (a, b, c) = (m.verts[t[0]], m.verts[t[1]], m.verts[t[2]]);
        v += a.dot(b.cross(c));
    }
    v / 6.0
}

fn centered_cuboid(w: f64, d: f64, h: f64) -> Mesh {
    let mut m = Mesh::cuboid(w, d, h);
    m.center_origin();
    m
}

// ---- CSG booleans ----------------------------------------------------------

#[test]
fn csg_difference_manifold() {
    // CHECKPOINT: a 40³ block with an 8-radius bore drilled all the way through
    // is a closed genus-1 manifold with outward normals.
    let block = centered_cuboid(40.0, 40.0, 40.0);
    let mut drill = Mesh::cylinder(8.0, 60.0, 48);
    drill.center_origin();
    let holed = csg::difference(&block, &drill).unwrap();

    assert!(
        holed.is_closed_manifold(),
        "bored block is a closed manifold"
    );
    assert_eq!(
        holed.euler_characteristic(),
        0,
        "one through-hole ⇒ genus 1"
    );
    assert!(volume(&holed) > 0.0, "outward normals");
    let expected = 64000.0 - std::f64::consts::PI * 64.0 * 40.0;
    assert!(
        (volume(&holed) - expected).abs() < expected * 0.01,
        "volume {} vs ~{}",
        volume(&holed),
        expected
    );
}

#[test]
fn csg_union_overlap() {
    // Two unit-ish cubes overlapping in one corner octant.
    let a = Mesh::cuboid(20.0, 20.0, 20.0); // 0..20
    let mut b = Mesh::cuboid(20.0, 20.0, 20.0);
    b.transform(&Mat4::translation(Vec3::new(10.0, 10.0, 10.0))); // 10..30
    let u = csg::union(&a, &b).unwrap();
    assert!(u.is_closed_manifold());
    // inclusion–exclusion: 8000 + 8000 − 1000 (10³ overlap)
    assert!(
        (volume(&u) - 15000.0).abs() < 1.0,
        "union vol {}",
        volume(&u)
    );
}

#[test]
fn csg_intersection() {
    let a = Mesh::cuboid(20.0, 20.0, 20.0);
    let mut b = Mesh::cuboid(20.0, 20.0, 20.0);
    b.transform(&Mat4::translation(Vec3::new(10.0, 10.0, 10.0)));
    let i = csg::intersection(&a, &b).unwrap();
    assert!(i.is_closed_manifold());
    assert!(
        (volume(&i) - 1000.0).abs() < 1.0,
        "overlap vol {}",
        volume(&i)
    );
}

#[test]
fn csg_disjoint_union() {
    let a = Mesh::cuboid(10.0, 10.0, 10.0);
    let mut b = Mesh::cuboid(10.0, 10.0, 10.0);
    b.transform(&Mat4::translation(Vec3::new(50.0, 0.0, 0.0)));
    let u = csg::union(&a, &b).unwrap();
    assert!(u.is_closed_manifold());
    assert_eq!(u.euler_characteristic(), 4, "two closed components ⇒ χ = 4");
    assert!((volume(&u) - 2000.0).abs() < 1.0);
}

#[test]
fn csg_rejects_non_manifold_input() {
    // An open sheet (two triangles) is not a closed manifold ⇒ clean error.
    let sheet = Mesh {
        verts: vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ],
        tris: vec![[0, 1, 2]],
    };
    let cube = Mesh::cuboid(10.0, 10.0, 10.0);
    assert!(csg::union(&sheet, &cube).is_err());
}

// ---- shell -----------------------------------------------------------------

#[test]
fn shell_box() {
    let bx = Mesh::cuboid(40.0, 40.0, 20.0);
    let s = csg::shell(&bx, 2.0).unwrap();
    assert!(s.is_closed_manifold(), "shelled box is a closed manifold");
    // outer − inner cavity: 40·40·20 − 36·36·16
    let expected = 40.0 * 40.0 * 20.0 - 36.0 * 36.0 * 16.0;
    assert!(
        (volume(&s) - expected).abs() < expected * 0.02,
        "shell vol {} vs ~{}",
        volume(&s),
        expected
    );
}

// ---- command auto-dispatch (2D vs 3D) --------------------------------------

#[test]
fn boolean_dispatch_by_kind() {
    let mut it = Interpreter::new();
    // 3D: box − drill → a Mesh that is a closed manifold.
    it.exec_line("box 40 40 20 name=block").unwrap();
    it.exec_line("cylinder 8 30 name=drill").unwrap();
    it.exec_line("translate 20 20 -5 target=drill").unwrap();
    it.exec_line("subtract block drill name=holed").unwrap();
    match &it.scene.get("holed").unwrap().geom {
        Geometry::Mesh(m) => assert!(m.is_closed_manifold(), "3D subtract is manifold"),
        _ => panic!("expected a mesh"),
    }

    // 2D still routes to the polygon clipper → a Shape.
    it.exec_line("rect 20 20 name=plate").unwrap();
    it.exec_line("circle 5 name=hole").unwrap();
    it.exec_line("subtract plate hole name=part2d").unwrap();
    assert!(matches!(
        it.scene.get("part2d").unwrap().geom,
        Geometry::Shape(_)
    ));

    // Mixing a 2D shape with a 3D mesh is an error.
    assert!(it.exec_line("subtract plate block").is_err());
}

#[test]
fn shell_command() {
    let mut it = Interpreter::new();
    it.exec_line("box 40 40 20 name=b").unwrap();
    it.exec_line("shell 2 target=b").unwrap();
    match &it.scene.get("b").unwrap().geom {
        Geometry::Mesh(m) => assert!(m.is_closed_manifold()),
        _ => panic!("expected a mesh"),
    }
}

// ---- rounded 3D edges via the Phase-1 profile workflow ---------------------

#[test]
fn rounded_extrude() {
    // Fillet the 2D profile, then extrude ⇒ rounded vertical edges, still a
    // closed manifold. (The roadmap's "fillet on extrude edges", done in 2D.)
    let profile = Shape::rect(40.0, 40.0, 0.0, 0).fillet(5.0, 8);
    let solid = geom3d::extrude(&profile, 10.0).unwrap();
    assert!(solid.is_closed_manifold());
    assert!(
        solid.verts.len() > 8,
        "rounded corners add vertices (got {})",
        solid.verts.len()
    );
}

//! Phase 13 integration tests (roadmap v1.3): 3D edge fillet & chamfer. The
//! bevel works by clipping each sharp convex edge with half-space planes
//! (`geom3d::bevel`, reusing the CSG kernel), scoped to convex/prismatic solids.
//! We assert the hard invariants — closed manifold, genus 0, volume reduced the
//! right way, bbox preserved — plus command auto-dispatch by geometry kind.

use cad_core::geom2d::Shape;
use cad_core::geom3d::{bevel, Mesh};
use cad_core::scene::Geometry;
use cad_core::Interpreter;

fn box_mesh(w: f64, d: f64, h: f64) -> Mesh {
    Mesh::cuboid(w, d, h)
}

#[test]
fn chamfer_box_is_manifold_and_smaller() {
    let b = box_mesh(40.0, 40.0, 20.0);
    let v0 = b.signed_volume();
    let c = bevel::chamfer(&b, 4.0, 20.0).unwrap();
    assert!(c.is_closed_manifold(), "chamfered box is a closed manifold");
    assert_eq!(c.euler_characteristic(), 2, "genus 0 (χ=2)");
    assert!(c.signed_volume() > 0.0, "outward normals");
    assert!(c.signed_volume() < v0, "chamfer removes material");
    // The bounding box is unchanged (chamfer cuts inward from the edges).
    let (a, z) = (b.bbox(), c.bbox());
    assert!((a.size().x - z.size().x).abs() < 1e-6, "bbox x preserved");
    assert!((a.size().z - z.size().z).abs() < 1e-6, "bbox z preserved");
}

#[test]
fn fillet_box_is_manifold_and_between() {
    let b = box_mesh(40.0, 40.0, 20.0);
    let v0 = b.signed_volume();
    let f = bevel::fillet(&b, 5.0, 3, 20.0).unwrap();
    assert!(f.is_closed_manifold(), "filleted box is a closed manifold");
    assert_eq!(f.euler_characteristic(), 2, "genus 0");
    assert!(f.verts.len() > b.verts.len(), "fillet adds geometry");
    // A rounded box has less volume than the sharp box but more than the deep
    // single-plane chamfer at the same size.
    let cham = bevel::chamfer(&b, 5.0, 20.0).unwrap();
    assert!(
        f.signed_volume() < v0 && f.signed_volume() > cham.signed_volume(),
        "fillet volume {} between chamfer {} and box {}",
        f.signed_volume(),
        cham.signed_volume(),
        v0
    );
}

#[test]
fn chamfer_extruded_triangular_prism() {
    // A triangular prism (extruded triangle) has 3 sharp vertical edges + rims.
    let tri = Shape::polygon(3, 20.0);
    let prism = cad_core::geom3d::extrude(&tri, 15.0).unwrap();
    let c = bevel::chamfer(&prism, 2.0, 20.0).unwrap();
    assert!(
        c.is_closed_manifold(),
        "chamfered prism is a closed manifold"
    );
    assert!(c.signed_volume() > 0.0 && c.signed_volume() < prism.signed_volume());
}

#[test]
fn fillet_dispatches_2d_vs_3d() {
    let mut it = Interpreter::new();
    // 2D path: rounds contour corners (unchanged behaviour).
    it.exec_line("rect 40 40 name=r").unwrap();
    it.exec_line("fillet 5 target=r").unwrap();
    match &it.scene.get("r").unwrap().geom {
        Geometry::Shape(s) => assert!(s.area() < 1600.0, "2D fillet trims corners"),
        _ => panic!("r should stay 2D"),
    }
    // 3D path: rounds mesh edges, still a manifold.
    it.exec_line("box 40 40 20 name=b").unwrap();
    it.exec_line("fillet 4 seg=3 target=b").unwrap();
    match &it.scene.get("b").unwrap().geom {
        Geometry::Mesh(m) => assert!(m.is_closed_manifold(), "3D fillet manifold"),
        _ => panic!("b should stay 3D"),
    }
}

#[test]
fn chamfer_command_on_mesh() {
    let mut it = Interpreter::new();
    it.exec_line("box 30 30 30 name=cube").unwrap();
    let msg = it.exec_line("chamfer 3 target=cube").unwrap();
    assert!(msg.contains("chamfered mesh"), "{msg}");
    match &it.scene.get("cube").unwrap().geom {
        Geometry::Mesh(m) => {
            assert!(m.is_closed_manifold());
            assert_eq!(m.euler_characteristic(), 2);
        }
        _ => panic!(),
    }
}

#[test]
fn degenerate_radius_errors() {
    let b = box_mesh(20.0, 20.0, 20.0);
    assert!(
        bevel::fillet(&b, 0.0, 3, 20.0).is_err(),
        "zero radius errors"
    );
    assert!(
        bevel::chamfer(&b, -1.0, 20.0).is_err(),
        "negative setback errors"
    );
}

#[test]
fn bevel_roundtrips_through_save() {
    let mut it = Interpreter::new();
    it.exec_line("box 40 40 20 name=b").unwrap();
    it.exec_line("chamfer 4 target=b").unwrap();
    let vol = match &it.scene.get("b").unwrap().geom {
        Geometry::Mesh(m) => m.signed_volume(),
        _ => panic!(),
    };
    let p = std::env::temp_dir().join("cad_p13.cad");
    it.exec_line(&format!("save {}", p.display())).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("chamfer 4"), "recorded:\n{text}");

    let mut it2 = Interpreter::new();
    it2.exec_line(&format!("load {}", p.display())).unwrap();
    match &it2.scene.get("b").unwrap().geom {
        Geometry::Mesh(m) => assert!((m.signed_volume() - vol).abs() < 1e-6, "replay matches"),
        _ => panic!(),
    }
}

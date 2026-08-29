//! ASCII STL writer for 3D printing / slicing. Units are millimetres.

use crate::geom3d::Mesh;

pub fn to_ascii_stl(meshes: &[(&str, &Mesh)], mm: f64) -> String {
    let mut out = String::new();
    out.push_str("solid cad_assist\n");
    for (_, mesh) in meshes {
        for &t in &mesh.tris {
            let a = mesh.verts[t[0]] * mm;
            let b = mesh.verts[t[1]] * mm;
            let c = mesh.verts[t[2]] * mm;
            let n = (b - a).cross(c - a).normalized();
            out.push_str(&format!("  facet normal {:.6} {:.6} {:.6}\n", n.x, n.y, n.z));
            out.push_str("    outer loop\n");
            for v in [a, b, c] {
                out.push_str(&format!("      vertex {:.6} {:.6} {:.6}\n", v.x, v.y, v.z));
            }
            out.push_str("    endloop\n");
            out.push_str("  endfacet\n");
        }
    }
    out.push_str("endsolid cad_assist\n");
    out
}

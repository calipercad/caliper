//! STL writers (ASCII + binary) for 3D printing / slicing. Units are millimetres.

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
            out.push_str(&format!(
                "  facet normal {:.6} {:.6} {:.6}\n",
                n.x, n.y, n.z
            ));
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

/// Binary STL: 80-byte header + u32 triangle count + 50 bytes per triangle
/// (12 little-endian f32 = normal + 3 vertices, then a u16 attribute = 0).
pub fn to_binary_stl(meshes: &[(&str, &Mesh)], mm: f64) -> Vec<u8> {
    let total: u32 = meshes.iter().map(|(_, m)| m.tris.len() as u32).sum();
    let mut out = Vec::with_capacity(84 + 50 * total as usize);
    out.extend_from_slice(&[0u8; 80]); // header
    out.extend_from_slice(&total.to_le_bytes());
    let push_f32 = |out: &mut Vec<u8>, x: f64| out.extend_from_slice(&(x as f32).to_le_bytes());
    for (_, mesh) in meshes {
        for &t in &mesh.tris {
            let a = mesh.verts[t[0]] * mm;
            let b = mesh.verts[t[1]] * mm;
            let c = mesh.verts[t[2]] * mm;
            let n = (b - a).cross(c - a).normalized();
            for v in [n, a, b, c] {
                push_f32(&mut out, v.x);
                push_f32(&mut out, v.y);
                push_f32(&mut out, v.z);
            }
            out.extend_from_slice(&0u16.to_le_bytes()); // attribute byte count
        }
    }
    out
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! 3D edge fillet & chamfer — round or bevel the sharp convex edges of a solid
//! `Mesh`. This complements the 2D-profile workflow (fillet the profile, then
//! `extrude`): it works on an existing solid, including the extrusion rims the
//! profile workflow leaves sharp.
//!
//! **Method.** A bevel of a convex solid is a convex polytope bounded by the
//! solid's original face planes plus one or more *cutting planes* per sharp
//! convex edge. We build that polytope directly by **vertex enumeration**: every
//! valid triple of planes gives a candidate vertex, we keep the ones inside all
//! the half-spaces, and assemble each plane's incident vertices into a face.
//! This is exact and closed-manifold by construction — it avoids the
//! accumulating fragility of many sequential BSP booleans.
//!
//! - **chamfer(d)** adds one bisector plane per convex edge, cutting it back by
//!   setback `d`.
//! - **fillet(r, seg)** adds `seg` planes per convex edge, each tangent to a
//!   radius-`r` cylinder — a faceted round, the kernel's "precision = segment
//!   count" convention. Corners blend automatically where edges meet.
//!
//! **Scope.** Convex / prismatic solids (boxes, extruded and revolved prisms,
//! wedges). A non-convex solid is rejected with a clean error, since the
//! plane-intersection polytope is only equal to the original for convex input.

use super::Mesh;
use crate::error::{CadError, Result};
use crate::math::{Vec3, EPS};

/// An oriented plane, half-space `normal · x ≤ w` (outward normal).
#[derive(Clone, Copy)]
struct Plane {
    n: Vec3,
    w: f64,
}

/// Chamfer every sharp convex edge back by `setback`.
pub fn chamfer(mesh: &Mesh, setback: f64, angle_deg: f64) -> Result<Mesh> {
    if setback <= EPS {
        return Err(CadError::Arg("chamfer distance must be positive".into()));
    }
    bevel(mesh, angle_deg, |edges, planes| {
        for eb in edges {
            // In-face inward direction on face 0, toward the material.
            let mut g0 = eb.e.cross(eb.n0).normalized();
            if (eb.f0 - eb.p).dot(g0) < 0.0 {
                g0 = g0 * -1.0;
            }
            let a0 = eb.p + g0 * setback;
            let n = (eb.n0 + eb.n1).normalized();
            planes.push(Plane { n, w: n.dot(a0) });
        }
    })
}

/// Round every sharp convex edge with `seg` facets tangent to a radius-`r`
/// cylinder (a faceted fillet).
pub fn fillet(mesh: &Mesh, radius: f64, seg: usize, angle_deg: f64) -> Result<Mesh> {
    if radius <= EPS {
        return Err(CadError::Arg("fillet radius must be positive".into()));
    }
    let seg = seg.max(1);
    bevel(mesh, angle_deg, |edges, planes| {
        for eb in edges {
            // Fillet cylinder axis: distance r inside both faces, in the edge's
            // cross-section plane. Solve [n0; n1; e]·δ = [-r; -r; 0].
            let det = eb.n0.dot(eb.n1.cross(eb.e));
            if det.abs() < 1e-9 {
                continue;
            }
            let delta =
                (eb.n1.cross(eb.e) * (-radius) + eb.e.cross(eb.n0) * (-radius)) * (1.0 / det);
            let c = eb.p + delta;
            let total = eb.n0.dot(eb.n1).clamp(-1.0, 1.0).acos();
            let axis = if rotate(eb.n0, eb.e, total).dot(eb.n1)
                >= rotate(eb.n0, eb.e * -1.0, total).dot(eb.n1)
            {
                eb.e
            } else {
                eb.e * -1.0
            };
            for j in 1..=seg {
                let theta = total * (j as f64) / (seg as f64 + 1.0);
                let n = rotate(eb.n0, axis, theta);
                planes.push(Plane {
                    n,
                    w: n.dot(c) + radius,
                });
            }
        }
    })
}

/// Shared driver: gather the solid's face planes + the op's cutting planes, then
/// rebuild the bounded convex polytope.
fn bevel(
    mesh: &Mesh,
    angle_deg: f64,
    add_cuts: impl Fn(&[EdgeBevel], &mut Vec<Plane>),
) -> Result<Mesh> {
    let mut planes = face_planes(mesh);
    if !is_convex(mesh, &planes) {
        return Err(CadError::Geometry(
            "3D fillet/chamfer needs a convex (prismatic) solid".into(),
        ));
    }
    let edges = convex_feature_edges(mesh, angle_deg);
    if edges.is_empty() {
        return Ok(mesh.clone());
    }
    add_cuts(&edges, &mut planes);
    let out = polytope_from_planes(&planes, mesh)?;
    if out.is_closed_manifold() {
        Ok(out)
    } else {
        Err(CadError::Geometry(
            "bevel: result is not a closed manifold (convex/prismatic solids only)".into(),
        ))
    }
}

/// Distinct outward face planes of the mesh (deduped by quantized normal+offset).
fn face_planes(mesh: &Mesh) -> Vec<Plane> {
    use std::collections::HashMap;
    let mut seen: HashMap<(i64, i64, i64, i64), ()> = HashMap::new();
    let mut out = Vec::new();
    let q = |x: f64| (x * 1e4).round() as i64;
    for t in &mesh.tris {
        let (a, b, c) = (mesh.verts[t[0]], mesh.verts[t[1]], mesh.verts[t[2]]);
        let nn = (b - a).cross(c - a);
        if nn.len() < EPS {
            continue;
        }
        let n = nn.normalized();
        let w = n.dot(a);
        if seen.insert((q(n.x), q(n.y), q(n.z), q(w)), ()).is_none() {
            out.push(Plane { n, w });
        }
    }
    out
}

/// Convex ⇔ every vertex is inside (or on) every face plane.
fn is_convex(mesh: &Mesh, planes: &[Plane]) -> bool {
    let tol = extent(mesh) * 1e-5;
    for v in &mesh.verts {
        for p in planes {
            if p.n.dot(*v) > p.w + tol {
                return false;
            }
        }
    }
    true
}

/// Rebuild the convex polytope bounded by `planes`: enumerate valid triple-plane
/// vertices, then assemble each plane's incident vertices into a face.
fn polytope_from_planes(planes: &[Plane], src: &Mesh) -> Result<Mesh> {
    let ext = extent(src);
    let tol = ext * 1e-6;
    let np = planes.len();

    // Candidate vertices: every plane triple that is inside all half-spaces.
    let mut verts: Vec<Vec3> = Vec::new();
    for i in 0..np {
        for j in (i + 1)..np {
            for k in (j + 1)..np {
                if let Some(x) = three_plane(&planes[i], &planes[j], &planes[k]) {
                    if planes.iter().all(|p| p.n.dot(x) <= p.w + tol) {
                        verts.push(x);
                    }
                }
            }
        }
    }
    if verts.len() < 4 {
        return Err(CadError::Geometry(
            "bevel produced a degenerate solid".into(),
        ));
    }

    // Weld coincident vertices on a grid.
    let (verts, _) = weld(&verts, (ext * 1e-6).max(EPS));

    // Assemble one face per plane from its incident vertices.
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for p in planes {
        let on: Vec<usize> = (0..verts.len())
            .filter(|&vi| (p.n.dot(verts[vi]) - p.w).abs() <= tol * 4.0)
            .collect();
        if on.len() < 3 {
            continue;
        }
        // Order the incident vertices CCW about the face centroid.
        let g = on.iter().fold(Vec3::ZERO, |a, &vi| a + verts[vi]) * (1.0 / on.len() as f64);
        let u = (verts[on[0]] - g).normalized();
        let vv = p.n.cross(u);
        let mut ring = on.clone();
        ring.sort_by(|&a, &b| {
            let aa = (verts[a] - g).dot(u).atan2((verts[a] - g).dot(vv));
            let bb = (verts[b] - g).dot(u).atan2((verts[b] - g).dot(vv));
            aa.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
        });
        // Fan-triangulate, winding so the normal faces outward (== p.n).
        for w in 1..(ring.len() - 1) {
            let (a, b, c) = (ring[0], ring[w], ring[w + 1]);
            let nrm = (verts[b] - verts[a]).cross(verts[c] - verts[a]);
            if nrm.dot(p.n) >= 0.0 {
                tris.push([a, b, c]);
            } else {
                tris.push([a, c, b]);
            }
        }
    }
    Ok(Mesh { verts, tris })
}

/// Intersection point of three planes, or `None` if near-parallel.
fn three_plane(a: &Plane, b: &Plane, c: &Plane) -> Option<Vec3> {
    let det = a.n.dot(b.n.cross(c.n));
    if det.abs() < 1e-9 {
        return None;
    }
    let x = (b.n.cross(c.n) * a.w + c.n.cross(a.n) * b.w + a.n.cross(b.n) * c.w) * (1.0 / det);
    Some(x)
}

/// Weld points within `grid` to unique representatives; returns (points, map).
fn weld(pts: &[Vec3], grid: f64) -> (Vec<Vec3>, Vec<usize>) {
    use std::collections::HashMap;
    let inv = 1.0 / grid;
    let mut map: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut out: Vec<Vec3> = Vec::new();
    let mut idx = vec![0usize; pts.len()];
    for (i, &p) in pts.iter().enumerate() {
        let key = (
            (p.x * inv).round() as i64,
            (p.y * inv).round() as i64,
            (p.z * inv).round() as i64,
        );
        let rep = *map.entry(key).or_insert_with(|| {
            out.push(p);
            out.len() - 1
        });
        idx[i] = rep;
    }
    (out, idx)
}

fn extent(mesh: &Mesh) -> f64 {
    let s = mesh.bbox().size();
    s.x.max(s.y).max(s.z).max(1.0)
}

/// Rodrigues rotation of `v` about unit axis `k` by `theta`.
fn rotate(v: Vec3, k: Vec3, theta: f64) -> Vec3 {
    let (s, c) = theta.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

/// A sharp convex edge and the two faces meeting at it.
struct EdgeBevel {
    p: Vec3,
    e: Vec3,
    n0: Vec3,
    n1: Vec3,
    f0: Vec3,
}

/// Sharp convex edges: dihedral steeper than `angle_deg`, two faces, convex.
fn convex_feature_edges(mesh: &Mesh, angle_deg: f64) -> Vec<EdgeBevel> {
    use std::collections::HashMap;
    let canon = mesh.canonical_index_map();
    let cos_thresh = angle_deg.to_radians().cos();
    let tol = extent(mesh) * 1e-6;

    let mut map: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (fi, t) in mesh.tris.iter().enumerate() {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ca, cb) = (canon[a], canon[b]);
            if ca == cb {
                continue;
            }
            let key = if ca < cb { (ca, cb) } else { (cb, ca) };
            map.entry(key).or_default().push(fi);
        }
    }

    let mut out = Vec::new();
    for ((ca, cb), faces) in map {
        if faces.len() != 2 {
            continue;
        }
        let t0 = mesh.tris[faces[0]];
        let t1 = mesh.tris[faces[1]];
        let n0 = mesh.face_normal(t0);
        let n1 = mesh.face_normal(t1);
        if n0.len() < 0.5 || n1.len() < 0.5 || n0.dot(n1) >= cos_thresh {
            continue;
        }
        let third = |t: [usize; 3]| -> Option<Vec3> {
            t.iter()
                .find(|&&v| canon[v] != ca && canon[v] != cb)
                .map(|&v| mesh.verts[v])
        };
        let (f0, f1) = match (third(t0), third(t1)) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        let w0 = n0.dot(mesh.verts[t0[0]]);
        let w1 = n1.dot(mesh.verts[t1[0]]);
        // Convex ⇔ each face's third vertex is inside the other's half-space.
        if n1.dot(f0) > w1 + tol || n0.dot(f1) > w0 + tol {
            continue;
        }
        let e = (mesh.verts[cb] - mesh.verts[ca]).normalized();
        if e.len() < 0.5 {
            continue;
        }
        out.push(EdgeBevel {
            p: mesh.verts[ca],
            e,
            n0,
            n1,
            f0,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chamfer_box_manifold() {
        let b = Mesh::cuboid(40.0, 40.0, 20.0);
        let c = chamfer(&b, 4.0, 20.0).unwrap();
        assert!(c.is_closed_manifold());
        assert_eq!(c.euler_characteristic(), 2);
        assert!(c.signed_volume() > 0.0 && c.signed_volume() < b.signed_volume());
    }

    #[test]
    fn fillet_box_manifold() {
        let b = Mesh::cuboid(40.0, 40.0, 20.0);
        let f = fillet(&b, 5.0, 3, 20.0).unwrap();
        assert!(f.is_closed_manifold());
        assert_eq!(f.euler_characteristic(), 2);
        assert!(f.signed_volume() < b.signed_volume());
    }

    #[test]
    fn non_convex_errors() {
        // An L-shape prism is not convex → clean error.
        let mut a = Mesh::cuboid(20.0, 20.0, 10.0);
        let mut b = Mesh::cuboid(10.0, 10.0, 10.0);
        b.transform(&crate::math::Mat4::translation(Vec3::new(20.0, 0.0, 0.0)));
        a.append(&b); // not a manifold/convex union, just coexisting boxes
        assert!(chamfer(&a, 2.0, 20.0).is_err());
    }
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Constructive solid geometry on triangle meshes — a BSP-tree boolean kernel
//! (`union` / `difference` / `intersection`) plus a convex-limited `shell`.
//!
//! This is the classic csg.js / BSP approach: each solid becomes a set of
//! convex [`Polygon`]s partitioned into a [`Node`] tree; the trees clip each
//! other, and the surviving polygons are recombined per the operation. All
//! classification is float with a small plane epsilon. Inputs must be closed
//! 2-manifolds (primitives and extrusions are); non-manifold input returns a
//! clean error rather than emitting junk.

use super::Mesh;
use crate::error::{CadError, Result};
use crate::math::{Vec3, EPS};
use std::collections::HashMap;

/// Front/back classification tolerance (coordinates are in scene units).
const PLANE_EPS: f64 = 1e-7;

// Vertex classification bitmask (values OR together across a polygon).
const COPLANAR: i32 = 0;
const FRONT: i32 = 1;
const BACK: i32 = 2;
const SPANNING: i32 = 3;

/// An oriented plane `normal · x = w` (`normal` unit length).
#[derive(Clone, Copy)]
struct Plane {
    normal: Vec3,
    w: f64,
}

impl Plane {
    fn from_points(a: Vec3, b: Vec3, c: Vec3) -> Option<Plane> {
        let n = (b - a).cross(c - a);
        if n.len() < EPS {
            return None; // degenerate / zero-area triangle
        }
        let normal = n.normalized();
        Some(Plane {
            normal,
            w: normal.dot(a),
        })
    }

    fn flip(&mut self) {
        self.normal = self.normal * -1.0;
        self.w = -self.w;
    }

    /// Split `polygon` by this plane, routing the pieces into the four buckets
    /// (csg.js semantics). Spanning polygons are cut, interpolating the crossing
    /// points along each straddling edge.
    fn split_polygon(
        &self,
        polygon: &Polygon,
        coplanar_front: &mut Vec<Polygon>,
        coplanar_back: &mut Vec<Polygon>,
        front: &mut Vec<Polygon>,
        back: &mut Vec<Polygon>,
    ) {
        let mut polygon_type = 0;
        let mut types = Vec::with_capacity(polygon.verts.len());
        for v in &polygon.verts {
            let t = self.normal.dot(*v) - self.w;
            let ty = if t < -PLANE_EPS {
                BACK
            } else if t > PLANE_EPS {
                FRONT
            } else {
                COPLANAR
            };
            polygon_type |= ty;
            types.push(ty);
        }

        match polygon_type {
            COPLANAR => {
                if self.normal.dot(polygon.plane.normal) > 0.0 {
                    coplanar_front.push(polygon.clone());
                } else {
                    coplanar_back.push(polygon.clone());
                }
            }
            FRONT => front.push(polygon.clone()),
            BACK => back.push(polygon.clone()),
            _ => {
                // SPANNING: cut the polygon against the plane.
                let mut f: Vec<Vec3> = Vec::new();
                let mut b: Vec<Vec3> = Vec::new();
                let n = polygon.verts.len();
                for i in 0..n {
                    let j = (i + 1) % n;
                    let (ti, tj) = (types[i], types[j]);
                    let (vi, vj) = (polygon.verts[i], polygon.verts[j]);
                    if ti != BACK {
                        f.push(vi);
                    }
                    if ti != FRONT {
                        b.push(vi);
                    }
                    if (ti | tj) == SPANNING {
                        let t = (self.w - self.normal.dot(vi)) / self.normal.dot(vj - vi);
                        let v = vi.lerp(vj, t);
                        f.push(v);
                        b.push(v);
                    }
                }
                if f.len() >= 3 {
                    front.push(Polygon::with_plane(f, polygon.plane));
                }
                if b.len() >= 3 {
                    back.push(Polygon::with_plane(b, polygon.plane));
                }
            }
        }
    }
}

/// A convex polygon face with a cached plane (CCW, outward-facing).
#[derive(Clone)]
struct Polygon {
    verts: Vec<Vec3>,
    plane: Plane,
}

impl Polygon {
    fn new(verts: Vec<Vec3>) -> Option<Polygon> {
        let plane = Plane::from_points(verts[0], verts[1], verts[2])?;
        Some(Polygon { verts, plane })
    }

    /// Build a polygon reusing a known plane (the pieces of a split share the
    /// original's plane, so we never recompute it from possibly-degenerate cuts).
    fn with_plane(verts: Vec<Vec3>, plane: Plane) -> Polygon {
        Polygon { verts, plane }
    }

    fn flip(&mut self) {
        self.verts.reverse();
        self.plane.flip();
    }
}

/// A node of the BSP tree: a partitioning plane, the coplanar polygons on it,
/// and front/back subtrees.
#[derive(Clone, Default)]
struct Node {
    plane: Option<Plane>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polygons: Vec<Polygon>,
}

impl Node {
    fn new(polygons: Vec<Polygon>) -> Node {
        let mut node = Node::default();
        if !polygons.is_empty() {
            node.build(polygons);
        }
        node
    }

    /// Convert this solid to its complement (flip every polygon and the tree).
    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
        if let Some(pl) = &mut self.plane {
            pl.flip();
        }
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// Recursively remove the parts of `polygons` that are inside this solid.
    fn clip_polygons(&self, polygons: &[Polygon]) -> Vec<Polygon> {
        let plane = match &self.plane {
            Some(p) => *p,
            None => return polygons.to_vec(),
        };
        let (mut cf, mut cb) = (Vec::new(), Vec::new());
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for p in polygons {
            plane.split_polygon(p, &mut cf, &mut cb, &mut front, &mut back);
        }
        // Coplanar-front travels with front, coplanar-back with back.
        front.extend(cf);
        back.extend(cb);
        let mut result = match &self.front {
            Some(n) => n.clip_polygons(&front),
            None => front,
        };
        if let Some(n) = &self.back {
            result.extend(n.clip_polygons(&back));
        }
        // No back subtree ⇒ `back` polygons are inside this solid ⇒ dropped.
        result
    }

    /// Clip this solid's polygons to the volume of `bsp`.
    fn clip_to(&mut self, bsp: &Node) {
        self.polygons = bsp.clip_polygons(&self.polygons);
        if let Some(f) = &mut self.front {
            f.clip_to(bsp);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(bsp);
        }
    }

    fn all_polygons(&self) -> Vec<Polygon> {
        let mut result = self.polygons.clone();
        if let Some(f) = &self.front {
            result.extend(f.all_polygons());
        }
        if let Some(b) = &self.back {
            result.extend(b.all_polygons());
        }
        result
    }

    fn build(&mut self, polygons: Vec<Polygon>) {
        if polygons.is_empty() {
            return;
        }
        if self.plane.is_none() {
            self.plane = Some(polygons[0].plane);
        }
        let plane = self.plane.unwrap();
        let (mut cf, mut cb) = (Vec::new(), Vec::new());
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for p in &polygons {
            plane.split_polygon(p, &mut cf, &mut cb, &mut front, &mut back);
        }
        // Coplanar polygons (either side) stay on this node.
        self.polygons.extend(cf);
        self.polygons.extend(cb);
        if !front.is_empty() {
            self.front
                .get_or_insert_with(|| Box::new(Node::default()))
                .build(front);
        }
        if !back.is_empty() {
            self.back
                .get_or_insert_with(|| Box::new(Node::default()))
                .build(back);
        }
    }
}

// ---- mesh <-> polygon conversion ------------------------------------------

fn mesh_to_polygons(mesh: &Mesh) -> Vec<Polygon> {
    let mut polys = Vec::with_capacity(mesh.tris.len());
    for t in &mesh.tris {
        if let Some(p) = Polygon::new(vec![mesh.verts[t[0]], mesh.verts[t[1]], mesh.verts[t[2]]]) {
            polys.push(p); // zero-area tris are skipped (None)
        }
    }
    polys
}

/// Fan-triangulate the convex output polygons and weld coincident vertices so
/// the result is a clean, watertight mesh.
fn polygons_to_mesh(polys: &[Polygon]) -> Mesh {
    // Grid for welding, scaled to the output extent.
    let (mut lo, mut hi) = (
        Vec3::new(f64::MAX, f64::MAX, f64::MAX),
        Vec3::new(f64::MIN, f64::MIN, f64::MIN),
    );
    for p in polys {
        for v in &p.verts {
            lo = Vec3::new(lo.x.min(v.x), lo.y.min(v.y), lo.z.min(v.z));
            hi = Vec3::new(hi.x.max(v.x), hi.y.max(v.y), hi.z.max(v.z));
        }
    }
    let ext = (hi.x - lo.x).max(hi.y - lo.y).max(hi.z - lo.z).max(1.0);
    let grid = (ext * 1e-6).max(1e-9);
    let inv = 1.0 / grid;
    let key = |v: Vec3| -> (i64, i64, i64) {
        (
            (v.x * inv).round() as i64,
            (v.y * inv).round() as i64,
            (v.z * inv).round() as i64,
        )
    };

    let mut map: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut mesh = Mesh::new();
    let index = |mesh: &mut Mesh, map: &mut HashMap<(i64, i64, i64), usize>, v: Vec3| -> usize {
        *map.entry(key(v)).or_insert_with(|| {
            mesh.verts.push(v);
            mesh.verts.len() - 1
        })
    };

    for p in polys {
        if p.verts.len() < 3 {
            continue;
        }
        let i0 = index(&mut mesh, &mut map, p.verts[0]);
        for k in 1..p.verts.len() - 1 {
            let i1 = index(&mut mesh, &mut map, p.verts[k]);
            let i2 = index(&mut mesh, &mut map, p.verts[k + 1]);
            if i0 != i1 && i1 != i2 && i2 != i0 {
                mesh.tris.push([i0, i1, i2]);
            }
        }
    }
    mesh
}

// ---- public boolean operations --------------------------------------------

fn prep(a: &Mesh, b: &Mesh) -> Result<(Vec<Polygon>, Vec<Polygon>)> {
    if !a.is_closed_manifold() || !b.is_closed_manifold() {
        return Err(CadError::Geometry(
            "boolean input is not a closed manifold".into(),
        ));
    }
    Ok((mesh_to_polygons(a), mesh_to_polygons(b)))
}

/// `a ∪ b`.
pub fn union(a: &Mesh, b: &Mesh) -> Result<Mesh> {
    let (pa, pb) = prep(a, b)?;
    let mut na = Node::new(pa);
    let mut nb = Node::new(pb);
    na.clip_to(&nb);
    nb.clip_to(&na);
    nb.invert();
    nb.clip_to(&na);
    nb.invert();
    na.build(nb.all_polygons());
    Ok(heal_tjunctions(&polygons_to_mesh(&na.all_polygons())))
}

/// `a − b`.
pub fn difference(a: &Mesh, b: &Mesh) -> Result<Mesh> {
    let (pa, pb) = prep(a, b)?;
    let mut na = Node::new(pa);
    let mut nb = Node::new(pb);
    na.invert();
    na.clip_to(&nb);
    nb.clip_to(&na);
    nb.invert();
    nb.clip_to(&na);
    nb.invert();
    na.build(nb.all_polygons());
    na.invert();
    Ok(heal_tjunctions(&polygons_to_mesh(&na.all_polygons())))
}

/// `a ∩ b`.
pub fn intersection(a: &Mesh, b: &Mesh) -> Result<Mesh> {
    let (pa, pb) = prep(a, b)?;
    let mut na = Node::new(pa);
    let mut nb = Node::new(pb);
    na.invert();
    nb.clip_to(&na);
    nb.invert();
    na.clip_to(&nb);
    nb.clip_to(&na);
    na.build(nb.all_polygons());
    na.invert();
    Ok(heal_tjunctions(&polygons_to_mesh(&na.all_polygons())))
}

// ---- shell -----------------------------------------------------------------

/// Hollow a solid: erode it inward by `t` and subtract, leaving walls of
/// thickness `t`. Exact for convex / prismatic solids (box, cylinder, cone,
/// convex extrusions); for shapes the convex erosion cannot represent it
/// returns a clean error rather than junk.
pub fn shell(mesh: &Mesh, t: f64) -> Result<Mesh> {
    if t <= EPS {
        return Err(CadError::Geometry(
            "shell thickness must be positive".into(),
        ));
    }
    if !mesh.is_closed_manifold() {
        return Err(CadError::Geometry(
            "shell input is not a closed manifold".into(),
        ));
    }
    let reach = mesh.bbox().size();
    let reach = reach.x.max(reach.y).max(reach.z).max(1.0) * 4.0;

    // Erode: intersect the solid with each distinct face plane pushed inward by
    // `t`. For a convex solid this is exactly the inward offset.
    let mut inner = mesh.clone();
    for (normal, w) in face_planes(mesh) {
        let keep = halfspace_box(normal, w - t, reach);
        inner = intersection(&inner, &keep)?;
        if inner.tris.is_empty() {
            return Err(CadError::Geometry(
                "shell: thickness too large (nothing left to hollow)".into(),
            ));
        }
    }

    let result = difference(mesh, &inner)?;
    if !result.is_closed_manifold() {
        return Err(CadError::Geometry(
            "shell: geometry too concave or thickness too large".into(),
        ));
    }
    Ok(result)
}

/// Distinct outward face planes of a mesh (deduped by a quantized key).
fn face_planes(mesh: &Mesh) -> Vec<(Vec3, f64)> {
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
            out.push((n, w));
        }
    }
    out
}

/// A large oriented box solid covering the half-space `normal · x ≤ w`, so
/// intersecting a mesh with it clips the mesh by that plane. Reuses
/// [`Mesh::cuboid`]'s correct outward winding under a right-handed basis.
pub(crate) fn halfspace_box(normal: Vec3, w: f64, reach: f64) -> Mesh {
    let n = normal.normalized();
    // Build a right-handed frame {t1, t2, n} (t1 × t2 = n).
    let a = if n.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let t1 = (a - n * n.dot(a)).normalized();
    let t2 = n.cross(t1);

    // Local box: tangents in [-reach, reach], normal in [w - reach, w]
    // (the keep side). Map local X→t1, Y→t2, Z→n.
    let mut m = Mesh::cuboid(2.0 * reach, 2.0 * reach, reach);
    for p in &mut m.verts {
        let lx = -reach + p.x;
        let ly = -reach + p.y;
        let lz = (w - reach) + p.z;
        *p = t1 * lx + t2 * ly + n * lz;
    }
    m
}
/// Eliminate T-junctions: where a vertex lies on another triangle's edge
/// (unavoidable in BSP CSG, since adjacent faces split at different depths),
/// re-triangulate that triangle so the edge is subdivided at the vertex. Turns
/// a watertight-but-non-manifold CSG result into a strict closed 2-manifold.
fn heal_tjunctions(mesh: &Mesh) -> Mesh {
    let verts = &mesh.verts;
    if verts.is_empty() {
        return mesh.clone();
    }
    // Tolerances scaled to the model extent.
    let bb = mesh.bbox();
    let ext = {
        let s = bb.size();
        s.x.max(s.y).max(s.z).max(1.0)
    };
    let tol2 = (ext * 1e-6) * (ext * 1e-6); // squared perpendicular distance

    // Candidate T-vertices strictly interior to segment a→b, sorted along it.
    let edge_pts = |a: Vec3, b: Vec3, skip: [usize; 3]| -> Vec<usize> {
        let ab = b - a;
        let len2 = ab.len_sq();
        if len2 < EPS {
            return Vec::new();
        }
        let mut pts: Vec<usize> = (0..verts.len())
            .filter(|&w| {
                if skip.contains(&w) {
                    return false;
                }
                let ap = verts[w] - a;
                let t = ap.dot(ab) / len2;
                if !(1e-6..=1.0 - 1e-6).contains(&t) {
                    return false;
                }
                (verts[w] - (a + ab * t)).len_sq() < tol2
            })
            .collect();
        pts.sort_by(|&i, &j| {
            let (di, dj) = ((verts[i] - a).dot(ab), (verts[j] - a).dot(ab));
            di.partial_cmp(&dj).unwrap_or(std::cmp::Ordering::Equal)
        });
        pts
    };

    let mut out = Mesh {
        verts: verts.clone(),
        tris: Vec::with_capacity(mesh.tris.len()),
    };
    for &[a, b, c] in &mesh.tris {
        let (pa, pb, pc) = (verts[a], verts[b], verts[c]);
        // Boundary of the (convex) triangle with T-vertices inserted per edge.
        let mut poly = vec![a];
        poly.extend(edge_pts(pa, pb, [a, b, c]));
        poly.push(b);
        poly.extend(edge_pts(pb, pc, [a, b, c]));
        poly.push(c);
        poly.extend(edge_pts(pc, pa, [a, b, c]));
        // Fan from the first corner (valid: the triangle stays convex).
        for k in 1..poly.len() - 1 {
            let (i0, i1, i2) = (poly[0], poly[k], poly[k + 1]);
            if i0 != i1 && i1 != i2 && i2 != i0 {
                out.tris.push([i0, i1, i2]);
            }
        }
    }
    out
}

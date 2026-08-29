//! 3D geometry: triangle meshes, solid primitives and extrusion.

use crate::bbox::Aabb3;
use crate::error::{CadError, Result};
use crate::geom2d::Shape;
use crate::math::{Mat4, Vec2, Vec3, EPS, TAU};

/// A triangle mesh. Faces index into `verts` and are wound counter-clockwise
/// when viewed from outside (right-hand-rule outward normals).
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub verts: Vec<Vec3>,
    pub tris: Vec<[usize; 3]>,
}

impl Mesh {
    pub fn new() -> Self {
        Mesh::default()
    }

    pub fn bbox(&self) -> Aabb3 {
        let mut b = Aabb3::empty();
        for &v in &self.verts {
            b.expand(v);
        }
        b
    }

    pub fn transform(&mut self, m: &Mat4) {
        for v in &mut self.verts {
            *v = m.apply_point(*v);
        }
    }

    pub fn center_origin(&mut self) {
        let c = self.bbox().center();
        self.transform(&Mat4::translation(-c));
    }

    /// Merge another mesh into this one (a naive boolean-free union — the two
    /// solids simply coexist; useful for assemblies and multi-part prints).
    pub fn append(&mut self, other: &Mesh) {
        let base = self.verts.len();
        self.verts.extend_from_slice(&other.verts);
        for t in &other.tris {
            self.tris.push([t[0] + base, t[1] + base, t[2] + base]);
        }
    }

    /// Geometric normal of a face.
    pub fn face_normal(&self, t: [usize; 3]) -> Vec3 {
        let a = self.verts[t[0]];
        let b = self.verts[t[1]];
        let c = self.verts[t[2]];
        (b - a).cross(c - a).normalized()
    }

    /// Unique undirected edges, for wireframe rendering.
    pub fn edges(&self) -> Vec<(usize, usize)> {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        for t in &self.tris {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let e = if a < b { (a, b) } else { (b, a) };
                set.insert(e);
            }
        }
        set.into_iter().collect()
    }

    /// Feature edges for a clean wireframe: boundary edges plus creases where
    /// the two adjacent faces meet at more than `angle_deg`. This drops the
    /// interior diagonals of flat quads and cap fans, so cylinders and boxes
    /// render as crisp silhouettes rather than triangle soup.
    pub fn feature_edges(&self, angle_deg: f64) -> Vec<(usize, usize)> {
        use std::collections::HashMap;
        let cos_thresh = angle_deg.to_radians().cos();
        let mut map: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (fi, t) in self.tris.iter().enumerate() {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let e = if a < b { (a, b) } else { (b, a) };
                map.entry(e).or_default().push(fi);
            }
        }
        let mut out = Vec::new();
        for (edge, faces) in map {
            let keep = if faces.len() < 2 {
                true // boundary/naked edge
            } else {
                let n0 = self.face_normal(self.tris[faces[0]]);
                let n1 = self.face_normal(self.tris[faces[1]]);
                n0.dot(n1) < cos_thresh // crease sharper than threshold
            };
            if keep {
                out.push(edge);
            }
        }
        out
    }

    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    // ---- primitives --------------------------------------------------------

    /// Axis-aligned box with one corner at the origin.
    pub fn cuboid(w: f64, d: f64, h: f64) -> Mesh {
        let mut m = Mesh::new();
        let v = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(w, 0.0, 0.0),
            Vec3::new(w, d, 0.0),
            Vec3::new(0.0, d, 0.0),
            Vec3::new(0.0, 0.0, h),
            Vec3::new(w, 0.0, h),
            Vec3::new(w, d, h),
            Vec3::new(0.0, d, h),
        ];
        m.verts.extend_from_slice(&v);
        // each face CCW from outside
        let quad = |m: &mut Mesh, a, b, c, d| {
            m.tris.push([a, b, c]);
            m.tris.push([a, c, d]);
        };
        quad(&mut m, 0, 3, 2, 1); // bottom (-z)
        quad(&mut m, 4, 5, 6, 7); // top (+z)
        quad(&mut m, 0, 1, 5, 4); // front (-y)
        quad(&mut m, 2, 3, 7, 6); // back (+y)
        quad(&mut m, 1, 2, 6, 5); // right (+x)
        quad(&mut m, 3, 0, 4, 7); // left (-x)
        m
    }

    /// Cylinder along +Z, base centered at origin.
    pub fn cylinder(r: f64, h: f64, seg: usize) -> Mesh {
        let seg = seg.max(3);
        let mut m = Mesh::new();
        // ring vertices bottom then top
        for lvl in 0..2 {
            let z = if lvl == 0 { 0.0 } else { h };
            for i in 0..seg {
                let a = TAU * (i as f64 / seg as f64);
                m.verts.push(Vec3::new(a.cos() * r, a.sin() * r, z));
            }
        }
        let bc = m.verts.len();
        m.verts.push(Vec3::new(0.0, 0.0, 0.0)); // bottom center
        let tc = m.verts.len();
        m.verts.push(Vec3::new(0.0, 0.0, h)); // top center
        for i in 0..seg {
            let n = (i + 1) % seg;
            let b0 = i;
            let b1 = n;
            let t0 = seg + i;
            let t1 = seg + n;
            // side
            m.tris.push([b0, b1, t1]);
            m.tris.push([b0, t1, t0]);
            // bottom cap (-z, wind so normal points down)
            m.tris.push([bc, b1, b0]);
            // top cap (+z)
            m.tris.push([tc, t0, t1]);
        }
        m
    }

    /// Cone along +Z, base centered at origin, apex at (0,0,h).
    pub fn cone(r: f64, h: f64, seg: usize) -> Mesh {
        let seg = seg.max(3);
        let mut m = Mesh::new();
        for i in 0..seg {
            let a = TAU * (i as f64 / seg as f64);
            m.verts.push(Vec3::new(a.cos() * r, a.sin() * r, 0.0));
        }
        let apex = m.verts.len();
        m.verts.push(Vec3::new(0.0, 0.0, h));
        let center = m.verts.len();
        m.verts.push(Vec3::new(0.0, 0.0, 0.0));
        for i in 0..seg {
            let n = (i + 1) % seg;
            m.tris.push([i, n, apex]); // side
            m.tris.push([center, n, i]); // base
        }
        m
    }

    /// UV sphere centered at origin.
    pub fn sphere(r: f64, seg: usize) -> Mesh {
        let seg = seg.max(4);
        let rings = (seg / 2).max(2);
        let mut m = Mesh::new();
        for j in 0..=rings {
            let v = j as f64 / rings as f64;
            let phi = v * std::f64::consts::PI; // 0..pi
            let (sp, cp) = phi.sin_cos();
            for i in 0..=seg {
                let u = i as f64 / seg as f64;
                let theta = u * TAU;
                let (st, ct) = theta.sin_cos();
                m.verts
                    .push(Vec3::new(r * sp * ct, r * sp * st, r * cp));
            }
        }
        let stride = seg + 1;
        for j in 0..rings {
            for i in 0..seg {
                let a = j * stride + i;
                let b = a + stride;
                m.tris.push([a, b, a + 1]);
                m.tris.push([a + 1, b, b + 1]);
            }
        }
        m
    }
}

/// Extrude a 2D shape along +Z by `height`. Caps are triangulated from the
/// outer contour (index 0) via ear clipping; side walls are generated for every
/// closed contour so holes become through-walls.
pub fn extrude(shape: &Shape, height: f64) -> Result<Mesh> {
    if shape.contours.is_empty() {
        return Err(CadError::Geometry("cannot extrude empty shape".into()));
    }
    let mut m = Mesh::new();
    let outer = &shape.contours[0];
    if outer.points.len() < 3 {
        return Err(CadError::Geometry(
            "outer contour needs at least 3 points to extrude".into(),
        ));
    }

    // Bottom and top rings for every contour, remembering index offsets.
    let mut offsets = Vec::new();
    for c in &shape.contours {
        let base = m.verts.len();
        offsets.push((base, c.points.len()));
        for &p in &c.points {
            m.verts.push(Vec3::new(p.x, p.y, 0.0));
        }
        for &p in &c.points {
            m.verts.push(Vec3::new(p.x, p.y, height));
        }
    }

    // Side walls per contour.
    for (ci, c) in shape.contours.iter().enumerate() {
        let (base, n) = offsets[ci];
        if !c.closed || n < 2 {
            continue;
        }
        let top = base + n;
        for i in 0..n {
            let ni = (i + 1) % n;
            let b0 = base + i;
            let b1 = base + ni;
            let t0 = top + i;
            let t1 = top + ni;
            m.tris.push([b0, b1, t1]);
            m.tris.push([b0, t1, t0]);
        }
    }

    // Caps from the outer contour.
    let mut poly = outer.clone();
    poly.ensure_ccw();
    let tris2d = triangulate(&poly.points)?;
    let (base, n) = offsets[0];
    // Map ear-clip indices (into poly.points) back onto the mesh ring. Because
    // ensure_ccw may have reversed the point order, rebuild the ring here.
    let ring_base = m.verts.len();
    for &p in &poly.points {
        m.verts.push(Vec3::new(p.x, p.y, 0.0));
    }
    let ring_top = m.verts.len();
    for &p in &poly.points {
        m.verts.push(Vec3::new(p.x, p.y, height));
    }
    let _ = (base, n); // side walls already used the original ring
    for t in &tris2d {
        // bottom faces downward (-z): reverse winding
        m.tris.push([ring_base + t[0], ring_base + t[2], ring_base + t[1]]);
        // top faces upward (+z)
        m.tris.push([ring_top + t[0], ring_top + t[1], ring_top + t[2]]);
    }

    Ok(m)
}

/// Ear-clipping triangulation of a simple polygon (CCW). Returns index triples
/// into `pts`. Robust enough for convex and mildly concave shapes.
pub fn triangulate(pts: &[Vec2]) -> Result<Vec<[usize; 3]>> {
    let n = pts.len();
    if n < 3 {
        return Err(CadError::Geometry("need >= 3 points to triangulate".into()));
    }
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    let mut guard = 0;
    let max_guard = n * n + 16;

    while idx.len() > 3 {
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let i0 = idx[(i + m - 1) % m];
            let i1 = idx[i];
            let i2 = idx[(i + 1) % m];
            let a = pts[i0];
            let b = pts[i1];
            let c = pts[i2];
            // convex vertex? (CCW polygon => cross > 0)
            if (b - a).cross(c - b) <= EPS {
                continue;
            }
            // no other vertex inside triangle a,b,c
            let mut ear = true;
            for &k in &idx {
                if k == i0 || k == i1 || k == i2 {
                    continue;
                }
                if point_in_tri(pts[k], a, b, c) {
                    ear = false;
                    break;
                }
            }
            if ear {
                out.push([i0, i1, i2]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        guard += 1;
        if !clipped || guard > max_guard {
            // Fallback: fan triangulation from the first vertex. Not ideal for
            // concave polygons but always terminates and never panics.
            out.clear();
            for i in 1..n - 1 {
                out.push([0, i, i + 1]);
            }
            return Ok(out);
        }
    }
    out.push([idx[0], idx[1], idx[2]]);
    Ok(out)
}

fn point_in_tri(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = (p - a).cross(b - a);
    let d2 = (p - b).cross(c - b);
    let d3 = (p - c).cross(a - c);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

/// Revolve an open profile (list of XY points, X = radius, Y = height) around
/// the Z axis to produce a solid of revolution (lathe).
pub fn revolve(profile: &[Vec2], seg: usize) -> Result<Mesh> {
    if profile.len() < 2 {
        return Err(CadError::Geometry("revolve profile needs >= 2 points".into()));
    }
    let seg = seg.max(3);
    let mut m = Mesh::new();
    let rows = profile.len();
    for i in 0..seg {
        let a = TAU * (i as f64 / seg as f64);
        let (s, c) = a.sin_cos();
        for &p in profile {
            m.verts.push(Vec3::new(p.x * c, p.x * s, p.y));
        }
    }
    for i in 0..seg {
        let ni = (i + 1) % seg;
        for j in 0..rows - 1 {
            let a = i * rows + j;
            let b = ni * rows + j;
            let c = ni * rows + j + 1;
            let d = i * rows + j + 1;
            m.tris.push([a, b, c]);
            m.tris.push([a, c, d]);
        }
    }
    Ok(m)
}

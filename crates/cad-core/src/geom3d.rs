//! 3D geometry: triangle meshes, solid primitives and extrusion.

pub mod csg;
pub mod section;

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
                m.verts.push(Vec3::new(r * sp * ct, r * sp * st, r * cp));
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

    // ---- topology checks ---------------------------------------------------

    /// Map every vertex index to a canonical representative index after
    /// snap-deduping coincident vertices on a grid ~1e-6 of the extent. This is
    /// what makes the shared cap/wall ring vertices (which appear once in
    /// `verts` but are referenced by both caps and walls) count as a single
    /// topological vertex.
    fn canonical_index_map(&self) -> Vec<usize> {
        use std::collections::HashMap;
        let n = self.verts.len();
        let mut canon = vec![0usize; n];
        if n == 0 {
            return canon;
        }
        let bb = self.bbox();
        let sz = bb.size();
        let ext = sz.x.max(sz.y).max(sz.z).max(1.0);
        let grid = (ext * 1e-6).max(EPS);
        let inv = 1.0 / grid;
        let mut map: HashMap<(i64, i64, i64), usize> = HashMap::new();
        for (i, &v) in self.verts.iter().enumerate() {
            let k = (
                ((v.x - bb.min.x) * inv).round() as i64,
                ((v.y - bb.min.y) * inv).round() as i64,
                ((v.z - bb.min.z) * inv).round() as i64,
            );
            let rep = *map.entry(k).or_insert(i);
            canon[i] = rep;
        }
        canon
    }

    /// A closed orientable 2-manifold: every undirected edge is shared by
    /// exactly two oppositely-wound triangles. Vertices are snap-deduped first
    /// so the single shared ring welds caps to walls.
    pub fn is_closed_manifold(&self) -> bool {
        use std::collections::HashMap;
        if self.tris.is_empty() {
            return false;
        }
        let canon = self.canonical_index_map();
        let mut dir: HashMap<(usize, usize), i32> = HashMap::new();
        for t in &self.tris {
            let a = canon[t[0]];
            let b = canon[t[1]];
            let c = canon[t[2]];
            if a == b || b == c || c == a {
                return false; // degenerate triangle after dedup
            }
            for (u, v) in [(a, b), (b, c), (c, a)] {
                *dir.entry((u, v)).or_insert(0) += 1;
            }
        }
        for (&(u, v), &cnt) in &dir {
            if cnt != 1 {
                return false; // a directed edge used more than once
            }
            match dir.get(&(v, u)) {
                Some(&1) => {}
                _ => return false, // missing or duplicated opposite half-edge
            }
        }
        true
    }

    /// Euler characteristic `V - E + F` over snap-deduped vertices and unique
    /// undirected edges. For a closed manifold this equals `2 - 2*genus`, so a
    /// plain solid gives 2, one through-hole (genus 1) gives 0, three bores
    /// (genus 3) give -4.
    pub fn euler_characteristic(&self) -> i64 {
        use std::collections::HashSet;
        let canon = self.canonical_index_map();
        let mut vset: HashSet<usize> = HashSet::new();
        for &c in &canon {
            vset.insert(c);
        }
        let mut eset: HashSet<(usize, usize)> = HashSet::new();
        for t in &self.tris {
            let a = canon[t[0]];
            let b = canon[t[1]];
            let c = canon[t[2]];
            for (u, w) in [(a, b), (b, c), (c, a)] {
                let e = if u < w { (u, w) } else { (w, u) };
                eset.insert(e);
            }
        }
        let v = vset.len() as i64;
        let e = eset.len() as i64;
        let f = self.tris.len() as i64;
        v - e + f
    }

    /// Signed volume via the divergence theorem (`Σ a·(b×c)/6`). For a closed
    /// mesh with outward-facing (CCW-from-outside) triangles this is positive
    /// and equals the enclosed volume.
    pub fn signed_volume(&self) -> f64 {
        let mut v = 0.0;
        for t in &self.tris {
            let a = self.verts[t[0]];
            let b = self.verts[t[1]];
            let c = self.verts[t[2]];
            v += a.dot(b.cross(c));
        }
        v / 6.0
    }

    /// Total surface area — the sum of triangle areas.
    pub fn surface_area(&self) -> f64 {
        let mut s = 0.0;
        for t in &self.tris {
            let a = self.verts[t[0]];
            let b = self.verts[t[1]];
            let c = self.verts[t[2]];
            s += (b - a).cross(c - a).len() * 0.5;
        }
        s
    }

    /// Volume centroid (center of mass of the enclosed solid), via signed
    /// tetrahedra to the origin — the partner of [`Mesh::signed_volume`]. Falls
    /// back to the bounding-box center for a degenerate (zero-volume) mesh.
    pub fn centroid(&self) -> Vec3 {
        let mut vol = 0.0;
        let mut acc = Vec3::ZERO;
        for t in &self.tris {
            let a = self.verts[t[0]];
            let b = self.verts[t[1]];
            let c = self.verts[t[2]];
            let v = a.dot(b.cross(c)); // 6× the signed tet volume
            vol += v;
            acc = acc + (a + b + c) * v; // tet centroid = (a+b+c+0)/4; the /4 cancels
        }
        if vol.abs() < EPS {
            return self.bbox().center();
        }
        acc * (1.0 / (vol * 4.0))
    }
}

/// Extrude a 2D shape along +Z by `height`. The shape is normalized (solid
/// outers CCW, holes CW) and split into regions; each region's cap is
/// triangulated with hole bridging and both caps plus the side walls index
/// into ONE shared bottom+top vertex ring (the weld fix), so the result is a
/// watertight closed manifold and every bore becomes a true through-wall.
pub fn extrude(shape: &Shape, height: f64) -> Result<Mesh> {
    let mut s = shape.clone();
    s.normalize_windings();
    if s.is_empty() {
        return Err(CadError::Geometry("cannot extrude empty shape".into()));
    }

    let mut m = Mesh::new();
    let mut built = false;

    for region in s.regions() {
        if region.outer.points.len() < 3 {
            return Err(CadError::Geometry(
                "outer contour needs at least 3 points to extrude".into(),
            ));
        }
        let holes_slices: Vec<&[Vec2]> = region.holes.iter().map(|h| h.points.as_slice()).collect();
        // `ring` = outer verts followed by each hole's verts, in the CW order
        // normalize produced. `tris2d` are CCW index triples into `ring`.
        let (ring, tris2d) = triangulate_with_holes(&region.outer.points, &holes_slices)?;

        // ONE shared bottom ring and ONE shared top ring — both caps and walls
        // index into these (the weld fix). Bridges live only in `tris2d`
        // topology, never as extra vertices.
        let b0 = m.verts.len();
        for &p in &ring {
            m.verts.push(Vec3::new(p.x, p.y, 0.0));
        }
        let t0 = m.verts.len();
        for &p in &ring {
            m.verts.push(Vec3::new(p.x, p.y, height));
        }

        // Caps from the triangulation.
        for t in &tris2d {
            // bottom face points -Z: reverse winding
            m.tris.push([b0 + t[0], b0 + t[2], b0 + t[1]]);
            // top face points +Z
            m.tris.push([t0 + t[0], t0 + t[1], t0 + t[2]]);
        }

        // Side walls, one contiguous span per original loop boundary inside
        // `ring`: the outer loop first, then each hole. The single quad formula
        // yields OUTWARD walls for the CCW outer and INWARD walls for the CW
        // holes — no per-loop special-casing.
        let mut start = 0usize;
        push_wall(&mut m, b0, t0, start, region.outer.points.len());
        start += region.outer.points.len();
        for h in &region.holes {
            let len = h.points.len();
            push_wall(&mut m, b0, t0, start, len);
            start += len;
        }

        built = true;
    }

    if !built {
        return Err(CadError::Geometry("cannot extrude empty shape".into()));
    }
    Ok(m)
}

/// Emit the side-wall quads for one loop occupying `[start, start+len)` inside
/// the shared bottom (`b0`) and top (`t0`) rings.
fn push_wall(m: &mut Mesh, b0: usize, t0: usize, start: usize, len: usize) {
    if len < 2 {
        return;
    }
    for i in 0..len {
        let ni = (i + 1) % len;
        let bi = b0 + start + i;
        let bni = b0 + start + ni;
        let ti = t0 + start + i;
        let tni = t0 + start + ni;
        m.tris.push([bi, bni, tni]);
        m.tris.push([bi, tni, ti]);
    }
}

/// Triangulate a CCW `outer` polygon with CW `holes` using Eberly's
/// keyhole hole-bridging. Returns `(ring, tris)` where `ring` is the flat
/// vertex list — `outer` verts followed by each hole's verts in order — and
/// `tris` are CCW index triples into `ring`. Bridges appear only as interior
/// diagonals shared by two triangles (they cancel), so the cap is watertight
/// and the ring layout matches the extrude wall spans exactly.
pub fn triangulate_with_holes(
    outer: &[Vec2],
    holes: &[&[Vec2]],
) -> Result<(Vec<Vec2>, Vec<[usize; 3]>)> {
    if outer.len() < 3 {
        return Err(CadError::Geometry(
            "outer contour needs at least 3 points to triangulate".into(),
        ));
    }

    // Shared ring: outer verts, then each hole's verts in the given order.
    let mut ring: Vec<Vec2> = Vec::with_capacity(outer.len());
    ring.extend_from_slice(outer);
    let mut hole_base: Vec<usize> = Vec::with_capacity(holes.len());
    for h in holes {
        hole_base.push(ring.len());
        ring.extend_from_slice(h);
    }

    // Working simple polygon as a list of ring indices; start with the outer
    // loop (CCW).
    let mut merged: Vec<usize> = (0..outer.len()).collect();

    // Merge holes right-to-left (descending rightmost x) so successive keyhole
    // bridges never overlap an already-inserted hole.
    let mut order: Vec<usize> = (0..holes.len()).collect();
    order.sort_by(|&i, &j| {
        let xi = rightmost_x(holes[i]);
        let xj = rightmost_x(holes[j]);
        xj.partial_cmp(&xi).unwrap_or(std::cmp::Ordering::Equal)
    });

    for &hi in &order {
        let hole = holes[hi];
        if hole.len() < 3 {
            continue; // degenerate hole: leave it to the wall pass only
        }
        let hk = hole.len();
        let hb = hole_base[hi];

        // Rightmost vertex M of the hole.
        let mut mi = 0usize;
        for i in 1..hk {
            if hole[i].x > hole[mi].x {
                mi = i;
            }
        }
        let m_pt = hole[mi];

        // Mutually visible position P in the current merged polygon.
        let pp = match find_bridge_vertex(&merged, &ring, m_pt) {
            Some(p) => p,
            None => continue, // no visible edge (degenerate); skip its cap hole
        };

        // Splice the hole into the merged polygon via the two-way bridge
        // P<->M: after P, walk the hole in its stored CW order from M back to M,
        // then duplicate P. The two coincident bridge edges form the seam.
        let mut hole_seq: Vec<usize> = Vec::with_capacity(hk + 1);
        for step in 0..=hk {
            hole_seq.push(hb + (mi + step) % hk);
        }
        let mut next: Vec<usize> = Vec::with_capacity(merged.len() + hole_seq.len() + 1);
        next.extend_from_slice(&merged[..=pp]);
        next.extend_from_slice(&hole_seq);
        next.push(merged[pp]);
        next.extend_from_slice(&merged[pp + 1..]);
        merged = next;
    }

    // Ear-clip the merged simple polygon (reusing the shared ear clipper /
    // fan fallback), then remap the local ears back to ring indices.
    let positions: Vec<Vec2> = merged.iter().map(|&ri| ring[ri]).collect();
    let local = triangulate(&positions)?;
    let tris: Vec<[usize; 3]> = local
        .iter()
        .map(|t| [merged[t[0]], merged[t[1]], merged[t[2]]])
        .collect();

    Ok((ring, tris))
}

/// Largest x coordinate among a point list (helper for hole ordering).
fn rightmost_x(pts: &[Vec2]) -> f64 {
    pts.iter().fold(f64::NEG_INFINITY, |acc, p| acc.max(p.x))
}

/// Eberly mutual-visibility search: from hole vertex `m`, cast a +X ray to the
/// nearest merged-polygon edge, take that edge's larger-x endpoint as the
/// candidate `P`, then, if a reflex vertex lies inside triangle `(M, hit, P)`,
/// choose the reflex vertex with the smallest angle to +X instead. Returns the
/// position of `P` within `merged`.
fn find_bridge_vertex(merged: &[usize], ring: &[Vec2], m: Vec2) -> Option<usize> {
    let n = merged.len();
    if n < 3 {
        return None;
    }

    // Nearest +X ray hit against merged edges crossing the line y = m.y.
    let mut best_x = f64::INFINITY;
    let mut hit_edge: Option<(usize, usize)> = None;
    let mut hit_pt = Vec2::ZERO;
    for i in 0..n {
        let ai = i;
        let bi = (i + 1) % n;
        let a = ring[merged[ai]];
        let b = ring[merged[bi]];
        // half-open straddle test avoids double-counting shared vertices
        let straddle = (a.y <= m.y && b.y > m.y) || (b.y <= m.y && a.y > m.y);
        if !straddle {
            continue;
        }
        let dy = b.y - a.y;
        if dy.abs() < EPS {
            continue;
        }
        let t = (m.y - a.y) / dy;
        let x = a.x + t * (b.x - a.x);
        if x >= m.x - EPS && x < best_x {
            best_x = x;
            hit_edge = Some((ai, bi));
            hit_pt = Vec2::new(x, m.y);
        }
    }

    let (ai, bi) = hit_edge?;
    let a = ring[merged[ai]];
    let b = ring[merged[bi]];
    // Candidate P: the intersected edge's endpoint with the larger x.
    let p_pos = if a.x >= b.x { ai } else { bi };
    let p_pt = ring[merged[p_pos]];

    // If the ray hit a vertex directly, it is trivially visible.
    if (p_pt - hit_pt).len() < EPS {
        return Some(p_pos);
    }

    // Otherwise consult reflex vertices inside triangle (M, hit, P).
    let tri = [m, hit_pt, p_pt];
    let mut chosen = p_pos;
    let mut best_angle = (p_pt - m).y.atan2((p_pt - m).x).abs();
    let mut best_dist = (p_pt - m).len_sq();
    for i in 0..n {
        if i == p_pos {
            continue;
        }
        let vprev = ring[merged[(i + n - 1) % n]];
        let v = ring[merged[i]];
        let vnext = ring[merged[(i + 1) % n]];
        // reflex for a CCW polygon: interior turn is clockwise
        let reflex = (v - vprev).cross(vnext - v) < 0.0;
        if !reflex {
            continue;
        }
        if !point_in_tri(v, tri[0], tri[1], tri[2]) {
            continue;
        }
        let d = v - m;
        let ang = d.y.atan2(d.x).abs();
        let dist = d.len_sq();
        if ang < best_angle - EPS || (ang < best_angle + EPS && dist < best_dist) {
            best_angle = ang;
            best_dist = dist;
            chosen = i;
        }
    }
    Some(chosen)
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
            // no other vertex STRICTLY inside triangle a,b,c. Vertices that
            // coincide with an ear corner are skipped: keyhole hole-bridging
            // (see `triangulate_with_holes`) splices duplicate bridge endpoints
            // into the polygon, and a strict/coincidence-aware test is what lets
            // the ear clipper triangulate that zero-width slit instead of
            // stalling into the fan fallback.
            let mut ear = true;
            for &k in &idx {
                if k == i0 || k == i1 || k == i2 {
                    continue;
                }
                let p = pts[k];
                if (p - a).len_sq() < EPS || (p - b).len_sq() < EPS || (p - c).len_sq() < EPS {
                    continue; // coincident with a corner (bridge duplicate)
                }
                if point_in_tri_strict(p, a, b, c) {
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

/// Strictly-inside test (all three edge signs agree by more than `EPS`); points
/// on or near an edge are NOT inside. Used by the ear clipper so a vertex lying
/// on a candidate ear's boundary — common along keyhole bridge seams — does not
/// spuriously veto the ear.
fn point_in_tri_strict(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let d1 = (p - a).cross(b - a);
    let d2 = (p - b).cross(c - b);
    let d3 = (p - c).cross(a - c);
    (d1 < -EPS && d2 < -EPS && d3 < -EPS) || (d1 > EPS && d2 > EPS && d3 > EPS)
}

/// Revolve an open profile (list of XY points, X = radius, Y = height) around
/// the Z axis to produce a solid of revolution (lathe).
pub fn revolve(profile: &[Vec2], seg: usize) -> Result<Mesh> {
    if profile.len() < 2 {
        return Err(CadError::Geometry(
            "revolve profile needs >= 2 points".into(),
        ));
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

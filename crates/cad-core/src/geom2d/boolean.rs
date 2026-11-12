// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Exact-integer 2D boolean operations (blueprint §4).
//!
//! The combinatorial core is a **snap-rounded planar-overlay clipper**: both
//! shapes' edges are snapped onto a shared integer lattice, split into atomic
//! (crossing-free) sub-edges by exact `i128` intersection tests, classified as
//! result-boundary edges by probing membership on the ORIGINAL float loops, then
//! traced into loops by exact rotational ordering and classified into
//! outer/hole by nesting depth.
//!
//! Every *topological* decision (orientation, crossing, collinearity, angular
//! order) is made with exact `i128` arithmetic on the lattice, so the classic
//! float-fragility of boolean cores is removed. Only in/out *membership*
//! consults the float loops, via a ray-cast winding number that is immune to the
//! snap.

use crate::error::{CadError, Result};
use crate::geom2d::{Contour, Shape};
use crate::math::Vec2;
use std::collections::HashMap;

/// The four planar boolean operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Difference,
    Intersection,
    Xor,
}

/// A lattice point (snapped integer coordinate).
///
/// The snap grid is chosen so these stay bounded to ~2^30 (see [`boolean`]),
/// which keeps `orient` products (~2^61) and intersection numerators (~2^93)
/// inside `i128`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct IPt {
    x: i64,
    y: i64,
}

/// Exact orientation of `c` relative to the directed line `a -> b`.
/// `> 0` left (CCW), `< 0` right (CW), `0` collinear. Computed in `i128`.
#[inline]
fn orient(a: IPt, b: IPt, c: IPt) -> i128 {
    let abx = (b.x - a.x) as i128;
    let aby = (b.y - a.y) as i128;
    let acx = (c.x - a.x) as i128;
    let acy = (c.y - a.y) as i128;
    abx * acy - aby * acx
}

/// Whether collinear point `p` lies within the bounding box of segment `a-b`
/// (inclusive). Only meaningful when `p` is already known collinear with `a,b`.
#[inline]
fn between(a: IPt, b: IPt, p: IPt) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// Round the rational `num/den` to the nearest integer, deterministically
/// (ties round toward +∞). `den` may be negative.
#[inline]
fn round_div(num: i128, den: i128) -> i64 {
    let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
    // floor((num/den) + 1/2) == floor((2*num + den) / (2*den))
    (2 * num + den).div_euclid(2 * den) as i64
}

/// Exact intersection point of two segments known to cross in their interiors
/// (non-parallel). Rational coordinates are rounded to the nearest lattice
/// point; because BOTH edges are split at this same rounded point, the
/// arrangement stays consistent (they meet exactly at a shared `IPt`).
fn intersection_point(a1: IPt, a2: IPt, b1: IPt, b2: IPt) -> IPt {
    let rx = (a2.x - a1.x) as i128;
    let ry = (a2.y - a1.y) as i128;
    let sx = (b2.x - b1.x) as i128;
    let sy = (b2.y - b1.y) as i128;
    let denom = rx * sy - ry * sx; // nonzero: caller guarantees non-parallel
    let qx = (b1.x - a1.x) as i128;
    let qy = (b1.y - a1.y) as i128;
    let t_num = qx * sy - qy * sx;
    // point = a1 + (t_num / denom) * r
    let px = a1.x + round_div(t_num * rx, denom);
    let py = a1.y + round_div(t_num * ry, denom);
    IPt { x: px, y: py }
}

/// Signed winding number of `p` about a single closed float loop (Sunday's
/// algorithm). CCW loops contribute `+1` per enclosure, CW loops `-1`.
fn winding_number(p: Vec2, poly: &[Vec2]) -> i32 {
    let n = poly.len();
    if n < 3 {
        return 0;
    }
    let mut wn = 0i32;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        // is_left > 0 => p left of directed edge a->b
        let is_left = (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y);
        if a.y <= p.y {
            if b.y > p.y && is_left > 0.0 {
                wn += 1; // upward crossing, p left of edge
            }
        } else if b.y <= p.y && is_left < 0.0 {
            wn -= 1; // downward crossing, p right of edge
        }
    }
    wn
}

/// Nonzero-rule membership of `p` in the region defined by `loops` (all loops of
/// one operand). Solids are CCW, holes CW, so a point inside a bore nets to
/// winding 0 and reads as outside. Immune to the lattice snap (evaluated on the
/// original float coordinates).
fn winding_nonzero(p: Vec2, loops: &[&[Vec2]]) -> bool {
    let mut wind = 0i32;
    for l in loops {
        wind += winding_number(p, l);
    }
    wind != 0
}

/// Apply the op predicate to per-operand membership.
#[inline]
fn predicate(op: BoolOp, in_a: bool, in_b: bool) -> bool {
    match op {
        BoolOp::Union => in_a || in_b,
        BoolOp::Intersection => in_a && in_b,
        BoolOp::Difference => in_a && !in_b,
        BoolOp::Xor => in_a ^ in_b,
    }
}

/// Normalize `a - 0` into `(0, 2π]` (0 maps to 2π), so the reversed incoming
/// edge is ranked last when picking the next-clockwise edge.
#[inline]
fn norm_2pi(mut a: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    while a <= 0.0 {
        a += tau;
    }
    while a > tau {
        a -= tau;
    }
    a
}

/// The four public entry points delegate here.
pub fn union(a: &Shape, b: &Shape) -> Result<Shape> {
    boolean(a, b, BoolOp::Union)
}
pub fn difference(a: &Shape, b: &Shape) -> Result<Shape> {
    boolean(a, b, BoolOp::Difference)
}
pub fn intersection(a: &Shape, b: &Shape) -> Result<Shape> {
    boolean(a, b, BoolOp::Intersection)
}
pub fn xor(a: &Shape, b: &Shape) -> Result<Shape> {
    boolean(a, b, BoolOp::Xor)
}

/// Compute `a OP b` as a single winding-classified [`Shape`]. See the module
/// docs and blueprint §4 for the algorithm. An empty result is a `Shape` with no
/// contours (`is_empty()` true); the command layer turns that into a graceful
/// error.
pub fn boolean(a: &Shape, b: &Shape, op: BoolOp) -> Result<Shape> {
    // Normalize windings so "filled region" is the nonzero rule over each
    // operand's loops (solids CCW, holes CW). Cloning keeps callers' shapes
    // untouched (booleans are non-destructive).
    let mut a = a.clone();
    let mut b = b.clone();
    a.normalize_windings();
    b.normalize_windings();

    // Trivial-operand shortcuts (also the only place an empty operand is well
    // defined without an arrangement).
    let a_empty = a.is_empty();
    let b_empty = b.is_empty();
    if a_empty || b_empty {
        return Ok(trivial(&a, a_empty, &b, b_empty, op));
    }

    // Gather closed float loops of each operand (open/degenerate contours have
    // no interior and never participate).
    let a_loops: Vec<Vec<Vec2>> = a
        .contours
        .iter()
        .filter(|c| c.closed && c.points.len() >= 3)
        .map(|c| c.points.clone())
        .collect();
    let b_loops: Vec<Vec<Vec2>> = b
        .contours
        .iter()
        .filter(|c| c.closed && c.points.len() >= 3)
        .map(|c| c.points.clone())
        .collect();
    let a_refs: Vec<&[Vec2]> = a_loops.iter().map(|v| v.as_slice()).collect();
    let b_refs: Vec<&[Vec2]> = b_loops.iter().map(|v| v.as_slice()).collect();
    if a_loops.is_empty() && b_loops.is_empty() {
        return Ok(Shape { contours: vec![] });
    }

    // --- 1. Snap grid ------------------------------------------------------
    // ext = union-bbox max extent; grid = ext*1e-9 bounds snapped ints to ~2^30
    // (i128-safe for orient/intersection). origin = bbox min.
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for l in a_loops.iter().chain(b_loops.iter()) {
        for p in l {
            minx = minx.min(p.x);
            miny = miny.min(p.y);
            maxx = maxx.max(p.x);
            maxy = maxy.max(p.y);
        }
    }
    let origin = Vec2::new(minx, miny);
    let ext = (maxx - minx).max(maxy - miny).max(1.0);
    let grid = (ext * 1e-9).max(1e-9);
    let inv = 1.0 / grid;

    // Snap map (IPt -> exact float). Seed with original input coordinates so
    // exact input vertices survive; intersection points fall back to
    // origin + ipt*grid later.
    let mut to_float: HashMap<IPt, Vec2> = HashMap::new();
    let snap = |p: Vec2| -> IPt {
        IPt {
            x: (((p.x - origin.x) * inv).round()) as i64,
            y: (((p.y - origin.y) * inv).round()) as i64,
        }
    };

    // --- 2. Directed integer edges of every closed loop --------------------
    let mut edges: Vec<(IPt, IPt)> = Vec::new();
    for l in a_loops.iter().chain(b_loops.iter()) {
        let n = l.len();
        for i in 0..n {
            let p = l[i];
            let q = l[(i + 1) % n];
            let ip = snap(p);
            let iq = snap(q);
            to_float.entry(ip).or_insert(p);
            to_float.entry(iq).or_insert(q);
            if ip != iq {
                edges.push((ip, iq));
            }
        }
    }
    if edges.is_empty() {
        return Ok(Shape { contours: vec![] });
    }

    // --- 3. Arrangement: split points per edge -----------------------------
    // Each edge starts with its own endpoints; add every proper crossing and
    // every other-edge endpoint lying on it. O(n²) pairwise (fine for our
    // loop sizes).
    let mut splits: Vec<Vec<IPt>> = edges.iter().map(|&(a, b)| vec![a, b]).collect();
    let m = edges.len();
    for i in 0..m {
        for j in (i + 1)..m {
            let (a1, a2) = edges[i];
            let (b1, b2) = edges[j];
            let d1 = orient(b1, b2, a1);
            let d2 = orient(b1, b2, a2);
            let d3 = orient(a1, a2, b1);
            let d4 = orient(a1, a2, b2);
            if d1 == 0 && d2 == 0 {
                // Collinear: resolve overlap by adding each segment's endpoints
                // that lie on the other (no new vertex is created).
                for &p in &[b1, b2] {
                    if between(a1, a2, p) {
                        splits[i].push(p);
                    }
                }
                for &p in &[a1, a2] {
                    if between(b1, b2, p) {
                        splits[j].push(p);
                    }
                }
            } else {
                let strict = d1 != 0
                    && d2 != 0
                    && d3 != 0
                    && d4 != 0
                    && (d1 > 0) != (d2 > 0)
                    && (d3 > 0) != (d4 > 0);
                if strict {
                    let p = intersection_point(a1, a2, b1, b2);
                    to_float
                        .entry(p)
                        .or_insert_with(|| lattice_to_float(p, origin, grid));
                    splits[i].push(p);
                    splits[j].push(p);
                } else {
                    // Endpoint-on-segment touches (T-junctions, shared vertices).
                    if d1 == 0 && between(b1, b2, a1) {
                        splits[j].push(a1);
                    }
                    if d2 == 0 && between(b1, b2, a2) {
                        splits[j].push(a2);
                    }
                    if d3 == 0 && between(a1, a2, b1) {
                        splits[i].push(b1);
                    }
                    if d4 == 0 && between(a1, a2, b2) {
                        splits[i].push(b2);
                    }
                }
            }
        }
    }

    // --- 3b. Emit atomic sub-edges (sorted along each edge) ----------------
    let mut atomic: Vec<(IPt, IPt)> = Vec::new();
    for i in 0..m {
        let (a, b) = edges[i];
        let dx = (b.x - a.x) as i128;
        let dy = (b.y - a.y) as i128;
        let mut pts = std::mem::take(&mut splits[i]);
        pts.sort_by_key(|p| {
            let px = (p.x - a.x) as i128;
            let py = (p.y - a.y) as i128;
            px * dx + py * dy // monotone projection along the edge
        });
        pts.dedup();
        for w in pts.windows(2) {
            if w[0] != w[1] {
                atomic.push((w[0], w[1]));
            }
        }
    }

    // --- 4. Dedup atomic edges by unordered endpoint pair ------------------
    let mut seen: HashMap<(IPt, IPt), ()> = HashMap::new();
    let mut undirected: Vec<(IPt, IPt)> = Vec::new();
    for (s, e) in atomic {
        if s == e {
            continue;
        }
        let key = if s <= e { (s, e) } else { (e, s) };
        if seen.insert(key, ()).is_none() {
            undirected.push(key);
        }
    }

    // --- 5. Classify each atomic edge as a result boundary -----------------
    // Probe just off the midpoint on both sides; keep iff membership differs,
    // and orient the kept edge so the result interior is on its LEFT.
    let flt = |ip: IPt| -> Vec2 {
        *to_float
            .get(&ip)
            .unwrap_or(&lattice_to_float(ip, origin, grid))
    };
    let eps = grid * 0.5;
    let mut kept: Vec<(IPt, IPt)> = Vec::new();
    for (s, e) in undirected {
        let fs = flt(s);
        let fe = flt(e);
        let dir = (fe - fs).normalized();
        if dir.len_sq() < 0.5 {
            continue;
        }
        let nrm = dir.perp(); // +90°: points to the LEFT of s->e
        let mid = (fs + fe) * 0.5;
        let pl = mid + nrm * eps;
        let pr = mid - nrm * eps;
        let res_l = predicate(
            op,
            winding_nonzero(pl, &a_refs),
            winding_nonzero(pl, &b_refs),
        );
        let res_r = predicate(
            op,
            winding_nonzero(pr, &a_refs),
            winding_nonzero(pr, &b_refs),
        );
        if res_l == res_r {
            continue; // interior on neither / both sides: not a boundary
        }
        // Keep with interior on the LEFT: pl is left of s->e, so if the interior
        // is on the pl side keep s->e, otherwise reverse.
        if res_l {
            kept.push((s, e));
        } else {
            kept.push((e, s));
        }
    }
    if kept.is_empty() {
        return Ok(Shape { contours: vec![] });
    }

    // --- 6. Balance guard (G2) + loop tracing ------------------------------
    // Every lattice vertex must have equal in/out degree, else the kept set is
    // non-manifold / self-intersecting: fall back conservatively.
    let mut out_deg: HashMap<IPt, i32> = HashMap::new();
    let mut in_deg: HashMap<IPt, i32> = HashMap::new();
    let mut adj: HashMap<IPt, Vec<usize>> = HashMap::new();
    for (idx, &(s, e)) in kept.iter().enumerate() {
        *out_deg.entry(s).or_insert(0) += 1;
        *in_deg.entry(e).or_insert(0) += 1;
        adj.entry(s).or_default().push(idx);
    }
    for (v, &o) in &out_deg {
        if in_deg.get(v).copied().unwrap_or(0) != o {
            return Err(CadError::Geometry(
                "non-manifold or self-intersecting boolean input".into(),
            ));
        }
    }

    let mut used = vec![false; kept.len()];
    let mut loops: Vec<Vec<IPt>> = Vec::new();
    while let Some(start) = (0..kept.len()).find(|&i| !used[i]) {
        let start_vertex = kept[start].0;
        let mut loop_pts: Vec<IPt> = Vec::new();
        let mut ei = start;
        loop {
            used[ei] = true;
            let (s, e) = kept[ei];
            loop_pts.push(s);
            if e == start_vertex {
                break; // closed
            }
            // At vertex `e` (arrived from `s`), pick the next unused outgoing
            // edge: the first clockwise turn from the reversed incoming dir.
            let v = e;
            let back = ((s.y - v.y) as f64).atan2((s.x - v.x) as f64);
            let mut best: Option<usize> = None;
            let mut best_delta = f64::INFINITY;
            if let Some(cands) = adj.get(&v) {
                for &c in cands {
                    if used[c] {
                        continue;
                    }
                    let (_, ce) = kept[c];
                    let ang = ((ce.y - v.y) as f64).atan2((ce.x - v.x) as f64);
                    let delta = norm_2pi(back - ang);
                    if delta < best_delta {
                        best_delta = delta;
                        best = Some(c);
                    }
                }
            }
            match best {
                Some(nx) => ei = nx,
                None => {
                    return Err(CadError::Geometry(
                        "non-manifold or self-intersecting boolean input".into(),
                    ))
                }
            }
        }
        if loop_pts.len() >= 3 {
            loops.push(loop_pts);
        }
    }

    // --- 7. Lattice -> float contours, drop slivers ------------------------
    let sliver = grid * grid;
    let mut contours: Vec<Contour> = Vec::new();
    for lp in loops {
        let pts: Vec<Vec2> = lp.iter().map(|&ip| flt(ip)).collect();
        let c = Contour::new(pts, true);
        if c.signed_area().abs() < sliver {
            continue;
        }
        contours.push(c);
    }
    if contours.is_empty() {
        return Ok(Shape { contours: vec![] });
    }

    // --- 8. Authoritative outer/hole classification (G1) -------------------
    let mut result = Shape { contours };
    result.normalize_windings();
    Ok(result)
}

/// Map a computed lattice point back to float via the grid.
#[inline]
fn lattice_to_float(ip: IPt, origin: Vec2, grid: f64) -> Vec2 {
    Vec2::new(origin.x + ip.x as f64 * grid, origin.y + ip.y as f64 * grid)
}

/// Result of an op when at least one operand is empty (no arrangement needed).
fn trivial(a: &Shape, a_empty: bool, b: &Shape, b_empty: bool, op: BoolOp) -> Shape {
    let empty = Shape { contours: vec![] };
    match op {
        BoolOp::Union => {
            if a_empty && b_empty {
                empty
            } else if a_empty {
                b.clone()
            } else {
                a.clone()
            }
        }
        BoolOp::Intersection => empty,
        BoolOp::Xor => {
            if a_empty && b_empty {
                empty
            } else if a_empty {
                b.clone()
            } else {
                a.clone()
            }
        }
        BoolOp::Difference => {
            if a_empty {
                empty
            } else {
                a.clone() // a - (empty) = a
            }
        }
    }
}

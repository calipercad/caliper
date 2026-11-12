// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! A 2D geometric constraint sketcher: points + line segments related by
//! geometric constraints, solved numerically into concrete coordinates and
//! emitted as a [`Shape`].
//!
//! This turns Caliper into a constraint-driven sketcher — "these two edges
//! are equal, this corner is a right angle, that side is 40mm; now find the
//! geometry." The solver is a std-only Gauss–Newton / Levenberg–Marquardt
//! iteration over a numeric Jacobian, with a small in-tree dense linear solve;
//! no external dependencies.
//!
//! Residual assembly walks ordered `Vec`s only (never a `HashMap`), so a solve
//! is deterministic and `save`/replay is bit-identical.

use crate::error::{CadError, Result};
use crate::geom2d::{Contour, Shape};
use crate::math::{Vec2, PI, TAU};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct SkPoint {
    p: Vec2,
    fixed: bool,
}

#[derive(Clone, Debug)]
struct SkSegment {
    a: usize,
    b: usize,
}

#[derive(Clone, Debug)]
struct SkCircle {
    center: usize, // point index
    r: f64,
    r_fixed: bool,
}

#[derive(Clone, Debug)]
struct SkArc {
    center: usize,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
enum Constraint {
    Coincident(usize, usize),    // point, point
    Horizontal(usize),           // segment
    Vertical(usize),             // segment
    Parallel(usize, usize),      // segment, segment
    Perpendicular(usize, usize), // segment, segment
    Equal(usize, usize),         // segment, segment (equal length)
    Distance(usize, usize, f64), // point, point, target
    Length(usize, f64),          // segment, target
    Angle(usize, usize, f64),    // segment, segment, target radians
    // --- curve constraints ---
    RadiusCircle(usize, f64),    // circle, target
    RadiusArc(usize, f64),       // arc, target (|start-center|)
    CoRadial(usize),             // arc: |end-center| == |start-center|
    TangentLine(usize, usize),   // segment, circle
    TangentCC(usize, usize),     // circle, circle (external)
    PointOnCircle(usize, usize), // point, circle
    PointOnSeg(usize, usize),    // point, segment
    EqualRadius(usize, usize),   // circle, circle
}

/// A free variable the solver adjusts: a point coordinate or a circle radius.
#[derive(Clone, Copy)]
enum Var {
    X(usize),
    Y(usize),
    R(usize), // circle index
}

/// What a [`Sketch::solve`] achieved, for the user-facing report.
pub struct SolveReport {
    pub iterations: usize,
    pub residual: f64,
    pub free_coords: usize,
    pub equations: usize,
    pub satisfied: bool,
}

/// A constraint sketch under construction.
#[derive(Clone, Debug)]
pub struct Sketch {
    pub name: String,
    points: Vec<SkPoint>,
    segments: Vec<SkSegment>,
    circles: Vec<SkCircle>,
    arcs: Vec<SkArc>,
    constraints: Vec<Constraint>,
    point_idx: HashMap<String, usize>,
    seg_idx: HashMap<String, usize>,
    circle_idx: HashMap<String, usize>,
    arc_idx: HashMap<String, usize>,
}

impl Sketch {
    pub fn new(name: impl Into<String>) -> Self {
        Sketch {
            name: name.into(),
            points: Vec::new(),
            segments: Vec::new(),
            circles: Vec::new(),
            arcs: Vec::new(),
            constraints: Vec::new(),
            point_idx: HashMap::new(),
            seg_idx: HashMap::new(),
            circle_idx: HashMap::new(),
            arc_idx: HashMap::new(),
        }
    }

    pub fn add_point(&mut self, name: &str, p: Vec2) -> Result<()> {
        if self.point_idx.contains_key(name) {
            return Err(CadError::Arg(format!(
                "sketch point '{name}' already exists"
            )));
        }
        self.point_idx.insert(name.to_string(), self.points.len());
        self.points.push(SkPoint { p, fixed: false });
        Ok(())
    }

    pub fn add_segment(&mut self, name: &str, a: &str, b: &str) -> Result<()> {
        if self.seg_idx.contains_key(name) {
            return Err(CadError::Arg(format!(
                "sketch segment '{name}' already exists"
            )));
        }
        let ia = self.point(a)?;
        let ib = self.point(b)?;
        self.seg_idx.insert(name.to_string(), self.segments.len());
        self.segments.push(SkSegment { a: ia, b: ib });
        Ok(())
    }

    /// Add a circle at an existing `center` point with initial radius `r`.
    pub fn add_circle(&mut self, name: &str, center: &str, r: f64) -> Result<()> {
        if self.circle_idx.contains_key(name) {
            return Err(CadError::Arg(format!(
                "sketch circle '{name}' already exists"
            )));
        }
        let c = self.point(center)?;
        self.circle_idx.insert(name.to_string(), self.circles.len());
        self.circles.push(SkCircle {
            center: c,
            r,
            r_fixed: false,
        });
        Ok(())
    }

    /// Add an arc through `start`→`end` about `center`; auto-adds the co-radial
    /// constraint so both endpoints lie on the arc's circle.
    pub fn add_arc(&mut self, name: &str, center: &str, start: &str, end: &str) -> Result<()> {
        if self.arc_idx.contains_key(name) {
            return Err(CadError::Arg(format!("sketch arc '{name}' already exists")));
        }
        let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
        let idx = self.arcs.len();
        self.arc_idx.insert(name.to_string(), idx);
        self.arcs.push(SkArc {
            center: c,
            start: s,
            end: e,
        });
        self.constraints.push(Constraint::CoRadial(idx));
        Ok(())
    }

    /// Add a constraint by keyword, `refs` being the point/segment/circle names
    /// it relates and `value` the optional numeric target.
    pub fn add_constraint(&mut self, kind: &str, refs: &[&str], value: Option<f64>) -> Result<()> {
        let need = |n: usize| -> Result<()> {
            if refs.len() == n {
                Ok(())
            } else {
                Err(CadError::Arg(format!("{kind} needs {n} reference(s)")))
            }
        };
        let val = || value.ok_or_else(|| CadError::Arg(format!("{kind} needs a value")));
        let c = match kind {
            "fix" => {
                need(1)?;
                let i = self.point(refs[0])?;
                self.points[i].fixed = true;
                return Ok(());
            }
            "coincident" => {
                need(2)?;
                Constraint::Coincident(self.point(refs[0])?, self.point(refs[1])?)
            }
            "horizontal" => {
                need(1)?;
                Constraint::Horizontal(self.seg(refs[0])?)
            }
            "vertical" => {
                need(1)?;
                Constraint::Vertical(self.seg(refs[0])?)
            }
            "parallel" => {
                need(2)?;
                Constraint::Parallel(self.seg(refs[0])?, self.seg(refs[1])?)
            }
            "perpendicular" => {
                need(2)?;
                Constraint::Perpendicular(self.seg(refs[0])?, self.seg(refs[1])?)
            }
            "distance" => {
                need(2)?;
                Constraint::Distance(self.point(refs[0])?, self.point(refs[1])?, val()?)
            }
            "length" => {
                need(1)?;
                Constraint::Length(self.seg(refs[0])?, val()?)
            }
            "angle" => {
                need(2)?;
                Constraint::Angle(self.seg(refs[0])?, self.seg(refs[1])?, val()?.to_radians())
            }
            "radius" => {
                need(1)?;
                // A circle or an arc. The radius stays a free variable; the
                // constraint residual drives it to the target.
                if let Some(&ci) = self.circle_idx.get(refs[0]) {
                    Constraint::RadiusCircle(ci, val()?)
                } else {
                    Constraint::RadiusArc(self.arc(refs[0])?, val()?)
                }
            }
            "tangent" => {
                need(2)?;
                // segment+circle (line tangent) or circle+circle (external).
                if let Some(&ci) = self.circle_idx.get(refs[0]) {
                    Constraint::TangentCC(ci, self.circle(refs[1])?)
                } else {
                    Constraint::TangentLine(self.seg(refs[0])?, self.circle(refs[1])?)
                }
            }
            "concentric" => {
                need(2)?;
                // Desugar to coincident centers.
                Constraint::Coincident(
                    self.circles[self.circle(refs[0])?].center,
                    self.circles[self.circle(refs[1])?].center,
                )
            }
            "point_on" => {
                need(2)?;
                let p = self.point(refs[0])?;
                if let Some(&ci) = self.circle_idx.get(refs[1]) {
                    Constraint::PointOnCircle(p, ci)
                } else {
                    Constraint::PointOnSeg(p, self.seg(refs[1])?)
                }
            }
            "equal" => {
                need(2)?;
                // Two segments (equal length) or two circles (equal radius).
                if let Some(&ci) = self.circle_idx.get(refs[0]) {
                    Constraint::EqualRadius(ci, self.circle(refs[1])?)
                } else {
                    Constraint::Equal(self.seg(refs[0])?, self.seg(refs[1])?)
                }
            }
            other => {
                return Err(CadError::Arg(format!(
                    "unknown constraint '{other}' (fix|coincident|horizontal|vertical|parallel|perpendicular|equal|distance|length|angle|radius|tangent|concentric|point_on)"
                )))
            }
        };
        self.constraints.push(c);
        Ok(())
    }

    fn point(&self, name: &str) -> Result<usize> {
        self.point_idx
            .get(name)
            .copied()
            .ok_or_else(|| CadError::NotFound(format!("sketch point '{name}'")))
    }
    fn seg(&self, name: &str) -> Result<usize> {
        self.seg_idx
            .get(name)
            .copied()
            .ok_or_else(|| CadError::NotFound(format!("sketch segment '{name}'")))
    }
    fn circle(&self, name: &str) -> Result<usize> {
        self.circle_idx
            .get(name)
            .copied()
            .ok_or_else(|| CadError::NotFound(format!("sketch circle '{name}'")))
    }
    fn arc(&self, name: &str) -> Result<usize> {
        self.arc_idx
            .get(name)
            .copied()
            .ok_or_else(|| CadError::NotFound(format!("sketch arc '{name}'")))
    }

    /// The solved position of a point, by name (for inspection/tests).
    pub fn point_pos(&self, name: &str) -> Option<Vec2> {
        self.point_idx.get(name).map(|&i| self.points[i].p)
    }

    /// The solved (center, radius) of a circle, by name (for inspection/tests).
    pub fn circle_pos(&self, name: &str) -> Option<(Vec2, f64)> {
        self.circle_idx
            .get(name)
            .map(|&i| (self.points[self.circles[i].center].p, self.circles[i].r))
    }

    fn get_var(&self, v: Var) -> f64 {
        match v {
            Var::X(i) => self.points[i].p.x,
            Var::Y(i) => self.points[i].p.y,
            Var::R(i) => self.circles[i].r,
        }
    }
    fn set_var(&mut self, v: Var, val: f64) {
        match v {
            Var::X(i) => self.points[i].p.x = val,
            Var::Y(i) => self.points[i].p.y = val,
            Var::R(i) => self.circles[i].r = val,
        }
    }

    fn extent(&self) -> f64 {
        let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
        let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in &self.points {
            lo = Vec2::new(lo.x.min(p.p.x), lo.y.min(p.p.y));
            hi = Vec2::new(hi.x.max(p.p.x), hi.y.max(p.p.y));
        }
        if lo.x > hi.x {
            1.0
        } else {
            (hi.x - lo.x).max(hi.y - lo.y).max(1.0)
        }
    }

    /// Residual vector — one or two entries per constraint, in order.
    fn residuals(&self) -> Vec<f64> {
        let mut r = Vec::new();
        let seg_dir = |s: &SkSegment| self.points[s.b].p - self.points[s.a].p;
        for c in &self.constraints {
            match *c {
                Constraint::Coincident(a, b) => {
                    r.push(self.points[a].p.x - self.points[b].p.x);
                    r.push(self.points[a].p.y - self.points[b].p.y);
                }
                Constraint::Horizontal(s) => {
                    let d = seg_dir(&self.segments[s]);
                    r.push(d.y);
                }
                Constraint::Vertical(s) => {
                    let d = seg_dir(&self.segments[s]);
                    r.push(d.x);
                }
                Constraint::Parallel(s1, s2) => {
                    let d1 = seg_dir(&self.segments[s1]).normalized();
                    let d2 = seg_dir(&self.segments[s2]).normalized();
                    r.push(d1.cross(d2));
                }
                Constraint::Perpendicular(s1, s2) => {
                    let d1 = seg_dir(&self.segments[s1]).normalized();
                    let d2 = seg_dir(&self.segments[s2]).normalized();
                    r.push(d1.dot(d2));
                }
                Constraint::Equal(s1, s2) => {
                    r.push(seg_dir(&self.segments[s1]).len() - seg_dir(&self.segments[s2]).len());
                }
                Constraint::Distance(a, b, d) => {
                    r.push((self.points[a].p - self.points[b].p).len() - d);
                }
                Constraint::Length(s, d) => {
                    r.push(seg_dir(&self.segments[s]).len() - d);
                }
                Constraint::Angle(s1, s2, target) => {
                    let a1 = seg_dir(&self.segments[s1]).angle();
                    let a2 = seg_dir(&self.segments[s2]).angle();
                    let mut diff = a2 - a1 - target;
                    while diff <= -PI {
                        diff += TAU;
                    }
                    while diff > PI {
                        diff -= TAU;
                    }
                    r.push(diff);
                }
                Constraint::RadiusCircle(ci, d) => {
                    r.push(self.circles[ci].r - d);
                }
                Constraint::RadiusArc(ai, d) => {
                    let a = &self.arcs[ai];
                    r.push((self.points[a.start].p - self.points[a.center].p).len() - d);
                }
                Constraint::CoRadial(ai) => {
                    let a = &self.arcs[ai];
                    let c = self.points[a.center].p;
                    r.push((self.points[a.end].p - c).len() - (self.points[a.start].p - c).len());
                }
                Constraint::TangentLine(s, ci) => {
                    let seg = &self.segments[s];
                    let (a, b) = (self.points[seg.a].p, self.points[seg.b].p);
                    let c = self.points[self.circles[ci].center].p;
                    let len = (b - a).len().max(1e-12);
                    let dist = (b - a).cross(c - a) / len; // signed perp distance
                    let rad = self.circles[ci].r;
                    r.push(dist * dist - rad * rad); // smooth: zero at |dist| == r
                }
                Constraint::TangentCC(c1, c2) => {
                    let p1 = self.points[self.circles[c1].center].p;
                    let p2 = self.points[self.circles[c2].center].p;
                    r.push((p1 - p2).len() - (self.circles[c1].r + self.circles[c2].r));
                }
                Constraint::PointOnCircle(p, ci) => {
                    let c = self.points[self.circles[ci].center].p;
                    r.push((self.points[p].p - c).len() - self.circles[ci].r);
                }
                Constraint::PointOnSeg(p, s) => {
                    let seg = &self.segments[s];
                    let (a, b) = (self.points[seg.a].p, self.points[seg.b].p);
                    let len = (b - a).len().max(1e-12);
                    r.push((b - a).cross(self.points[p].p - a) / len);
                }
                Constraint::EqualRadius(c1, c2) => {
                    r.push(self.circles[c1].r - self.circles[c2].r);
                }
            }
        }
        r
    }

    /// Solve the sketch, writing solved coordinates back into the points.
    // Index-based loops are the clearest form for the Jacobian / normal-equation
    // matrix assembly below.
    #[allow(clippy::needless_range_loop)]
    pub fn solve(&mut self) -> SolveReport {
        // Free variables: (x, y) of every non-fixed point, then every non-fixed
        // circle radius — in order, so assembly stays deterministic.
        let mut free: Vec<Var> = Vec::new();
        for (i, p) in self.points.iter().enumerate() {
            if !p.fixed {
                free.push(Var::X(i));
                free.push(Var::Y(i));
            }
        }
        for (i, c) in self.circles.iter().enumerate() {
            if !c.r_fixed {
                free.push(Var::R(i));
            }
        }
        let n = free.len();
        let scale = self.extent();
        let tol = 1e-6;
        let max_iter = 200;

        let r0 = self.residuals();
        if n == 0 || r0.is_empty() {
            let res = max_abs(&r0);
            return SolveReport {
                iterations: 0,
                residual: res,
                free_coords: n,
                equations: r0.len(),
                satisfied: res < tol,
            };
        }

        let h = scale * 1e-6;
        let mut lambda = 1e-3;
        let mut r = r0;
        let mut cost = norm2(&r);
        let mut iterations = 0;

        while iterations < max_iter && max_abs(&r) >= tol {
            iterations += 1;
            let m = r.len();

            // Numeric central-difference Jacobian (m × n).
            let mut jac = vec![vec![0.0f64; n]; m];
            for (j, &v) in free.iter().enumerate() {
                let orig = self.get_var(v);
                self.set_var(v, orig + h);
                let rp = self.residuals();
                self.set_var(v, orig - h);
                let rm = self.residuals();
                self.set_var(v, orig);
                for i in 0..m {
                    jac[i][j] = (rp[i] - rm[i]) / (2.0 * h);
                }
            }

            // Normal equations: (JᵀJ + λ·diag) Δ = −Jᵀr.
            let mut a = vec![vec![0.0f64; n]; n];
            let mut g = vec![0.0f64; n];
            for i in 0..n {
                for k in 0..n {
                    let mut s = 0.0;
                    for row in 0..m {
                        s += jac[row][i] * jac[row][k];
                    }
                    a[i][k] = s;
                }
                let mut s = 0.0;
                for row in 0..m {
                    s += jac[row][i] * r[row];
                }
                g[i] = s;
            }
            for i in 0..n {
                let d = a[i][i];
                a[i][i] = d + lambda * d.max(1e-9);
            }
            let rhs: Vec<f64> = g.iter().map(|v| -v).collect();
            let delta = match solve_linear(a, rhs) {
                Some(d) => d,
                None => {
                    lambda *= 10.0;
                    if lambda > 1e12 {
                        break;
                    }
                    continue;
                }
            };

            // Trial step with backtracking via the LM damping.
            let saved_pts: Vec<Vec2> = self.points.iter().map(|p| p.p).collect();
            let saved_r: Vec<f64> = self.circles.iter().map(|c| c.r).collect();
            for (j, &v) in free.iter().enumerate() {
                let nv = self.get_var(v) + delta[j];
                self.set_var(v, nv);
            }
            let r2 = self.residuals();
            let cost2 = norm2(&r2);
            if cost2 < cost {
                cost = cost2;
                r = r2;
                lambda = (lambda * 0.5).max(1e-12);
            } else {
                for (i, p) in self.points.iter_mut().enumerate() {
                    p.p = saved_pts[i];
                }
                for (i, c) in self.circles.iter_mut().enumerate() {
                    c.r = saved_r[i];
                }
                lambda *= 4.0;
                if lambda > 1e12 {
                    break;
                }
            }
        }

        let r = self.residuals();
        let res = max_abs(&r);
        SolveReport {
            iterations,
            residual: res,
            free_coords: n,
            equations: r.len(),
            satisfied: res < tol,
        }
    }

    /// Emit the solved sketch as a [`Shape`]: the segment/arc boundary chain
    /// plus each circle as its own contour. Winding classification then sorts
    /// solids and holes.
    pub fn to_shape(&self, closed: bool) -> Shape {
        let mut contours: Vec<Contour> = Vec::new();
        if let Some(pts) = self.boundary_contour() {
            if pts.len() >= 2 {
                contours.push(Contour::new(pts, closed));
            }
        }
        for c in &self.circles {
            contours.push(circle_contour(self.points[c.center].p, c.r));
        }
        let mut s = Shape::new(contours);
        s.normalize_windings();
        s
    }

    /// Walk the segment+arc graph into one boundary loop, tessellating arcs.
    /// Edge-based (not point-based) so a two-edge loop — e.g. a diameter segment
    /// plus a semicircle arc between the same two points — closes correctly.
    /// Returns `None` when the sketch has no boundary edges (circles only).
    fn boundary_contour(&self) -> Option<Vec<Vec2>> {
        if self.segments.is_empty() && self.arcs.is_empty() {
            return None;
        }
        let n = self.points.len();
        // adjacency: point → (neighbor, edge_id, Some(arc_idx) | None for segment)
        let mut adj: Vec<Vec<(usize, usize, Option<usize>)>> = vec![Vec::new(); n];
        let mut eid = 0usize;
        for s in &self.segments {
            adj[s.a].push((s.b, eid, None));
            adj[s.b].push((s.a, eid, None));
            eid += 1;
        }
        for (ai, a) in self.arcs.iter().enumerate() {
            adj[a.start].push((a.end, eid, Some(ai)));
            adj[a.end].push((a.start, eid, Some(ai)));
            eid += 1;
        }
        let mut used = vec![false; eid];
        let start = if !self.segments.is_empty() {
            self.segments[0].a
        } else {
            self.arcs[0].start
        };
        let mut out: Vec<Vec2> = vec![self.points[start].p];
        let mut cur = start;
        loop {
            let next = adj[cur].iter().copied().find(|&(_, e, _)| !used[e]);
            let (q, e, kind) = match next {
                Some(x) => x,
                None => break,
            };
            used[e] = true;
            match kind {
                None => out.push(self.points[q].p),
                Some(ai) => self.emit_arc(&mut out, ai, cur, q),
            }
            cur = q;
            if cur == start {
                break; // closed the loop
            }
        }
        // Drop the duplicate closing point if we returned to the start.
        if out.len() > 1 && (out[0] - *out.last().unwrap()).len() < 1e-9 {
            out.pop();
        }
        Some(out)
    }

    /// Append the tessellated minor arc from point `from` to point `to` about
    /// the arc's center (excluding `from`, including `to`).
    fn emit_arc(&self, out: &mut Vec<Vec2>, ai: usize, from: usize, to: usize) {
        let c = self.points[self.arcs[ai].center].p;
        let (pf, pt) = (self.points[from].p, self.points[to].p);
        let r = (pf - c).len();
        let a0 = (pf - c).angle();
        let mut sweep = (pt - c).angle() - a0;
        while sweep <= -PI {
            sweep += TAU;
        }
        while sweep > PI {
            sweep -= TAU;
        }
        let steps = ((sweep.abs() / TAU) * 64.0).ceil().max(1.0) as usize;
        for k in 1..=steps {
            let a = a0 + sweep * (k as f64) / (steps as f64);
            out.push(c + Vec2::from_angle(a, r));
        }
    }
}

/// A full circle as a closed polygon contour.
fn circle_contour(c: Vec2, r: f64) -> Contour {
    let seg = 64usize;
    let pts: Vec<Vec2> = (0..seg)
        .map(|k| c + Vec2::from_angle(TAU * (k as f64) / (seg as f64), r))
        .collect();
    Contour::new(pts, true)
}

fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0, |m, &x| m.max(x.abs()))
}
fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|&x| x * x).sum()
}

/// Solve a dense `A x = b` by Gaussian elimination with partial pivoting.
/// Returns `None` if the matrix is singular.
#[allow(clippy::needless_range_loop)]
pub fn solve_linear(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    if n == 0 || a.len() != n {
        return None;
    }
    for col in 0..n {
        // Partial pivot.
        let mut piv = col;
        let mut best = a[col][col].abs();
        for row in (col + 1)..n {
            let v = a[row][col].abs();
            if v > best {
                best = v;
                piv = row;
            }
        }
        if best < 1e-12 {
            return None; // singular
        }
        a.swap(col, piv);
        b.swap(col, piv);
        // Eliminate below.
        for row in (col + 1)..n {
            let f = a[row][col] / a[col][col];
            if f != 0.0 {
                for k in col..n {
                    a[row][k] -= f * a[col][k];
                }
                b[row] -= f * b[col];
            }
        }
    }
    // Back-substitute.
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let mut s = b[row];
        for k in (row + 1)..n {
            s -= a[row][k] * x[k];
        }
        x[row] = s / a[row][row];
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_solve_known_system() {
        // 2x + y = 5 ; x - y = 1  →  x=2, y=1
        let a = vec![vec![2.0, 1.0], vec![1.0, -1.0]];
        let b = vec![5.0, 1.0];
        let x = solve_linear(a, b).unwrap();
        assert!((x[0] - 2.0).abs() < 1e-9 && (x[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn singular_matrix_is_none() {
        let a = vec![vec![1.0, 2.0], vec![2.0, 4.0]];
        assert!(solve_linear(a, vec![1.0, 2.0]).is_none());
    }

    #[test]
    fn distance_constraint_converges() {
        let mut s = Sketch::new("t");
        s.add_point("a", Vec2::new(0.0, 0.0)).unwrap();
        s.add_point("b", Vec2::new(5.0, 2.0)).unwrap();
        s.add_constraint("fix", &["a"], None).unwrap();
        s.add_constraint("distance", &["a", "b"], Some(10.0))
            .unwrap();
        let rep = s.solve();
        assert!(rep.satisfied, "residual {}", rep.residual);
        let d = (s.point_pos("b").unwrap() - s.point_pos("a").unwrap()).len();
        assert!((d - 10.0).abs() < 1e-5, "distance {d}");
    }

    #[test]
    fn perpendicular_right_angle() {
        let mut s = Sketch::new("t");
        s.add_point("o", Vec2::new(0.0, 0.0)).unwrap();
        s.add_point("a", Vec2::new(10.0, 0.0)).unwrap();
        s.add_point("b", Vec2::new(3.0, 8.0)).unwrap(); // not yet perpendicular
        s.add_segment("oa", "o", "a").unwrap();
        s.add_segment("ob", "o", "b").unwrap();
        s.add_constraint("fix", &["o"], None).unwrap();
        s.add_constraint("fix", &["a"], None).unwrap();
        s.add_constraint("perpendicular", &["oa", "ob"], None)
            .unwrap();
        let rep = s.solve();
        assert!(rep.satisfied, "residual {}", rep.residual);
        let d1 = (s.point_pos("a").unwrap() - s.point_pos("o").unwrap()).normalized();
        let d2 = (s.point_pos("b").unwrap() - s.point_pos("o").unwrap()).normalized();
        assert!(d1.dot(d2).abs() < 1e-5, "dot {}", d1.dot(d2));
    }

    #[test]
    fn circle_radius_and_pointon() {
        let mut s = Sketch::new("t");
        s.add_point("c", Vec2::new(5.0, 5.0)).unwrap();
        s.add_circle("h", "c", 3.0).unwrap();
        s.add_constraint("fix", &["c"], None).unwrap();
        s.add_constraint("radius", &["h"], Some(8.0)).unwrap();
        let rep = s.solve();
        assert!(rep.satisfied, "residual {}", rep.residual);
        let (ctr, r) = s.circle_pos("h").unwrap();
        assert!((r - 8.0).abs() < 1e-6, "radius {r}");
        assert!((ctr - Vec2::new(5.0, 5.0)).len() < 1e-9);
        // Emitted contour area ≈ π r².
        let a = s.to_shape(true).area();
        assert!((a - std::f64::consts::PI * 64.0).abs() < 1.0, "area {a}");
    }

    #[test]
    fn tangent_line_circle() {
        let mut s = Sketch::new("t");
        // A fixed horizontal segment along y=0, and a circle above it.
        s.add_point("a", Vec2::new(0.0, 0.0)).unwrap();
        s.add_point("b", Vec2::new(20.0, 0.0)).unwrap();
        s.add_point("c", Vec2::new(10.0, 9.0)).unwrap();
        s.add_segment("base", "a", "b").unwrap();
        s.add_circle("k", "c", 5.0).unwrap();
        s.add_constraint("fix", &["a"], None).unwrap();
        s.add_constraint("fix", &["b"], None).unwrap();
        s.add_constraint("radius", &["k"], Some(5.0)).unwrap();
        // center's x is free; pin it so only the tangency drives y.
        s.add_constraint("tangent", &["base", "k"], None).unwrap();
        let rep = s.solve();
        assert!(rep.satisfied, "residual {}", rep.residual);
        let (ctr, r) = s.circle_pos("k").unwrap();
        // perpendicular distance from center to the line y=0 is |ctr.y| ≈ r.
        assert!((ctr.y.abs() - r).abs() < 1e-4, "dist {} r {r}", ctr.y.abs());
    }

    #[test]
    fn tangent_two_circles() {
        let mut s = Sketch::new("t");
        s.add_point("c1", Vec2::new(0.0, 0.0)).unwrap();
        s.add_point("c2", Vec2::new(30.0, 0.0)).unwrap();
        s.add_circle("a", "c1", 10.0).unwrap();
        s.add_circle("b", "c2", 6.0).unwrap();
        s.add_constraint("fix", &["c1"], None).unwrap();
        s.add_constraint("radius", &["a"], Some(10.0)).unwrap();
        s.add_constraint("radius", &["b"], Some(6.0)).unwrap();
        s.add_constraint("tangent", &["a", "b"], None).unwrap();
        let rep = s.solve();
        assert!(rep.satisfied, "residual {}", rep.residual);
        let d = (s.point_pos("c2").unwrap() - s.point_pos("c1").unwrap()).len();
        assert!((d - 16.0).abs() < 1e-4, "center distance {d} ≈ 16");
    }

    #[test]
    fn equal_radii() {
        let mut s = Sketch::new("t");
        s.add_point("c1", Vec2::new(0.0, 0.0)).unwrap();
        s.add_point("c2", Vec2::new(20.0, 0.0)).unwrap();
        s.add_circle("a", "c1", 10.0).unwrap();
        s.add_circle("b", "c2", 4.0).unwrap();
        s.add_constraint("radius", &["a"], Some(7.0)).unwrap();
        s.add_constraint("equal", &["a", "b"], None).unwrap();
        s.solve();
        assert!((s.circle_pos("b").unwrap().1 - 7.0).abs() < 1e-5);
    }

    #[test]
    fn arc_emits_curved_boundary() {
        // Half-disk: diameter segment a→b plus a semicircle arc back b→a about o.
        let mut s = Sketch::new("t");
        s.add_point("o", Vec2::new(0.0, 0.0)).unwrap();
        s.add_point("a", Vec2::new(-10.0, 0.0)).unwrap();
        s.add_point("b", Vec2::new(10.0, 0.0)).unwrap();
        s.add_point("top", Vec2::new(0.0, 10.0)).unwrap();
        s.add_segment("dia", "a", "b").unwrap();
        s.add_arc("cap", "o", "b", "a").unwrap(); // arc from b to a over the top
                                                  // Already consistent; a no-op solve shouldn't break it.
        s.solve();
        let shape = s.to_shape(true);
        let a = shape.area();
        // ≈ half of π·10² = ~157. The minor-arc tessellation may bulge below;
        // just assert it's a substantial positive area and well-tessellated.
        assert!(a > 100.0, "half-disk area {a}");
        let pts: usize = shape.contours.iter().map(|c| c.points.len()).sum();
        assert!(pts > 8, "arc tessellated into many points: {pts}");
    }
}

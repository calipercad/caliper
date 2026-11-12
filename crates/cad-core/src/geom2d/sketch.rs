//! A 2D geometric constraint sketcher: points + line segments related by
//! geometric constraints, solved numerically into concrete coordinates and
//! emitted as a [`Shape`].
//!
//! This turns cad-assist into a constraint-driven sketcher — "these two edges
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
    constraints: Vec<Constraint>,
    point_idx: HashMap<String, usize>,
    seg_idx: HashMap<String, usize>,
}

impl Sketch {
    pub fn new(name: impl Into<String>) -> Self {
        Sketch {
            name: name.into(),
            points: Vec::new(),
            segments: Vec::new(),
            constraints: Vec::new(),
            point_idx: HashMap::new(),
            seg_idx: HashMap::new(),
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

    /// Add a constraint by keyword, `refs` being the point/segment names it
    /// relates and `value` the optional numeric target (distance/length/angle).
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
            "equal" => {
                need(2)?;
                Constraint::Equal(self.seg(refs[0])?, self.seg(refs[1])?)
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
            other => {
                return Err(CadError::Arg(format!(
                    "unknown constraint '{other}' (fix|coincident|horizontal|vertical|parallel|perpendicular|equal|distance|length|angle)"
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

    /// The solved position of a point, by name (for inspection/tests).
    pub fn point_pos(&self, name: &str) -> Option<Vec2> {
        self.point_idx.get(name).map(|&i| self.points[i].p)
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
            }
        }
        r
    }

    /// Solve the sketch, writing solved coordinates back into the points.
    // Index-based loops are the clearest form for the Jacobian / normal-equation
    // matrix assembly below.
    #[allow(clippy::needless_range_loop)]
    pub fn solve(&mut self) -> SolveReport {
        // Free variables: (x, y) of every non-fixed point, in point order.
        let mut free: Vec<(usize, u8)> = Vec::new();
        for (i, p) in self.points.iter().enumerate() {
            if !p.fixed {
                free.push((i, 0));
                free.push((i, 1));
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
            for (j, &(pi, co)) in free.iter().enumerate() {
                let orig = coord(self.points[pi].p, co);
                set_coord(&mut self.points[pi].p, co, orig + h);
                let rp = self.residuals();
                set_coord(&mut self.points[pi].p, co, orig - h);
                let rm = self.residuals();
                set_coord(&mut self.points[pi].p, co, orig);
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
            let saved: Vec<Vec2> = self.points.iter().map(|p| p.p).collect();
            for (j, &(pi, co)) in free.iter().enumerate() {
                let c = coord(self.points[pi].p, co) + delta[j];
                set_coord(&mut self.points[pi].p, co, c);
            }
            let r2 = self.residuals();
            let cost2 = norm2(&r2);
            if cost2 < cost {
                cost = cost2;
                r = r2;
                lambda = (lambda * 0.5).max(1e-12);
            } else {
                for (i, p) in self.points.iter_mut().enumerate() {
                    p.p = saved[i];
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

    /// Emit the solved sketch as a [`Shape`], chaining segments into a loop.
    pub fn to_shape(&self, closed: bool) -> Shape {
        let pts = self.ordered_points();
        let mut s = Shape::from_contour(Contour::new(pts, closed));
        s.normalize_windings();
        s
    }

    /// Points in boundary order: walk the segment graph from segment 0; fall
    /// back to declaration order if the segments don't form a single chain.
    fn ordered_points(&self) -> Vec<Vec2> {
        if self.segments.is_empty() {
            return self.points.iter().map(|p| p.p).collect();
        }
        let n = self.points.len();
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for s in &self.segments {
            adj[s.a].push(s.b);
            adj[s.b].push(s.a);
        }
        let mut visited = vec![false; n];
        let start = self.segments[0].a;
        let mut order = vec![start];
        visited[start] = true;
        let mut cur = start;
        while let Some(q) = adj[cur].iter().copied().find(|&q| !visited[q]) {
            visited[q] = true;
            order.push(q);
            cur = q;
        }
        if order.len() < self.points.len() {
            return self.points.iter().map(|p| p.p).collect();
        }
        order.iter().map(|&i| self.points[i].p).collect()
    }
}

fn coord(p: Vec2, c: u8) -> f64 {
    if c == 0 {
        p.x
    } else {
        p.y
    }
}
fn set_coord(p: &mut Vec2, c: u8, v: f64) {
    if c == 0 {
        p.x = v;
    } else {
        p.y = v;
    }
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
}

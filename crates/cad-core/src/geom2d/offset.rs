// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! 2D polygon offset / inset — grow or shrink a [`Shape`] by a distance.
//!
//! This is the geometry behind laser kerf and CNC tool-radius compensation,
//! hole clearances, gaskets, and a correct inward offset for shells.
//!
//! **Approach.** Each contour is offset independently into one simple polygon:
//! every edge is moved along its outward normal by the signed distance, and
//! consecutive moved edges are reconnected at each vertex by a *join* — an arc
//! (round), the edge intersection (miter), or a straight chord (bevel) on convex
//! corners, and the edge-line intersection on reflex corners. The sign of the
//! distance alone decides grow-vs-shrink: `−perp(edge_dir)·dist` already points
//! the right way for a solid (CCW) *or* a hole (CW), because the winding encodes
//! which side is material. The offset contours are then winding-classified
//! ([`Shape::normalize_windings`]) so outer-vs-hole is correct, and any contour
//! that collapsed or inverted (inset past its own width) is dropped — an empty
//! result is how "inset too large" surfaces.
//!
//! No boolean union is used, so the result is fast and robust on high-vertex and
//! concave contours alike (for offsets smaller than the feature size; a very
//! large inset that would fold a concave region is dropped rather than returned
//! as self-intersecting junk).

use super::{Contour, Shape};
use crate::error::Result;
use crate::math::{Vec2, EPS};
use std::f64::consts::PI;

/// How convex corners are finished.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JoinKind {
    /// Arc (circular) corners — the robust default.
    Round,
    /// Sharp corners, extended to the edge intersection and clamped by a miter
    /// limit (ratio of miter length to the offset distance); beyond it a bevel.
    Miter(f64),
    /// Flat (chamfered) corners.
    Bevel,
}

/// Offset `shape` by `dist` (positive = outward/grow, negative = inward/shrink).
/// `seg` controls the smoothness of round corners (segments per full circle).
pub fn offset(shape: &Shape, dist: f64, join: JoinKind, seg: usize) -> Result<Shape> {
    if dist.abs() <= EPS {
        return Ok(shape.clone());
    }
    let seg = seg.max(8);
    let mut contours: Vec<Contour> = Vec::new();
    for c in &shape.contours {
        if !c.closed || c.points.len() < 3 {
            // Open polylines and degenerate loops are passed through unchanged;
            // a region offset is defined on closed boundaries.
            contours.push(c.clone());
            continue;
        }
        let orig_sign = c.signed_area().signum();
        let pts = match offset_contour(&c.points, dist, join, seg) {
            Some(p) if p.len() >= 3 => p,
            // `None` = an edge reversed (the inset consumed it): the feature
            // eroded away, so the contour is dropped.
            _ => continue,
        };
        let out = Contour::new(pts, true);
        // Also drop a loop that collapsed to ≈0 area or inverted its winding.
        if out.area() <= EPS || out.signed_area().signum() != orig_sign {
            continue;
        }
        contours.push(out);
    }
    let mut result = Shape::new(contours);
    result.normalize_windings();
    Ok(result)
}

/// Offset one closed polygon's points by `dist`. Returns `None` if any edge
/// reversed — the signal that an inset consumed an edge and the contour should
/// be dropped.
fn offset_contour(pts: &[Vec2], dist: f64, join: JoinKind, seg: usize) -> Option<Vec<Vec2>> {
    let n = pts.len();
    let r = dist.abs();
    let mut out: Vec<Vec2> = Vec::new();
    // `first[i]` = index in `out` of the first point emitted for vertex i.
    let mut first = vec![0usize; n];
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let v = pts[i];
        let next = pts[(i + 1) % n];
        let u = (v - prev).normalized(); // incoming edge direction
        let w = (next - v).normalized(); // outgoing edge direction
        first[i] = out.len();
        if u.len() <= EPS || w.len() <= EPS {
            continue; // skip duplicate points
        }
        // Offset of the incoming edge ends at `b`; the outgoing edge starts at
        // `a`. `−perp(dir)·dist` is the outward move for either winding.
        let b = v - u.perp() * dist;
        let a = v - w.perp() * dist;
        let cross = u.cross(w);
        if cross.abs() <= 1e-12 {
            out.push(a); // straight-through (a ≈ b)
            continue;
        }
        // Convex in the offset direction → outer corner (arc/miter/bevel);
        // reflex → inner corner (single edge-intersection point).
        if cross * dist > 0.0 {
            match join {
                JoinKind::Round => emit_arc(&mut out, v, b, a, r, seg),
                JoinKind::Bevel => {
                    out.push(b);
                    out.push(a);
                }
                JoinKind::Miter(limit) => match line_intersect(b, u, a, w) {
                    Some(apex) if (apex - v).len() <= limit.max(1.0) * r => out.push(apex),
                    _ => {
                        out.push(b);
                        out.push(a);
                    }
                },
            }
        } else {
            match line_intersect(b, u, a, w) {
                Some(apex) => out.push(apex),
                None => {
                    out.push(b);
                    out.push(a);
                }
            }
        }
        if out.len() == first[i] {
            return None; // a vertex produced no point: degenerate
        }
    }
    let m = out.len();
    if m < 3 {
        return None;
    }
    // Validity: each original edge's offset must still run in its original
    // direction. If one reversed, the inset ate through that edge → collapse.
    for i in 0..n {
        let f = first[(i + 1) % n];
        let edge = out[f % m] - out[(f + m - 1) % m];
        let orig = pts[(i + 1) % n] - pts[i];
        if edge.dot(orig) < -EPS {
            return None;
        }
    }
    Some(out)
}

/// Push the minor arc from `from` to `to` around `center` (radius `r`), inclusive
/// of both endpoints, using about `seg` segments per full turn.
fn emit_arc(out: &mut Vec<Vec2>, center: Vec2, from: Vec2, to: Vec2, r: f64, seg: usize) {
    let a0 = (from - center).angle();
    let mut delta = (to - center).angle() - a0;
    while delta > PI {
        delta -= 2.0 * PI;
    }
    while delta < -PI {
        delta += 2.0 * PI;
    }
    let steps = ((delta.abs() / (2.0 * PI)) * seg as f64).ceil().max(1.0) as usize;
    for k in 0..=steps {
        let a = a0 + delta * (k as f64) / (steps as f64);
        out.push(center + Vec2::from_angle(a, r));
    }
}

/// Intersection of the line through `p` (direction `u`) and the line through `q`
/// (direction `v`). `None` when the directions are (near) parallel.
fn line_intersect(p: Vec2, u: Vec2, q: Vec2, v: Vec2) -> Option<Vec2> {
    let denom = u.cross(v);
    if denom.abs() <= EPS {
        return None;
    }
    let t = (q - p).cross(v) / denom;
    Some(p + u * t)
}

impl Shape {
    /// Offset every contour by `dist` (+ grows, − shrinks). See [`offset`].
    pub fn offset(&self, dist: f64, join: JoinKind, seg: usize) -> Result<Shape> {
        offset(self, dist, join, seg)
    }
}

/// Parse a `join=` argument value.
pub fn parse_join(s: &str, miter_limit: f64) -> Result<JoinKind> {
    match s {
        "round" => Ok(JoinKind::Round),
        "miter" => Ok(JoinKind::Miter(miter_limit)),
        "bevel" => Ok(JoinKind::Bevel),
        other => Err(crate::error::CadError::Arg(format!(
            "unknown join '{other}' (round|miter|bevel)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(side: f64) -> Shape {
        Shape::polyline(
            vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(side, 0.0),
                Vec2::new(side, side),
                Vec2::new(0.0, side),
            ],
            true,
        )
    }

    #[test]
    fn outward_grows_area_and_bbox() {
        let s = square(40.0);
        let o = offset(&s, 5.0, JoinKind::Round, 32).unwrap();
        let b = o.bbox();
        assert!((b.width() - 50.0).abs() < 0.2, "width {}", b.width());
        assert!((b.height() - 50.0).abs() < 0.2, "height {}", b.height());
        // 50×50 minus the four 90° corner bits the rounds don't cover:
        // 2500 - (4 - pi) * 25 ≈ 2478.5.
        let expect = 2500.0 - (4.0 - PI) * 25.0;
        assert!(
            (o.area() - expect).abs() < 3.0,
            "area {} ≈ {expect}",
            o.area()
        );
    }

    #[test]
    fn inward_shrinks_area() {
        let s = square(40.0);
        let o = offset(&s, -5.0, JoinKind::Round, 32).unwrap();
        let b = o.bbox();
        assert!((b.width() - 30.0).abs() < 0.2, "width {}", b.width());
        assert!((o.area() - 900.0).abs() < 2.0, "area {}", o.area());
    }

    #[test]
    fn bevel_corner_is_flat() {
        let s = square(40.0);
        let o = offset(&s, 5.0, JoinKind::Bevel, 32).unwrap();
        assert!((o.bbox().width() - 50.0).abs() < 0.2);
        let pts: usize = o.contours.iter().map(|c| c.points.len()).sum();
        assert!(pts <= 12, "bevel corners stay simple: {pts} pts");
    }

    #[test]
    fn miter_corner_is_sharp() {
        let s = square(40.0);
        let o = offset(&s, 5.0, JoinKind::Miter(4.0), 32).unwrap();
        // A mitered square offset is again a sharp square: 50×50, 4 corners.
        assert!((o.bbox().width() - 50.0).abs() < 0.2);
        assert!((o.area() - 2500.0).abs() < 2.0, "area {}", o.area());
    }

    #[test]
    fn inset_to_nothing_is_empty() {
        let s = square(10.0);
        let o = offset(&s, -6.0, JoinKind::Round, 16).unwrap();
        assert!(o.is_empty() || o.area() < 1.0, "area {}", o.area());
    }

    #[test]
    fn zero_distance_is_identity() {
        let s = square(10.0);
        let o = offset(&s, 0.0, JoinKind::Round, 16).unwrap();
        assert!((o.area() - 100.0).abs() < 1e-6);
    }
}

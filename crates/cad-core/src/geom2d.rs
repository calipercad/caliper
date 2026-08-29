//! 2D geometry: contours and shapes.
//!
//! All curved primitives are tessellated to polylines on construction so that
//! the rest of the pipeline (export, render, extrude) is uniform. A [`Shape`]
//! is a set of [`Contour`]s; multiple contours model holes and compound cuts
//! (think: a washer, or a panel with mounting holes).

use crate::bbox::Aabb2;
use crate::math::{rad, Mat4, Vec2, Vec3, EPS, TAU};

/// A single open or closed polyline.
#[derive(Clone, Debug)]
pub struct Contour {
    pub points: Vec<Vec2>,
    pub closed: bool,
}

impl Contour {
    pub fn new(points: Vec<Vec2>, closed: bool) -> Self {
        Contour { points, closed }
    }

    /// Signed area (positive = counter-clockwise) via the shoelace formula.
    pub fn signed_area(&self) -> f64 {
        let n = self.points.len();
        if n < 3 {
            return 0.0;
        }
        let mut a = 0.0;
        for i in 0..n {
            let p = self.points[i];
            let q = self.points[(i + 1) % n];
            a += p.cross(q);
        }
        a * 0.5
    }

    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }

    /// Approximate perimeter length.
    pub fn length(&self) -> f64 {
        let n = self.points.len();
        if n < 2 {
            return 0.0;
        }
        let mut total = 0.0;
        let last = if self.closed { n } else { n - 1 };
        for i in 0..last {
            total += (self.points[(i + 1) % n] - self.points[i]).len();
        }
        total
    }

    /// Ensure counter-clockwise winding (used by triangulation).
    pub fn ensure_ccw(&mut self) {
        if self.signed_area() < 0.0 {
            self.points.reverse();
        }
    }
}

/// A 2D shape: one or more contours. The first is treated as the outer
/// boundary; later contours may be holes.
#[derive(Clone, Debug)]
pub struct Shape {
    pub contours: Vec<Contour>,
}

impl Shape {
    pub fn new(contours: Vec<Contour>) -> Self {
        Shape { contours }
    }

    pub fn from_contour(c: Contour) -> Self {
        Shape { contours: vec![c] }
    }

    pub fn bbox(&self) -> Aabb2 {
        let mut b = Aabb2::empty();
        for c in &self.contours {
            for &p in &c.points {
                b.expand(p);
            }
        }
        b
    }

    /// Total area (outer minus holes, by winding).
    pub fn area(&self) -> f64 {
        let mut a = 0.0;
        for (i, c) in self.contours.iter().enumerate() {
            if i == 0 {
                a += c.area();
            } else {
                a -= c.area();
            }
        }
        a.max(0.0)
    }

    /// Apply a transform (in the XY plane) in place.
    pub fn transform(&mut self, m: &Mat4) {
        for c in &mut self.contours {
            for p in &mut c.points {
                let r = m.apply_point(Vec3::new(p.x, p.y, 0.0));
                *p = Vec2::new(r.x, r.y);
            }
        }
    }

    /// Recenter the shape so its bounding-box center is at the origin.
    pub fn center_origin(&mut self) {
        let c = self.bbox().center();
        self.transform(&Mat4::translation(Vec3::new(-c.x, -c.y, 0.0)));
    }

    // ---- primitive builders ------------------------------------------------

    /// Axis-aligned rectangle with its lower-left corner at the origin.
    /// `rx` > 0 rounds the corners with `seg` segments per quarter.
    pub fn rect(w: f64, h: f64, rx: f64, seg: usize) -> Shape {
        if rx <= EPS {
            let pts = vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(w, 0.0),
                Vec2::new(w, h),
                Vec2::new(0.0, h),
            ];
            return Shape::from_contour(Contour::new(pts, true));
        }
        let r = rx.min(w / 2.0).min(h / 2.0);
        let seg = seg.max(2);
        let mut pts = Vec::new();
        // corner centers: BL, BR, TR, TL and their starting angles
        let corners = [
            (Vec2::new(r, r), rad(180.0)),
            (Vec2::new(w - r, r), rad(270.0)),
            (Vec2::new(w - r, h - r), rad(0.0)),
            (Vec2::new(r, h - r), rad(90.0)),
        ];
        for (center, start) in corners {
            for i in 0..=seg {
                let a = start + rad(90.0) * (i as f64 / seg as f64);
                pts.push(center + Vec2::from_angle(a, r));
            }
        }
        Shape::from_contour(Contour::new(pts, true))
    }

    /// Circle centered at the origin, approximated with `seg` segments.
    pub fn circle(r: f64, seg: usize) -> Shape {
        let seg = seg.max(3);
        let mut pts = Vec::with_capacity(seg);
        for i in 0..seg {
            let a = TAU * (i as f64 / seg as f64);
            pts.push(Vec2::from_angle(a, r));
        }
        Shape::from_contour(Contour::new(pts, true))
    }

    /// Regular polygon centered at the origin, first vertex pointing +X.
    pub fn polygon(sides: usize, r: f64) -> Shape {
        Shape::circle(r, sides.max(3))
    }

    /// A star with `points` spikes.
    pub fn star(points: usize, r_outer: f64, r_inner: f64) -> Shape {
        let points = points.max(2);
        let mut pts = Vec::with_capacity(points * 2);
        for i in 0..points * 2 {
            let a = TAU * (i as f64 / (points * 2) as f64) + rad(90.0);
            let r = if i % 2 == 0 { r_outer } else { r_inner };
            pts.push(Vec2::from_angle(a, r));
        }
        Shape::from_contour(Contour::new(pts, true))
    }

    /// An open polyline from a flat list of coordinates.
    pub fn polyline(points: Vec<Vec2>, closed: bool) -> Shape {
        Shape::from_contour(Contour::new(points, closed))
    }

    /// A circular arc (open) from `start_deg` to `end_deg`.
    pub fn arc(r: f64, start_deg: f64, end_deg: f64, seg: usize) -> Shape {
        let seg = seg.max(1);
        let (a0, a1) = (rad(start_deg), rad(end_deg));
        let mut pts = Vec::with_capacity(seg + 1);
        for i in 0..=seg {
            let a = a0 + (a1 - a0) * (i as f64 / seg as f64);
            pts.push(Vec2::from_angle(a, r));
        }
        Shape::from_contour(Contour::new(pts, false))
    }

    /// Add a contour as a hole (used by skills producing plates with cutouts).
    pub fn add_hole(&mut self, c: Contour) {
        self.contours.push(c);
    }
}

//! 2D geometry: contours and shapes.
//!
//! All curved primitives are tessellated to polylines on construction so that
//! the rest of the pipeline (export, render, extrude) is uniform. A [`Shape`]
//! is a set of [`Contour`]s; multiple contours model holes and compound cuts
//! (think: a washer, or a panel with mounting holes).
//!
//! Each [`Contour`] additionally carries a [`ContourKind`] hint recording when
//! its tessellated `points` came from a known primitive (a full circle or an
//! arc). The `points` remain the single source of truth for render / extrude /
//! boolean; the hint is used only by exporters to emit native CIRCLE/ARC
//! entities. Any point-mutating operation (boolean, fillet, chamfer, non-rigid
//! transform) resets the hint to [`ContourKind::Polyline`].

pub mod boolean;
pub mod text;

use crate::bbox::Aabb2;
use crate::math::{rad, Mat4, Vec2, Vec3, EPS, PI, TAU};

/// Records the primitive origin of a contour's tessellated points, so exporters
/// can round-trip true CIRCLE/ARC entities. Angles are stored in DEGREES.
#[derive(Clone, Debug)]
pub enum ContourKind {
    Polyline,
    Circle { c: Vec2, r: f64 },
    Arc { c: Vec2, r: f64, a0: f64, a1: f64 },
}

/// A single open or closed polyline.
#[derive(Clone, Debug)]
pub struct Contour {
    pub points: Vec<Vec2>,
    pub closed: bool,
    pub kind: ContourKind,
}

impl Contour {
    /// Construct a plain polyline contour. Signature unchanged; the kind hint is
    /// always [`ContourKind::Polyline`].
    pub fn new(points: Vec<Vec2>, closed: bool) -> Self {
        Contour {
            points,
            closed,
            kind: ContourKind::Polyline,
        }
    }

    /// Construct a contour carrying an explicit primitive hint.
    pub fn with_kind(points: Vec<Vec2>, closed: bool, kind: ContourKind) -> Self {
        Contour {
            points,
            closed,
            kind,
        }
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

    /// Counter-clockwise iff the signed area is strictly positive.
    pub fn is_ccw(&self) -> bool {
        self.signed_area() > 0.0
    }

    /// A copy with the point order reversed. The primitive hint is dropped
    /// (a reversed arc is no longer described by the stored `a0..a1` sweep).
    pub fn reversed(&self) -> Contour {
        let mut pts = self.points.clone();
        pts.reverse();
        Contour {
            points: pts,
            closed: self.closed,
            kind: ContourKind::Polyline,
        }
    }

    /// Round the corners of this contour with radius `r`, tessellating each
    /// fillet arc into `seg` segments. See blueprint §7. Produces a
    /// [`ContourKind::Polyline`]. Open contours keep their two endpoints; only
    /// interior joints are rounded.
    pub fn fillet(&self, r: f64, seg: usize) -> Contour {
        let n = self.points.len();
        if n < 3 || r <= EPS {
            return Contour::new(self.points.clone(), self.closed);
        }
        let seg = seg.max(1);
        let mut out: Vec<Vec2> = Vec::new();
        for i in 0..n {
            // Open contours leave their endpoints untouched.
            if !self.closed && (i == 0 || i == n - 1) {
                out.push(self.points[i]);
                continue;
            }
            let p = self.points[(i + n - 1) % n];
            let v = self.points[i];
            let q = self.points[(i + 1) % n];

            let u = (v - p).normalized();
            let w = (q - v).normalized();
            let cross = u.cross(w);
            // Skip near-collinear joints (leaves a finely-tessellated circle
            // untouched).
            if cross.abs() < 1e-9 {
                out.push(v);
                continue;
            }
            let dotv = u.dot(w).clamp(-1.0, 1.0);
            // Turn angle between (v-p) and (q-v), in (0, PI).
            let theta = cross.abs().atan2(dotv);
            // Interior half-angle.
            let phi = (PI - theta) * 0.5;
            let tanphi = phi.tan();
            if !tanphi.is_finite() || tanphi.abs() < 1e-12 {
                out.push(v);
                continue;
            }
            let len_pv = (v - p).len();
            let len_qv = (q - v).len();
            // Setback, clamped so we never cross a corner.
            let mut t = r / tanphi;
            t = t.min(0.5 * len_pv).min(0.5 * len_qv);
            if t <= EPS {
                out.push(v);
                continue;
            }
            // Effective radius consistent with the (possibly clamped) setback.
            let r_eff = t * tanphi;
            let a = v - u * t;
            let b = v + w * t;
            // Inward normal of edge (p->v) toward the corner interior; sign
            // from cross(u, w) so convex and reflex corners both work.
            let sign = if cross > 0.0 { 1.0 } else { -1.0 };
            let n_a = u.perp() * sign;
            let center = a + n_a * r_eff;

            let ang0 = (a - center).angle();
            let ang1 = (b - center).angle();
            // Minor arc: the tangent points are separated by exactly `theta`
            // (< PI), so wrapping the delta to (-PI, PI] picks the right sweep
            // and direction automatically.
            let mut d = ang1 - ang0;
            while d <= -PI {
                d += TAU;
            }
            while d > PI {
                d -= TAU;
            }
            for k in 0..=seg {
                let ak = ang0 + d * (k as f64 / seg as f64);
                out.push(center + Vec2::from_angle(ak, r_eff));
            }
        }
        Contour::new(out, self.closed)
    }

    /// Bevel the corners of this contour with leg length `d`. See blueprint §7.
    /// Produces a [`ContourKind::Polyline`]. Open contours keep their endpoints.
    pub fn chamfer(&self, d: f64) -> Contour {
        let n = self.points.len();
        if n < 3 || d <= EPS {
            return Contour::new(self.points.clone(), self.closed);
        }
        let mut out: Vec<Vec2> = Vec::new();
        for i in 0..n {
            if !self.closed && (i == 0 || i == n - 1) {
                out.push(self.points[i]);
                continue;
            }
            let p = self.points[(i + n - 1) % n];
            let v = self.points[i];
            let q = self.points[(i + 1) % n];

            let u = (v - p).normalized();
            let w = (q - v).normalized();
            let cross = u.cross(w);
            if cross.abs() < 1e-9 {
                out.push(v);
                continue;
            }
            let len_pv = (v - p).len();
            let len_qv = (q - v).len();
            let dd = d.min(0.5 * len_pv).min(0.5 * len_qv);
            if dd <= EPS {
                out.push(v);
                continue;
            }
            out.push(v - u * dd);
            out.push(v + w * dd);
        }
        Contour::new(out, self.closed)
    }
}

/// A shell (`outer`) together with the holes nested directly inside it.
pub struct Region<'a> {
    pub outer: &'a Contour,
    pub holes: Vec<&'a Contour>,
}

/// A 2D shape: one or more contours. Solids are CCW, holes CW (after
/// [`Shape::normalize_windings`]); nesting depth decides which is which.
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

    /// Total area: solids (CCW) minus holes (CW), floored at zero. The shape is
    /// cloned and winding-normalized first so legacy CCW-hole shapes are scored
    /// correctly (blueprint §5).
    pub fn area(&self) -> f64 {
        let mut s = self.clone();
        s.normalize_windings();
        let mut a = 0.0;
        for c in &s.contours {
            let sa = c.signed_area();
            if sa > 0.0 {
                a += sa;
            } else {
                a -= sa.abs();
            }
        }
        a.max(0.0)
    }

    /// Classify every contour by nesting-depth parity (even = solid, odd =
    /// hole) and force solids CCW, holes CW. Legacy CCW-hole shapes (e.g. a
    /// washer authored as two CCW loops) are thereby repaired into true
    /// through-holes. Blueprint §5.
    pub fn normalize_windings(&mut self) {
        let n = self.contours.len();
        if n == 0 {
            return;
        }
        // Snapshot polygons and interior test points before mutating.
        let polys: Vec<Vec<Vec2>> = self.contours.iter().map(|c| c.points.clone()).collect();
        let interiors: Vec<Option<Vec2>> =
            polys.iter().map(|p| contour_interior_point(p)).collect();

        // Index-based: `i` indexes both `self.contours` and `interiors`.
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            let tp = match interiors[i] {
                Some(p) => p,
                None => continue,
            };
            let mut depth = 0usize;
            for j in 0..n {
                if i == j {
                    continue;
                }
                if polys[j].len() >= 3 && point_in_polygon(tp, &polys[j]) {
                    depth += 1;
                }
            }
            // Even depth => solid shell (CCW); odd depth => hole (CW).
            let want_ccw = depth.is_multiple_of(2);
            let is_ccw = self.contours[i].signed_area() > 0.0;
            if want_ccw != is_ccw {
                self.contours[i].points.reverse();
                // Points changed order: drop any primitive hint.
                self.contours[i].kind = ContourKind::Polyline;
            }
        }
    }

    /// Group contours into regions: each hole (CW) is assigned to the
    /// smallest-area shell (CCW) whose polygon contains the hole's interior test
    /// point. Nested solids become their own regions. Blueprint §5.
    pub fn regions(&self) -> Vec<Region<'_>> {
        let mut shells: Vec<usize> = Vec::new();
        let mut holes: Vec<usize> = Vec::new();
        for (i, c) in self.contours.iter().enumerate() {
            if c.points.len() < 3 {
                continue;
            }
            if c.signed_area() > 0.0 {
                shells.push(i);
            } else {
                holes.push(i);
            }
        }
        // Ascending shell area, so the first containing shell is the smallest.
        shells.sort_by(|&a, &b| {
            self.contours[a]
                .area()
                .partial_cmp(&self.contours[b].area())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut regions: Vec<Region<'_>> = shells
            .iter()
            .map(|&si| Region {
                outer: &self.contours[si],
                holes: Vec::new(),
            })
            .collect();

        for &hi in &holes {
            let tp = match contour_interior_point(&self.contours[hi].points) {
                Some(p) => p,
                None => continue,
            };
            let mut assigned: Option<usize> = None;
            for (pos, &si) in shells.iter().enumerate() {
                if point_in_polygon(tp, &self.contours[si].points) {
                    assigned = Some(pos);
                    break;
                }
            }
            match assigned {
                Some(pos) => regions[pos].holes.push(&self.contours[hi]),
                None => debug_assert!(false, "hole with no containing shell"),
            }
        }
        regions
    }

    /// Empty iff there are no contours, or every contour is degenerate (< 3
    /// points).
    pub fn is_empty(&self) -> bool {
        self.contours.is_empty() || self.contours.iter().all(|c| c.points.len() < 3)
    }

    // ---- boolean ops (delegate to the exact-integer clipper) ----------------

    pub fn union(&self, o: &Shape) -> crate::error::Result<Shape> {
        boolean::union(self, o)
    }

    pub fn difference(&self, o: &Shape) -> crate::error::Result<Shape> {
        boolean::difference(self, o)
    }

    pub fn intersection(&self, o: &Shape) -> crate::error::Result<Shape> {
        boolean::intersection(self, o)
    }

    pub fn xor(&self, o: &Shape) -> crate::error::Result<Shape> {
        boolean::xor(self, o)
    }

    // ---- fillet / chamfer ---------------------------------------------------

    /// Round every contour's corners, preserving each contour's winding.
    pub fn fillet(&self, r: f64, seg: usize) -> Shape {
        Shape {
            contours: self.contours.iter().map(|c| c.fillet(r, seg)).collect(),
        }
    }

    /// Bevel every contour's corners, preserving each contour's winding.
    pub fn chamfer(&self, d: f64) -> Shape {
        Shape {
            contours: self.contours.iter().map(|c| c.chamfer(d)).collect(),
        }
    }

    /// Apply a transform (in the XY plane) in place, preserving the primitive
    /// hint only when the transform keeps the primitive's shape (blueprint §2,
    /// graft G4).
    ///
    /// - full circles tolerate uniform scale + rotation/reflection + translation;
    /// - arcs are kept only under pure positive uniform scale + translation
    ///   (no rotation/shear/reflection, which would invalidate `a0..a1`);
    /// - anything else downgrades to [`ContourKind::Polyline`] (conservative).
    pub fn transform(&mut self, m: &Mat4) {
        // Characterize the linear (3x3) part in the XY plane.
        let ex = m.apply_dir(Vec3::new(1.0, 0.0, 0.0)).xy();
        let ey = m.apply_dir(Vec3::new(0.0, 1.0, 0.0)).xy();
        let sx = ex.len();
        let sy = ey.len();
        let uniform = sx > EPS && (sx - sy).abs() <= EPS * (sx + sy + 1.0);
        let orthogonal = ex.dot(ey).abs() <= EPS * (sx * sy + 1.0);
        let axis_aligned = ex.y.abs() <= EPS * (sx + 1.0)
            && ey.x.abs() <= EPS * (sy + 1.0)
            && ex.x > EPS
            && ey.y > EPS;
        let s = sx; // uniform scale factor

        for c in &mut self.contours {
            for p in &mut c.points {
                let r = m.apply_point(Vec3::new(p.x, p.y, 0.0));
                *p = Vec2::new(r.x, r.y);
            }
            c.kind = match &c.kind {
                ContourKind::Polyline => ContourKind::Polyline,
                ContourKind::Circle { c: center, r } => {
                    if uniform && orthogonal {
                        let nc = m.apply_point(center.to3()).xy();
                        ContourKind::Circle { c: nc, r: r * s }
                    } else {
                        ContourKind::Polyline
                    }
                }
                ContourKind::Arc {
                    c: center,
                    r,
                    a0,
                    a1,
                } => {
                    if uniform && axis_aligned {
                        let nc = m.apply_point(center.to3()).xy();
                        ContourKind::Arc {
                            c: nc,
                            r: r * s,
                            a0: *a0,
                            a1: *a1,
                        }
                    } else {
                        ContourKind::Polyline
                    }
                }
            };
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

    /// Circle centered at the origin, approximated with `seg` segments. Stamps a
    /// [`ContourKind::Circle`] hint for exporters.
    pub fn circle(r: f64, seg: usize) -> Shape {
        let seg = seg.max(3);
        let mut pts = Vec::with_capacity(seg);
        for i in 0..seg {
            let a = TAU * (i as f64 / seg as f64);
            pts.push(Vec2::from_angle(a, r));
        }
        Shape::from_contour(Contour::with_kind(
            pts,
            true,
            ContourKind::Circle { c: Vec2::ZERO, r },
        ))
    }

    /// Regular polygon centered at the origin, first vertex pointing +X. Kept as
    /// a [`ContourKind::Polyline`] (not a circle primitive).
    pub fn polygon(sides: usize, r: f64) -> Shape {
        let seg = sides.max(3);
        let mut pts = Vec::with_capacity(seg);
        for i in 0..seg {
            let a = TAU * (i as f64 / seg as f64);
            pts.push(Vec2::from_angle(a, r));
        }
        Shape::from_contour(Contour::new(pts, true))
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

    /// A circular arc (open) from `start_deg` to `end_deg`. Stamps a
    /// [`ContourKind::Arc`] hint for exporters (angles in degrees).
    pub fn arc(r: f64, start_deg: f64, end_deg: f64, seg: usize) -> Shape {
        let seg = seg.max(1);
        let (a0, a1) = (rad(start_deg), rad(end_deg));
        let mut pts = Vec::with_capacity(seg + 1);
        for i in 0..=seg {
            let a = a0 + (a1 - a0) * (i as f64 / seg as f64);
            pts.push(Vec2::from_angle(a, r));
        }
        Shape::from_contour(Contour::with_kind(
            pts,
            false,
            ContourKind::Arc {
                c: Vec2::ZERO,
                r,
                a0: start_deg,
                a1: end_deg,
            },
        ))
    }

    /// Add a contour as a hole (used by skills producing plates with cutouts).
    pub fn add_hole(&mut self, c: Contour) {
        self.contours.push(c);
    }
}

// ---- private helpers -------------------------------------------------------

/// Even-odd ray-cast point-in-polygon test on a simple closed polygon.
/// Private to this file (blueprint: each file defines its own).
fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let a = poly[i];
        let b = poly[j];
        if (a.y > p.y) != (b.y > p.y) {
            let t = (p.y - a.y) / (b.y - a.y);
            let x = a.x + t * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Find a point strictly inside a simple closed polygon by nudging just off an
/// edge midpoint along its normal. Returns `None` if no interior point can be
/// found (degenerate polygon).
fn contour_interior_point(poly: &[Vec2]) -> Option<Vec2> {
    let n = poly.len();
    if n < 3 {
        return None;
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in poly {
        minx = minx.min(p.x);
        miny = miny.min(p.y);
        maxx = maxx.max(p.x);
        maxy = maxy.max(p.y);
    }
    let ext = (maxx - minx).max(maxy - miny);
    let eps = (ext * 1e-6).max(1e-9);

    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let d = (b - a).normalized();
        if d.len_sq() < 0.5 {
            continue; // zero-length edge
        }
        let mid = (a + b) * 0.5;
        let nrm = d.perp();
        for &s in &[1.0f64, -1.0f64] {
            let cand = mid + nrm * (eps * s);
            if point_in_polygon(cand, poly) {
                return Some(cand);
            }
        }
    }
    None
}

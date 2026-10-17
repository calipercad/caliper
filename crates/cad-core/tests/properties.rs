//! Randomized property tests (std-only): a tiny seeded PRNG drives many cases
//! through triangulate / boolean / CSG and checks the structural invariants.
//! Deterministic (fixed seeds) so a failing case is reproducible — this is the
//! std-only stand-in for a fuzz corpus.

use cad_core::geom2d::{boolean, Shape};
use cad_core::geom3d::{self, Mesh};
use cad_core::math::{Mat4, Vec2, Vec3, TAU};

/// SplitMix64 — a 64-bit seeded PRNG in ~10 lines, no dependencies.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in `[lo, hi)`.
    fn f(&mut self, lo: f64, hi: f64) -> f64 {
        let u = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        lo + u * (hi - lo)
    }
    /// Uniform integer in `[lo, hi]`.
    fn i(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next_u64() % ((hi - lo + 1) as u64)) as i64
    }
}

fn poly_area(pts: &[Vec2]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for k in 0..n {
        let p = pts[k];
        let q = pts[(k + 1) % n];
        a += p.x * q.y - q.x * p.y;
    }
    a * 0.5
}

// ---- triangulate -----------------------------------------------------------

#[test]
fn triangulate_convex_is_exact() {
    let mut rng = Rng::new(0x7213_ABCD_EF01);
    for _ in 0..300 {
        // A convex polygon = points on a circle at strictly increasing angles.
        let n = rng.i(3, 10) as usize;
        let r = rng.f(5.0, 25.0);
        let mut angs: Vec<f64> = (0..n).map(|_| rng.f(0.0, TAU)).collect();
        angs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        // Reject near-duplicate angles that would make a degenerate edge.
        if angs.windows(2).any(|w| (w[1] - w[0]) < 1e-3) {
            continue;
        }
        let pts: Vec<Vec2> = angs.iter().map(|&a| Vec2::from_angle(a, r)).collect();

        let tris = geom3d::triangulate(&pts).unwrap();
        assert_eq!(tris.len(), n - 2, "a simple polygon gives n-2 triangles");

        let mut area = 0.0;
        for t in &tris {
            let (a, b, c) = (pts[t[0]], pts[t[1]], pts[t[2]]);
            area += (b - a).cross(c - a) * 0.5;
        }
        let expected = poly_area(&pts);
        assert!(
            (area - expected).abs() < 1e-6 * expected.abs().max(1.0),
            "triangle areas sum to the polygon area ({area} vs {expected})"
        );
    }
}

// ---- 2D boolean ------------------------------------------------------------

fn rand_rect(rng: &mut Rng) -> Shape {
    let (x0, y0) = (rng.f(-6.0, 6.0), rng.f(-6.0, 6.0));
    let (w, h) = (rng.f(6.0, 16.0), rng.f(6.0, 16.0));
    Shape::polyline(
        vec![
            Vec2::new(x0, y0),
            Vec2::new(x0 + w, y0),
            Vec2::new(x0 + w, y0 + h),
            Vec2::new(x0, y0 + h),
        ],
        true,
    )
}

#[test]
fn boolean_inclusion_exclusion() {
    let mut rng = Rng::new(0xB0017E55);
    for _ in 0..150 {
        let a = rand_rect(&mut rng);
        let b = rand_rect(&mut rng);
        let u = boolean::union(&a, &b).unwrap();
        let i = boolean::intersection(&a, &b).unwrap();
        // |A∪B| + |A∩B| == |A| + |B|, for any overlap (incl. disjoint → ∩ = 0).
        let lhs = u.area() + i.area();
        let rhs = a.area() + b.area();
        assert!(
            (lhs - rhs).abs() < 1e-4 * rhs,
            "inclusion-exclusion: {lhs} vs {rhs}"
        );
        assert!(u.area() >= a.area() - 1e-6 && u.area() >= b.area() - 1e-6);
        // Determinism: identical inputs → byte-identical contour points.
        let u2 = boolean::union(&a, &b).unwrap();
        assert_eq!(u.contours.len(), u2.contours.len());
        for (c1, c2) in u.contours.iter().zip(&u2.contours) {
            assert_eq!(c1.points, c2.points);
        }
    }
}

// ---- 3D CSG ----------------------------------------------------------------

fn rand_cuboid(rng: &mut Rng) -> Mesh {
    let (w, d, h) = (rng.f(10.0, 20.0), rng.f(10.0, 20.0), rng.f(10.0, 20.0));
    let mut m = Mesh::cuboid(w, d, h);
    m.center_origin();
    // Offsets < size/2 keep the pair overlapping near the origin.
    m.transform(&Mat4::translation(Vec3::new(
        rng.f(-6.0, 6.0),
        rng.f(-6.0, 6.0),
        rng.f(-6.0, 6.0),
    )));
    m
}

#[test]
fn csg_results_are_manifold_solids() {
    let mut rng = Rng::new(0xC56_C56);
    for _ in 0..25 {
        let a = rand_cuboid(&mut rng);
        let b = rand_cuboid(&mut rng);
        let u = geom3d::csg::union(&a, &b).unwrap();
        let i = geom3d::csg::intersection(&a, &b).unwrap();
        let d = geom3d::csg::difference(&a, &b).unwrap();

        for (name, m) in [("union", &u), ("intersection", &i), ("difference", &d)] {
            if !m.tris.is_empty() {
                assert!(m.is_closed_manifold(), "{name} is a closed manifold");
                assert!(m.signed_volume() > 0.0, "{name} has outward normals");
            }
        }
        // Inclusion-exclusion on volumes.
        let lhs = u.signed_volume() + i.signed_volume();
        let rhs = a.signed_volume() + b.signed_volume();
        assert!(
            (lhs - rhs).abs() < 1e-2 * rhs,
            "volume inclusion-exclusion: {lhs} vs {rhs}"
        );
    }
}

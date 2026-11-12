// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Finger-jointed box generator for laser cutters and CNC routers.
//!
//! Emits the flat panels of a box (bottom + four walls, optional lid) laid out
//! side-by-side as separate 2D entities so the whole scene can be exported to a
//! single SVG/DXF cut file. Edges that join neighbours get castellated
//! "comb" fingers whose depth equals the material thickness.

use crate::args::Args;
use crate::error::Result;
use crate::geom2d::{Contour, Shape};
use crate::math::Vec2;
use crate::scene::{Geometry, Scene};
use crate::skills::Skill;

pub struct FingerBoxSkill;

impl Skill for FingerBoxSkill {
    fn name(&self) -> &'static str {
        "boxjoint"
    }
    fn summary(&self) -> &'static str {
        "Finger-jointed box panels laid out flat for laser/CNC cutting"
    }
    fn usage(&self) -> &'static str {
        "boxjoint width=<mm> depth=<mm> height=<mm> thickness=<mm> [finger=<mm>] [lid=false] [gap=10]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let w = args.f64_or("width", 80.0)?;
        let d = args.f64_or("depth", 60.0)?;
        let h = args.f64_or("height", 40.0)?;
        let t = args.f64_or("thickness", 3.0)?;
        let finger = args.f64_or("finger", (3.0 * t).max(6.0))?;
        let lid = args.flag("lid");
        let gap = args.f64_or("gap", 10.0)?;
        let prefix = args.str_or("name", "box").to_string();

        // Each panel: [bottom, right, top, left] edges. `fingered` toggles combs.
        // Phase alternation makes opposing edges interlock.
        let bottom = panel(
            w,
            d,
            [
                edge(true, false),
                edge(true, false),
                edge(true, false),
                edge(true, false),
            ],
            t,
            finger,
        );
        let front = panel(
            w,
            h,
            [
                edge(true, true),
                edge(true, false),
                edge(false, false),
                edge(true, false),
            ],
            t,
            finger,
        );
        let back = front.clone();
        let left = panel(
            d,
            h,
            [
                edge(true, true),
                edge(true, true),
                edge(false, false),
                edge(true, true),
            ],
            t,
            finger,
        );
        let right = left.clone();

        // Lay panels out in a row with a gap between them.
        let mut cursor = 0.0f64;
        let mut place = |scene: &mut Scene, name: String, mut s: Shape| {
            let bb = s.bbox();
            let dx = cursor - bb.min.x;
            s.transform(&crate::math::Mat4::translation(crate::math::Vec3::new(
                dx, 0.0, 0.0,
            )));
            cursor += bb.width() + gap;
            scene.add(name, Geometry::Shape(s));
        };

        place(scene, format!("{prefix}_bottom"), bottom);
        place(scene, format!("{prefix}_front"), front);
        place(scene, format!("{prefix}_back"), back);
        place(scene, format!("{prefix}_left"), left);
        place(scene, format!("{prefix}_right"), right);
        let mut count = 5;
        if lid {
            let top = panel(w, d, [edge(false, false); 4], t, finger);
            place(scene, format!("{prefix}_lid"), top);
            count += 1;
        }

        Ok(format!(
            "box '{prefix}': {count} panels ({w}×{d}×{h}mm, {t}mm stock) laid out flat"
        ))
    }
}

#[derive(Clone, Copy)]
struct EdgeSpec {
    fingered: bool,
    phase: bool,
}

fn edge(fingered: bool, phase: bool) -> EdgeSpec {
    EdgeSpec { fingered, phase }
}

/// Build one panel outline (CCW) of size `w`×`h` with the given per-edge combs.
fn panel(w: f64, h: f64, edges: [EdgeSpec; 4], depth: f64, finger: f64) -> Shape {
    let corners = [
        (Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(0.0, -1.0)), // bottom
        (Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(1.0, 0.0)),      // right
        (Vec2::new(w, h), Vec2::new(0.0, h), Vec2::new(0.0, 1.0)),      // top
        (Vec2::new(0.0, h), Vec2::new(0.0, 0.0), Vec2::new(-1.0, 0.0)), // left
    ];
    let mut pts: Vec<Vec2> = Vec::new();
    for (i, (a, b, out_n)) in corners.iter().enumerate() {
        let spec = edges[i];
        if spec.fingered {
            let seg = fingered_edge(*a, *b, *out_n, depth, finger, spec.phase);
            append_dedup(&mut pts, &seg);
        } else {
            append_dedup(&mut pts, &[*a, *b]);
        }
    }
    Shape::from_contour(Contour::new(dedup_closed(pts), true))
}

/// Castellated edge from `a` to `b`. "Out" segments are pushed by `depth` along
/// `out_normal`; the edge starts and ends flush so corners stay clean.
fn fingered_edge(
    a: Vec2,
    b: Vec2,
    out_normal: Vec2,
    depth: f64,
    finger: f64,
    phase: bool,
) -> Vec<Vec2> {
    let len = (b - a).len();
    if len < 1e-6 {
        return vec![a, b];
    }
    let d = (b - a) / len;
    let n = out_normal.normalized() * depth;
    // Odd number of segments so the edge begins and ends on the baseline.
    let approx = (len / finger).round().max(1.0) as usize;
    let nseg = if approx.is_multiple_of(2) {
        approx + 1
    } else {
        approx
    }
    .max(3);
    let sl = len / nseg as f64;

    let mut out = Vec::with_capacity(nseg * 2);
    for j in 0..nseg {
        let s0 = a + d * (sl * j as f64);
        let s1 = a + d * (sl * (j + 1) as f64);
        // interior odd segments are tabs; `phase` flips the pattern
        let high = (j % 2 == 1) ^ phase;
        if high {
            out.push(s0 + n);
            out.push(s1 + n);
        } else {
            out.push(s0);
            out.push(s1);
        }
    }
    out
}

fn append_dedup(dst: &mut Vec<Vec2>, src: &[Vec2]) {
    for &p in src {
        if let Some(&last) = dst.last() {
            if (last - p).len() < 1e-6 {
                continue;
            }
        }
        dst.push(p);
    }
}

fn dedup_closed(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    if pts.len() > 1 {
        if let (Some(&first), Some(&last)) = (pts.first(), pts.last()) {
            if (first - last).len() < 1e-6 {
                pts.pop();
            }
        }
    }
    pts
}

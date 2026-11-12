// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Honeycomb panel: a rectangular plate perforated with a hexagonal grid.
//!
//! Great for speaker grilles, vents and decorative laser/CNC panels. Produces a
//! single 2D entity: the outer rectangle plus one hex contour per cell (holes).

use crate::args::Args;
use crate::error::Result;
use crate::geom2d::{Contour, Shape};
use crate::math::{Vec2, TAU};
use crate::scene::{Geometry, Scene};
use crate::skills::Skill;

pub struct HoneycombSkill;

impl Skill for HoneycombSkill {
    fn name(&self) -> &'static str {
        "honeycomb"
    }
    fn summary(&self) -> &'static str {
        "Rectangular plate perforated with a hexagonal grid"
    }
    fn usage(&self) -> &'static str {
        "honeycomb width=<mm> height=<mm> [cell=<mm>] [wall=<mm>] [margin=<mm>] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let w = args.f64_or("width", 100.0)?;
        let h = args.f64_or("height", 80.0)?;
        let cell = args.f64_or("cell", 10.0)?; // hex circumradius
        let wall = args.f64_or("wall", 2.0)?;
        let margin = args.f64_or("margin", 5.0)?;

        // Flat-top hex: width across flats = sqrt(3)*R, across corners = 2R.
        let r = cell;
        let dx = 1.5 * r + wall; // horizontal centre spacing
        let dy = 3f64.sqrt() * r + wall; // vertical centre spacing

        let mut shape = Shape::rect(w, h, 0.0, 1);
        let mut count = 0usize;

        let mut col = 0;
        let mut cx = margin + r;
        while cx <= w - margin - r {
            let offset = if col % 2 == 1 { dy / 2.0 } else { 0.0 };
            let mut cy = margin + r + offset;
            while cy <= h - margin - r {
                shape.add_hole(hexagon(Vec2::new(cx, cy), r));
                count += 1;
                cy += dy;
            }
            cx += dx;
            col += 1;
        }

        let name = args.name().unwrap_or_else(|| scene.auto_name("honeycomb"));
        scene.add(name.clone(), Geometry::Shape(shape));
        Ok(format!(
            "honeycomb '{name}': {count} cells in {w}×{h}mm plate"
        ))
    }
}

/// Flat-top hexagon centred at `c` with circumradius `r`.
fn hexagon(c: Vec2, r: f64) -> Contour {
    let mut pts = Vec::with_capacity(6);
    for i in 0..6 {
        let a = TAU * (i as f64 / 6.0);
        pts.push(c + Vec2::from_angle(a, r));
    }
    Contour::new(pts, true)
}

//! Involute spur gear generator.
//!
//! Produces a true involute tooth profile from the standard parameters used by
//! machinists: number of teeth, module (tooth size) and pressure angle. If a
//! `thickness` is given the profile is extruded into a printable/machinable 3D
//! gear; otherwise a flat 2D profile is emitted for laser/CNC/vinyl.

use crate::args::Args;
use crate::error::{CadError, Result};
use crate::geom2d::{Contour, Shape};
use crate::geom3d::extrude;
use crate::math::{rad, Vec2, PI, TAU};
use crate::scene::{Geometry, Scene};
use crate::skills::Skill;

pub struct GearSkill;

impl Skill for GearSkill {
    fn name(&self) -> &'static str {
        "gear"
    }
    fn summary(&self) -> &'static str {
        "Involute spur gear (2D profile or extruded solid)"
    }
    fn usage(&self) -> &'static str {
        "gear teeth=<n> module=<mm> [pressure=20] [bore=<mm>] [thickness=<mm>] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let teeth = args.usize_or("teeth", 20)?;
        if teeth < 4 {
            return Err(CadError::Arg("gear needs teeth>=4".into()));
        }
        let module = args.f64_or("module", 2.0)?;
        let pressure = args.f64_or("pressure", 20.0)?;
        let bore = args.f64_or("bore", 0.0)?;
        let thickness = args.f64_or("thickness", 0.0)?;

        let profile = involute_gear(teeth, module, pressure)?;
        let mut shape = Shape::from_contour(profile);
        if bore > 0.0 {
            let hole = Shape::circle(bore / 2.0, 48).contours.remove(0);
            shape.add_hole(hole);
        }

        let name = args.name().unwrap_or_else(|| scene.auto_name("gear"));

        if thickness > 0.0 {
            let mesh = extrude(&shape, thickness)?;
            scene.add(name.clone(), Geometry::Mesh(mesh));
            Ok(format!(
                "gear '{name}': {teeth}T module {module} → 3D, {:.1}mm thick",
                thickness
            ))
        } else {
            scene.add(name.clone(), Geometry::Shape(shape));
            Ok(format!(
                "gear '{name}': {teeth}T module {module}, pitch Ø{:.2}",
                module * teeth as f64
            ))
        }
    }
}

/// Build a closed involute gear contour centred at the origin.
pub fn involute_gear(teeth: usize, module: f64, pressure_deg: f64) -> Result<Contour> {
    let z = teeth as f64;
    let alpha = rad(pressure_deg);
    let r_pitch = module * z / 2.0;
    let r_base = r_pitch * alpha.cos();
    let r_out = r_pitch + module; // addendum = 1*module
    let r_root = (r_pitch - 1.25 * module).max(0.1); // dedendum = 1.25*module

    if r_out <= r_base {
        return Err(CadError::Geometry(
            "gear geometry degenerate (increase teeth or module)".into(),
        ));
    }

    // Roll-angle bounds along the involute.
    let t_max = ((r_out / r_base).powi(2) - 1.0).sqrt();
    let t_root = if r_root > r_base {
        ((r_root / r_base).powi(2) - 1.0).sqrt()
    } else {
        0.0
    };
    let t_pitch = alpha.tan(); // roll angle where radius == pitch radius

    // Local involute point at roll angle t (start point sits on +X axis).
    let inv_pt = |t: f64| -> Vec2 {
        Vec2::new(
            r_base * (t.cos() + t * t.sin()),
            r_base * (t.sin() - t * t.cos()),
        )
    };
    // Local polar angle of the involute at the pitch circle.
    let g = inv_pt(t_pitch).angle();

    let half_tooth = PI / (2.0 * z); // angular half tooth-thickness at pitch
    let flank_seg = 8usize;
    let tip_seg = 3usize;
    let root_seg = 3usize;

    let rotate = |p: Vec2, a: f64| -> Vec2 {
        let (s, c) = a.sin_cos();
        Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
    };

    let mut pts: Vec<Vec2> = Vec::new();

    for k in 0..teeth {
        let theta_c = TAU * (k as f64) / z;
        // Rotate the right flank so its pitch crossing lands at theta_c - half.
        let rot_r = theta_c - half_tooth - g;

        // Right-flank samples (root → tip).
        let mut right: Vec<Vec2> = Vec::new();
        if r_root < r_base {
            // radial land from root circle up to base circle
            let base_pt = rotate(inv_pt(0.0), rot_r);
            let dir = base_pt.angle();
            right.push(Vec2::from_angle(dir, r_root));
        }
        for i in 0..=flank_seg {
            let t = t_root + (t_max - t_root) * (i as f64 / flank_seg as f64);
            right.push(rotate(inv_pt(t), rot_r));
        }

        // Left flank = mirror of right across the tooth centre line.
        let mirror = |p: Vec2| -> Vec2 {
            let a = p.angle();
            Vec2::from_angle(2.0 * theta_c - a, p.len())
        };
        let left: Vec<Vec2> = right.iter().rev().map(|&p| mirror(p)).collect();

        // Emit: right flank up, tip arc, left flank down.
        pts.extend_from_slice(&right);
        let tip_a0 = right.last().unwrap().angle();
        let tip_a1 = left.first().unwrap().angle();
        for i in 1..tip_seg {
            let a = tip_a0 + (tip_a1 - tip_a0) * (i as f64 / tip_seg as f64);
            pts.push(Vec2::from_angle(a, r_out));
        }
        pts.extend_from_slice(&left);

        // Root arc across the gap to the next tooth's right-flank start.
        let gap_a0 = left.last().unwrap().angle();
        let next_c = TAU * ((k + 1) as f64) / z;
        let gap_a1 = next_c - half_tooth - g + rotate(inv_pt(0.0), 0.0).angle();
        // Approximate the next start angle directly on the root circle:
        let gap_a1 = if r_root < r_base {
            let base_pt = rotate(inv_pt(0.0), next_c - half_tooth - g);
            base_pt.angle()
        } else {
            gap_a1
        };
        for i in 1..=root_seg {
            let a = gap_a0 + (gap_a1 - gap_a0) * (i as f64 / (root_seg + 1) as f64);
            pts.push(Vec2::from_angle(a, r_root));
        }
    }

    Ok(Contour::new(pts, true))
}

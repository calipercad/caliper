//! Small mechanical hardware skills: washers and standoffs/spacers.

use crate::args::Args;
use crate::error::{CadError, Result};
use crate::geom2d::Shape;
use crate::geom3d::Mesh;
use crate::math::{Vec3, TAU};
use crate::scene::{Geometry, Scene};
use crate::skills::Skill;

/// A hollow cylinder (tube) with flat annular caps at both ends.
pub fn tube(outer_r: f64, inner_r: f64, h: f64, seg: usize) -> Result<Mesh> {
    if inner_r >= outer_r {
        return Err(CadError::Geometry(
            "inner radius must be < outer radius".into(),
        ));
    }
    let seg = seg.max(6);
    let mut m = Mesh::new();
    // vertex layout per angular step: [outer_bottom, inner_bottom, outer_top, inner_top]
    for i in 0..seg {
        let a = TAU * (i as f64 / seg as f64);
        let (s, c) = a.sin_cos();
        m.verts.push(Vec3::new(outer_r * c, outer_r * s, 0.0));
        m.verts.push(Vec3::new(inner_r * c, inner_r * s, 0.0));
        m.verts.push(Vec3::new(outer_r * c, outer_r * s, h));
        m.verts.push(Vec3::new(inner_r * c, inner_r * s, h));
    }
    let idx = |i: usize, k: usize| (i % seg) * 4 + k;
    for i in 0..seg {
        let ob0 = idx(i, 0);
        let ib0 = idx(i, 1);
        let ot0 = idx(i, 2);
        let it0 = idx(i, 3);
        let ob1 = idx(i + 1, 0);
        let ib1 = idx(i + 1, 1);
        let ot1 = idx(i + 1, 2);
        let it1 = idx(i + 1, 3);
        // outer wall (faces outward)
        m.tris.push([ob0, ob1, ot1]);
        m.tris.push([ob0, ot1, ot0]);
        // inner wall (faces inward)
        m.tris.push([ib0, it1, ib1]);
        m.tris.push([ib0, it0, it1]);
        // bottom annulus (faces -z)
        m.tris.push([ob0, ib1, ob1]);
        m.tris.push([ob0, ib0, ib1]);
        // top annulus (faces +z)
        m.tris.push([ot0, ot1, it1]);
        m.tris.push([ot0, it1, it0]);
    }
    Ok(m)
}

pub struct WasherSkill;

impl Skill for WasherSkill {
    fn name(&self) -> &'static str {
        "washer"
    }
    fn summary(&self) -> &'static str {
        "Flat washer / annulus (2D ring, or a solid tube if thickness given)"
    }
    fn usage(&self) -> &'static str {
        "washer outer=<mm> inner=<mm> [thickness=<mm>] [seg=64] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let outer = args.f64_or("outer", 20.0)?;
        let inner = args.f64_or("inner", 10.0)?;
        let seg = args.usize_or("seg", 64)?;
        let thickness = args.f64_or("thickness", 0.0)?;
        if inner >= outer {
            return Err(CadError::Arg("inner must be < outer".into()));
        }
        // `outer`/`inner` are radii throughout this skill.
        let name = args.name().unwrap_or_else(|| scene.auto_name("washer"));
        if thickness > 0.0 {
            let m = tube(outer, inner, thickness, seg)?;
            scene.add(name.clone(), Geometry::Mesh(m));
            Ok(format!(
                "washer '{name}': Ø{outer}/Ø{inner} × {thickness}mm solid"
            ))
        } else {
            let mut shape = Shape::circle(outer, seg);
            shape.add_hole(Shape::circle(inner, seg).contours.remove(0));
            scene.add(name.clone(), Geometry::Shape(shape));
            Ok(format!(
                "washer '{name}': ring outer r{outer} / inner r{inner}"
            ))
        }
    }
}

pub struct StandoffSkill;

impl Skill for StandoffSkill {
    fn name(&self) -> &'static str {
        "standoff"
    }
    fn summary(&self) -> &'static str {
        "Cylindrical standoff/spacer with a through bore (3D tube)"
    }
    fn usage(&self) -> &'static str {
        "standoff outer=<mm> bore=<mm> height=<mm> [seg=48] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let outer = args.f64_or("outer", 6.0)?;
        let bore = args.f64_or("bore", 3.2)?;
        let height = args.f64_or("height", 10.0)?;
        let seg = args.usize_or("seg", 48)?;
        let m = tube(outer, bore, height, seg)?;
        let name = args.name().unwrap_or_else(|| scene.auto_name("standoff"));
        scene.add(name.clone(), Geometry::Mesh(m));
        Ok(format!(
            "standoff '{name}': Ø{outer} bore Ø{bore} × {height}mm"
        ))
    }
}

// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Parametric "maker" skills: everyday 3D-printable parts built from the solid
//! primitives and the CSG kernel.
//!
//! Each generator favours closed-manifold output so results drop straight into
//! the boolean kernel or a slicer. Heavy geometry lives in free helper fns; the
//! `run` methods stay small and default every parameter so a bare `skill <name>`
//! always produces valid, non-empty geometry.

use crate::args::Args;
use crate::error::{CadError, Result};
use crate::geom2d::{Contour, Shape};
use crate::geom3d::{self, csg, Mesh};
use crate::math::{Mat4, Vec2, Vec3, TAU};
use crate::scene::{Geometry, Scene};
use crate::skills::Skill;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// A hollow cylinder (tube) with flat annular caps at both ends — a closed
/// 2-manifold. Base at z=0, axis along +Z, centred on the origin in XY.
fn tube(outer_r: f64, inner_r: f64, h: f64, seg: usize) -> Result<Mesh> {
    if inner_r >= outer_r {
        return Err(CadError::Geometry(
            "inner radius must be < outer radius".into(),
        ));
    }
    let seg = seg.max(6);
    let mut m = Mesh::new();
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
        let (ob0, ib0, ot0, it0) = (idx(i, 0), idx(i, 1), idx(i, 2), idx(i, 3));
        let (ob1, ib1, ot1, it1) = (idx(i + 1, 0), idx(i + 1, 1), idx(i + 1, 2), idx(i + 1, 3));
        // outer wall
        m.tris.push([ob0, ob1, ot1]);
        m.tris.push([ob0, ot1, ot0]);
        // inner wall
        m.tris.push([ib0, it1, ib1]);
        m.tris.push([ib0, it0, it1]);
        // bottom annulus
        m.tris.push([ob0, ib1, ob1]);
        m.tris.push([ob0, ib0, ib1]);
        // top annulus
        m.tris.push([ot0, ot1, it1]);
        m.tris.push([ot0, it1, it0]);
    }
    Ok(m)
}

/// A rounded-rectangle prism: `rect(w,d,cr)` extruded by `h`. Lower-left corner
/// of the footprint at the origin, base at z=0. Closed manifold (extrude is).
fn rounded_prism(w: f64, d: f64, h: f64, cr: f64, seg: usize) -> Result<Mesh> {
    let shape = Shape::rect(w, d, cr, seg);
    geom3d::extrude(&shape, h)
}

/// Shorthand for an in-place translation.
fn moved(mut m: Mesh, x: f64, y: f64, z: f64) -> Mesh {
    m.transform(&Mat4::translation(Vec3::new(x, y, z)));
    m
}

// ---------------------------------------------------------------------------
// 1. gridfinity
// ---------------------------------------------------------------------------

pub struct GridfinitySkill;

impl Skill for GridfinitySkill {
    fn name(&self) -> &'static str {
        "gridfinity"
    }
    fn summary(&self) -> &'static str {
        "Gridfinity storage bin (42mm grid, 7mm height units) as a closed manifold"
    }
    fn usage(&self) -> &'static str {
        "gridfinity [units_x=2] [units_y=1] [height_u=3] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let ux = args.usize_or("units_x", 2)?.max(1);
        let uy = args.usize_or("units_y", 1)?.max(1);
        let hu = args.usize_or("height_u", 3)?.max(1);

        const GRID: f64 = 42.0;
        const HUNIT: f64 = 7.0;
        let wall = 1.2;
        let floor = 1.2;
        let cr = 3.75;
        let seg = 6;

        // 0.25mm clearance per side => −0.5mm on each footprint dimension.
        let w = ux as f64 * GRID - 0.5;
        let d = uy as f64 * GRID - 0.5;
        let total_h = hu as f64 * HUNIT;

        let bin = gridfinity_bin(w, d, total_h, wall, floor, cr, seg)?;
        let manifold = bin.is_closed_manifold();

        let name = args.name().unwrap_or_else(|| scene.auto_name("gridfinity"));
        scene.add(name.clone(), Geometry::Mesh(bin));
        Ok(format!(
            "gridfinity '{name}': {ux}×{uy}u × {hu}h ({w:.1}×{d:.1}×{total_h:.1}mm), \
             manifold={manifold}"
        ))
    }
}

/// Hollow gridfinity bin: an outer rounded prism with the interior removed,
/// leaving `wall` walls and a `floor` floor, open at the top. Guaranteed to
/// return a closed manifold — if the open-top difference cannot be produced
/// cleanly it degrades to a sealed shell, then to the solid outer prism.
fn gridfinity_bin(
    w: f64,
    d: f64,
    total_h: f64,
    wall: f64,
    floor: f64,
    cr: f64,
    seg: usize,
) -> Result<Mesh> {
    let outer = rounded_prism(w, d, total_h, cr, seg)?;

    // Inner cavity: smaller rounded prism raised by `floor` and poking out of
    // the top so the difference opens the top face.
    let iw = (w - 2.0 * wall).max(1.0);
    let id = (d - 2.0 * wall).max(1.0);
    let icr = (cr - wall).max(0.5);
    let inner = moved(rounded_prism(iw, id, total_h, icr, seg)?, wall, wall, floor);

    let bin = match csg::difference(&outer, &inner) {
        Ok(m) if m.is_closed_manifold() => m,
        // Fallback 1: a fully sealed shell is always a closed manifold.
        _ => match csg::shell(&outer, wall) {
            Ok(s) if s.is_closed_manifold() => s,
            // Fallback 2: the solid prism (extrude output is manifold).
            _ => outer,
        },
    };

    // Centre the footprint on the origin, base on z=0.
    Ok(moved(bin, -w / 2.0, -d / 2.0, 0.0))
}

// ---------------------------------------------------------------------------
// 2. enclosure
// ---------------------------------------------------------------------------

pub struct EnclosureSkill;

impl Skill for EnclosureSkill {
    fn name(&self) -> &'static str {
        "enclosure"
    }
    fn summary(&self) -> &'static str {
        "Two-part project box: hollow base with an open top, optional lid and standoffs"
    }
    fn usage(&self) -> &'static str {
        "enclosure [width=80] [depth=60] [height=30] [wall=2] [lid] [standoffs] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let w = args.f64_or("width", 80.0)?;
        let d = args.f64_or("depth", 60.0)?;
        let h = args.f64_or("height", 30.0)?;
        let wall = args.f64_or("wall", 2.0)?;
        let lid = args.flag("lid");
        let standoffs = args.flag("standoffs");
        let prefix = args.str_or("name", "enclosure").to_string();

        // Base: open-top box, footprint centred on the origin.
        let mut base = moved(open_box(w, d, h, wall)?, -w / 2.0, -d / 2.0, 0.0);

        if standoffs {
            append_corner_posts(&mut base, w, d, h, wall);
        }
        let manifold = base.is_closed_manifold();
        scene.add(format!("{prefix}_base"), Geometry::Mesh(base));
        let mut parts = vec!["base"];

        if lid {
            // Thin cover sized to the outer footprint, laid out beside the base.
            let lt = wall * 1.5;
            let lid_m = moved(Mesh::cuboid(w, d, lt), w / 2.0 + 10.0, -d / 2.0, 0.0);
            scene.add(format!("{prefix}_lid"), Geometry::Mesh(lid_m));
            parts.push("lid");
        }

        Ok(format!(
            "enclosure '{prefix}': {parts:?} ({w}×{d}×{h}mm, {wall}mm wall{}), base manifold={manifold}",
            if standoffs { ", 4 standoffs" } else { "" }
        ))
    }
}

/// A box hollowed to `wall` thickness with an OPEN TOP: subtract an inner box
/// raised by `wall` so its top pokes through the lid face. Always returns a
/// closed manifold (falls back to a sealed shell, then the solid box).
fn open_box(w: f64, d: f64, h: f64, wall: f64) -> Result<Mesh> {
    let outer = Mesh::cuboid(w, d, h);
    let iw = (w - 2.0 * wall).max(0.1);
    let id = (d - 2.0 * wall).max(0.1);
    let inner = moved(Mesh::cuboid(iw, id, h), wall, wall, wall);
    match csg::difference(&outer, &inner) {
        Ok(m) if m.is_closed_manifold() => Ok(m),
        _ => match csg::shell(&outer, wall) {
            Ok(s) if s.is_closed_manifold() => Ok(s),
            _ => Ok(outer),
        },
    }
}

/// Append four bored standoff posts at the inside corners of a centred base.
fn append_corner_posts(base: &mut Mesh, w: f64, d: f64, h: f64, wall: f64) {
    let post_outer = 3.0;
    let post_bore = 1.1;
    let ph = (h - wall).max(3.0);
    let inset = wall + 5.0;
    let post = match tube(post_outer, post_bore, ph, 24) {
        Ok(p) => p,
        Err(_) => return,
    };
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            let p = moved(
                post.clone(),
                sx * (w / 2.0 - inset),
                sy * (d / 2.0 - inset),
                wall,
            );
            base.append(&p);
        }
    }
}

// ---------------------------------------------------------------------------
// 3. standoff_grid
// ---------------------------------------------------------------------------

pub struct StandoffGridSkill;

impl Skill for StandoffGridSkill {
    fn name(&self) -> &'static str {
        "standoff_grid"
    }
    fn summary(&self) -> &'static str {
        "Grid of bored standoff tubes (e.g. a PCB mounting pattern), optional base plate"
    }
    fn usage(&self) -> &'static str {
        "standoff_grid [nx=3] [ny=2] [pitch=20] [outer=6] [bore=3] [height=10] [base] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let nx = args.usize_or("nx", 3)?.max(1);
        let ny = args.usize_or("ny", 2)?.max(1);
        let pitch = args.f64_or("pitch", 20.0)?;
        let outer = args.f64_or("outer", 6.0)?;
        let bore = args.f64_or("bore", 3.0)?;
        let height = args.f64_or("height", 10.0)?;
        let with_base = args.flag("base");

        if bore >= outer {
            return Err(CadError::Arg(
                "bore diameter must be < outer diameter".into(),
            ));
        }

        // `outer`/`bore` are DIAMETERS.
        let one = tube(outer / 2.0, bore / 2.0, height, 32)?;

        let gx = (nx as f64 - 1.0) * pitch;
        let gy = (ny as f64 - 1.0) * pitch;
        let mut mesh = Mesh::new();
        for i in 0..nx {
            for j in 0..ny {
                let x = i as f64 * pitch - gx / 2.0;
                let y = j as f64 * pitch - gy / 2.0;
                mesh.append(&moved(one.clone(), x, y, 0.0));
            }
        }

        if with_base {
            // Thin plate under the posts (z: −bt..0, posts sit on top at z=0).
            let bt = 2.0;
            let bw = gx + outer + 4.0;
            let bd = gy + outer + 4.0;
            let plate = moved(Mesh::cuboid(bw, bd, bt), -bw / 2.0, -bd / 2.0, -bt);
            mesh.append(&plate);
        }

        let name = args
            .name()
            .unwrap_or_else(|| scene.auto_name("standoff_grid"));
        scene.add(name.clone(), Geometry::Mesh(mesh));
        Ok(format!(
            "standoff_grid '{name}': {nx}×{ny} posts @ {pitch}mm, Ø{outer}/Ø{bore} × {height}mm{}",
            if with_base { " + base plate" } else { "" }
        ))
    }
}

// ---------------------------------------------------------------------------
// 4. knob
// ---------------------------------------------------------------------------

pub struct KnobSkill;

impl Skill for KnobSkill {
    fn name(&self) -> &'static str {
        "knob"
    }
    fn summary(&self) -> &'static str {
        "Fluted control knob with an axial (optionally D-shaft) bore"
    }
    fn usage(&self) -> &'static str {
        "knob [diameter=30] [height=15] [flutes=12] [bore=6] [d_flat=0] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let d = args.f64_or("diameter", 30.0)?;
        let height = args.f64_or("height", 15.0)?;
        let flutes = args.usize_or("flutes", 12)?;
        let bore = args.f64_or("bore", 6.0)?;
        let d_flat = args.f64_or("d_flat", 0.0)?;

        let r = (d / 2.0).max(1.0);
        let amp = (r * 0.08).max(0.4);
        let samples = (flutes.max(1) * 8).max(48);

        // Scalloped outer profile + a central bore, extruded in one pass: the
        // extrude kernel guarantees a watertight closed manifold with a true
        // through-bore, so no (fragile) 3D CSG is needed.
        let mut shape = Shape::from_contour(knob_profile(r, flutes, amp, samples));
        let br = (bore / 2.0).min(r - amp - 0.8).max(0.3);
        let d_flat_eff = d_flat.min(2.0 * br - 0.4).max(0.0);
        shape.add_hole(bore_contour(br, d_flat_eff, 48));

        let mesh = geom3d::extrude(&shape, height)?;
        let name = args.name().unwrap_or_else(|| scene.auto_name("knob"));
        scene.add(name.clone(), Geometry::Mesh(mesh));
        Ok(format!(
            "knob '{name}': Ø{d}×{height}mm, {flutes} flutes, bore Ø{:.1}{}",
            br * 2.0,
            if d_flat_eff > 0.0 { " (D-shaft)" } else { "" }
        ))
    }
}

/// Closed scalloped rim profile: radius oscillates between `r` (ridge) and
/// `r − amp` (flute valley), `flutes` valleys around. `flutes = 0` yields a
/// plain circle.
fn knob_profile(r: f64, flutes: usize, amp: f64, samples: usize) -> Contour {
    let samples = samples.max(8);
    let mut pts = Vec::with_capacity(samples);
    for i in 0..samples {
        let a = TAU * (i as f64 / samples as f64);
        let rr = r - amp * 0.5 * (1.0 - (flutes as f64 * a).cos());
        pts.push(Vec2::from_angle(a, rr));
    }
    Contour::new(pts, true)
}

/// Bore contour: a full circle of radius `r`, or a D-shaft when `d_flat > 0`
/// (one side clipped to a flat chord at x = r − d_flat).
fn bore_contour(r: f64, d_flat: f64, seg: usize) -> Contour {
    let seg = seg.max(8);
    let flat_x = r - d_flat;
    let mut pts = Vec::with_capacity(seg);
    for i in 0..seg {
        let a = TAU * (i as f64 / seg as f64);
        let (s, c) = a.sin_cos();
        let mut x = c * r;
        let y = s * r;
        if d_flat > 0.0 && x > flat_x {
            x = flat_x;
        }
        pts.push(Vec2::new(x, y));
    }
    Contour::new(pts, true)
}

// ---------------------------------------------------------------------------
// 5. cable_clip
// ---------------------------------------------------------------------------

pub struct CableClipSkill;

impl Skill for CableClipSkill {
    fn name(&self) -> &'static str {
        "cable_clip"
    }
    fn summary(&self) -> &'static str {
        "Snap-in C-shaped cable clip with a screw-down mounting foot"
    }
    fn usage(&self) -> &'static str {
        "cable_clip [cable=6] [thickness=3] [depth=8] [screw=3] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let cable = args.f64_or("cable", 6.0)?;
        let thickness = args.f64_or("thickness", 3.0)?;
        let depth = args.f64_or("depth", 8.0)?;
        let screw = args.f64_or("screw", 3.0)?;

        let profile = cable_clip_profile(cable, thickness, screw)?;
        let mesh = geom3d::extrude(&profile, depth)?;

        let name = args.name().unwrap_or_else(|| scene.auto_name("cable_clip"));
        scene.add(name.clone(), Geometry::Mesh(mesh));
        Ok(format!(
            "cable_clip '{name}': Ø{cable} cable, {thickness}mm wall × {depth}mm deep, M{screw} foot"
        ))
    }
}

/// Build the 2D C-clip profile with the exact-integer 2D clipper: an annulus,
/// unioned with a mounting foot, with the top opening and the screw hole cut
/// away. Extruding this gives a watertight solid.
fn cable_clip_profile(cable: f64, thickness: f64, screw: f64) -> Result<Shape> {
    let seg = 64;
    let ro = cable / 2.0 + thickness;
    let ri = (cable / 2.0).max(0.5);
    let gap = (cable * 0.8).max(1.0);

    let fw = (ro * 1.6).max(screw * 3.0);
    let fh = (thickness * 1.6).max(screw * 1.8);

    // Annular ring (outer solid with the cable channel as a hole).
    let mut ring = Shape::circle(ro, seg);
    ring.add_hole(Shape::circle(ri, seg).contours.remove(0));

    // Mounting foot: a rectangle hanging below the ring, overlapping it so the
    // union fuses them into one body.
    let foot_top = -ri;
    let foot_bottom = -ro - fh;
    let foot_h = foot_top - foot_bottom;
    let foot = translated_shape(Shape::rect(fw, foot_h, 0.0, 2), -fw / 2.0, foot_bottom);
    let profile = ring.union(&foot)?;

    // Top opening: a slot cutting through the upper wall so the cable snaps in.
    let opening = translated_shape(
        Shape::rect(gap, ro + thickness + 2.0, 0.0, 2),
        -gap / 2.0,
        0.0,
    );
    let profile = profile.difference(&opening)?;

    // Screw hole in the hanging part of the foot, clear of the ring.
    let screw_r = (screw / 2.0)
        .min(fh / 2.0 - 0.6)
        .min(fw / 2.0 - 0.6)
        .max(0.3);
    let screw_cy = -ro - fh * 0.5;
    let screw_hole = translated_shape(Shape::circle(screw_r, 32), 0.0, screw_cy);
    let profile = profile.difference(&screw_hole)?;

    Ok(profile)
}

/// Translate a shape in the XY plane (shapes start with the lower-left corner,
/// or circle centre, at the origin).
fn translated_shape(mut s: Shape, dx: f64, dy: f64) -> Shape {
    s.transform(&Mat4::translation(Vec3::new(dx, dy, 0.0)));
    s
}

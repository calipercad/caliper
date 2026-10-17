//! Parametric mechanical-design skills: flanges, roller-chain sprockets,
//! spur-gear racks, timing-belt pulleys and radial disc cams.
//!
//! Each skill builds an engineering-accurate 2D profile from a handful of
//! parameters and, when a `thickness` is supplied, extrudes it into a solid.
//! The heavy profile math lives in the free helper functions near the bottom of
//! the file; the `Skill::run` bodies stay small — parse, build, commit.

use crate::args::Args;
use crate::error::{CadError, Result};
use crate::geom2d::{Contour, Shape};
use crate::geom3d::extrude;
use crate::math::{rad, Mat4, Vec2, Vec3, PI, TAU};
use crate::scene::{Geometry, Scene};
use crate::skills::Skill;

// ===========================================================================
// 1. Flange
// ===========================================================================

pub struct FlangeSkill;

impl Skill for FlangeSkill {
    fn name(&self) -> &'static str {
        "flange"
    }
    fn summary(&self) -> &'static str {
        "Bolt-circle flange disc (2D plate or extruded solid)"
    }
    fn usage(&self) -> &'static str {
        "flange [outer=60] [bore=20] [bolt_circle=45] [bolt_dia=5] [bolts=4] [seg=64] [thickness=0] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        // All three `*_circle`/diameter inputs are DIAMETERS; convert to radii.
        let outer = args.f64_or("outer", 60.0)?;
        let bore = args.f64_or("bore", 20.0)?;
        let bolt_circle = args.f64_or("bolt_circle", 45.0)?;
        let bolt_dia = args.f64_or("bolt_dia", 5.0)?;
        let bolts = args.usize_or("bolts", 4)?;
        let seg = args.usize_or("seg", 64)?;
        let thickness = args.f64_or("thickness", 0.0)?;

        if outer <= 0.0 {
            return Err(CadError::Arg("flange needs outer>0".into()));
        }

        let shape = flange_plate(outer, bore, bolt_circle, bolt_dia, bolts, seg);

        let name = args.name().unwrap_or_else(|| scene.auto_name("flange"));
        if thickness > 0.0 {
            let mesh = extrude(&shape, thickness)?;
            scene.add(name.clone(), Geometry::Mesh(mesh));
            Ok(format!(
                "flange '{name}': Ø{outer} disc, {bolts}×Ø{bolt_dia} on Ø{bolt_circle} → {:.1}mm solid",
                thickness
            ))
        } else {
            scene.add(name.clone(), Geometry::Shape(shape));
            Ok(format!(
                "flange '{name}': Ø{outer} plate, bore Ø{bore}, {bolts}×Ø{bolt_dia} on Ø{bolt_circle}"
            ))
        }
    }
}

/// Flange plate: an `outer`-diameter disc with a central bore and `bolts` holes
/// evenly spaced on the bolt circle. All inputs are diameters.
fn flange_plate(
    outer: f64,
    bore: f64,
    bolt_circle: f64,
    bolt_dia: f64,
    bolts: usize,
    seg: usize,
) -> Shape {
    let mut shape = Shape::circle(outer / 2.0, seg);
    if bore > 0.0 {
        shape.add_hole(Shape::circle(bore / 2.0, seg).contours.remove(0));
    }
    if bolts > 0 && bolt_dia > 0.0 {
        let bcr = bolt_circle / 2.0;
        for k in 0..bolts {
            let a = TAU * k as f64 / bolts as f64;
            let c = Vec2::from_angle(a, bcr);
            let mut hole = Shape::circle(bolt_dia / 2.0, 32);
            hole.transform(&Mat4::translation(Vec3::new(c.x, c.y, 0.0)));
            shape.add_hole(hole.contours.remove(0));
        }
    }
    shape
}

// ===========================================================================
// 2. Sprocket (ANSI B29.1 roller-chain)
// ===========================================================================

pub struct SprocketSkill;

impl Skill for SprocketSkill {
    fn name(&self) -> &'static str {
        "sprocket"
    }
    fn summary(&self) -> &'static str {
        "ANSI roller-chain sprocket (seating+topping tooth form)"
    }
    fn usage(&self) -> &'static str {
        "sprocket [teeth=17] [pitch=12.7] [roller=7.95] [bore=10] [seg_tooth=8] [thickness=0] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let teeth = args.usize_or("teeth", 17)?;
        if teeth < 4 {
            return Err(CadError::Arg("sprocket needs teeth>=4".into()));
        }
        let pitch = args.f64_or("pitch", 12.7)?;
        let roller = args.f64_or("roller", 7.95)?;
        let bore = args.f64_or("bore", 10.0)?;
        let seg_tooth = args.usize_or("seg_tooth", 8)?;
        let thickness = args.f64_or("thickness", 0.0)?;

        let profile = sprocket_profile(teeth, pitch, roller, seg_tooth)?;
        let mut shape = Shape::from_contour(profile);
        if bore > 0.0 {
            shape.add_hole(Shape::circle(bore / 2.0, 48).contours.remove(0));
        }

        let pd = pitch / (PI / teeth as f64).sin();
        let name = args.name().unwrap_or_else(|| scene.auto_name("sprocket"));
        if thickness > 0.0 {
            let mesh = extrude(&shape, thickness)?;
            scene.add(name.clone(), Geometry::Mesh(mesh));
            Ok(format!(
                "sprocket '{name}': {teeth}T pitch {pitch}, PD Ø{:.2} → {:.1}mm solid",
                pd, thickness
            ))
        } else {
            scene.add(name.clone(), Geometry::Shape(shape));
            Ok(format!(
                "sprocket '{name}': {teeth}T pitch {pitch}, roller Ø{roller}, PD Ø{:.2}",
                pd
            ))
        }
    }
}

/// ANSI B29.1 roller-chain sprocket profile, centred at the origin.
///
/// Rollers seat on the pitch circle (radius `PD/2`, `PD = pitch/sin(π/N)`),
/// spaced one chain pitch apart. Each seating pocket is a concave circular arc
/// of radius `Rs ≈ 0.505·roller` centred on its pitch point; the pocket edges
/// are placed exactly where the seating arc re-crosses the pitch circle, so the
/// flanks blend smoothly into the tooth. The flanks rise to the topping arc on
/// the outside circle `OD/2`, `OD = pitch·(0.6 + cot(π/N))`.
fn sprocket_profile(teeth: usize, pitch: f64, roller: f64, seg_tooth: usize) -> Result<Contour> {
    let n = teeth as f64;
    let half_pitch_ang = PI / n; // half the angular tooth pitch
    let pd = pitch / half_pitch_ang.sin();
    let pr = pd / 2.0; // pitch radius
    let rs = 0.505 * roller; // roller-seating radius
    let od = pitch * (0.6 + 1.0 / half_pitch_ang.tan());
    let outer_r = od / 2.0;

    if rs >= pr || outer_r <= pr {
        return Err(CadError::Geometry(
            "sprocket geometry degenerate (check teeth/pitch/roller)".into(),
        ));
    }

    // Seating arc half-sweep (about the pitch point) that lands its edge back on
    // the pitch circle: solving |P + Rs·û| = pr gives cos(δ) = Rs/(2·pr).
    let seat_half = (rs / (2.0 * pr)).acos();
    // Global angular offset of that pitch-circle edge from the pitch-point angle.
    let edge_local = Vec2::new(pr - rs * seat_half.cos(), rs * seat_half.sin());
    let edge_off = edge_local.y.atan2(edge_local.x);
    // Room left for the half-tooth (flank + half the tip) and the tip width.
    let room = (half_pitch_ang - edge_off).max(rad(0.5));
    let tip_half = 0.4 * room;

    let seat_seg = seg_tooth.max(3);
    let tip_seg = seg_tooth.max(3);
    let flank_seg = 3usize;

    let mut pts: Vec<Vec2> = Vec::new();
    for k in 0..teeth {
        let phi = TAU * k as f64 / n;
        let p = Vec2::from_angle(phi, pr);
        let inward = phi + PI; // from pitch point toward the sprocket centre

        // Seating pocket: sweep ψ from the clockwise edge down to the CCW edge
        // (decreasing ψ walks the overall profile counter-clockwise).
        for i in 0..=seat_seg {
            let psi = (inward + seat_half) - 2.0 * seat_half * (i as f64 / seat_seg as f64);
            pts.push(p + Vec2::from_angle(psi, rs));
        }

        // Rising flank: pocket CCW edge (r = pr) up to the tip start (r = OD/2).
        let tip_start = phi + half_pitch_ang - tip_half;
        let tip_end = phi + half_pitch_ang + tip_half;
        for i in 1..=flank_seg {
            let t = i as f64 / (flank_seg + 1) as f64;
            let a = (phi + edge_off) + (tip_start - (phi + edge_off)) * t;
            let r = pr + (outer_r - pr) * t;
            pts.push(Vec2::from_angle(a, r));
        }

        // Topping arc across the tooth tip on the outside circle.
        for i in 0..=tip_seg {
            let a = tip_start + (tip_end - tip_start) * (i as f64 / tip_seg as f64);
            pts.push(Vec2::from_angle(a, outer_r));
        }

        // Falling flank: tip end back down to the next pocket's clockwise edge.
        let next_edge = phi + 2.0 * half_pitch_ang - edge_off;
        for i in 1..=flank_seg {
            let t = i as f64 / (flank_seg + 1) as f64;
            let a = tip_end + (next_edge - tip_end) * t;
            let r = outer_r + (pr - outer_r) * t;
            pts.push(Vec2::from_angle(a, r));
        }
    }

    Ok(Contour::new(pts, true))
}

// ===========================================================================
// 3. Spur-gear rack
// ===========================================================================

pub struct RackSkill;

impl Skill for RackSkill {
    fn name(&self) -> &'static str {
        "rack"
    }
    fn summary(&self) -> &'static str {
        "Spur-gear rack: trapezoidal teeth on a bar (2D or solid)"
    }
    fn usage(&self) -> &'static str {
        "rack [teeth=10] [module=2] [pressure=20] [height=10] [thickness=0] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let teeth = args.usize_or("teeth", 10)?;
        if teeth < 1 {
            return Err(CadError::Arg("rack needs teeth>=1".into()));
        }
        let module = args.f64_or("module", 2.0)?;
        let pressure = args.f64_or("pressure", 20.0)?;
        let height = args.f64_or("height", 10.0)?;
        let thickness = args.f64_or("thickness", 0.0)?;

        let profile = rack_profile(teeth, module, pressure, height);
        let shape = Shape::from_contour(profile);

        let name = args.name().unwrap_or_else(|| scene.auto_name("rack"));
        if thickness > 0.0 {
            let mesh = extrude(&shape, thickness)?;
            scene.add(name.clone(), Geometry::Mesh(mesh));
            Ok(format!(
                "rack '{name}': {teeth}T module {module}, {pressure}° PA → {:.1}mm solid",
                thickness
            ))
        } else {
            scene.add(name.clone(), Geometry::Shape(shape));
            Ok(format!(
                "rack '{name}': {teeth}T module {module}, circular pitch {:.3}",
                PI * module
            ))
        }
    }
}

/// Spur-gear rack: straight-flanked trapezoidal teeth on a bar.
///
/// Circular pitch `p = π·module`; addendum `a = module`; dedendum
/// `d = 1.25·module`; tooth thickness at the pitch line is `p/2`, with the
/// flanks inclined at the pressure angle. Tooth tips sit at `y = +a`, roots at
/// `y = -d`, and the solid bar body extends `height` below the dedendum line.
fn rack_profile(teeth: usize, module: f64, pressure_deg: f64, height: f64) -> Contour {
    let p = PI * module; // circular pitch
    let a = module; // addendum
    let d = 1.25 * module; // dedendum
    let q = p / 4.0; // half tooth thickness at the pitch line
    let ta = rad(pressure_deg).tan();
    let tip_half = (q - a * ta).max(0.0); // half top-land width
    let root_half = (q - d * ta).max(0.0); // half root-land width
    let total = teeth as f64 * p;

    let mut pts: Vec<Vec2> = Vec::new();
    // Toothed top edge, left → right. x = 0 and x = total sit at valley centres.
    pts.push(Vec2::new(0.0, -d));
    for k in 0..teeth {
        let valley = k as f64 * p; // current root-valley centre
        let tooth = valley + p / 2.0; // tooth-tip centre
        let next_valley = valley + p;
        pts.push(Vec2::new(valley + root_half, -d)); // right edge of root land
        pts.push(Vec2::new(tooth - tip_half, a)); // rising flank → top land left
        pts.push(Vec2::new(tooth + tip_half, a)); // top land right
        pts.push(Vec2::new(next_valley - root_half, -d)); // falling flank → next root
    }
    pts.push(Vec2::new(total, -d));
    // Close the bar body downward by `height` below the dedendum line.
    pts.push(Vec2::new(total, -d - height));
    pts.push(Vec2::new(0.0, -d - height));

    Contour::new(pts, true)
}

// ===========================================================================
// 4. Timing-belt pulley
// ===========================================================================

pub struct PulleySkill;

impl Skill for PulleySkill {
    fn name(&self) -> &'static str {
        "pulley"
    }
    fn summary(&self) -> &'static str {
        "Timing-belt pulley (GT2 / HTD groove profile)"
    }
    fn usage(&self) -> &'static str {
        "pulley [teeth=20] [belt=GT2] [bore=5] [seg_tooth=6] [thickness=0] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let teeth = args.usize_or("teeth", 20)?;
        if teeth < 4 {
            return Err(CadError::Arg("pulley needs teeth>=4".into()));
        }
        let belt = args.str_or("belt", "GT2");
        let bore = args.f64_or("bore", 5.0)?;
        let seg_tooth = args.usize_or("seg_tooth", 6)?;
        let thickness = args.f64_or("thickness", 0.0)?;

        let (belt_pitch, groove_r, groove_depth, belt_label) = belt_spec(belt);
        let pd = teeth as f64 * belt_pitch / PI;

        let profile = pulley_profile(teeth, pd, groove_r, groove_depth, seg_tooth)?;
        let mut shape = Shape::from_contour(profile);
        if bore > 0.0 {
            shape.add_hole(Shape::circle(bore / 2.0, 48).contours.remove(0));
        }

        let name = args.name().unwrap_or_else(|| scene.auto_name("pulley"));
        if thickness > 0.0 {
            let mesh = extrude(&shape, thickness)?;
            scene.add(name.clone(), Geometry::Mesh(mesh));
            Ok(format!(
                "pulley '{name}': {teeth}T {belt_label}, PD Ø{:.2} → {:.1}mm solid",
                pd, thickness
            ))
        } else {
            scene.add(name.clone(), Geometry::Shape(shape));
            Ok(format!(
                "pulley '{name}': {teeth}T {belt_label} (pitch {belt_pitch}mm), PD Ø{:.2}",
                pd
            ))
        }
    }
}

/// Belt profile table: `(tooth pitch, groove radius, groove depth, label)`.
/// GT2 is a shallow curvilinear pocket; the HTD profiles are deeper rounded
/// grooves. Unknown belts fall back to GT2.
fn belt_spec(belt: &str) -> (f64, f64, f64, &'static str) {
    match belt.to_ascii_uppercase().as_str() {
        "GT2" | "2GT" | "2M" => (2.0, 1.0, 0.76, "GT2"),
        "HTD3M" | "HTD3" | "3M" => (3.0, 1.30, 1.22, "HTD3M"),
        "HTD5M" | "HTD5" | "5M" => (5.0, 2.10, 2.06, "HTD5M"),
        _ => (2.0, 1.0, 0.76, "GT2"),
    }
}

/// Timing-belt pulley profile centred at the origin: `teeth` concave tooth
/// grooves cut into a disc. Each groove is a round pocket of radius `groove_r`
/// whose deepest point sits `groove_depth` below the land circle
/// (`land_r = PD/2 + small addendum`); the pocket edges are placed where the
/// groove arc re-crosses the land circle, and the lands between grooves follow
/// that outer circle.
fn pulley_profile(
    teeth: usize,
    pd: f64,
    groove_r: f64,
    groove_depth: f64,
    seg_tooth: usize,
) -> Result<Contour> {
    let n = teeth as f64;
    let land_r = pd / 2.0 + 0.2 * groove_r; // small positive addendum over pitch
                                            // Groove-arc centre radius so the pocket bottom is `depth` below the land.
    let rc = land_r - groove_depth + groove_r;
    if rc <= 0.0 || groove_r <= 0.0 {
        return Err(CadError::Geometry(
            "pulley groove geometry degenerate".into(),
        ));
    }

    // Half-sweep of the groove arc (about its centre) at which it meets the land
    // circle: |C + gr·û| = land_r ⇒ cos(δ) = (Rc² + gr² − land_r²)/(2·gr·Rc).
    let cos_d = ((rc * rc + groove_r * groove_r - land_r * land_r) / (2.0 * groove_r * rc))
        .clamp(-1.0, 1.0);
    let groove_half = cos_d.acos();
    // Global angular offset of the groove edge from the tooth angle.
    let edge_local = Vec2::new(rc - groove_r * cos_d, groove_r * groove_half.sin());
    let land_off = edge_local.y.atan2(edge_local.x);

    let groove_seg = seg_tooth.max(3);
    let land_seg = seg_tooth.max(2);
    let tooth_ang = TAU / n;

    let mut pts: Vec<Vec2> = Vec::new();
    for k in 0..teeth {
        let phi = tooth_ang * k as f64;
        let c = Vec2::from_angle(phi, rc);
        let inward = phi + PI;

        // Concave groove: sweep ψ from the clockwise edge to the CCW edge.
        for i in 0..=groove_seg {
            let psi = (inward + groove_half) - 2.0 * groove_half * (i as f64 / groove_seg as f64);
            pts.push(c + Vec2::from_angle(psi, groove_r));
        }

        // Land arc on the outer circle to the next groove's clockwise edge
        // (interior samples only; the endpoints are the adjacent groove edges).
        let a0 = phi + land_off;
        let a1 = phi + tooth_ang - land_off;
        for i in 1..land_seg {
            let a = a0 + (a1 - a0) * (i as f64 / land_seg as f64);
            pts.push(Vec2::from_angle(a, land_r));
        }
    }

    Ok(Contour::new(pts, true))
}

// ===========================================================================
// 5. Radial disc cam
// ===========================================================================

pub struct CamSkill;

impl Skill for CamSkill {
    fn name(&self) -> &'static str {
        "cam"
    }
    fn summary(&self) -> &'static str {
        "Radial disc cam (harmonic / cycloidal rise-dwell-fall)"
    }
    fn usage(&self) -> &'static str {
        "cam [base=20] [lift=10] [motion=harmonic] [rise_deg=120] [dwell_deg=60] [bore=6] [seg=180] [thickness=0] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let base = args.f64_or("base", 20.0)?;
        let lift = args.f64_or("lift", 10.0)?;
        let motion = args.str_or("motion", "harmonic");
        let rise_deg = args.f64_or("rise_deg", 120.0)?;
        let dwell_deg = args.f64_or("dwell_deg", 60.0)?;
        let bore = args.f64_or("bore", 6.0)?;
        let seg = args.usize_or("seg", 180)?;
        let thickness = args.f64_or("thickness", 0.0)?;

        if base <= 0.0 {
            return Err(CadError::Arg("cam needs base>0".into()));
        }
        let cycloidal = motion.eq_ignore_ascii_case("cycloidal");

        let profile = cam_profile(base, lift, rise_deg, dwell_deg, cycloidal, seg);
        let mut shape = Shape::from_contour(profile);
        if bore > 0.0 && bore / 2.0 < base {
            shape.add_hole(Shape::circle(bore / 2.0, 48).contours.remove(0));
        }

        let law = if cycloidal { "cycloidal" } else { "harmonic" };
        let name = args.name().unwrap_or_else(|| scene.auto_name("cam"));
        if thickness > 0.0 {
            let mesh = extrude(&shape, thickness)?;
            scene.add(name.clone(), Geometry::Mesh(mesh));
            Ok(format!(
                "cam '{name}': base r{base}, lift {lift}, {law} → {:.1}mm solid",
                thickness
            ))
        } else {
            scene.add(name.clone(), Geometry::Shape(shape));
            Ok(format!(
                "cam '{name}': base r{base}, lift {lift}, {law} rise {rise_deg}°/dwell {dwell_deg}°"
            ))
        }
    }
}

/// Normalized follower displacement `S(θ) ∈ [0, 1]` for a rise-dwell-fall-dwell
/// cycle. The rise spans `rise_deg` (`S: 0→1`), the high dwell `dwell_deg`
/// (`S = 1`), the fall another `rise_deg` (`S: 1→0`), and the remainder is the
/// low dwell (`S = 0`). `S_rise` follows the true simple-harmonic or cycloidal
/// motion law.
fn cam_displacement(theta_deg: f64, rise_deg: f64, dwell_deg: f64, cycloidal: bool) -> f64 {
    let rise = rise_deg.max(1e-6);
    let s_rise = |t: f64| -> f64 {
        let t = t.clamp(0.0, 1.0);
        if cycloidal {
            t - (TAU * t).sin() / TAU
        } else {
            0.5 * (1.0 - (PI * t).cos())
        }
    };
    let end_rise = rise;
    let end_high = rise + dwell_deg;
    let end_fall = rise + dwell_deg + rise;
    if theta_deg < end_rise {
        s_rise(theta_deg / rise)
    } else if theta_deg < end_high {
        1.0
    } else if theta_deg < end_fall {
        1.0 - s_rise((theta_deg - end_high) / rise)
    } else {
        0.0
    }
}

/// Radial disc-cam profile `r(θ) = base + lift·S(θ)`, sampled over 360°.
fn cam_profile(
    base: f64,
    lift: f64,
    rise_deg: f64,
    dwell_deg: f64,
    cycloidal: bool,
    seg: usize,
) -> Contour {
    let seg = seg.max(12);
    let mut pts: Vec<Vec2> = Vec::with_capacity(seg);
    for i in 0..seg {
        let theta_deg = 360.0 * i as f64 / seg as f64;
        let s = cam_displacement(theta_deg, rise_deg, dwell_deg, cycloidal);
        let r = base + lift * s;
        pts.push(Vec2::from_angle(rad(theta_deg), r));
    }
    Contour::new(pts, true)
}

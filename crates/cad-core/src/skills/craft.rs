//! Craft / laser skills: parametric 2D generators for paper-craft, card, vinyl
//! and laser-cutting workflows.
//!
//! Each skill authors geometry directly in scene units and places entities on
//! fabrication *roles* (`cut` / `score` / `engrave`) so the layered exporters
//! can map them to machine layers. Every parameter is defaulted, so a bare
//! `skill <name>` always yields valid, non-empty geometry.

use crate::args::Args;
use crate::error::Result;
use crate::geom2d::text::{stroke_text, Align};
use crate::geom2d::{Contour, Shape};
use crate::math::{Mat4, Vec2, Vec3, PI, TAU};
use crate::scene::{Geometry, Scene};
use crate::skills::Skill;

// ============================================================================
// 1. monogram
// ============================================================================

pub struct MonogramSkill;

impl Skill for MonogramSkill {
    fn name(&self) -> &'static str {
        "monogram"
    }
    fn summary(&self) -> &'static str {
        "Centered single-stroke initials inside an optional cut frame"
    }
    fn usage(&self) -> &'static str {
        "monogram [text=AB] [size=30] [frame=circle|rect|none] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let text = args.str_or("text", "AB").to_string();
        let size = args.f64_or("size", 30.0)?;
        let frame = args.str_or("frame", "circle").to_string();

        // Centered stroke text, recentred about the origin.
        let mut ink = stroke_text(&text, size, Align::Center);
        if !ink.bbox().is_empty() {
            let c = ink.bbox().center();
            ink.transform(&Mat4::translation(Vec3::new(-c.x, -c.y, 0.0)));
        }
        let bb = ink.bbox();
        let (tw, th) = (bb.width().max(size), bb.height().max(size));

        let prefix = args.name().unwrap_or_else(|| scene.auto_name("monogram"));

        match frame.as_str() {
            "none" => {
                scene.add(prefix.clone(), Geometry::Shape(ink));
                scene.set_attrs(Some(prefix.as_str()), None, Some("engrave".into()))?;
                Ok(format!("monogram '{prefix}': '{text}' (no frame)"))
            }
            other => {
                let frame_shape = if other == "rect" {
                    let w = tw + size * 0.9;
                    let h = th + size * 0.9;
                    let mut r = Shape::rect(w, h, (size * 0.3).min(w.min(h) / 2.0), 8);
                    r.transform(&Mat4::translation(Vec3::new(-w / 2.0, -h / 2.0, 0.0)));
                    r
                } else {
                    // default: circle
                    let radius = (tw.max(th) / 2.0 + size * 0.35).max(size * 0.9);
                    Shape::circle(radius, 72)
                };
                let cut = format!("{prefix}_cut");
                let eng = format!("{prefix}_engrave");
                scene.add(cut.clone(), Geometry::Shape(frame_shape));
                scene.set_attrs(Some(cut.as_str()), None, Some("cut".into()))?;
                scene.add(eng.clone(), Geometry::Shape(ink));
                scene.set_attrs(Some(eng.as_str()), None, Some("engrave".into()))?;
                Ok(format!(
                    "monogram '{prefix}': '{text}' in {other} frame (cut + engrave)"
                ))
            }
        }
    }
}

// ============================================================================
// 2. mandala
// ============================================================================

pub struct MandalaSkill;

impl Skill for MandalaSkill {
    fn name(&self) -> &'static str {
        "mandala"
    }
    fn summary(&self) -> &'static str {
        "Decorative polar petal motif for laser/vinyl cutting"
    }
    fn usage(&self) -> &'static str {
        "mandala [petals=12] [rings=2] [r=40] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let petals = args.usize_or("petals", 12)?.max(1);
        let rings = args.usize_or("rings", 2)?.max(1);
        let r = args.f64_or("r", 40.0)?;

        let contours = build_mandala(petals, rings, r);
        let count = contours.len();
        let shape = Shape::new(contours);

        let name = args.name().unwrap_or_else(|| scene.auto_name("mandala"));
        scene.add(name.clone(), Geometry::Shape(shape));
        scene.set_attrs(Some(name.as_str()), None, Some("cut".into()))?;
        Ok(format!(
            "mandala '{name}': {petals} petals × {rings} rings ({count} contours), r{r}"
        ))
    }
}

/// A leaf/lens petal along +X, from `(0,0)` to `(len,0)`, bulging to +/-`w`.
fn petal(len: f64, w: f64, seg: usize) -> Contour {
    let seg = seg.max(4);
    let mut pts = Vec::with_capacity(seg * 2);
    // Top edge 0 -> len.
    for i in 0..=seg {
        let t = i as f64 / seg as f64;
        pts.push(Vec2::new(len * t, w * (PI * t).sin()));
    }
    // Bottom edge len -> 0 (skip shared endpoints).
    for i in (1..seg).rev() {
        let t = i as f64 / seg as f64;
        pts.push(Vec2::new(len * t, -w * (PI * t).sin()));
    }
    Contour::new(pts, true)
}

fn build_mandala(petals: usize, rings: usize, r: f64) -> Vec<Contour> {
    let mut out: Vec<Contour> = Vec::new();
    let ring_step = r / rings as f64;
    for ri in 0..rings {
        let base = ri as f64 * ring_step;
        let len = ring_step * 0.95;
        let w = ring_step * 0.35;
        for k in 0..petals {
            let ang = TAU * (k as f64 / petals as f64);
            // Apply translate-out first, then rotate about the origin.
            let m = Mat4::rotation_z(ang).mul(&Mat4::translation(Vec3::new(base, 0.0, 0.0)));
            let mut p = Shape::from_contour(petal(len, w, 10));
            p.transform(&m);
            out.append(&mut p.contours);
        }
    }
    // Small decorative hub.
    out.push(circle_contour(Vec2::ZERO, (r * 0.08).max(1.0), 24));
    out
}

// ============================================================================
// 3. card
// ============================================================================

pub struct CardSkill;

impl Skill for CardSkill {
    fn name(&self) -> &'static str {
        "card"
    }
    fn summary(&self) -> &'static str {
        "Folding greeting-card net (cut outline + score fold, optional window)"
    }
    fn usage(&self) -> &'static str {
        "card [width=100] [height=70] [fold=left|top] [window=0] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let w = args.f64_or("width", 100.0)?;
        let h = args.f64_or("height", 70.0)?;
        let fold = args.str_or("fold", "left").to_string();
        let window = args.f64_or("window", 0.0)?;
        let prefix = args.str_or("name", "card").to_string();

        // Outer cut outline.
        let mut cut = Shape::rect(w, h, 0.0, 1);

        // Fold line + optional window placement depend on orientation.
        let (p0, p1, win_c) = if fold == "top" {
            (
                Vec2::new(0.0, h / 2.0),
                Vec2::new(w, h / 2.0),
                Vec2::new(w / 2.0, 3.0 * h / 4.0), // in the top panel
            )
        } else {
            (
                Vec2::new(w / 2.0, 0.0),
                Vec2::new(w / 2.0, h),
                Vec2::new(3.0 * w / 4.0, h / 2.0), // in the right panel
            )
        };

        if window > 0.0 {
            let mut win = Shape::circle(window / 2.0, 48);
            win.transform(&Mat4::translation(Vec3::new(win_c.x, win_c.y, 0.0)));
            cut.add_hole(win.contours.remove(0));
        }

        let score = Shape::polyline(vec![p0, p1], false);

        let cut_name = format!("{prefix}_cut");
        let score_name = format!("{prefix}_score");
        scene.add(cut_name.clone(), Geometry::Shape(cut));
        scene.set_attrs(Some(cut_name.as_str()), None, Some("cut".into()))?;
        scene.add(score_name.clone(), Geometry::Shape(score));
        scene.set_attrs(Some(score_name.as_str()), None, Some("score".into()))?;

        let win_note = if window > 0.0 {
            format!(", Ø{window} window")
        } else {
            String::new()
        };
        Ok(format!(
            "card '{prefix}': {w}×{h} {fold}-fold net (cut + score{win_note})"
        ))
    }
}

// ============================================================================
// 4. box_template
// ============================================================================

pub struct BoxTemplateSkill;

impl Skill for BoxTemplateSkill {
    fn name(&self) -> &'static str {
        "box_template"
    }
    fn summary(&self) -> &'static str {
        "Flat unfolded tuck-box net: perimeter cut + fold score lines"
    }
    fn usage(&self) -> &'static str {
        "box_template [width=60] [depth=40] [height=30] [tab=10] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let width = args.f64_or("width", 60.0)?;
        let depth = args.f64_or("depth", 40.0)?;
        let height = args.f64_or("height", 30.0)?;
        let tab = args.f64_or("tab", 10.0)?;
        let prefix = args.str_or("name", "box").to_string();

        // Panel x-boundaries along the strip: width, depth, width, depth.
        let xs = [
            0.0,
            width,
            width + depth,
            2.0 * width + depth,
            2.0 * (width + depth),
        ];
        let strip_w = xs[4];
        // Relief gap between adjacent flaps so the perimeter is a clean comb.
        let gap = (tab * 0.5).min(width.min(depth) * 0.25).max(0.0);

        let perimeter = box_perimeter(&xs, height, tab, gap);
        let folds = box_folds(&xs, height, strip_w);

        let cut_name = format!("{prefix}_cut");
        let score_name = format!("{prefix}_score");
        scene.add(
            cut_name.clone(),
            Geometry::Shape(Shape::from_contour(perimeter)),
        );
        scene.set_attrs(Some(cut_name.as_str()), None, Some("cut".into()))?;
        scene.add(score_name.clone(), Geometry::Shape(Shape::new(folds)));
        scene.set_attrs(Some(score_name.as_str()), None, Some("score".into()))?;

        Ok(format!(
            "box_template '{prefix}': {width}×{depth}×{height} tuck-box net (cut + score, tab {tab})"
        ))
    }
}

/// Trace the outer perimeter of the net: a 4-panel strip with per-panel top and
/// bottom flaps plus a glue tab on the right. Returns one closed contour.
fn box_perimeter(xs: &[f64; 5], height: f64, tab: f64, gap: f64) -> Contour {
    let strip_w = xs[4];
    let mut pts: Vec<Vec2> = Vec::new();

    pts.push(Vec2::new(0.0, 0.0));
    // Bottom edge, left -> right, dipping down for each panel's bottom flap.
    for i in 0..4 {
        let a = xs[i] + gap;
        let b = xs[i + 1] - gap;
        if b > a {
            pts.push(Vec2::new(a, 0.0));
            pts.push(Vec2::new(a, -tab));
            pts.push(Vec2::new(b, -tab));
            pts.push(Vec2::new(b, 0.0));
        }
        pts.push(Vec2::new(xs[i + 1], 0.0));
    }
    // Glue tab on the right side (full height).
    pts.push(Vec2::new(strip_w + tab, 0.0));
    pts.push(Vec2::new(strip_w + tab, height));
    pts.push(Vec2::new(strip_w, height));
    // Top edge, right -> left, rising for each panel's top flap.
    for i in (0..4).rev() {
        let a = xs[i] + gap;
        let b = xs[i + 1] - gap;
        if b > a {
            pts.push(Vec2::new(b, height));
            pts.push(Vec2::new(b, height + tab));
            pts.push(Vec2::new(a, height + tab));
            pts.push(Vec2::new(a, height));
        }
        pts.push(Vec2::new(xs[i], height));
    }
    // Close down the left edge implicitly.
    Contour::new(pts, true)
}

/// Internal fold lines: vertical panel/glue-tab folds and the two horizontal
/// flap folds. Each is an open 2-point contour.
fn box_folds(xs: &[f64; 5], height: f64, strip_w: f64) -> Vec<Contour> {
    let mut folds: Vec<Contour> = Vec::new();
    // Vertical folds between panels and at the glue tab.
    for &x in &xs[1..=4] {
        folds.push(Contour::new(
            vec![Vec2::new(x, 0.0), Vec2::new(x, height)],
            false,
        ));
    }
    // Horizontal flap folds along the whole strip.
    folds.push(Contour::new(
        vec![Vec2::new(0.0, 0.0), Vec2::new(strip_w, 0.0)],
        false,
    ));
    folds.push(Contour::new(
        vec![Vec2::new(0.0, height), Vec2::new(strip_w, height)],
        false,
    ));
    folds
}

// ============================================================================
// 5. living_hinge
// ============================================================================

pub struct LivingHingeSkill;

impl Skill for LivingHingeSkill {
    fn name(&self) -> &'static str {
        "living_hinge"
    }
    fn summary(&self) -> &'static str {
        "Laser living-hinge panel: rect filled with staggered flex slots"
    }
    fn usage(&self) -> &'static str {
        "living_hinge [width=80] [height=40] [slot=15] [gap=3] [rows=0] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let width = args.f64_or("width", 80.0)?;
        let height = args.f64_or("height", 40.0)?;
        let slot = args.f64_or("slot", 15.0)?.max(0.1);
        let gap = args.f64_or("gap", 3.0)?.max(0.1);
        let rows = args.usize_or("rows", 0)?;

        // Outline on the cut layer, then all slots pushed into the same shape.
        let mut shape = Shape::rect(width, height, 0.0, 1);
        let n = fill_hinge_slots(&mut shape, width, height, slot, gap, rows);

        let name = args
            .name()
            .unwrap_or_else(|| scene.auto_name("living_hinge"));
        scene.add(name.clone(), Geometry::Shape(shape));
        scene.set_attrs(Some(name.as_str()), None, Some("cut".into()))?;
        Ok(format!(
            "living_hinge '{name}': {width}×{height} panel, {n} flex slots"
        ))
    }
}

/// Push a staggered (brick-pattern) field of vertical slot cuts into `shape`.
/// Returns the slot count.
fn fill_hinge_slots(
    shape: &mut Shape,
    width: f64,
    height: f64,
    slot: f64,
    gap: f64,
    rows: usize,
) -> usize {
    let margin = gap.min(width / 4.0).min(height / 4.0);
    let period = slot + gap; // vertical pitch
    let col_spacing = (slot * 0.5).max(gap * 1.5);

    let usable_w = (width - 2.0 * margin).max(0.0);
    let ncols = ((usable_w / col_spacing).floor() as usize).max(2);

    // Slots per column (height budget), honoring an explicit `rows`.
    let auto_rows = (((height - 2.0 * margin) / period).floor() as usize).max(1);
    let per_col = if rows > 0 { rows } else { auto_rows };

    let mut count = 0usize;
    for j in 0..=ncols {
        let x = margin + j as f64 * col_spacing;
        if x > width - margin + 1e-9 {
            break;
        }
        // Brick offset on alternating columns.
        let offset = if j % 2 == 1 { period / 2.0 } else { 0.0 };
        for row in 0..per_col {
            let y0 = margin + offset + row as f64 * period;
            let y1 = y0 + slot;
            if y1 > height - margin + 1e-9 {
                break;
            }
            shape.contours.push(Contour::new(
                vec![Vec2::new(x, y0), Vec2::new(x, y1)],
                false,
            ));
            count += 1;
        }
    }
    count
}

// ============================================================================
// shared helpers
// ============================================================================

/// A closed circle contour centered at `c` (local polyline, no primitive hint).
fn circle_contour(c: Vec2, r: f64, seg: usize) -> Contour {
    let seg = seg.max(8);
    let mut pts = Vec::with_capacity(seg);
    for i in 0..seg {
        let a = TAU * (i as f64 / seg as f64);
        pts.push(c + Vec2::from_angle(a, r));
    }
    Contour::new(pts, true)
}

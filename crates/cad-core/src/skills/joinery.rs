// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Joinery & fixture skills: flat 2D laser/CNC panel generators for shop
//! furniture and panel-joining hardware.
//!
//! Like the craft skills, each generator authors geometry directly in scene
//! units and places entities on fabrication *roles* (`cut` / `engrave`). Every
//! parameter is defaulted, so a bare `skill <name>` always yields valid,
//! non-empty, closed-contour geometry ready for a layered exporter.

use crate::args::Args;
use crate::error::Result;
use crate::geom2d::text::{stroke_text, Align};
use crate::geom2d::{Contour, Shape};
use crate::math::{Mat4, Vec2, Vec3};
use crate::scene::{Geometry, Scene};
use crate::skills::Skill;

// ============================================================================
// 1. shelf_pin
// ============================================================================

pub struct ShelfPinSkill;

impl Skill for ShelfPinSkill {
    fn name(&self) -> &'static str {
        "shelf_pin"
    }
    fn summary(&self) -> &'static str {
        "Shelf-pin drilling template: a thin strip with a row of evenly-spaced holes"
    }
    fn usage(&self) -> &'static str {
        "shelf_pin [count=6] [spacing=32] [dia=5] [margin=12] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let count = args.usize_or("count", 6)?.max(1);
        let spacing = args.f64_or("spacing", 32.0)?.max(1.0);
        let dia = args.f64_or("dia", 5.0)?.max(0.2);
        let margin = args.f64_or("margin", 12.0)?.max(dia);
        let r = dia / 2.0;

        // Strip outline: hole centres sit on the mid-line, `margin` from each end
        // and edge, so the strip is `2*margin` tall and spans the whole row.
        let length = 2.0 * margin + (count as f64 - 1.0) * spacing;
        let height = 2.0 * margin;
        let mut strip = Shape::rect(length, height, (margin * 0.3).min(height / 2.0), 8);

        // Holes as inner contours so they export as cut circles.
        let cy = height / 2.0;
        for i in 0..count {
            let cx = margin + i as f64 * spacing;
            let mut hole = Shape::circle(r, 32);
            hole.transform(&Mat4::translation(Vec3::new(cx, cy, 0.0)));
            strip.add_hole(hole.contours.remove(0));
        }

        let name = args.name().unwrap_or_else(|| scene.auto_name("shelf_pin"));
        scene.add(name.clone(), Geometry::Shape(strip));
        scene.set_attrs(Some(name.as_str()), None, Some("cut".into()))?;
        Ok(format!(
            "shelf_pin '{name}': {count} × Ø{dia} holes @ {spacing}mm ({length:.0}×{height:.0}mm strip)"
        ))
    }
}

// ============================================================================
// 2. drawer
// ============================================================================

pub struct DrawerSkill;

impl Skill for DrawerSkill {
    fn name(&self) -> &'static str {
        "drawer"
    }
    fn summary(&self) -> &'static str {
        "Flat-pack drawer: front, back, two sides and a base laid out for laser cutting"
    }
    fn usage(&self) -> &'static str {
        "drawer [width=200] [depth=150] [height=80] [thickness=3] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let width = args.f64_or("width", 200.0)?.max(1.0);
        let depth = args.f64_or("depth", 150.0)?.max(1.0);
        let height = args.f64_or("height", 80.0)?.max(1.0);
        let thickness = args.f64_or("thickness", 3.0)?.max(0.1);
        let prefix = args.str_or("name", "drawer").to_string();

        // Butt-jointed panels (outer dimensions). Front/back share the face size,
        // the two sides share the depth face, and the base spans the footprint.
        let panels: [(&str, f64, f64); 5] = [
            ("front", width, height),
            ("back", width, height),
            ("left", depth, height),
            ("right", depth, height),
            ("base", width, depth),
        ];

        let gap = (thickness * 3.0).max(8.0);
        let mut cut = Shape::new(Vec::new());
        let mut labels = Shape::new(Vec::new());
        let mut cursor = 0.0f64;
        for (label, w, h) in panels {
            // Panel outline placed at the running cursor along +X.
            let mut rect = Shape::rect(w, h, 0.0, 1);
            rect.transform(&Mat4::translation(Vec3::new(cursor, 0.0, 0.0)));
            cut.contours.append(&mut rect.contours);

            // Centred engrave label naming each panel.
            let size = (w.min(h) * 0.2).clamp(4.0, 12.0);
            let mut ink = stroke_text(label, size, Align::Center);
            if !ink.bbox().is_empty() {
                let c = ink.bbox().center();
                ink.transform(&Mat4::translation(Vec3::new(
                    cursor + w / 2.0 - c.x,
                    h / 2.0 - c.y,
                    0.0,
                )));
                labels.contours.append(&mut ink.contours);
            }
            cursor += w + gap;
        }

        let cut_name = format!("{prefix}_cut");
        let label_name = format!("{prefix}_label");
        scene.add(cut_name.clone(), Geometry::Shape(cut));
        scene.set_attrs(Some(cut_name.as_str()), None, Some("cut".into()))?;
        scene.add(label_name.clone(), Geometry::Shape(labels));
        scene.set_attrs(Some(label_name.as_str()), None, Some("engrave".into()))?;

        Ok(format!(
            "drawer '{prefix}': 5 panels ({width}×{depth}×{height}mm, {thickness}mm stock) laid out flat (cut + label)"
        ))
    }
}

// ============================================================================
// 3. tslot_joint
// ============================================================================

pub struct TSlotJointSkill;

impl Skill for TSlotJointSkill {
    fn name(&self) -> &'static str {
        "tslot_joint"
    }
    fn summary(&self) -> &'static str {
        "T-slot joint sample: a captive-nut slot panel and its mating tab panel"
    }
    fn usage(&self) -> &'static str {
        "tslot_joint [length=60] [thickness=3] [bolt=5] [name=<id>]"
    }

    fn run(&self, args: &Args, scene: &mut Scene) -> Result<String> {
        let length = args.f64_or("length", 60.0)?.max(10.0);
        let thickness = args.f64_or("thickness", 3.0)?.max(0.5);
        let bolt = args.f64_or("bolt", 5.0)?.max(1.0);
        let prefix = args.str_or("name", "tslot").to_string();

        // Panel height scaled so the T-slot and tab always fit with margins.
        let ph = (length * 0.6).max(bolt * 8.0);

        let slot_panel = build_tslot_panel(length, ph, bolt);
        let tab_panel = build_tab_panel(length, ph, thickness, bolt);

        // Lay the two panels side by side with a gap.
        let gap = (thickness * 3.0).max(8.0);
        let mut tab = tab_panel;
        tab.transform(&Mat4::translation(Vec3::new(length + gap, 0.0, 0.0)));

        let slot_name = format!("{prefix}_slot");
        let tab_name = format!("{prefix}_tab");
        scene.add(slot_name.clone(), Geometry::Shape(slot_panel));
        scene.set_attrs(Some(slot_name.as_str()), None, Some("cut".into()))?;
        scene.add(tab_name.clone(), Geometry::Shape(tab));
        scene.set_attrs(Some(tab_name.as_str()), None, Some("cut".into()))?;

        Ok(format!(
            "tslot_joint '{prefix}': slot + tab panels (M{bolt} captive nut, {thickness}mm stock)"
        ))
    }
}

/// The slot panel: a rectangle with a T-shaped captive-nut cutout — a narrow
/// bolt-shaft channel (stem) opening into a wider nut pocket (crossbar).
fn build_tslot_panel(length: f64, ph: f64, bolt: f64) -> Shape {
    let mut panel = Shape::rect(length, ph, 0.0, 1);

    let cx = length / 2.0;
    let hs = (bolt / 2.0).min(length / 6.0); // half bolt-shaft width
    let nut_w = (bolt * 1.8).min(length * 0.6); // nut pocket width (crossbar)
    let nut_h = (bolt * 0.9).min(ph * 0.25); // nut pocket height
    let shaft_len = (ph * 0.3).max(bolt); // stem length toward the edge
    let cy = ph * 0.35; // stem/pocket junction height

    // T outline: stem pointing down, nut pocket (crossbar) on top. CCW.
    let t = vec![
        Vec2::new(cx - hs, cy - shaft_len),
        Vec2::new(cx + hs, cy - shaft_len),
        Vec2::new(cx + hs, cy),
        Vec2::new(cx + nut_w / 2.0, cy),
        Vec2::new(cx + nut_w / 2.0, cy + nut_h),
        Vec2::new(cx - nut_w / 2.0, cy + nut_h),
        Vec2::new(cx - nut_w / 2.0, cy),
        Vec2::new(cx - hs, cy),
    ];
    panel.add_hole(Contour::new(t, true));
    panel
}

/// The tab panel: a rectangle with a protruding mating tab on its right edge and
/// a bolt clearance hole on the bolt axis.
fn build_tab_panel(length: f64, ph: f64, thickness: f64, bolt: f64) -> Shape {
    let tab_w = ph * 0.4; // tab height (across the edge)
    let tab_len = (thickness * 2.0).max(4.0); // tab protrusion
    let yb = (ph - tab_w) / 2.0;
    let yt = (ph + tab_w) / 2.0;

    // Rectangle [0,length]×[0,ph] with a bump out of the right edge. CCW.
    let outline = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(length, 0.0),
        Vec2::new(length, yb),
        Vec2::new(length + tab_len, yb),
        Vec2::new(length + tab_len, yt),
        Vec2::new(length, yt),
        Vec2::new(length, ph),
        Vec2::new(0.0, ph),
    ];
    let mut panel = Shape::from_contour(Contour::new(outline, true));

    // Bolt clearance hole on the joint axis (mid-height), set in from the edge.
    let r = (bolt / 2.0).min(tab_w / 3.0).max(0.3);
    let mut hole = Shape::circle(r, 32);
    hole.transform(&Mat4::translation(Vec3::new(length * 0.75, ph / 2.0, 0.0)));
    panel.add_hole(hole.contours.remove(0));
    panel
}

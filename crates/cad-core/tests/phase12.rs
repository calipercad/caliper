// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! Phase 12 integration tests (roadmap v1.2): assemblies & BOM — the `part`
//! label, the `assembly` alias (reusing group transforms), group-aware export,
//! the `bom` report, and save round-tripping of the new attributes.

use cad_core::scene::Geometry;
use cad_core::Interpreter;

#[test]
fn part_label_and_bom_count() {
    let mut it = Interpreter::new();
    it.exec_line("circle 3 name=bolt").unwrap();
    it.exec_line("part M3 target=bolt").unwrap();
    assert_eq!(
        it.scene.get("bolt").unwrap().part.as_deref(),
        Some("M3"),
        "part label set"
    );
    let bom = it.exec_line("bom").unwrap();
    assert!(bom.contains("M3") && bom.contains("×1"), "{bom}");
}

#[test]
fn array_groups_into_bom_quantity() {
    // CHECKPOINT: a polar array of a bolt → bom shows the right quantity.
    let mut it = Interpreter::new();
    it.exec_line("circle 2 name=bolt").unwrap();
    it.exec_line("translate 20 0 target=bolt").unwrap();
    it.exec_line("array 6 kind=polar about=origin target=bolt name=ring")
        .unwrap();
    // Copies are bolt, bolt1..bolt5 → stripped to "bolt" ×6.
    let bom = it.exec_line("bom").unwrap();
    assert!(bom.contains("bolt") && bom.contains("×6"), "{bom}");
}

#[test]
fn explicit_part_label_overrides_name_grouping() {
    let mut it = Interpreter::new();
    it.exec_line("circle 2 name=a").unwrap();
    it.exec_line("circle 2 name=b").unwrap();
    it.exec_line("part washer target=a").unwrap();
    it.exec_line("part washer target=b").unwrap();
    let bom = it.exec_line("bom").unwrap();
    // Both collapse under the explicit label despite different names.
    assert!(bom.contains("washer") && bom.contains("×2"), "{bom}");
    assert!(
        bom.contains("1 part types") || bom.contains("1 part type"),
        "{bom}"
    );
}

#[test]
fn assembly_tags_group_and_moves_together() {
    let mut it = Interpreter::new();
    it.exec_line("rect 10 10 name=base").unwrap();
    it.exec_line("rect 4 4 name=peg").unwrap();
    it.exec_line("assembly widget base peg").unwrap();
    assert_eq!(
        it.scene.get("base").unwrap().group.as_deref(),
        Some("widget")
    );
    // A transform on the assembly tag moves every member.
    it.exec_line("translate 100 0 target=widget").unwrap();
    for n in ["base", "peg"] {
        if let Geometry::Shape(s) = &it.scene.get(n).unwrap().geom {
            assert!(s.bbox().min.x >= 99.0, "{n} moved: {}", s.bbox().min.x);
        } else {
            panic!();
        }
    }
}

#[test]
fn bom_scoped_to_one_assembly() {
    let mut it = Interpreter::new();
    it.exec_line("circle 2 name=screw").unwrap();
    it.exec_line("circle 5 name=spacer").unwrap();
    it.exec_line("assembly kit screw").unwrap();
    let scoped = it.exec_line("bom group=kit").unwrap();
    assert!(scoped.contains("screw"), "{scoped}");
    assert!(
        !scoped.contains("spacer"),
        "scoped bom excludes non-members:\n{scoped}"
    );
}

#[test]
fn group_aware_export_writes_only_members() {
    let mut it = Interpreter::new();
    it.exec_line("rect 10 10 name=a").unwrap();
    it.exec_line("rect 10 10 name=b").unwrap();
    it.exec_line("rect 10 10 name=c").unwrap();
    it.exec_line("assembly sub a b").unwrap();
    let p = std::env::temp_dir().join("cad_p12_group.svg");
    let msg = it
        .exec_line(&format!("export svg {} group=sub", p.display()))
        .unwrap();
    assert!(msg.contains("wrote 2 entities"), "{msg}");
}

#[test]
fn part_and_assembly_roundtrip_through_save() {
    let mut it = Interpreter::new();
    it.exec_line("circle 3 name=bolt").unwrap();
    it.exec_line("part M3 target=bolt").unwrap();
    it.exec_line("rect 20 20 name=plate").unwrap();
    it.exec_line("assembly jig bolt plate").unwrap();

    let p = std::env::temp_dir().join("cad_p12_save.cad");
    it.exec_line(&format!("save {}", p.display())).unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("part M3"), "part recorded:\n{text}");
    assert!(text.contains("assembly jig"), "assembly recorded:\n{text}");

    let mut it2 = Interpreter::new();
    it2.exec_line(&format!("load {}", p.display())).unwrap();
    assert_eq!(it2.scene.get("bolt").unwrap().part.as_deref(), Some("M3"));
    assert_eq!(
        it2.scene.get("plate").unwrap().group.as_deref(),
        Some("jig")
    );
}

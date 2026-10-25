//! Phase 9 integration tests (roadmap v1.1): parametric variables & expressions
//! driven end-to-end through the interpreter. The evaluator's own unit tests
//! live in `src/script/expr.rs`; these exercise the wiring: `set`/`unset`/`vars`,
//! `$var`/`${expr}` expansion in command arguments, undo of a variable, and
//! that `save` preserves the expressions verbatim (parametric round-trip).

use cad_core::scene::Geometry;
use cad_core::Interpreter;
use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cad_p9_{name}"))
}

fn shape_bbox(it: &Interpreter, name: &str) -> (f64, f64) {
    match &it.scene.get(name).unwrap().geom {
        Geometry::Shape(s) => {
            let b = s.bbox();
            (b.width(), b.height())
        }
        _ => panic!("{name} is not a shape"),
    }
}

#[test]
fn variable_drives_rect_dimensions() {
    // CHECKPOINT: one `set` drives the whole part; `$w` and `$w*2` evaluate.
    let mut it = Interpreter::new();
    it.exec_line("set w 40").unwrap();
    it.exec_line("rect $w $w*2 name=plate").unwrap();
    let (w, h) = shape_bbox(&it, "plate");
    assert!(
        (w - 40.0).abs() < 1e-6 && (h - 80.0).abs() < 1e-6,
        "{w}×{h}"
    );
}

#[test]
fn expression_in_option_value() {
    // circle radius from `$w/4` → r=10 → diameter-20 bbox.
    let mut it = Interpreter::new();
    it.exec_line("set w 40").unwrap();
    it.exec_line("circle $w/4 name=hole").unwrap();
    let (w, h) = shape_bbox(&it, "hole");
    assert!((w - 20.0).abs() < 0.3 && (h - 20.0).abs() < 0.3, "{w}×{h}");
}

#[test]
fn braced_expression_without_variable() {
    // `${...}` is an escape hatch needing no variable.
    let mut it = Interpreter::new();
    it.exec_line("rect ${40*2} ${30+10} name=r").unwrap();
    let (w, h) = shape_bbox(&it, "r");
    assert!(
        (w - 80.0).abs() < 1e-6 && (h - 40.0).abs() < 1e-6,
        "{w}×{h}"
    );
}

#[test]
fn set_references_prior_variable() {
    let mut it = Interpreter::new();
    it.exec_line("set w 40").unwrap();
    it.exec_line("set h $w*2").unwrap();
    assert_eq!(it.vars["h"], 80.0);
    // Whitespace in the expression is fine (tokens rejoined with spaces).
    it.exec_line("set d $w / 2").unwrap();
    assert_eq!(it.vars["d"], 20.0);
}

#[test]
fn unset_and_vars_listing() {
    let mut it = Interpreter::new();
    it.exec_line("set a 1").unwrap();
    it.exec_line("set b 2").unwrap();
    let listing = it.exec_line("vars").unwrap();
    assert!(
        listing.contains("a = 1") && listing.contains("b = 2"),
        "{listing}"
    );
    it.exec_line("unset a").unwrap();
    assert!(!it.vars.contains_key("a"));
    assert!(
        it.exec_line("unset a").is_err(),
        "unset of missing var errors"
    );
}

#[test]
fn undo_restores_variable() {
    // CHECKPOINT: undo reverts a variable edit, not just scene geometry.
    let mut it = Interpreter::new();
    it.exec_line("set w 40").unwrap();
    it.exec_line("set w 99").unwrap();
    assert_eq!(it.vars["w"], 99.0);
    it.exec_line("undo").unwrap();
    assert_eq!(it.vars["w"], 40.0, "undo restores the prior value");
    it.exec_line("redo").unwrap();
    assert_eq!(it.vars["w"], 99.0, "redo reapplies it");
}

#[test]
fn save_preserves_expressions_and_reloads() {
    // CHECKPOINT: a saved script keeps the parametric intent verbatim, and
    // re-loading reproduces the same geometry.
    let p = tmp("param.cad");
    let mut it = Interpreter::new();
    it.exec_line("set w 40").unwrap();
    it.exec_line("rect $w $w*2 name=plate").unwrap();
    it.exec_line(&format!("save {}", p.display())).unwrap();

    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("set w 40"), "set line preserved:\n{text}");
    assert!(
        text.contains("rect $w $w*2"),
        "expression preserved unexpanded:\n{text}"
    );

    let mut it2 = Interpreter::new();
    it2.exec_line(&format!("load {}", p.display())).unwrap();
    let (w, h) = shape_bbox(&it2, "plate");
    assert!(
        (w - 40.0).abs() < 1e-6 && (h - 80.0).abs() < 1e-6,
        "{w}×{h}"
    );
    assert_eq!(it2.vars["w"], 40.0, "load re-establishes the variable");
}

#[test]
fn variables_feed_skill_arguments() {
    // Skill args see variable expansion too.
    let mut it = Interpreter::new();
    it.exec_line("set t 12").unwrap();
    let msg = it.exec_line("skill gear teeth=$t name=g").unwrap();
    assert!(msg.to_lowercase().contains("gear"), "{msg}");
    assert!(it.scene.get("g").is_some(), "gear entity created");
}

#[test]
fn backward_compatible_without_dollar() {
    // No `$` anywhere → identical behaviour to pre-Phase-9: literals untouched,
    // filenames with no sigil pass through.
    let mut it = Interpreter::new();
    it.exec_line("rect 25 15 name=r").unwrap();
    let (w, h) = shape_bbox(&it, "r");
    assert!(
        (w - 25.0).abs() < 1e-6 && (h - 15.0).abs() < 1e-6,
        "{w}×{h}"
    );
}

#[test]
fn clear_error_messages() {
    let mut it = Interpreter::new();
    // Unknown variable surfaces a clear error.
    assert!(it.exec_line("rect $w 10").is_err());
    // Invalid variable name rejected.
    assert!(it.exec_line("set 2bad 10").is_err());
    // set with no value rejected.
    assert!(it.exec_line("set w").is_err());
}

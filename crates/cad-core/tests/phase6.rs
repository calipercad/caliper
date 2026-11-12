//! Phase 6 integration tests (roadmap v0.7): the AI copilot loop, driven fully
//! offline by a scripted `MockTransport` — no network, no API key.

use cad_core::ai::agent::{run_agent, AgentMode};
use cad_core::ai::MockTransport;
use cad_core::scene::Geometry;
use cad_core::Interpreter;

/// CHECKPOINT: a first attempt that errors is repaired on the second turn, and
/// the error is fed back to the model.
#[test]
fn converges_after_one_repair() {
    let tx = MockTransport::new(vec![
        "widget 5".to_string(), // unknown command → error
        "circle 8 name=c\nextrude 5 target=c name=part\nDONE".to_string(),
    ]);
    let mut it = Interpreter::new();
    let r = run_agent(&tx, &mut it, "an extruded disc", AgentMode::Create, 4).unwrap();

    assert!(r.converged, "copilot converged after the repair");
    assert_eq!(r.steps, 2, "one failed step + one good step");
    assert!(
        matches!(it.scene.get("part").unwrap().geom, Geometry::Mesh(_)),
        "the repaired script built the part"
    );
    // The second model call must have been told about the first attempt's error.
    let saw_error = tx
        .messages_on(1)
        .iter()
        .any(|m| m.content.contains("widget 5"));
    assert!(saw_error, "the error was fed back to the model");
}

/// A one-shot valid reply with DONE converges in a single call.
#[test]
fn one_shot_done() {
    let tx = MockTransport::new(vec!["circle 8 name=c\nDONE".to_string()]);
    let mut it = Interpreter::new();
    let r = run_agent(&tx, &mut it, "a circle", AgentMode::Create, 4).unwrap();
    assert!(r.converged);
    assert_eq!(r.steps, 1);
    assert_eq!(tx.calls(), 1);
    assert!(it.scene.get("c").is_some());
}

/// The loop never runs away: with no DONE it stops at exactly the budget.
#[test]
fn stops_at_budget() {
    let tx = MockTransport::new(vec![
        "circle 1 name=c".to_string(),
        "circle 2 name=c".to_string(),
        "circle 3 name=c".to_string(),
    ]);
    let mut it = Interpreter::new();
    let r = run_agent(&tx, &mut it, "never done", AgentMode::Create, 3).unwrap();
    assert!(!r.converged);
    assert_eq!(r.steps, 3);
    assert_eq!(tx.calls(), 3, "stopped at the budget, no runaway");
    // An unconverged run leaves repair context for `ai fix`.
    assert!(it.last_ai.is_some());
}

/// A step whose later line errors is rolled back atomically — earlier lines in
/// that step do not persist.
#[test]
fn failed_turn_rolls_back() {
    let tx = MockTransport::new(vec![
        "circle 8 name=good\nbadcmd 1".to_string(), // line 1 ok, line 2 errors
        "DONE".to_string(),
    ]);
    let mut it = Interpreter::new();
    let r = run_agent(&tx, &mut it, "atomic", AgentMode::Create, 4).unwrap();
    assert!(r.converged, "second reply was DONE");
    assert!(
        it.scene.get("good").is_none(),
        "the rolled-back step left no 'good' entity"
    );
    assert!(it.history.is_empty(), "history rolled back too");
}

/// The model's prose (explanation sentences) is ignored, not executed as
/// commands — so a reply mixing narration + commands + a trailing "DONE."
/// converges in one step without spurious "unknown command" errors.
#[test]
fn prose_lines_are_ignored() {
    let tx = MockTransport::new(vec!["Here's a sensible starter project.\n\
         circle 8 name=c\n\
         extrude 5 target=c name=part\n\
         The project is a simple extruded disc.\n\
         DONE."
        .to_string()]);
    let mut it = Interpreter::new();
    let r = run_agent(&tx, &mut it, "a disc", AgentMode::Create, 4).unwrap();
    assert!(r.converged, "converged despite the prose");
    assert_eq!(r.steps, 1, "no wasted repair step for prose");
    assert!(
        matches!(it.scene.get("part").unwrap().geom, Geometry::Mesh(_)),
        "the real commands still ran"
    );
    assert!(
        !r.transcript.contains("unknown command"),
        "prose did not surface as an error:\n{}",
        r.transcript
    );
    // Quieter transcript: the prose sentence is not echoed, but the commands
    // that ran are listed.
    assert!(
        !r.transcript.contains("sensible starter"),
        "prose sentence is not echoed:\n{}",
        r.transcript
    );
    assert!(
        r.transcript.contains("circle 8 name=c"),
        "ran commands are listed:\n{}",
        r.transcript
    );
}

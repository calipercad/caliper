//! The copilot loop: generate commands → run them → inspect the result → feed
//! back errors/state → refine, until the model says `DONE` or a step budget is
//! hit. Driven by any [`Transport`], so it runs fully offline in tests via
//! [`super::MockTransport`].

use super::{Msg, Transport};
use crate::error::{CadError, Result};
use crate::script::Interpreter;

/// How the copilot should treat the request.
pub enum AgentMode {
    /// Build a new design from a description.
    Create,
    /// Refine the current scene with a change.
    Edit,
    /// Repair the previous (failed) attempt.
    Fix,
}

/// Outcome of a copilot run.
pub struct AgentReport {
    pub transcript: String,
    pub steps: usize,
    pub converged: bool,
}

/// Default number of model turns a copilot run may take.
pub const DEFAULT_BUDGET: usize = 4;

/// Run the inspect→run→repair loop against `interp`, returning a transcript.
pub fn run_agent(
    tx: &dyn Transport,
    interp: &mut Interpreter,
    task: &str,
    mode: AgentMode,
    budget: usize,
) -> Result<AgentReport> {
    let system = build_system(interp);

    let seed = match mode {
        AgentMode::Create => format!("Request: {task}\nBuild it with Caliper commands."),
        AgentMode::Edit => format!(
            "The current scene is:\n{}\n\nChange requested: {task}\nApply it.",
            interp.exec_line("list").unwrap_or_default()
        ),
        AgentMode::Fix => match &interp.last_ai {
            Some((prev, err)) => format!(
                "The previous request was: {prev}\nIt ended with: {err}\n\
                 Diagnose and rebuild it correctly."
            ),
            None => return Err(CadError::Arg("nothing to fix (no prior AI run)".into())),
        },
    };

    let mut messages: Vec<Msg> = vec![Msg::user(seed)];
    let mut transcript = String::new();
    let mut converged = false;
    let mut steps = 0usize;
    let mut last_err: Option<String> = None;

    while steps < budget {
        let reply = tx.chat(&system, &messages)?;
        steps += 1;
        let (lines, done) = parse_reply(&reply);
        transcript.push_str(&format!("── step {steps} ──\n"));

        // Run the step atomically: snapshot, and on the first real error roll
        // the scene + history back so each turn is all-or-nothing. Lines that
        // aren't commands at all (the model's prose/explanation) are skipped as
        // narration rather than treated as errors — the interpreter decides what
        // is a command, so there's no brittle keyword list here.
        let scene_snap = interp.scene.clone();
        let hist_snap = interp.history.len();
        let mut step_err: Option<(String, String)> = None;
        let mut ran: Vec<String> = Vec::new();
        let mut skipped = 0usize;
        for line in &lines {
            match interp.exec_line(line) {
                Ok(_) => ran.push(line.clone()), // keep it concise: the command, not its echo
                Err(e) if is_prose(line, &e.to_string()) => {
                    skipped += 1; // narration, not a command
                }
                Err(e) => {
                    step_err = Some((line.clone(), e.to_string()));
                    break;
                }
            }
        }

        if let Some((line, e)) = step_err {
            interp.scene = scene_snap;
            interp.history.truncate(hist_snap);
            last_err = Some(e.clone());
            transcript.push_str(&format!("  ✗ {line}\n      {e}\n"));
            messages.push(Msg::assistant(reply));
            messages.push(Msg::user(format!(
                "Error running `{line}`: {e}\nFix it and resend the whole step."
            )));
            continue;
        }

        // Concise per-step summary: the commands that ran (prose suppressed).
        for cmd in &ran {
            transcript.push_str(&format!("  + {cmd}\n"));
        }
        if skipped > 0 {
            transcript.push_str(&format!("  (ignored {skipped} prose line(s))\n"));
        }
        if done {
            converged = true;
            break;
        }
        messages.push(Msg::assistant(reply));
        messages.push(Msg::user(format!(
            "{}\nContinue with more commands, or reply DONE if this matches the request.",
            observe(interp)
        )));
    }

    // Remember context for `ai fix`.
    interp.last_ai = if converged {
        None
    } else {
        Some((
            task.to_string(),
            last_err.unwrap_or_else(|| format!("did not converge within {budget} steps")),
        ))
    };

    Ok(AgentReport {
        transcript,
        steps,
        converged,
    })
}

/// Whether a failed line is the model's prose rather than a (mistyped) command.
/// It must be an unknown-command error AND its first token must not look like a
/// command keyword — a command is a lowercase identifier (`extrude`, `skill`),
/// whereas prose starts with a capital or contains punctuation (`The`,
/// `There's`, `I'll`). A lowercase typo like `widget` is kept as a real error so
/// the model still gets a chance to repair it.
fn is_prose(line: &str, err: &str) -> bool {
    if !err.contains("unknown command") {
        return false;
    }
    let first = line.split_whitespace().next().unwrap_or("");
    let command_shaped = !first.is_empty()
        && first.starts_with(|c: char| c.is_ascii_lowercase())
        && first
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    !command_shaped
}

/// Split a model reply into runnable command lines + whether it signalled DONE.
fn parse_reply(reply: &str) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut done = false;
    for l in reply.lines() {
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        // A line that is just "DONE" (tolerating trailing punctuation or a
        // leading marker like "- ") signals completion.
        let bare = t
            .trim_start_matches(['-', '*', '#', ' '])
            .trim_end_matches(['.', '!']);
        if bare.eq_ignore_ascii_case("done") {
            done = true;
            continue;
        }
        lines.push(t.to_string());
    }
    (lines, done)
}

/// Observation fed back to the model after a successful step.
fn observe(interp: &mut Interpreter) -> String {
    let list = interp.exec_line("list").unwrap_or_default();
    let render = interp.exec_line("render").unwrap_or_default();
    format!("{list}\n{render}")
}

/// System prompt: persona + loop protocol + skills digest + language reference.
fn build_system(interp: &Interpreter) -> String {
    let mut skills = String::new();
    for s in interp.skills.iter() {
        skills.push_str(&format!(
            "  {} — {}\n    {}\n",
            s.name(),
            s.summary(),
            s.usage()
        ));
    }
    format!(
        "You are an expert CAD designer driving `Caliper` in an iterative loop.\n\
         Each turn, reply with ONLY Caliper commands, ONE PER LINE. No prose, no \
         explanation, no markdown fences, no comments — any non-command line is ignored.\n\
         After you reply the commands run; you then receive the entity list and a render.\n\
         Emit more commands to fix or extend the design. When it fully matches the request, \
         reply with exactly one line containing only: DONE\n\
         Prefer parametric `skill` commands when one fits. Give entities clear names.\n\n\
         ==== SKILLS ====\n{skills}\n==== LANGUAGE REFERENCE ====\n{reference}",
        reference = crate::script::LANGUAGE_REFERENCE
    )
}

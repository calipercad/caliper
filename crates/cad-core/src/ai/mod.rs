//! AI bridge. Turns a natural-language request into cad-assist script lines by
//! calling the Claude Messages API. To stay dependency-free we shell out to
//! `curl` (POST) and `jq` (parse) — both ubiquitous on macOS/Linux.
//!
//! Set `ANTHROPIC_API_KEY` in the environment. The model can be overridden with
//! `CAD_AI_MODEL` (default: `claude-sonnet-5`).

use crate::error::{CadError, Result};
use crate::script;
use std::io::Write;
use std::process::{Command, Stdio};

const DEFAULT_MODEL: &str = "claude-sonnet-5";
const API_URL: &str = "https://api.anthropic.com/v1/messages";

/// Build the system prompt: who the assistant is + the full language reference.
fn system_prompt() -> String {
    format!(
        "You are an expert CAD designer driving `cad-assist`, a terminal CAD tool.\n\
         Translate the user's request into a cad-assist script.\n\
         Output ONLY script lines — no prose, no markdown fences, no explanation.\n\
         Prefer parametric `skill` commands when one fits. Give entities clear names.\n\
         End with a `render` line so the user sees the result.\n\n\
         ==== LANGUAGE REFERENCE ====\n{}",
        script::LANGUAGE_REFERENCE
    )
}

/// Ask the model to generate a script for `request`. Returns raw script text.
pub fn generate(request: &str) -> Result<String> {
    let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        CadError::Ai(
            "ANTHROPIC_API_KEY is not set. Export your key to enable AI generation:\n\
             \texport ANTHROPIC_API_KEY=sk-ant-..."
                .into(),
        )
    })?;
    let model = std::env::var("CAD_AI_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    // Build request JSON by hand (small + fully escaped).
    let body = format!(
        "{{\"model\":\"{model}\",\"max_tokens\":2000,\
         \"system\":{sys},\
         \"messages\":[{{\"role\":\"user\",\"content\":{usr}}}]}}",
        sys = json_string(&system_prompt()),
        usr = json_string(request),
    );

    let output = Command::new("curl")
        .args([
            "-sS",
            "-X",
            "POST",
            API_URL,
            "-H",
            "content-type: application/json",
            "-H",
            &format!("x-api-key: {key}"),
            "-H",
            "anthropic-version: 2023-06-01",
            "-d",
            "@-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(body.as_bytes())?;
            child.wait_with_output()
        })
        .map_err(|e| CadError::Ai(format!("failed to run curl: {e}")))?;

    if !output.status.success() {
        return Err(CadError::Ai(format!(
            "curl failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let response = String::from_utf8_lossy(&output.stdout).to_string();
    // Extract the text block (or a surfaced API error) with jq.
    let text = jq(&response, ".content[0].text // .error.message // empty")?;
    if text.trim().is_empty() {
        return Err(CadError::Ai(format!(
            "empty/unrecognized API response: {}",
            response.chars().take(300).collect::<String>()
        )));
    }
    Ok(strip_fences(&text))
}

/// Run `jq -r <filter>` over `input`.
fn jq(input: &str, filter: &str) -> Result<String> {
    let out = Command::new("jq")
        .args(["-r", filter])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(input.as_bytes())?;
            child.wait_with_output()
        })
        .map_err(|e| CadError::Ai(format!("failed to run jq: {e}")))?;
    if !out.status.success() {
        return Err(CadError::Ai(format!(
            "jq failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Remove ```-fenced code blocks the model may add despite instructions.
fn strip_fences(s: &str) -> String {
    let mut lines: Vec<&str> = s.lines().collect();
    if lines.first().map(|l| l.trim_start().starts_with("```")) == Some(true) {
        lines.remove(0);
    }
    if lines.last().map(|l| l.trim().starts_with("```")) == Some(true) {
        lines.pop();
    }
    lines.join("\n")
}

/// Encode a Rust string as a JSON string literal (including surrounding quotes).
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

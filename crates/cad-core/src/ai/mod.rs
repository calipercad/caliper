// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Caliper CAD

//! AI bridge. Turns natural language into Caliper script via the Claude
//! Messages API. To stay dependency-free we shell out to `curl` (POST) and
//! `jq` (parse) — both ubiquitous on macOS/Linux — behind a [`Transport`] trait
//! so the copilot loop ([`agent`]) can be driven offline by a [`MockTransport`]
//! in tests.
//!
//! Set `ANTHROPIC_API_KEY` in the environment. The model can be overridden with
//! `CAD_AI_MODEL` (default: `claude-sonnet-5-5`), and the API host with
//! `ANTHROPIC_BASE_URL` (optional; default: `https://api.anthropic.com`).

pub mod agent;

use crate::error::{CadError, Result};
use crate::script;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Write;
use std::process::{Command, Stdio};

const DEFAULT_MODEL: &str = "claude-sonnet-5-5";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// The Messages API endpoint. The host defaults to [`DEFAULT_BASE_URL`] but can
/// be overridden with `ANTHROPIC_BASE_URL` (a trailing `/` is trimmed so both
/// `https://host` and `https://host/` work).
fn api_url() -> String {
    let base = match std::env::var("ANTHROPIC_BASE_URL") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => DEFAULT_BASE_URL.to_string(),
    };
    format!("{}/v1/messages", base.trim_end_matches('/'))
}

/// One message in a chat conversation.
#[derive(Clone, Debug)]
pub struct Msg {
    pub role: &'static str, // "user" | "assistant"
    pub content: String,
}

impl Msg {
    pub fn user(content: impl Into<String>) -> Msg {
        Msg {
            role: "user",
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Msg {
        Msg {
            role: "assistant",
            content: content.into(),
        }
    }
}

/// A chat backend: send a system prompt + conversation, get the assistant's
/// reply text. The real one hits the API; tests use [`MockTransport`].
pub trait Transport {
    fn chat(&self, system: &str, messages: &[Msg]) -> Result<String>;
}

/// The real transport: `curl` to the Anthropic Messages API, `jq` to parse.
pub struct CurlTransport;

impl Transport for CurlTransport {
    fn chat(&self, system: &str, messages: &[Msg]) -> Result<String> {
        let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            CadError::Ai(
                "ANTHROPIC_API_KEY is not set. Export your key to enable AI:\n\
                 \texport ANTHROPIC_API_KEY=sk-ant-..."
                    .into(),
            )
        })?;
        let model = std::env::var("CAD_AI_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let url = api_url();

        let msgs: Vec<String> = messages
            .iter()
            .map(|m| {
                format!(
                    "{{\"role\":\"{}\",\"content\":{}}}",
                    m.role,
                    json_string(&m.content)
                )
            })
            .collect();
        let body = format!(
            "{{\"model\":\"{model}\",\"max_tokens\":2000,\"system\":{sys},\"messages\":[{msgs}]}}",
            sys = json_string(system),
            msgs = msgs.join(","),
        );

        let output = Command::new("curl")
            .args([
                "-sS",
                "-X",
                "POST",
                &url,
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

        // Success: the assistant text. The `?` after each field suppresses jq's
        // "cannot index" errors, so an unexpected response shape (e.g. a proxy
        // that returns `{"error":"..."}` where `.error` is a *string*) never
        // crashes jq — we fall through to error handling instead.
        if let Ok(text) = jq(&response, ".content[0].text? // empty") {
            if !text.trim().is_empty() {
                return Ok(strip_fences(&text));
            }
        }

        // Otherwise surface an API/proxy error message if the response carries
        // one, across the common shapes: Anthropic (`.error.message`), a bare
        // `{"error":"..."}` string, or a top-level `.message`.
        if let Ok(err) = jq(
            &response,
            ".error.message? // (.error | strings) // .message? // empty",
        ) {
            if !err.trim().is_empty() {
                return Err(CadError::Ai(format!("API error: {}", err.trim())));
            }
        }

        // Non-JSON or an unrecognized shape: show the raw response so the user
        // can see what the host/proxy actually returned.
        Err(CadError::Ai(format!(
            "unrecognized API response (check ANTHROPIC_BASE_URL and the model): {}",
            response.chars().take(400).collect::<String>().trim()
        )))
    }
}

/// A scripted, offline transport for tests: returns canned replies in order and
/// records the `(system, messages)` of every call for assertions.
pub struct MockTransport {
    responses: RefCell<VecDeque<String>>,
    seen: RefCell<Vec<(String, Vec<Msg>)>>,
}

impl MockTransport {
    pub fn new(responses: Vec<String>) -> MockTransport {
        MockTransport {
            responses: RefCell::new(responses.into_iter().collect()),
            seen: RefCell::new(Vec::new()),
        }
    }
    /// Number of `chat` calls made so far.
    pub fn calls(&self) -> usize {
        self.seen.borrow().len()
    }
    /// The last user-message content sent on call `i` (0-based), if any.
    pub fn messages_on(&self, call: usize) -> Vec<Msg> {
        self.seen
            .borrow()
            .get(call)
            .map(|(_, m)| m.clone())
            .unwrap_or_default()
    }
}

impl Transport for MockTransport {
    fn chat(&self, system: &str, messages: &[Msg]) -> Result<String> {
        self.seen
            .borrow_mut()
            .push((system.to_string(), messages.to_vec()));
        self.responses
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| CadError::Ai("mock transport exhausted".into()))
    }
}

/// One-shot convenience: generate a script for `request` in a single call
/// (no inspect/repair loop). Kept for callers that want the fast path.
pub fn generate(request: &str) -> Result<String> {
    CurlTransport.chat(&one_shot_system_prompt(), &[Msg::user(request)])
}

/// System prompt for the legacy one-shot [`generate`].
fn one_shot_system_prompt() -> String {
    format!(
        "You are an expert CAD designer driving `Caliper`, a terminal CAD tool.\n\
         Translate the user's request into a Caliper script.\n\
         Output ONLY script lines — no prose, no markdown fences, no explanation.\n\
         Prefer parametric `skill` commands when one fits. Give entities clear names.\n\
         End with a `render` line so the user sees the result.\n\n\
         ==== LANGUAGE REFERENCE ====\n{}",
        script::LANGUAGE_REFERENCE
    )
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
pub(crate) fn strip_fences(s: &str) -> String {
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

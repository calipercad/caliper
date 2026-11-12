//! `cad tui` — a full-screen, live-preview interactive mode.
//!
//! Zero crates: raw mode is toggled by shelling `stty` (an external program at
//! the edge, like `curl`/`jq`), and the screen is drawn with ANSI escapes. A
//! [`RawGuard`] restores the terminal (saved `stty` settings, cursor, main
//! screen) on `Drop`, so even a panic leaves a usable terminal.
//!
//! The render pane updates live: arrow keys orbit the 3D view, and each command
//! you type runs and re-renders. The pure pieces — [`decode_key`] and
//! [`compose_frame`] — are unit-tested; the raw-IO loop just drives them.

use cad_core::script::Interpreter;
use std::io::{self, Read, Write};
use std::process::{Command, ExitCode, Stdio};

/// Run `stty <args>` capturing stdout, with the **terminal inherited on stdin**
/// (so `stty` can query/set it). `Command::output()` would otherwise give the
/// child a closed stdin, making `stty -g`/`size` fail even on a real TTY.
fn stty_out(args: &[&str]) -> Option<String> {
    let out = Command::new("stty")
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run `stty <args>` for its effect (raw mode, restore), terminal on stdin.
fn stty_do(args: &[&str]) {
    let _ = Command::new("stty")
        .args(args)
        .stdin(Stdio::inherit())
        .status();
}

/// A decoded keypress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Quit,
    Ignore,
}

/// Decode one keypress from `first` (already read), pulling more bytes from
/// `rest` for escape sequences (arrow keys). Pure — unit-tested.
pub fn decode_key(first: u8, rest: &mut dyn Iterator<Item = u8>) -> Key {
    match first {
        0x03 | 0x04 => Key::Quit, // Ctrl-C / Ctrl-D
        b'\r' | b'\n' => Key::Enter,
        0x7f | 0x08 => Key::Backspace,
        0x1b => match rest.next() {
            Some(b'[') => match rest.next() {
                Some(b'A') => Key::Up,
                Some(b'B') => Key::Down,
                Some(b'C') => Key::Right,
                Some(b'D') => Key::Left,
                _ => Key::Ignore,
            },
            _ => Key::Ignore, // bare ESC
        },
        b if b == b' ' || b.is_ascii_graphic() => Key::Char(b as char),
        _ => Key::Ignore,
    }
}

/// Compose a full `rows`×`cols` frame: the render pane, a status line, and the
/// input line — each clipped to `cols` and cleared to end-of-line so the screen
/// is fully overwritten. Pure — unit-tested. (The render is plain braille/HLR
/// text with no ANSI, so char-clipping is safe.)
pub fn compose_frame(render: &str, status: &str, input: &str, rows: usize, cols: usize) -> String {
    let body = rows.saturating_sub(2).max(1);
    let rlines: Vec<&str> = render.lines().collect();
    let mut out = String::new();
    for i in 0..body {
        out.push_str(&clip(rlines.get(i).copied().unwrap_or(""), cols));
        out.push_str("\x1b[K\r\n");
    }
    out.push_str(&clip(status, cols));
    out.push_str("\x1b[K\r\n");
    out.push_str(&clip(&format!("cad> {input}"), cols));
    // Clear each line to EOL (above) and everything below the frame (here), so
    // stale content — e.g. a prior render showing through a terminal without
    // alt-screen support — can never linger.
    out.push_str("\x1b[K\x1b[J");
    out
}

fn clip(line: &str, cols: usize) -> String {
    line.chars().take(cols).collect()
}

fn first_line(s: &str) -> String {
    s.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .to_string()
}

/// Restores the terminal on drop: saved `stty` settings, cursor, main screen.
struct RawGuard {
    saved: String,
}
impl Drop for RawGuard {
    fn drop(&mut self) {
        stty_do(&[&self.saved]);
        print!("\x1b[?25h\x1b[?1049l"); // show cursor, leave alt screen
        let _ = io::stdout().flush();
    }
}

/// Current terminal size `(rows, cols)` via `stty size`, defaulting to 24×80.
fn term_size() -> (usize, usize) {
    if let Some(s) = stty_out(&["size"]) {
        let mut it = s.split_whitespace();
        if let (Some(r), Some(c)) = (it.next(), it.next()) {
            if let (Ok(r), Ok(c)) = (r.parse::<usize>(), c.parse::<usize>()) {
                if r > 0 && c > 0 {
                    return (r, c);
                }
            }
        }
    }
    (24, 80)
}

/// Launch the live-preview TUI. Falls back to the REPL when stdin isn't a TTY.
pub fn run() -> ExitCode {
    // Save terminal settings; a failure means we're not on a TTY.
    let saved = match stty_out(&["-g"]) {
        Some(s) if !s.is_empty() => s,
        _ => {
            eprintln!("tui needs an interactive terminal — falling back to the REPL.");
            return crate::repl::run();
        }
    };
    let _guard = RawGuard { saved };
    stty_do(&["raw", "-echo"]);
    print!("\x1b[?1049h\x1b[?25l\x1b[2J"); // alt screen, hide cursor, clear
    let _ = io::stdout().flush();

    let mut it = Interpreter::new();
    it.load_config();
    let (mut yaw, mut pitch) = (35.0f64, 25.0f64);
    let mut input = String::new();
    let mut status = String::from("arrows: orbit · type a command + Enter · q to quit");

    let mut raw = io::BufReader::new(io::stdin())
        .bytes()
        .map(|r| r.unwrap_or(0));
    loop {
        let (rows, cols) = term_size();
        let body = rows.saturating_sub(2).max(1);
        let render = it
            .exec_line(&format!(
                "render w={} h={} mode=hlr yaw={:.0} pitch={:.0}",
                cols.min(240),
                body,
                yaw,
                pitch
            ))
            .unwrap_or_default();
        print!(
            "\x1b[H{}",
            compose_frame(&render, &status, &input, rows, cols)
        );
        let _ = io::stdout().flush();

        let b = match raw.next() {
            Some(b) => b,
            None => break,
        };
        match decode_key(b, &mut raw) {
            Key::Quit => break,
            Key::Enter => {
                let line = input.trim().to_string();
                input.clear();
                if matches!(line.as_str(), "quit" | "exit" | ":q") {
                    break;
                }
                if !line.is_empty() {
                    status = match it.exec_line(&line) {
                        Ok(out) if first_line(&out).is_empty() => format!("ran: {line}"),
                        Ok(out) => first_line(&out),
                        Err(e) => format!("! {e}"),
                    };
                }
            }
            Key::Backspace => {
                input.pop();
            }
            Key::Char(c) => input.push(c),
            Key::Up => pitch = (pitch + 5.0).min(89.0),
            Key::Down => pitch = (pitch - 5.0).max(-89.0),
            Key::Left => yaw -= 5.0,
            Key::Right => yaw += 5.0,
            Key::Ignore => {}
        }
    }
    ExitCode::SUCCESS // RawGuard restores the terminal on drop
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> std::iter::Empty<u8> {
        std::iter::empty()
    }

    #[test]
    fn decodes_basic_keys() {
        assert_eq!(decode_key(b'a', &mut none()), Key::Char('a'));
        assert_eq!(decode_key(b' ', &mut none()), Key::Char(' '));
        assert_eq!(decode_key(b'\r', &mut none()), Key::Enter);
        assert_eq!(decode_key(0x7f, &mut none()), Key::Backspace);
        assert_eq!(decode_key(0x03, &mut none()), Key::Quit);
    }

    #[test]
    fn decodes_arrow_escape() {
        let mut rest = vec![b'[', b'A'].into_iter();
        assert_eq!(decode_key(0x1b, &mut rest), Key::Up);
        let mut rest = vec![b'[', b'C'].into_iter();
        assert_eq!(decode_key(0x1b, &mut rest), Key::Right);
        // Bare ESC with nothing following is ignored, not a crash.
        assert_eq!(decode_key(0x1b, &mut none()), Key::Ignore);
    }

    #[test]
    fn frame_has_exact_row_count_and_clips() {
        let render = "AAAAAAAAAA\nBBBB\nCCCC"; // 3 lines
        let f = compose_frame(render, "status", "box 40", 6, 5);
        // 6 rows total: 4 body + status + input.
        let lines: Vec<&str> = f.split("\r\n").collect();
        assert_eq!(lines.len(), 6, "rows: {}", lines.len());
        // First body line clipped to 5 cols (before the \x1b[K).
        assert!(lines[0].starts_with("AAAAA\x1b[K"), "clip: {:?}", lines[0]);
        // Input line shows the prompt (clipped here to 5 cols = "cad> ").
        assert!(lines[5].starts_with("cad> "), "input line: {:?}", lines[5]);
        // With room, the typed text is visible.
        let wide = compose_frame(render, "status", "box 40", 6, 20);
        let wlines: Vec<&str> = wide.split("\r\n").collect();
        assert!(
            wlines[5].starts_with("cad> box 40"),
            "input: {:?}",
            wlines[5]
        );
    }
}

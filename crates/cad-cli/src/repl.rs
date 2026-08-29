//! Interactive shell. Reads one command per line and prints the result.

use cad_core::script::Interpreter;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

const BANNER: &str = "\
┌─────────────────────────────────────────────────────────────┐
│  cad-assist — terminal CAD with AI + parametric skills        │
│  type `help` for the language, `skills` to list generators    │
│  `ai <describe it>` to design with Claude · `quit` to exit     │
└─────────────────────────────────────────────────────────────┘";

pub fn run() -> ExitCode {
    println!("{BANNER}");
    let mut it = Interpreter::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        // Prompt shows the current selection for context.
        let sel = it
            .scene
            .selected
            .as_deref()
            .map(|s| format!("({s})"))
            .unwrap_or_default();
        print!("cad{sel}> ");
        let _ = stdout.flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                println!();
                break; // EOF (Ctrl-D)
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("input error: {e}");
                break;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if matches!(trimmed, "quit" | "exit" | ":q") {
            break;
        }

        match it.exec_line(trimmed) {
            Ok(out) => {
                if !out.is_empty() {
                    print!("{out}");
                    if !out.ends_with('\n') {
                        println!();
                    }
                }
            }
            Err(e) => eprintln!("! {e}"),
        }
    }

    ExitCode::SUCCESS
}

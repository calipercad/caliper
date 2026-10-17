//! `cad` — the cad-assist command-line front end.
//!
//! Usage:
//!   cad                     start the interactive shell (REPL)
//!   cad run <file.cad>      execute a script and print its output
//!   cad <file.cad>          shorthand for `run`
//!   cad watch <file.cad>    re-run + render whenever the file is saved
//!   cad ai <describe it>    generate a design with Claude and print/run it
//!   cad new <name>          scaffold a starter script
//!   cad plugin list|new|add|remove   manage ~/.cad-assist/skills plugins
//!   cad skills              list available parametric skills
//!   cad help                print the language reference
//!   cad version

mod repl;

use cad_core::script::{Interpreter, LANGUAGE_REFERENCE};
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        return repl::run();
    }

    match args[0].as_str() {
        "help" | "-h" | "--help" => {
            print!("{LANGUAGE_REFERENCE}");
            ExitCode::SUCCESS
        }
        "version" | "-V" | "--version" => {
            println!("cad-assist {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "skills" => {
            let mut it = Interpreter::new();
            print!("{}", it.exec_line("skills").unwrap_or_default());
            ExitCode::SUCCESS
        }
        "run" => match args.get(1) {
            Some(path) => run_file(path),
            None => {
                eprintln!("usage: cad run <file.cad>");
                ExitCode::FAILURE
            }
        },
        "watch" => match args.get(1) {
            Some(path) => watch_file(path),
            None => {
                eprintln!("usage: cad watch <file.cad>");
                ExitCode::FAILURE
            }
        },
        "ai" => {
            let prompt = args[1..].join(" ");
            if prompt.trim().is_empty() {
                eprintln!("usage: cad ai <describe what to make>");
                return ExitCode::FAILURE;
            }
            let mut it = Interpreter::new();
            match it.exec_line(&format!("ai {prompt}")) {
                Ok(out) => {
                    print!("{out}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        "new" => {
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("design");
            scaffold(name)
        }
        "plugin" => plugin(&args),
        // shorthand: `cad foo.cad`
        path if path.ends_with(".cad") => run_file(path),
        other => {
            eprintln!("unknown command '{other}'. Try `cad help` or just `cad`.");
            ExitCode::FAILURE
        }
    }
}

fn run_file(path: &str) -> ExitCode {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut it = Interpreter::new();
    it.load_config();
    match it.exec_script(&text) {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Re-run a script and print its output whenever the file changes on disk.
fn watch_file(path: &str) -> ExitCode {
    use std::io::Write;
    use std::time::{Duration, SystemTime};

    eprintln!("watching {path} (Ctrl-C to stop)…");
    let mut last: Option<SystemTime> = None;
    loop {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if modified != last {
            last = modified;
            print!("\x1b[2J\x1b[H"); // clear screen, home cursor
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    let mut it = Interpreter::new();
                    it.load_config();
                    match it.exec_script(&text) {
                        Ok(out) => print!("{out}"),
                        Err(e) => println!("error: {e}"),
                    }
                }
                Err(e) => println!("cannot read {path}: {e}"),
            }
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// `cad plugin list|new|add|remove` — manage script-skills (`.cad` plugins) in
/// `~/.cad-assist/skills/`. A script-skill is runnable as `skill <name>` and
/// shows up in `skills` with no recompile.
fn plugin(args: &[String]) -> ExitCode {
    let dir = match Interpreter::new().skill_dir() {
        Some(d) => d.to_path_buf(),
        None => {
            eprintln!("no skills directory (is HOME set?)");
            return ExitCode::FAILURE;
        }
    };
    match args.get(1).map(|s| s.as_str()) {
        Some("list") | None => {
            // Built-ins + the user's script-skills (the interpreter lists both).
            let mut it = Interpreter::new();
            print!("{}", it.exec_line("skills").unwrap_or_default());
            ExitCode::SUCCESS
        }
        Some("new") => {
            let Some(name) = args.get(2) else {
                eprintln!("usage: cad plugin new <name>");
                return ExitCode::FAILURE;
            };
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("cannot create {}: {e}", dir.display());
                return ExitCode::FAILURE;
            }
            let path = dir.join(format!("{name}.cad"));
            if path.exists() {
                eprintln!("{} already exists", path.display());
                return ExitCode::FAILURE;
            }
            let body = format!(
                "# {name} — a cad-assist script-skill (plugin)\n\
                 # invoke:  skill {name} size=40\n\
                 # ${{key:default}} placeholders are filled from `key=value` args.\n\
                 rect ${{size:40}} ${{size:40}} rx=4 name={name}\n\
                 render\n"
            );
            match std::fs::write(&path, body) {
                Ok(_) => {
                    println!("created {}\n  run it with:  skill {name}", path.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cannot write {}: {e}", path.display());
                    ExitCode::FAILURE
                }
            }
        }
        Some("add") => {
            let Some(src) = args.get(2) else {
                eprintln!("usage: cad plugin add <file.cad>");
                return ExitCode::FAILURE;
            };
            let stem = Path::new(src)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("plugin");
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("cannot create {}: {e}", dir.display());
                return ExitCode::FAILURE;
            }
            let dest = dir.join(format!("{stem}.cad"));
            match std::fs::copy(src, &dest) {
                Ok(_) => {
                    println!(
                        "installed '{stem}' → {}\n  run it with:  skill {stem}",
                        dest.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cannot copy {src}: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("remove") => {
            let Some(name) = args.get(2) else {
                eprintln!("usage: cad plugin remove <name>");
                return ExitCode::FAILURE;
            };
            let path = dir.join(format!("{name}.cad"));
            if !path.exists() {
                eprintln!("no plugin '{name}' in {}", dir.display());
                return ExitCode::FAILURE;
            }
            match std::fs::remove_file(&path) {
                Ok(_) => {
                    println!("removed plugin '{name}'");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("cannot remove {}: {e}", path.display());
                    ExitCode::FAILURE
                }
            }
        }
        Some(other) => {
            eprintln!("unknown: cad plugin {other} (use list|new|add|remove)");
            ExitCode::FAILURE
        }
    }
}

fn scaffold(name: &str) -> ExitCode {
    let path = format!("{name}.cad");
    if std::path::Path::new(&path).exists() {
        eprintln!("{path} already exists");
        return ExitCode::FAILURE;
    }
    let body = format!(
        "# {name}.cad — cad-assist design\n\
         units mm\n\n\
         # a parametric gear, ready to slice\n\
         skill gear teeth=24 module=2 bore=5 thickness=6 name=gear\n\n\
         render\n\
         # export stl {name}.stl name=gear\n"
    );
    match std::fs::write(&path, body) {
        Ok(_) => {
            println!("created {path}\n  run it with:  cad run {path}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cannot write {path}: {e}");
            ExitCode::FAILURE
        }
    }
}

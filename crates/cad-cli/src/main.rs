//! `cad` — the Caliper command-line front end.
//!
//! Usage:
//!   cad                     start the interactive shell (REPL)
//!   cad run <file.cad>      execute a script and print its output
//!   cad <file.cad>          shorthand for `run`
//!   cad watch <file.cad>    re-run + render whenever the file is saved
//!   cad ai <describe it>    generate a design with Claude and print/run it
//!   cad new <name> [kind]   scaffold a starter script (kind: gear|box|enclosure|sketch|washer|blank)
//!   cad tui                 live-preview interactive mode
//!   cad tutorial            a 60-second guided tour
//!   cad plugin list|new|add|remove   manage ~/.caliper/skills plugins
//!   cad skills              list available parametric skills
//!   cad help                print the language reference
//!   cad version

mod repl;
mod tui;

use cad_core::script::{Interpreter, LANGUAGE_REFERENCE};
use std::path::Path;
use std::process::ExitCode;

/// Subcommands, for "did you mean" on a typo'd verb.
const SUBCOMMANDS: &[&str] = &[
    "run", "watch", "ai", "new", "tui", "tutorial", "plugin", "skills", "help", "version",
];

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
            println!("Caliper {}", env!("CARGO_PKG_VERSION"));
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
            if args.get(1).map(|s| s.as_str()) == Some("--list") {
                print!("{}", template_catalog());
                return ExitCode::SUCCESS;
            }
            let name = args.get(1).map(|s| s.as_str()).unwrap_or("design");
            let kind = args.get(2).map(|s| s.as_str()).unwrap_or("gear");
            scaffold(name, kind)
        }
        "tui" => tui::run(),
        "tutorial" => {
            print!(
                "{}",
                Interpreter::new().exec_line("tutorial").unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        "plugin" => plugin(&args),
        // shorthand: `cad foo.cad`
        path if path.ends_with(".cad") => run_file(path),
        other => {
            let hint = nearest(other, SUBCOMMANDS)
                .map(|s| format!(" (did you mean `cad {s}`?)"))
                .unwrap_or_default();
            eprintln!("unknown command '{other}'{hint}. Try `cad help` or just `cad`.");
            ExitCode::FAILURE
        }
    }
}

/// Nearest word in `choices` within a small edit budget (CLI subcommand hints).
fn nearest(input: &str, choices: &[&'static str]) -> Option<&'static str> {
    let budget = if input.len() <= 4 { 2 } else { 3 };
    choices
        .iter()
        .map(|&c| (c, edit_distance(input, c)))
        .filter(|&(_, d)| d <= budget)
        .min_by_key(|&(_, d)| d)
        .map(|(c, _)| c)
}

/// Levenshtein edit distance (two-row DP).
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
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
/// `~/.caliper/skills/`. A script-skill is runnable as `skill <name>` and
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
                "# {name} — a Caliper script-skill (plugin)\n\
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

/// A starter-script body for a template `kind`, or `None` for an unknown kind.
fn template_body(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "gear" => "units mm\n\n\
                   # a parametric gear, ready to slice\n\
                   skill gear teeth=24 module=2 bore=5 thickness=6 name=gear\n\n\
                   render\n\
                   # export stl design.stl name=gear\n",
        "box" => "units mm\n\n\
                  # a bored block (3D CSG)\n\
                  box 40 40 20 name=blk\n\
                  cylinder 8 30 name=bore\n\
                  translate 20 20 -5 target=bore\n\
                  subtract blk bore name=part\n\n\
                  render mode=hlr\n\
                  # export stl design.stl name=part\n",
        "enclosure" => "units mm\n\n\
                        # a two-part snap enclosure with standoffs\n\
                        skill enclosure width=80 depth=60 height=30 wall=2 lid standoffs name=box\n\n\
                        render view=iso\n",
        "sketch" => "units mm\n\n\
                     # a constraint-solved rectangle: edit the lengths, re-solve\n\
                     sketch plate\n\
                     point bl 0 0\n\
                     point br 70 4\n\
                     point tr 76 45\n\
                     point tl 3 48\n\
                     segment bottom bl br\n\
                     segment right br tr\n\
                     segment top tr tl\n\
                     segment left tl bl\n\
                     constrain fix bl\n\
                     constrain horizontal bottom\n\
                     constrain horizontal top\n\
                     constrain vertical left\n\
                     constrain vertical right\n\
                     constrain length bottom 80\n\
                     constrain length left 50\n\
                     solve\n\
                     sketch close name=plate\n\n\
                     render mode=2d\n",
        "washer" => "units mm\n\n\
                     # a washer from two concentric constrained circles\n\
                     sketch washer\n\
                     point c 0 0\n\
                     circle outer c 15\n\
                     circle bore c 5\n\
                     constrain fix c\n\
                     constrain concentric outer bore\n\
                     constrain radius outer 20\n\
                     constrain radius bore 8\n\
                     solve\n\
                     sketch close name=washer\n\n\
                     render mode=2d\n",
        "blank" => "units mm\n\n\
                    # start here\n\
                    rect 40 40 name=part\n\
                    render mode=2d\n",
        _ => return None,
    })
}

/// Human-readable list of the available `new` templates.
fn template_catalog() -> String {
    "cad new <name> [template]  — templates:\n\
     \x20 gear       a parametric involute gear (default)\n\
     \x20 box        a bored block (3D CSG)\n\
     \x20 enclosure  a two-part snap box with standoffs\n\
     \x20 sketch     a constraint-solved rectangle\n\
     \x20 washer     two concentric constrained circles\n\
     \x20 blank      a minimal starting point\n"
        .to_string()
}

fn scaffold(name: &str, kind: &str) -> ExitCode {
    let Some(tpl) = template_body(kind) else {
        eprintln!("unknown template '{kind}'.\n{}", template_catalog());
        return ExitCode::FAILURE;
    };
    let path = format!("{name}.cad");
    if std::path::Path::new(&path).exists() {
        eprintln!("{path} already exists");
        return ExitCode::FAILURE;
    }
    let body = format!("# {name}.cad — Caliper design ({kind} template)\n{tpl}");
    match std::fs::write(&path, body) {
        Ok(_) => {
            println!("created {path} ({kind})\n  run it with:  cad run {path}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("cannot write {path}: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_present_and_unknown_is_none() {
        for k in ["gear", "box", "enclosure", "sketch", "washer", "blank"] {
            let b = template_body(k).unwrap_or_else(|| panic!("missing template {k}"));
            assert!(b.contains("units mm"), "{k} template runnable");
        }
        assert!(template_body("nope").is_none());
    }

    #[test]
    fn subcommand_suggestions() {
        assert_eq!(nearest("runn", SUBCOMMANDS), Some("run"));
        assert_eq!(nearest("tutrial", SUBCOMMANDS), Some("tutorial"));
        assert_eq!(nearest("zzzzzz", SUBCOMMANDS), None);
    }
}

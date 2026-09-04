use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use stupid_comments::{analyze_file, hook, lang::Lang, policy, rules::Severity, scan};

#[derive(Parser)]
#[command(name = "stupid-comments", version, about = "Enforces your comment policy against LLM-generated code.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check files or directories against the resolved policy.
    Check {
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
        /// Permit deletion as a remedy; for the human-invoked sweep only.
        #[arg(long)]
        adjudicate: bool,
    },
    /// Handle a hook payload on stdin.
    Hook {
        #[arg(value_parser = ["claude"])]
        client: String,
    },
    /// Print the resolved policy and where it came from.
    Policy,
}

fn main() -> ExitCode {
    let command = Cli::parse().command;
    // The hook fails open — an unreadable config must never block a write.
    // Everything else says so, rather than passing for a clean run.
    let fail_open = matches!(command, Command::Hook { .. });

    match dispatch(command) {
        Ok(code) => code,
        Err(_) if fail_open => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("stupid-comments: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(command: Command) -> Result<ExitCode> {
    match command {
        Command::Check { paths, json, adjudicate } => check(paths, json, adjudicate),
        Command::Hook { .. } => run_hook(),
        Command::Policy => show_policy(),
    }
}

fn check(paths: Vec<PathBuf>, json: bool, adjudicate: bool) -> Result<ExitCode> {
    let root = paths.first().cloned().unwrap_or_else(|| PathBuf::from("."));
    let Some(policy) = policy::resolve(&root)? else {
        if json {
            println!("[]");
        } else {
            eprintln!("No comment policy found. Nothing to enforce.");
        }
        return Ok(ExitCode::SUCCESS);
    };

    let mut findings = Vec::new();
    let mut checked: Vec<PathBuf> = Vec::new();
    let mut skipped: Vec<PathBuf> = Vec::new();
    let mut excluded: Vec<PathBuf> = Vec::new();
    let mut missing: Vec<PathBuf> = Vec::new();

    for path in &paths {
        if !path.exists() {
            missing.push(path.clone());
            continue;
        }
        let scan = scan(path);
        for file in &scan.files {
            findings.extend(analyze_file(file, &policy, adjudicate));
        }
        // A file the config excludes was not checked either; folding it into
        // the checked count is the same lie in a friendlier costume.
        let (dropped, seen): (Vec<PathBuf>, Vec<PathBuf>) = scan
            .files
            .into_iter()
            .partition(|f| stupid_comments::excluded(f, &policy));
        checked.extend(seen);
        excluded.extend(dropped);
        skipped.extend(scan.skipped);
    }

    // Coverage goes to stderr so --json consumers keep a clean stdout.
    eprintln!("{}", coverage(&checked, &skipped, &excluded, &missing));

    if json {
        println!("{}", serde_json::to_string_pretty(&findings)?);
    } else if checked.is_empty() {
        println!("Nothing checked. Policy source: {}", policy.source);
    } else if findings.is_empty() {
        println!("No violations. Policy source: {}", policy.source);
    } else {
        for f in &findings {
            println!(
                "{}:{}:{} [{}] {}\n  > {}",
                f.file,
                f.start_line,
                match f.severity {
                    Severity::Block => "block",
                    Severity::Warn => "warn",
                },
                f.rule,
                f.message,
                f.excerpt
            );
        }
    }

    let blocked = findings.iter().any(|f| f.severity == Severity::Block);
    Ok(match blocked || !missing.is_empty() {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    })
}

/// Names what was and was not parsed. A file with no grammar is not a passing
/// file, and reporting it as one is how whole directories go unchecked.
fn coverage(
    checked: &[PathBuf],
    skipped: &[PathBuf],
    excluded: &[PathBuf],
    missing: &[PathBuf],
) -> String {
    let mut s = format!(
        "Checked {} file{} ({}).",
        checked.len(),
        plural(checked.len()),
        tally(checked, |p| Lang::from_file(p).map(|l| l.name().to_string()))
    );
    if !skipped.is_empty() {
        s.push_str(&format!(
            "\nNot checked — no grammar for {} file{}: {}",
            skipped.len(),
            plural(skipped.len()),
            tally(skipped, |p| match p.extension().and_then(|e| e.to_str()) {
                Some(ext) => Some(format!(".{ext}")),
                None => p.file_name()?.to_str().map(str::to_string),
            })
        ));
    }
    if !excluded.is_empty() {
        s.push_str(&format!(
            "\nNot checked — excluded by config: {} file{}",
            excluded.len(),
            plural(excluded.len())
        ));
    }
    for path in missing {
        s.push_str(&format!("\nNo such path: {}", path.display()));
    }
    s
}

fn tally(paths: &[PathBuf], key: impl Fn(&PathBuf) -> Option<String>) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for path in paths {
        let Some(k) = key(path) else { continue };
        match counts.iter_mut().find(|(name, _)| *name == k) {
            Some((_, n)) => *n += 1,
            None => counts.push((k, 1)),
        }
    }
    if counts.is_empty() {
        return "none".into();
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let shown = counts.len().min(6);
    let mut out = counts[..shown]
        .iter()
        .map(|(name, n)| format!("{name} {n}"))
        .collect::<Vec<_>>()
        .join(", ");
    if counts.len() > shown {
        out.push_str(&format!(", +{} more", counts.len() - shown));
    }
    out
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn run_hook() -> Result<ExitCode> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    let outcome = hook::run(&input)?;
    if outcome.message.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("{}", outcome.message);
    Ok(match outcome.block {
        true => ExitCode::from(2),
        false => ExitCode::SUCCESS,
    })
}

fn show_policy() -> Result<ExitCode> {
    let cwd = std::env::current_dir()?;
    match policy::resolve(&cwd)? {
        None => println!("No comment policy found. stupid-comments is inert."),
        Some(p) => {
            println!("source: {}", p.source);
            println!("mode:   {:?}", p.rules.mode);
            println!("rules:  {:#?}\n", p.rules);
            println!("--- policy text ---\n{}", p.prose);
        }
    }
    Ok(ExitCode::SUCCESS)
}

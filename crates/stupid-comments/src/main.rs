use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use stupid_comments::{analyze_file, collect_files, hook, policy, rules::Severity};

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
    match dispatch() {
        Ok(code) => code,
        Err(_) => ExitCode::SUCCESS,
    }
}

fn dispatch() -> Result<ExitCode> {
    match Cli::parse().command {
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
    for path in &paths {
        for file in collect_files(path) {
            findings.extend(analyze_file(&file, &policy, adjudicate));
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&findings)?);
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

    Ok(match findings.iter().any(|f| f.severity == Severity::Block) {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    })
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

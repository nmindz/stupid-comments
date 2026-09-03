use crate::comments::{Extraction, Kind};
use crate::policy::{Mode, Policy};
use crate::rules::{Context, Finding, Severity};
use serde::Deserialize;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const MAX_COMMENTS: usize = 20;
const TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Deserialize)]
struct Verdict {
    index: usize,
    violation: bool,
    #[serde(default)]
    reason: String,
}

/// Opt-in taste judgement. Off unless configured, and every failure path is
/// silent — a judge that cannot answer must never block a write.
pub fn judge(ctx: &Context, extraction: &Extraction, policy: &Policy) -> Vec<Finding> {
    if ctx.rules.semantic == Mode::Shadow || policy.prose.trim().is_empty() {
        return Vec::new();
    }
    let prose: Vec<_> = extraction
        .comments
        .iter()
        .filter(|c| c.kind == Kind::Prose)
        .take(MAX_COMMENTS)
        .collect();

    if prose.is_empty() {
        return Vec::new();
    }

    let listing = prose
        .iter()
        .enumerate()
        .map(|(i, c)| format!("[{i}] line {}:\n{}", c.start_line, c.raw))
        .collect::<Vec<_>>()
        .join("\n\n");

    let prompt = format!(
        "You are judging code comments against a policy. Reply with JSON only.\n\n\
         POLICY:\n{}\n\nCOMMENTS:\n{listing}\n\n\
         For each comment decide whether it violates the policy. A comment that earns its place \
         by aiding visual scanning is not a violation. Reply with a JSON array of objects \
         {{\"index\": <number>, \"violation\": <bool>, \"reason\": <short string>}} and nothing else.",
        policy.prose
    );

    let Some(output) = invoke(&ctx.rules.semantic_command, &prompt) else {
        return Vec::new();
    };
    let Some(verdicts) = parse(&output) else {
        return Vec::new();
    };

    let severity = match ctx.rules.semantic {
        Mode::Block => Severity::Block,
        _ => Severity::Warn,
    };

    verdicts
        .into_iter()
        .filter(|v| v.violation)
        .filter_map(|v| prose.get(v.index).map(|c| (v, c)))
        .map(|(v, c)| Finding {
            file: ctx.file.clone(),
            rule: "semantic",
            severity,
            kind: c.kind,
            start_line: c.start_line,
            end_line: c.end_line,
            message: format!(
                "{} Rewrite it to earn its place. Deleting the comment is not compliance.",
                punctuate(&v.reason)
            ),
            excerpt: c.raw.lines().next().unwrap_or_default().trim().to_string(),
        })
        .collect()
}

fn punctuate(reason: &str) -> String {
    let trimmed = reason.trim();
    match trimmed.chars().last() {
        Some('.') | Some('!') | Some('?') => trimmed.to_string(),
        Some(_) => format!("{trimmed}."),
        None => "Fails the policy.".to_string(),
    }
}

fn invoke(command: &[String], prompt: &str) -> Option<String> {
    let (program, args) = command.split_first()?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    child.stdin.take()?.write_all(prompt.as_bytes()).ok()?;

    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result.ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()));
    });

    match rx.recv_timeout(TIMEOUT) {
        Ok(out) => {
            let _ = handle.join();
            out
        }
        Err(_) => None,
    }
}

fn parse(output: &str) -> Option<Vec<Verdict>> {
    let start = output.find('[')?;
    let end = output.rfind(']')?;
    serde_json::from_str(output.get(start..=end)?).ok()
}

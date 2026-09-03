use crate::policy::{self, Policy};
use crate::rules::{Finding, Severity};
use crate::session::Tracker;
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const DISARM_ENV: &str = "STUPID_COMMENTS";

pub struct Outcome {
    pub block: bool,
    pub message: String,
}

pub fn run(input: &str) -> Result<Outcome> {
    let quiet = Outcome { block: false, message: String::new() };

    if std::env::var(DISARM_ENV).map(|v| v == "0").unwrap_or(false) {
        return Ok(quiet);
    }
    let Ok(payload) = serde_json::from_str::<Value>(input) else {
        return Ok(quiet);
    };

    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let Some(policy) = policy::resolve(&cwd)? else {
        return Ok(quiet);
    };

    let (mut findings, counts) = match payload.get("hook_event_name").and_then(Value::as_str) {
        Some("PreToolUse") => pre_tool_use(&payload, &policy),
        Some("Stop") | Some("SubagentStop") => {
            if payload.get("stop_hook_active").and_then(Value::as_bool) == Some(true) {
                return Ok(quiet);
            }
            stop(&cwd, &policy)
        }
        _ => (Vec::new(), Vec::new()),
    };

    if let Some(id) = payload.get("session_id").and_then(Value::as_str) {
        if let Some(mut tracker) = Tracker::load(id) {
            for (file, count) in &counts {
                findings.extend(tracker.observe(file, *count));
            }
            tracker.save();
        }
    }

    if findings.is_empty() {
        return Ok(quiet);
    }
    Ok(Outcome {
        block: findings.iter().any(|f| f.severity == Severity::Block),
        message: render(&findings, &policy),
    })
}

type Analysis = (Vec<Finding>, Vec<(String, usize)>);

fn pre_tool_use(payload: &Value, policy: &Policy) -> Analysis {
    let tool = payload.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let input = payload.get("tool_input").unwrap_or(&Value::Null);
    let Some(path) = input.get("file_path").and_then(Value::as_str) else {
        return (Vec::new(), Vec::new());
    };
    let path = Path::new(path);
    let Some(lang) = crate::lang::Lang::from_path(path) else {
        return (Vec::new(), Vec::new());
    };

    let (source, touched) = match tool {
        "Write" => match input.get("content").and_then(Value::as_str) {
            Some(c) => (c.to_string(), None),
            None => return (Vec::new(), Vec::new()),
        },
        "Edit" | "MultiEdit" => match reconstruct(path, input) {
            Some(pair) => pair,
            None => return (Vec::new(), Vec::new()),
        },
        _ => return (Vec::new(), Vec::new()),
    };

    let name = path.display().to_string();
    let head = crate::vcs::show_head(path);
    let findings =
        crate::analyze_source_with(&name, &source, lang, policy, false, head.as_deref());
    let findings = match touched {
        Some(ranges) => findings
            .into_iter()
            .filter(|f| intersects(f, &ranges))
            .collect(),
        None => findings,
    };
    let counts = vec![(name, crate::prose_comment_count(&source, lang))];
    (findings, counts)
}

/// Applies the pending edit in memory so rules see whole-file context,
/// while reporting only lines the edit actually introduced.
fn reconstruct(path: &Path, input: &Value) -> Option<(String, Option<Vec<(usize, usize)>>)> {
    let original = std::fs::read_to_string(path).ok()?;
    let mut source = original;
    let mut ranges = Vec::new();

    let edits: Vec<&Value> = match input.get("edits").and_then(Value::as_array) {
        Some(list) => list.iter().collect(),
        None => vec![input],
    };

    for edit in edits {
        let old = edit.get("old_string").and_then(Value::as_str)?;
        let new = edit.get("new_string").and_then(Value::as_str)?;
        let at = source.find(old)?;
        let start_line = source[..at].lines().count().max(1);
        source.replace_range(at..at + old.len(), new);
        ranges.push((start_line, start_line + new.lines().count()));
    }
    Some((source, Some(ranges)))
}

fn intersects(f: &Finding, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(s, e)| f.end_line >= *s && f.start_line <= *e)
}

fn stop(cwd: &Path, policy: &Policy) -> Analysis {
    let mut out = Vec::new();
    let mut counts = Vec::new();

    for (file, ranges) in changed_files(cwd) {
        let findings = crate::analyze_file(&file, policy, false);
        out.extend(findings.into_iter().filter(|f| match &ranges {
            Some(r) => intersects(f, r),
            None => true,
        }));

        if let (Some(lang), Ok(source)) = (
            crate::lang::Lang::from_path(&file),
            std::fs::read_to_string(&file),
        ) {
            counts.push((
                file.display().to_string(),
                crate::prose_comment_count(&source, lang),
            ));
        }
    }
    (out, counts)
}

/// None ranges means the whole file is new. Any git failure yields nothing.
fn changed_files(cwd: &Path) -> Vec<(PathBuf, Option<Vec<(usize, usize)>>)> {
    let mut out = Vec::new();

    if let Some(diff) = crate::vcs::run(cwd, &["diff", "--unified=0", "HEAD"]) {
        out.extend(parse_diff(cwd, &diff));
    }
    if let Some(list) = crate::vcs::run(cwd, &["ls-files", "--others", "--exclude-standard"]) {
        for line in list.lines().filter(|l| !l.trim().is_empty()) {
            out.push((cwd.join(line.trim()), None));
        }
    }
    out
}

fn parse_diff(cwd: &Path, diff: &str) -> Vec<(PathBuf, Option<Vec<(usize, usize)>>)> {
    let mut out: Vec<(PathBuf, Option<Vec<(usize, usize)>>)> = Vec::new();
    let mut current: Option<PathBuf> = None;

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ b/") {
            current = Some(cwd.join(rest.trim()));
            continue;
        }
        let Some(header) = line.strip_prefix("@@ ") else {
            continue;
        };
        let Some(path) = current.clone() else { continue };
        let Some(added) = header.split_whitespace().find(|t| t.starts_with('+')) else {
            continue;
        };
        let mut parts = added.trim_start_matches('+').split(',');
        let Some(start) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
            continue;
        };
        let count = parts.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(1);
        if count == 0 {
            continue;
        }
        let range = (start, start + count);

        match out.iter_mut().find(|(p, _)| *p == path) {
            Some((_, Some(ranges))) => ranges.push(range),
            Some((_, slot)) => *slot = Some(vec![range]),
            None => out.push((path, Some(vec![range]))),
        }
    }
    out
}

fn render(findings: &[Finding], policy: &Policy) -> String {
    let mut s = String::from("Comment policy violations in code you just wrote:\n\n");
    for f in findings {
        s.push_str(&format!(
            "  {}:{}-{}  [{}] {}\n    > {}\n",
            f.file, f.start_line, f.end_line, f.rule, f.message, f.excerpt
        ));
    }
    if !policy.prose.trim().is_empty() {
        s.push_str(&format!(
            "\nThe policy you are bound by, verbatim from {}:\n\n{}\n",
            policy.source, policy.prose
        ));
    }
    s.push_str("\nRewrite the offending comments. Removing them is not a valid fix.\n");
    s
}

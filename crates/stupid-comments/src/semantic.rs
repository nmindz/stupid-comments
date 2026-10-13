use crate::comments::{Comment, Extraction, Kind};
use crate::policy::{Mode, Policy};
use crate::rules::{Context, Finding, Severity};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, LazyLock};
use std::time::{Duration, Instant};

const MAX_COMMENTS: usize = 20;
const CONTEXT_LINES: usize = 3;
const DEFAULT_BUDGET_MS: u64 = 10_000;
const DEFAULT_THRESHOLD: f64 = 0.5;
const SYSTEMONE_PATH: &str = "/v1/systemone";

/// A harness CLI run one-shot: `bin <pre> [<model_flag> <model>] <post> <prompt>`.
pub struct Harness {
    pub name: &'static str,
    pre: &'static [&'static str],
    post: &'static [&'static str],
    model_flag: Option<&'static str>,
}

/// Default precedence. Tools are off wherever the CLI allows it: the judge reads, it never acts.
pub const HARNESSES: &[Harness] = &[
    Harness {
        name: "omp",
        pre: &[
            "-p", "--no-tools", "--no-session", "--no-extensions", "--no-skills", "--no-rules",
            "--no-lsp", "--no-title", "--thinking", "off",
        ],
        post: &[],
        model_flag: Some("--model"),
    },
    Harness { name: "dsh", pre: &["--profile", "headless"], post: &[], model_flag: None },
    Harness { name: "opencode", pre: &["run", "--pure"], post: &[], model_flag: Some("-m") },
    Harness { name: "claude", pre: &["-p", "--no-session-persistence"], post: &[], model_flag: Some("--model") },
    Harness { name: "codex", pre: &["exec", "--skip-git-repo-check"], post: &[], model_flag: Some("-m") },
    Harness {
        name: "pi",
        pre: &["-p", "--no-session", "--no-tools", "--no-extensions"],
        post: &[],
        model_flag: Some("--model"),
    },
    Harness { name: "cursor-agent", pre: &["-p"], post: &[], model_flag: Some("--model") },
    Harness { name: "gemini", pre: &[], post: &[], model_flag: Some("-m") },
    Harness { name: "copilot", pre: &[], post: &["-p"], model_flag: Some("--model") },
];

#[derive(Clone, Copy, Debug, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    /// System One when questions and endpoints are configured, else a harness.
    #[default]
    Auto,
    Systemone,
    Harness,
    Command,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    pub url: String,
    pub model: String,
    /// Name of the variable holding a bearer token; the token never lives in config.
    #[serde(default)]
    pub api_key_env: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Question {
    pub id: String,
    pub instructions: String,
    #[serde(default)]
    pub threshold: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JudgeConfig {
    pub backend: Option<Backend>,
    pub systemone: Option<Vec<Endpoint>>,
    pub questions: Option<Vec<Question>>,
    pub harness: Option<String>,
    pub harnesses: Option<Vec<String>>,
    pub model: Option<String>,
    pub command: Option<Vec<String>>,
    pub budget_ms: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Judge {
    pub backend: Backend,
    pub systemone: Vec<Endpoint>,
    pub questions: Vec<Question>,
    pub harness: Option<String>,
    pub harnesses: Vec<String>,
    pub model: Option<String>,
    pub command: Vec<String>,
    pub budget_ms: u64,
}

impl Default for Judge {
    fn default() -> Self {
        Self {
            backend: Backend::Auto,
            systemone: Vec::new(),
            questions: Vec::new(),
            harness: None,
            harnesses: HARNESSES.iter().map(|h| h.name.to_string()).collect(),
            model: None,
            command: Vec::new(),
            budget_ms: DEFAULT_BUDGET_MS,
        }
    }
}

impl Judge {
    pub fn apply(&mut self, c: JudgeConfig) {
        if let Some(v) = c.backend {
            self.backend = v;
        }
        if let Some(v) = c.systemone {
            self.systemone = v;
        }
        if let Some(v) = c.questions {
            self.questions = v;
        }
        if let Some(v) = c.harness {
            self.harness = Some(v);
        }
        if let Some(v) = c.harnesses {
            self.harnesses = v;
        }
        if let Some(v) = c.model {
            self.model = Some(v);
        }
        if let Some(v) = c.command {
            self.command = v;
        }
        if let Some(v) = c.budget_ms {
            self.budget_ms = v;
        }
    }

    /// The pinned harness, else the first of `harnesses` found on PATH.
    pub fn detect(&self) -> Option<(&'static Harness, PathBuf)> {
        let order: Vec<&str> = match &self.harness {
            Some(pinned) => vec![pinned.as_str()],
            None => self.harnesses.iter().map(String::as_str).collect(),
        };
        order.into_iter().find_map(|name| {
            let harness = HARNESSES.iter().find(|h| h.name == name)?;
            Some((harness, on_path(harness.name)?))
        })
    }

    pub fn describe(&self) -> String {
        let mut s = format!("backend {:?}, budget {} ms", self.backend, self.budget_ms);
        let endpoints: Vec<String> =
            self.systemone.iter().map(|e| format!("{} ({})", e.url, e.model)).collect();
        s.push_str(&format!(
            "\n  systemone: {}",
            if endpoints.is_empty() { "none configured".into() } else { endpoints.join(" -> ") }
        ));
        s.push_str(&format!("\n  questions: {}", self.questions.len()));
        for q in &self.questions {
            s.push_str(&format!(
                "\n    {} (>= {}): {}",
                q.id,
                q.threshold.unwrap_or(DEFAULT_THRESHOLD),
                q.instructions
            ));
        }
        s.push_str(&match self.detect() {
            Some((h, path)) => format!("\n  harness: {} at {}", h.name, path.display()),
            None => format!("\n  harness: none found among {}", self.harnesses.join(", ")),
        });
        if !self.command.is_empty() {
            s.push_str(&format!("\n  command: {}", self.command.join(" ")));
        }
        s
    }
}

#[derive(Deserialize)]
struct Verdict {
    index: usize,
    violation: bool,
    #[serde(default)]
    reason: String,
}

/// Opt-in taste judgement. Every failure path is silent — a judge that cannot
/// answer must never block a write, and must never outlive the hook's budget.
pub fn judge(ctx: &Context, extraction: &Extraction, policy: &Policy) -> Vec<Finding> {
    if ctx.rules.semantic == Mode::Shadow {
        return Vec::new();
    }
    let prose: Vec<&Comment> = extraction
        .comments
        .iter()
        .filter(|c| c.kind == Kind::Prose)
        .filter(|c| match ctx.focus {
            Some(ranges) => ranges.iter().any(|(s, e)| c.end_line >= *s && c.start_line <= *e),
            None => true,
        })
        .take(MAX_COMMENTS)
        .collect();
    if prose.is_empty() {
        return Vec::new();
    }

    let judge = &ctx.rules.judge;
    let systemone_ready = !judge.questions.is_empty() && !judge.systemone.is_empty();
    let found = match judge.backend {
        Backend::Systemone => systemone(ctx, &prose),
        Backend::Harness => harness(ctx, &prose, policy),
        Backend::Command => command(ctx, &prose, policy),
        Backend::Auto if systemone_ready => {
            systemone(ctx, &prose).or_else(|| harness(ctx, &prose, policy))
        }
        Backend::Auto => harness(ctx, &prose, policy),
    };
    found.unwrap_or_default()
}

fn severity(ctx: &Context) -> Severity {
    match ctx.rules.semantic {
        Mode::Block => Severity::Block,
        _ => Severity::Warn,
    }
}

fn finding(ctx: &Context, c: &Comment, reason: String) -> Finding {
    Finding {
        file: ctx.file.clone(),
        rule: "semantic",
        severity: severity(ctx),
        kind: c.kind,
        start_line: c.start_line,
        end_line: c.end_line,
        message: format!("{reason} Rewrite it to earn its place. Deleting the comment is not compliance."),
        excerpt: c.raw.lines().next().unwrap_or_default().trim().to_string(),
    }
}

/// One request per comment carrying every question. None when no endpoint
/// answered at all, so `auto` can fall through to a harness.
fn systemone(ctx: &Context, prose: &[&Comment]) -> Option<Vec<Finding>> {
    let judge = &ctx.rules.judge;
    if judge.questions.is_empty() {
        return None;
    }
    let questions: serde_json::Map<String, Value> = judge
        .questions
        .iter()
        .map(|q| (q.id.clone(), json!({ "type": "noul", "instructions": q.instructions })))
        .collect();

    let mut alive: Vec<&Endpoint> = judge.systemone.iter().collect();
    let mut answered = false;
    let mut out = Vec::new();
    for c in prose {
        let state = with_context(ctx.source, c);
        let answers = loop {
            let Some(endpoint) = alive.first() else { break None };
            match ask(endpoint, &state, &questions, remaining(judge.budget_ms)?) {
                Some(a) => break Some(a),
                None => {
                    alive.remove(0);
                }
            }
        };
        let Some(answers) = answers else { break };
        answered = true;
        for q in &judge.questions {
            let Some(p) = answers.get(&q.id).and_then(|a| a.get("noul")).and_then(Value::as_f64) else {
                continue;
            };
            if p >= q.threshold.unwrap_or(DEFAULT_THRESHOLD) {
                out.push(finding(ctx, c, format!("The judge answered yes to \"{}\" ({p:.2}).", q.instructions)));
            }
        }
    }
    answered.then_some(out)
}

fn ask(endpoint: &Endpoint, state: &str, questions: &serde_json::Map<String, Value>, timeout: Duration) -> Option<Value> {
    let base = endpoint.url.trim_end_matches('/');
    let url = if base.ends_with(SYSTEMONE_PATH) { base.to_string() } else { format!("{base}{SYSTEMONE_PATH}") };
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut request = agent.post(&url);
    if let Some(var) = &endpoint.api_key_env {
        let token = std::env::var(var).ok().filter(|t| !t.is_empty())?;
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let body = json!({ "model": endpoint.model, "state": state, "questions": questions });
    let mut response = request.send_json(&body).ok()?;
    let value: Value = response.body_mut().read_json().ok()?;
    value.get("answers").cloned().filter(Value::is_object)
}

/// The comment plus the code it sits above, so "restates the code" is decidable.
fn with_context(source: &str, c: &Comment) -> String {
    let after: Vec<&str> = source
        .lines()
        .skip(c.end_line)
        .filter(|l| !l.trim().is_empty())
        .take(CONTEXT_LINES)
        .collect();
    match after.is_empty() {
        true => c.raw.clone(),
        false => format!("{}\n{}", c.raw, after.join("\n")),
    }
}

fn harness(ctx: &Context, prose: &[&Comment], policy: &Policy) -> Option<Vec<Finding>> {
    let judge = &ctx.rules.judge;
    let (h, path) = judge.detect()?;
    let mut args: Vec<String> = h.pre.iter().map(|s| s.to_string()).collect();
    if let (Some(flag), Some(model)) = (h.model_flag, &judge.model) {
        args.extend([flag.to_string(), model.clone()]);
    }
    args.extend(h.post.iter().map(|s| s.to_string()));
    args.push(prompt(prose, policy, &judge.questions));
    let output = run(path.as_os_str(), &args, None, remaining(judge.budget_ms)?)?;
    verdicts(ctx, prose, &output)
}

fn command(ctx: &Context, prose: &[&Comment], policy: &Policy) -> Option<Vec<Finding>> {
    let judge = &ctx.rules.judge;
    let (program, args) = judge.command.split_first()?;
    let text = prompt(prose, policy, &judge.questions);
    let output = run(program.as_ref(), args, Some(&text), remaining(judge.budget_ms)?)?;
    verdicts(ctx, prose, &output)
}

fn prompt(prose: &[&Comment], policy: &Policy, questions: &[Question]) -> String {
    let listing = prose
        .iter()
        .enumerate()
        .map(|(i, c)| format!("[{i}] line {}:\n{}", c.start_line, c.raw))
        .collect::<Vec<_>>()
        .join("\n\n");
    let checks = match questions.is_empty() {
        true => String::new(),
        false => format!(
            "A comment violates the policy when any of these is true:\n{}\n\n",
            questions.iter().map(|q| format!("- {}", q.instructions)).collect::<Vec<_>>().join("\n")
        ),
    };
    format!(
        "You are judging code comments against a policy. Reply with JSON only.\n\n\
         POLICY:\n{}\n\n{checks}COMMENTS:\n{listing}\n\n\
         For each comment decide whether it violates the policy. A comment that earns its place \
         by aiding visual scanning is not a violation. Reply with a JSON array of objects \
         {{\"index\": <number>, \"violation\": <bool>, \"reason\": <short string>}} and nothing else.",
        policy.prose
    )
}

fn verdicts(ctx: &Context, prose: &[&Comment], output: &str) -> Option<Vec<Finding>> {
    let start = output.find('[')?;
    let end = output.rfind(']')?;
    let verdicts: Vec<Verdict> = serde_json::from_str(output.get(start..=end)?).ok()?;
    Some(
        verdicts
            .into_iter()
            .filter(|v| v.violation)
            .filter_map(|v| prose.get(v.index).map(|c| finding(ctx, c, punctuate(&v.reason))))
            .collect(),
    )
}

fn punctuate(reason: &str) -> String {
    let trimmed = reason.trim();
    match trimmed.chars().last() {
        Some('.') | Some('!') | Some('?') => trimmed.to_string(),
        Some(_) => format!("{trimmed}."),
        None => "Fails the policy.".to_string(),
    }
}

/// Time left of the per-process judging budget; None once it is spent.
fn remaining(budget_ms: u64) -> Option<Duration> {
    static START: LazyLock<Instant> = LazyLock::new(Instant::now);
    Duration::from_millis(budget_ms).checked_sub(START.elapsed()).filter(|d| !d.is_zero())
}

/// Runs a judge process with the plugin disarmed inside it, so a harness that
/// loads this plugin cannot judge its own judging. Killed whole at the deadline.
fn run(program: &std::ffi::OsStr, args: &[String], stdin: Option<&str>, timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env(crate::hook::DISARM_ENV, "0")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().ok()?;

    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    if let Some(text) = stdin {
        let wrote = child.stdin.take().map(|mut s| s.write_all(text.as_bytes()).is_ok());
        if wrote != Some(true) {
            kill(&mut child);
            return None;
        }
    }

    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                kill(&mut child);
                return None;
            }
        }
    }
    // A grandchild can hold stdout open after the judge exits.
    let output = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok();
    kill(&mut child);
    output
}

fn kill(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{}", child.id())])
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| executable(p))
}

fn executable(p: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

use crate::semantic::{Judge, JudgeConfig};
use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const CONFIG_NAME: &str = ".stupid-comments.jsonc";
/// Points at the user-level config; overrides the XDG location.
pub const USER_CONFIG_ENV: &str = "STUPID_COMMENTS_CONFIG";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Shadow,
    Warn,
    Block,
}

impl Mode {
    pub fn blocks(self) -> bool {
        self == Mode::Block
    }
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub mode: Option<Mode>,
    pub prose: Option<String>,
    pub max_prose_comment_lines: Option<usize>,
    pub max_comment_ratio: Option<f64>,
    pub min_prose_comments_for_ratio: Option<usize>,
    pub max_doc_comment_lines: Option<usize>,
    pub banned_patterns: Option<Vec<String>>,
    pub redundancy: Option<Mode>,
    pub semantic: Option<Mode>,
    pub judge: Option<JudgeConfig>,
    pub exclude: Option<Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Rules {
    pub mode: Mode,
    pub max_prose_comment_lines: usize,
    pub max_comment_ratio: f64,
    pub min_prose_comments_for_ratio: usize,
    pub max_doc_comment_lines: usize,
    pub banned_patterns: Vec<String>,
    pub redundancy: Mode,
    pub semantic: Mode,
    pub judge: Judge,
    pub exclude: Vec<String>,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            mode: Mode::Shadow,
            max_prose_comment_lines: 5,
            max_comment_ratio: 0.35,
            min_prose_comments_for_ratio: 4,
            max_doc_comment_lines: 40,
            banned_patterns: Vec::new(),
            redundancy: Mode::Warn,
            semantic: Mode::Shadow,
            judge: Judge::default(),
            exclude: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub prose: String,
    pub source: String,
    pub rules: Rules,
}

/// Absent policy means the tool stays completely silent. The user config is the
/// global baseline; a project config overrides it key by key.
pub fn resolve(start: &Path) -> Result<Option<Policy>> {
    let user_path = user_config_path().filter(|p| p.is_file());
    let project_path = find_upward(start, CONFIG_NAME);
    let mut layers = Vec::new();
    for path in user_path.iter().chain(project_path.iter()) {
        layers.push(read_config(path)?);
    }

    let explicit = layers.iter().rev().find_map(|c| c.prose.as_deref());
    let (prose, source) = match resolve_prose(start, explicit) {
        Some(found) => found,
        None => match project_path.as_ref().or(user_path.as_ref()) {
            Some(p) => (String::new(), p.display().to_string()),
            None => return Ok(None),
        },
    };

    let mut rules = Rules::default();
    for layer in layers {
        apply(&mut rules, layer);
    }
    Ok(Some(Policy { prose, source, rules }))
}

/// `$STUPID_COMMENTS_CONFIG`, else `$XDG_CONFIG_HOME/stupid-comments/config.jsonc`,
/// else `~/.config/stupid-comments/config.jsonc`.
pub fn user_config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(USER_CONFIG_ENV) {
        if !p.trim().is_empty() {
            return Some(expand_home(&p));
        }
    }
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var("HOME").ok()?).join(".config"),
    };
    Some(base.join("stupid-comments").join("config.jsonc"))
}

fn apply(rules: &mut Rules, c: Config) {
    if let Some(v) = c.mode {
        rules.mode = v;
    }
    if let Some(v) = c.max_prose_comment_lines {
        rules.max_prose_comment_lines = v;
    }
    if let Some(v) = c.max_comment_ratio {
        rules.max_comment_ratio = v;
    }
    if let Some(v) = c.min_prose_comments_for_ratio {
        rules.min_prose_comments_for_ratio = v;
    }
    if let Some(v) = c.max_doc_comment_lines {
        rules.max_doc_comment_lines = v;
    }
    if let Some(v) = c.banned_patterns {
        rules.banned_patterns = v;
    }
    if let Some(v) = c.redundancy {
        rules.redundancy = v;
    }
    if let Some(v) = c.semantic {
        rules.semantic = v;
    }
    if let Some(v) = c.judge {
        rules.judge.apply(v);
    }
    if let Some(v) = c.exclude {
        rules.exclude = v;
    }
}

fn read_config(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)?;
    Ok(json5::from_str(&raw)?)
}

fn find_upward(start: &Path, name: &str) -> Option<PathBuf> {
    let mut dir = if start.is_dir() {
        Some(start)
    } else {
        start.parent()
    };
    while let Some(d) = dir {
        let candidate = d.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

fn resolve_prose(start: &Path, explicit: Option<&str>) -> Option<(String, String)> {
    if let Some(path) = explicit {
        let expanded = expand_home(path);
        let text = std::fs::read_to_string(&expanded).ok()?;
        // Agent memory carries more than the policy; quote only its section.
        let prose = extract_section(&text).unwrap_or_else(|| text.trim().to_string());
        return Some((prose, expanded.display().to_string()));
    }

    for candidate in memory_files(start) {
        let Ok(text) = std::fs::read_to_string(&candidate) else {
            continue;
        };
        if let Some(section) = extract_section(&text) {
            return Some((section, candidate.display().to_string()));
        }
    }
    None
}

/// Agent-home memory, then project memory, in a fixed order — the first file
/// carrying a policy section wins. Every supported harness keeps its own memory
/// file under its own home, and a machine that runs more than one of them
/// should not get a different policy depending on which harness asked.
fn memory_files(start: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (var, fallback, names) in AGENT_HOMES {
        let Some(dir) = agent_home(var, fallback) else {
            continue;
        };
        // A harness loads the first of its candidates, so that one alone speaks for it.
        if let Some(file) = names.iter().map(|n| dir.join(n)).find(|f| f.is_file()) {
            out.push(file);
        }
    }
    for name in PROJECT_MEMORY {
        if let Some(local) = find_upward(start, name) {
            out.push(local);
        }
    }
    out
}

/// (home override variable, path under $HOME, memory files in the order the
/// harness reads them). Pi and omp share one override variable.
const AGENT_HOMES: &[(&str, &str, &[&str])] = &[
    ("CLAUDE_CONFIG_DIR", ".claude", &["CLAUDE.md"]),
    ("DSH_HOME", ".dsh", &["AGENTS.md"]),
    (
        "PI_CODING_AGENT_DIR",
        ".pi/agent",
        &["AGENTS.override.md", "AGENTS.md", "AGENTS.MD", "CLAUDE.md", "CLAUDE.MD"],
    ),
    ("PI_CODING_AGENT_DIR", ".omp/agent", &["AGENTS.md"]),
    ("AGENTS_HOME", ".agents", &["AGENTS.md"]),
];

const PROJECT_MEMORY: &[&str] = &["CLAUDE.md", "AGENTS.md"];

fn agent_home(var: &str, fallback: &str) -> Option<PathBuf> {
    match std::env::var(var) {
        Ok(dir) if !dir.trim().is_empty() => Some(expand_home(&dir)),
        _ => Some(PathBuf::from(std::env::var("HOME").ok()?).join(fallback)),
    }
}

/// Matches a `# Comments Policy` heading at any level, case-insensitively,
/// and captures until the next heading.
pub fn extract_section(markdown: &str) -> Option<String> {
    let heading = regex::Regex::new(r"(?im)^#{1,6}[ \t]*comments?[ \t]+polic(?:y|ies)[ \t]*$").ok()?;
    let m = heading.find(markdown)?;
    let rest = &markdown[m.end()..];
    let next = regex::Regex::new(r"(?m)^#{1,6}[ \t]").ok()?;
    let body = match next.find(rest) {
        Some(n) => &rest[..n.start()],
        None => rest,
    };
    let body = body.trim();
    (!body.is_empty()).then(|| body.to_string())
}

fn expand_home(path: &str) -> PathBuf {
    let rest = match path {
        "~" => Some(""),
        _ => path.strip_prefix("~/"),
    };
    match (rest, std::env::var("HOME")) {
        (Some(rest), Ok(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

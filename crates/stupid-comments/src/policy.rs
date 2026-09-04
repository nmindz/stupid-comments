use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const CONFIG_NAME: &str = ".stupid-comments.jsonc";

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
    pub semantic_command: Option<Vec<String>>,
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
    pub semantic_command: Vec<String>,
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
            semantic_command: vec!["claude".into(), "-p".into()],
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

/// Absent policy means the tool stays completely silent.
pub fn resolve(start: &Path) -> Result<Option<Policy>> {
    let config_path = find_upward(start, CONFIG_NAME);
    let config = match &config_path {
        Some(p) => Some(read_config(p)?),
        None => None,
    };

    let (prose, source) = match resolve_prose(start, config.as_ref().and_then(|c| c.prose.as_deref())) {
        Some(found) => found,
        None => match &config_path {
            Some(p) => (String::new(), p.display().to_string()),
            None => return Ok(None),
        },
    };

    Ok(Some(Policy {
        prose,
        source,
        rules: merge(config),
    }))
}

fn merge(config: Option<Config>) -> Rules {
    let mut rules = Rules::default();
    let Some(c) = config else { return rules };

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
    if let Some(v) = c.semantic_command {
        if !v.is_empty() {
            rules.semantic_command = v;
        }
    }
    if let Some(v) = c.exclude {
        rules.exclude = v;
    }
    rules
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
        return Some((text.trim().to_string(), expanded.display().to_string()));
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

fn memory_files(start: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        out.push(PathBuf::from(&home).join(".claude/CLAUDE.md"));
    }
    if let Some(local) = find_upward(start, "CLAUDE.md") {
        out.push(local);
    }
    out
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
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

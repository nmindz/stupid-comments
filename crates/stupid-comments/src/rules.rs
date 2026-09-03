use crate::comments::{Comment, Extraction, Kind};
use crate::lang::Lang;
use crate::policy::{Mode, Rules};
use regex::Regex;
use serde::Serialize;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Block,
    Warn,
}

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub file: String,
    pub rule: &'static str,
    pub severity: Severity,
    pub kind: Kind,
    pub start_line: usize,
    pub end_line: usize,
    pub message: String,
    pub excerpt: String,
}

pub struct Context<'a> {
    pub file: String,
    pub source: &'a str,
    pub lang: Lang,
    pub rules: &'a Rules,
    /// Set by the human-invoked sweep; permits deletion as a remedy.
    pub adjudicate: bool,
}

pub fn evaluate(ctx: &Context, extraction: &Extraction) -> Vec<Finding> {
    let mut findings = Vec::new();
    let banned = compile(&ctx.rules.banned_patterns);
    let lines: Vec<&str> = ctx.source.lines().collect();

    for c in &extraction.comments {
        if matches!(c.kind, Kind::Directive | Kind::LicenseHeader) {
            continue;
        }
        check_banned(ctx, c, &banned, &mut findings);
        check_length(ctx, c, &mut findings);
        check_redundancy(ctx, c, &lines, &mut findings);
    }

    check_ratio(ctx, extraction, &mut findings);

    if ctx.lang.is_provisional() {
        for f in &mut findings {
            f.severity = Severity::Warn;
        }
    }
    if !ctx.rules.mode.blocks() {
        for f in &mut findings {
            f.severity = Severity::Warn;
        }
    }
    findings
}

fn compile(patterns: &[String]) -> Vec<Regex> {
    patterns.iter().filter_map(|p| Regex::new(p).ok()).collect()
}

fn check_banned(ctx: &Context, c: &Comment, banned: &[Regex], out: &mut Vec<Finding>) {
    for re in banned {
        if let Some(m) = re.find(&c.body) {
            out.push(Finding {
                file: ctx.file.clone(),
                rule: "banned-pattern",
                severity: Severity::Block,
                kind: c.kind,
                start_line: c.start_line,
                end_line: c.end_line,
                message: format!(
                    "Comment contains banned text {:?}. {}",
                    m.as_str(),
                    remedy(ctx, "Rewrite the comment without it")
                ),
                excerpt: excerpt(c),
            });
            break;
        }
    }
}

fn check_length(ctx: &Context, c: &Comment, out: &mut Vec<Finding>) {
    let (limit, rule) = match c.kind {
        Kind::DocComment => (ctx.rules.max_doc_comment_lines, "doc-comment-too-long"),
        _ => (ctx.rules.max_prose_comment_lines, "prose-comment-too-long"),
    };
    if c.line_count() <= limit {
        return;
    }
    out.push(Finding {
        file: ctx.file.clone(),
        rule,
        severity: if c.kind == Kind::DocComment {
            Severity::Warn
        } else {
            Severity::Block
        },
        kind: c.kind,
        start_line: c.start_line,
        end_line: c.end_line,
        message: format!(
            "{}-line comment exceeds the {}-line limit; long explanations belong in documentation. {}",
            c.line_count(),
            limit,
            remedy(ctx, "Condense it to the one line that aids visual scanning")
        ),
        excerpt: excerpt(c),
    });
}

fn check_ratio(ctx: &Context, extraction: &Extraction, out: &mut Vec<Finding>) {
    if ctx.lang.is_data_format() {
        return;
    }
    let prose: Vec<&Comment> = extraction
        .comments
        .iter()
        .filter(|c| c.kind == Kind::Prose)
        .collect();

    if prose.len() < ctx.rules.min_prose_comments_for_ratio {
        return;
    }
    let commented: usize = prose.iter().map(|c| c.line_count()).sum();
    let ratio = commented as f64 / extraction.total_lines as f64;
    if ratio <= ctx.rules.max_comment_ratio {
        return;
    }

    // Naming spans keeps "delete everything" from being the obvious remedy.
    let mut longest: Vec<&&Comment> = prose.iter().collect();
    longest.sort_by_key(|c| std::cmp::Reverse(c.line_count()));
    let named: Vec<String> = longest
        .iter()
        .take(3)
        .map(|c| format!("L{}-{}", c.start_line, c.end_line))
        .collect();

    out.push(Finding {
        file: ctx.file.clone(),
        rule: "comment-ratio",
        severity: Severity::Block,
        kind: Kind::Prose,
        start_line: longest[0].start_line,
        end_line: longest[0].end_line,
        message: format!(
            "Prose comments cover {commented} of {} lines. Start with {}. {}",
            extraction.total_lines,
            named.join(", "),
            remedy(ctx, "Tighten those specific comments")
        ),
        excerpt: excerpt(longest[0]),
    });
}

fn check_redundancy(ctx: &Context, c: &Comment, lines: &[&str], out: &mut Vec<Finding>) {
    if c.kind != Kind::Prose || ctx.rules.redundancy == Mode::Shadow {
        return;
    }
    let Some(code) = lines.get(c.end_line).map(|l| l.trim()) else {
        return;
    };
    if code.is_empty() {
        return;
    }
    let comment_tokens = tokenize(&c.body);
    if comment_tokens.is_empty() || comment_tokens.len() > 8 {
        return;
    }
    let code_tokens = tokenize(code);
    if !comment_tokens.iter().all(|t| code_tokens.contains(t)) {
        return;
    }

    out.push(Finding {
        file: ctx.file.clone(),
        rule: "redundant-comment",
        severity: Severity::Warn,
        kind: c.kind,
        start_line: c.start_line,
        end_line: c.end_line,
        message: format!(
            "Comment restates the code below it. {}",
            remedy(ctx, "Say why, not what — or drop it")
        ),
        excerpt: excerpt(c),
    });
}

fn remedy(ctx: &Context, rewrite: &str) -> String {
    if ctx.adjudicate {
        format!("{rewrite}, or remove it.")
    } else {
        format!("{rewrite}. Deleting the comment is not compliance.")
    }
}

fn excerpt(c: &Comment) -> String {
    let first = c.raw.lines().next().unwrap_or_default().trim().to_string();
    if first.chars().count() > 100 {
        format!("{}…", first.chars().take(99).collect::<String>())
    } else {
        first
    }
}

/// Lowercased word tokens, splitting snake_case and camelCase boundaries.
fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if ch.is_uppercase() && prev_lower && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            current.push(ch.to_ascii_lowercase());
            prev_lower = ch.is_lowercase() || ch.is_numeric();
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
            prev_lower = false;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.retain(|t| t.len() > 2 && !STOPWORDS.contains(&t.as_str()));
    out
}

const STOPWORDS: &[&str] = &[
    "the", "this", "that", "and", "for", "with", "into", "from", "then", "our", "its", "all", "not",
    "are", "was", "you", "use", "via", "per",
];

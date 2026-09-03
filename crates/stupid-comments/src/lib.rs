pub mod comments;
pub mod hook;
pub mod lang;
pub mod policy;
pub mod rules;
pub mod semantic;
pub mod session;
pub mod suppress;
pub mod vcs;

use globset::{Glob, GlobSetBuilder};
use lang::Lang;
use policy::Policy;
use rules::{Context, Finding};
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[".git", ".jj", "target", "node_modules", "dist", "build", "vendor"];

/// Every failure path yields no findings — the gate never blocks on ambiguity.
pub fn analyze_file(path: &Path, policy: &Policy, adjudicate: bool) -> Vec<Finding> {
    if excluded(path, policy) {
        return Vec::new();
    }
    let Some(lang) = Lang::from_path(path) else {
        return Vec::new();
    };
    let Ok(source) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let head = vcs::show_head(path);
    analyze_source_with(
        &path.display().to_string(),
        &source,
        lang,
        policy,
        adjudicate,
        head.as_deref(),
    )
}

pub fn analyze_source(
    file: &str,
    source: &str,
    lang: Lang,
    policy: &Policy,
    adjudicate: bool,
) -> Vec<Finding> {
    analyze_source_with(file, source, lang, policy, adjudicate, None)
}

pub fn analyze_source_with(
    file: &str,
    source: &str,
    lang: Lang,
    policy: &Policy,
    adjudicate: bool,
    head: Option<&str>,
) -> Vec<Finding> {
    let Some(extraction) = comments::extract(source, lang) else {
        return Vec::new();
    };
    let ctx = Context {
        file: file.to_string(),
        source,
        lang,
        rules: &policy.rules,
        adjudicate,
    };

    let mut findings = rules::evaluate(&ctx, &extraction);
    findings.extend(semantic::judge(&ctx, &extraction, policy));
    suppress::apply(findings, &extraction.comments, head)
}

/// Used by the gaming signal: how much prose a file currently carries.
pub fn prose_comment_count(source: &str, lang: Lang) -> usize {
    comments::extract(source, lang)
        .map(|e| {
            e.comments
                .iter()
                .filter(|c| c.kind == comments::Kind::Prose)
                .count()
        })
        .unwrap_or(0)
}

fn excluded(path: &Path, policy: &Policy) -> bool {
    if policy.rules.exclude.is_empty() {
        return false;
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in &policy.rules.exclude {
        if let Ok(g) = Glob::new(pattern) {
            builder.add(g);
        }
    }
    builder
        .build()
        .map(|set| set.is_match(path))
        .unwrap_or(false)
}

pub fn collect_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn walk(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if p.is_dir() {
            if !SKIP_DIRS.contains(&name.as_ref()) && !name.starts_with('.') {
                walk(&p, out);
            }
        } else if Lang::from_path(&p).is_some() {
            out.push(p);
        }
    }
}

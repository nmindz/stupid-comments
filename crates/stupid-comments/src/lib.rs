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
    let Some(lang) = Lang::from_file(path) else {
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

pub fn excluded(path: &Path, policy: &Policy) -> bool {
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

/// Files the tool can parse, and the ones it cannot. Silence about the second
/// list is how an unparsed file passes for a clean one.
#[derive(Default)]
pub struct Scan {
    pub files: Vec<PathBuf>,
    pub skipped: Vec<PathBuf>,
}

pub fn scan(root: &Path) -> Scan {
    let mut out = Scan::default();
    walk(root, &mut out);
    out
}

fn walk(path: &Path, out: &mut Scan) {
    if path.is_file() {
        match Lang::from_file(path) {
            Some(_) => out.files.push(path.to_path_buf()),
            None => out.skipped.push(path.to_path_buf()),
        }
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
        } else if Lang::from_file(&p).is_some() {
            out.files.push(p);
        } else {
            out.skipped.push(p);
        }
    }
}

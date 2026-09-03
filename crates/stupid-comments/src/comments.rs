use crate::lang::Lang;
use tree_sitter::{Node, Parser};

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Directive,
    LicenseHeader,
    DocComment,
    Prose,
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub kind: Kind,
    pub raw: String,
    pub body: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_col: usize,
}

impl Comment {
    pub fn line_count(&self) -> usize {
        self.end_line - self.start_line + 1
    }
}

pub struct Extraction {
    pub comments: Vec<Comment>,
    pub total_lines: usize,
    /// Comments came from a line scan because the grammar choked. Less certain,
    /// so callers soften what they do with them.
    pub recovered: bool,
}

/// Returns None on any parse failure — callers fail open.
pub fn extract(source: &str, lang: Lang) -> Option<Extraction> {
    let mut parser = Parser::new();
    parser.set_language(&lang.language()).ok()?;
    let tree = parser.parse(source, None)?;

    // A templated manifest parses to one bare ERROR node, comments and all.
    let recovered = tree.root_node().has_error() && lang.hash_line_comments();

    let mut comments = if recovered {
        scan_lines(source, lang)
    } else {
        let mut nodes = Vec::new();
        collect(tree.root_node(), &mut nodes);
        nodes.into_iter().map(|n| build(n, source, lang)).collect()
    };
    comments.sort_by_key(|c| c.start_line);

    Some(Extraction {
        comments: merge_runs(comments),
        total_lines: source.lines().count().max(1),
        recovered,
    })
}

/// Only whole-line comments count. A trailing `#` is left alone rather than
/// risk mistaking a `#` inside a quoted value for one.
fn scan_lines(source: &str, lang: Lang) -> Vec<Comment> {
    let mut out = Vec::new();
    let mut block_scalar_at: Option<usize> = None;

    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        // A block scalar's body is data; a `#` opening a line there is content.
        if let Some(base) = block_scalar_at {
            if trimmed.is_empty() || indent > base {
                continue;
            }
            block_scalar_at = None;
        }
        if lang == Lang::Yaml && opens_block_scalar(trimmed) {
            block_scalar_at = Some(indent);
            continue;
        }
        if !(trimmed.starts_with('#') || (lang == Lang::Hcl && trimmed.starts_with("//"))) {
            continue;
        }

        let raw = trimmed.trim_end().to_string();
        let body = strip_markers(&raw);
        let start_line = i + 1;
        let kind = if is_directive(&raw, &body) {
            Kind::Directive
        } else if is_license_header(&body, start_line) {
            Kind::LicenseHeader
        } else {
            Kind::Prose
        };
        out.push(Comment {
            kind,
            raw,
            body,
            start_line,
            end_line: start_line,
            start_col: indent,
        });
    }
    out
}

fn opens_block_scalar(trimmed: &str) -> bool {
    let head = match trimmed.find(" #") {
        Some(at) => &trimmed[..at],
        None => trimmed,
    };
    let head = head.trim_end();
    let head = head.strip_suffix(['-', '+']).unwrap_or(head);
    matches!(head.chars().last(), Some('|') | Some('>'))
}

fn collect<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind().contains("comment") {
            out.push(child);
        } else {
            collect(child, out);
        }
    }
}

fn build(node: Node, source: &str, lang: Lang) -> Comment {
    let raw = source[node.byte_range()].to_string();
    let body = strip_markers(&raw);
    let start_line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;
    let kind = classify(&raw, &body, start_line, node, lang);

    Comment {
        kind,
        raw,
        body,
        start_line,
        end_line,
        start_col: node.start_position().column,
    }
}

/// A stack of `//` lines is one comment to a reader, so it must be one to the
/// rules. Directives and license headers never merge — otherwise prepending a
/// lint pragma would launder the block that follows it.
fn merge_runs(comments: Vec<Comment>) -> Vec<Comment> {
    let mut out: Vec<Comment> = Vec::new();

    for c in comments {
        let mergeable = matches!(c.kind, Kind::Prose | Kind::DocComment) && is_line_comment(&c.raw);
        let joins = mergeable
            && out.last().is_some_and(|p| {
                p.kind == c.kind
                    && is_line_comment(&p.raw)
                    && p.end_line + 1 == c.start_line
                    && p.start_col == c.start_col
            });

        if joins {
            let prev = out.last_mut().expect("checked above");
            prev.raw.push('\n');
            prev.raw.push_str(&c.raw);
            prev.body.push('\n');
            prev.body.push_str(&c.body);
            prev.end_line = c.end_line;
        } else {
            out.push(c);
        }
    }
    out
}

fn is_line_comment(raw: &str) -> bool {
    (raw.starts_with("//") || raw.starts_with('#')) && !raw.starts_with("/*")
}

fn classify(raw: &str, body: &str, start_line: usize, node: Node, lang: Lang) -> Kind {
    if is_directive(raw, body) {
        return Kind::Directive;
    }
    if is_license_header(body, start_line) {
        return Kind::LicenseHeader;
    }
    if is_doc_comment(raw, node, lang) {
        return Kind::DocComment;
    }
    Kind::Prose
}

const DIRECTIVE_MARKERS: &[&str] = &[
    "eslint-disable",
    "eslint-enable",
    "prettier-ignore",
    "biome-ignore",
    "oxlint-disable",
    "stylelint-disable",
    "@ts-ignore",
    "@ts-expect-error",
    "@ts-nocheck",
    "deno-lint-ignore",
    "istanbul ignore",
    "c8 ignore",
    "v8 ignore",
    "type: ignore",
    "noqa",
    "pylint:",
    "fmt: off",
    "fmt: on",
    "clang-format",
    "nolint",
    "go:build",
    "go:generate",
    "go:embed",
    "go:nosplit",
    "+build",
    "frozen_string_literal",
    "cspell:",
    "spell-checker:",
    "yaml-language-server:",
    "yamllint disable",
    "yamllint enable",
    "tflint-ignore",
    "tfsec:ignore",
    "checkov:skip",
    "kics-scan",
    "nosemgrep",
    "#region",
    "#endregion",
    "code generated by",
    "do not edit",
    "stupid-comments:",
];

fn is_directive(raw: &str, body: &str) -> bool {
    if raw.starts_with("#!") {
        return true;
    }
    let probe = body.trim().to_ascii_lowercase();
    DIRECTIVE_MARKERS.iter().any(|m| probe.starts_with(m) || probe.contains(m))
}

const LICENSE_MARKERS: &[&str] = &[
    "spdx-license-identifier",
    "copyright ",
    "all rights reserved",
    "licensed under",
    "gnu general public",
    "mit license",
    "apache license",
    "bsd license",
];

fn is_license_header(body: &str, start_line: usize) -> bool {
    let probe = body.to_ascii_lowercase();
    if probe.contains("spdx-license-identifier") {
        return true;
    }
    start_line <= 15 && LICENSE_MARKERS.iter().any(|m| probe.contains(m))
}

fn is_doc_comment(raw: &str, node: Node, lang: Lang) -> bool {
    if lang.doc_prefixes().iter().any(|p| raw.starts_with(p)) {
        return true;
    }
    // Go's doc convention is a plain line comment attached to a declaration.
    lang == Lang::Go && precedes_declaration(node, lang)
}

fn precedes_declaration(node: Node, lang: Lang) -> bool {
    let mut next = node.next_named_sibling();
    while let Some(n) = next {
        if n.kind().contains("comment") {
            next = n.next_named_sibling();
            continue;
        }
        return declaration_kinds(lang).contains(&n.kind());
    }
    false
}

fn declaration_kinds(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Go => &[
            "function_declaration",
            "method_declaration",
            "type_declaration",
            "var_declaration",
            "const_declaration",
        ],
        _ => &[],
    }
}

fn strip_markers(raw: &str) -> String {
    let mut out = Vec::new();
    for line in raw.lines() {
        let mut s = line.trim();
        for p in ["///", "//!", "//", "/**", "/*", "*/", "#"] {
            if let Some(rest) = s.strip_prefix(p) {
                s = rest.trim_start();
                break;
            }
        }
        if let Some(rest) = s.strip_prefix("* ") {
            s = rest;
        } else if s == "*" {
            s = "";
        }
        let s = s.trim_end().trim_end_matches("*/").trim_end();
        out.push(s.to_string());
    }
    out.join("\n").trim().to_string()
}

use crate::comments::{Comment, Kind};
use crate::rules::Finding;

const PRAGMA: &str = "stupid-comments:";
const REACH: usize = 3;

#[derive(Debug)]
enum Scope {
    File,
    Lines(usize, usize),
}

/// Suppressions count only if they already exist in HEAD. A pragma introduced
/// in the same change as the violation it silences is ignored, so the enforced
/// party cannot write its own exemption.
pub fn apply(findings: Vec<Finding>, comments: &[Comment], head: Option<&str>) -> Vec<Finding> {
    let scopes = collect(comments, head);
    if scopes.is_empty() {
        return findings;
    }
    findings
        .into_iter()
        .filter(|f| !scopes.iter().any(|s| covers(s, f)))
        .collect()
}

fn collect(comments: &[Comment], head: Option<&str>) -> Vec<Scope> {
    let Some(head) = head else { return Vec::new() };

    comments
        .iter()
        .filter(|c| c.kind == Kind::Directive)
        .filter_map(|c| {
            let body = c.body.trim();
            let rest = body.strip_prefix(PRAGMA)?.trim();
            let anchor = c.raw.lines().next()?.trim();
            if !head.contains(anchor) {
                return None;
            }
            Some(match rest {
                "ignore-file" => Scope::File,
                _ => Scope::Lines(c.end_line + 1, c.end_line + REACH),
            })
        })
        .collect()
}

fn covers(scope: &Scope, f: &Finding) -> bool {
    match scope {
        Scope::File => true,
        Scope::Lines(start, end) => f.start_line >= *start && f.start_line <= *end,
    }
}

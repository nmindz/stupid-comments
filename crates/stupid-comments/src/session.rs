use crate::comments::Kind;
use crate::rules::{Finding, Severity};
use std::collections::HashMap;
use std::path::PathBuf;

/// Remembers how much prose each file carried, so a file that goes bare right
/// after a block is visible. A gate measured only by violation count cannot
/// tell "learned taste" from "stopped writing comments".
pub struct Tracker {
    path: PathBuf,
    counts: HashMap<String, usize>,
}

impl Tracker {
    pub fn load(session_id: &str) -> Option<Self> {
        let dir = PathBuf::from(std::env::var("HOME").ok()?)
            .join(".cache/stupid-comments/sessions");
        let path = dir.join(format!("{}.json", sanitize(session_id)));
        let counts = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Some(Self { path, counts })
    }

    pub fn observe(&mut self, file: &str, current: usize) -> Option<Finding> {
        let previous = self.counts.insert(file.to_string(), current).unwrap_or(0);
        (previous > 0 && current == 0).then(|| Finding {
            file: file.to_string(),
            rule: "comments-removed",
            severity: Severity::Warn,
            kind: Kind::Prose,
            start_line: 1,
            end_line: 1,
            message: format!(
                "This file carried {previous} prose comment(s) earlier in the session and now has none. \
                 Stripping comments is not how you satisfy a comment policy — restore the ones that \
                 aided scanning and shorten the rest."
            ),
            excerpt: String::new(),
        })
    }

    pub fn save(&self) {
        let Some(dir) = self.path.parent() else { return };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        if let Ok(raw) = serde_json::to_string(&self.counts) {
            let _ = std::fs::write(&self.path, raw);
        }
    }
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .take(64)
        .collect()
}

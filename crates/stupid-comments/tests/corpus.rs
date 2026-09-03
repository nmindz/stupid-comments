use stupid_comments::lang::Lang;
use stupid_comments::policy::{Mode, Policy, Rules};
use stupid_comments::rules::Severity;
use stupid_comments::{analyze_source, policy::extract_section};

fn policy(banned: &[&str]) -> Policy {
    Policy {
        prose: "test policy".into(),
        source: "test".into(),
        rules: Rules {
            mode: Mode::Block,
            banned_patterns: banned.iter().map(|s| s.to_string()).collect(),
            ..Rules::default()
        },
    }
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR")))
        .expect("fixture readable")
}

#[test]
fn traps_produce_no_findings() {
    let p = policy(&[r"\bPRDs?[- ]?\d*\b"]);
    for (name, lang) in [("traps.ts", Lang::TypeScript), ("traps.go", Lang::Go)] {
        let findings = analyze_source(name, &fixture(name), lang, &p, false);
        assert!(
            findings.is_empty(),
            "false positives in {name}: {:#?}",
            findings
        );
    }
}

#[test]
fn banned_pattern_and_length_are_caught() {
    let p = policy(&[r"\bPRDs?[- ]?\d*\b"]);
    let findings = analyze_source("violations.ts", &fixture("violations.ts"), Lang::TypeScript, &p, false);
    let rules: Vec<&str> = findings.iter().map(|f| f.rule).collect();

    assert!(rules.contains(&"banned-pattern"), "got {rules:?}");
    assert!(rules.contains(&"prose-comment-too-long"), "got {rules:?}");
    assert!(findings.iter().all(|f| f.severity == Severity::Block));
}

#[test]
fn deletion_is_never_offered_as_a_remedy_in_guard_mode() {
    let p = policy(&[r"\bPRDs?[- ]?\d*\b"]);
    let findings = analyze_source("violations.ts", &fixture("violations.ts"), Lang::TypeScript, &p, false);
    assert!(!findings.is_empty());
    for f in &findings {
        assert!(
            f.message.contains("Deleting the comment is not compliance"),
            "guard mode must refuse deletion: {}",
            f.message
        );
    }
}

#[test]
fn adjudicate_mode_permits_removal() {
    let p = policy(&[r"\bPRDs?[- ]?\d*\b"]);
    let findings = analyze_source("violations.ts", &fixture("violations.ts"), Lang::TypeScript, &p, true);
    assert!(findings.iter().all(|f| f.message.contains("or remove it")));
}

#[test]
fn shadow_mode_never_blocks() {
    let mut p = policy(&[r"\bPRDs?[- ]?\d*\b"]);
    p.rules.mode = Mode::Shadow;
    let findings = analyze_source("violations.ts", &fixture("violations.ts"), Lang::TypeScript, &p, false);
    assert!(!findings.is_empty());
    assert!(findings.iter().all(|f| f.severity == Severity::Warn));
}

#[test]
fn directives_never_merge_into_the_prose_that_follows() {
    let src = "// eslint-disable-next-line no-console\n// one\n// two\n// three\n// four\n// five\n// six\nconsole.log(1);\n";
    let p = policy(&[]);
    let findings = analyze_source("x.ts", src, Lang::TypeScript, &p, false);
    assert!(
        findings.iter().any(|f| f.rule == "prose-comment-too-long"),
        "a lint pragma must not launder the block beneath it: {findings:#?}"
    );
}

#[test]
fn heading_is_matched_case_insensitively_at_any_level() {
    let md = "# Intro\n\n### comment policy\nBe brief.\n\n## Next\nother\n";
    assert_eq!(extract_section(md).as_deref(), Some("Be brief."));
    assert_eq!(extract_section("# Other\nnothing here\n"), None);
}

const SUPPRESSED: &str = "// stupid-comments: ignore\n// one\n// two\n// three\n// four\n// five\n// six\nexport const x = 1;\n";

#[test]
fn a_pragma_already_in_head_suppresses() {
    let p = policy(&[]);
    let findings = stupid_comments::analyze_source_with(
        "x.ts",
        SUPPRESSED,
        Lang::TypeScript,
        &p,
        false,
        Some(SUPPRESSED),
    );
    assert!(findings.is_empty(), "committed pragma must hold: {findings:#?}");
}

#[test]
fn a_pragma_written_in_this_change_does_not_suppress() {
    let p = policy(&[]);
    let head = "export const x = 1;\n";
    let findings = stupid_comments::analyze_source_with(
        "x.ts",
        SUPPRESSED,
        Lang::TypeScript,
        &p,
        false,
        Some(head),
    );
    assert!(
        findings.iter().any(|f| f.rule == "prose-comment-too-long"),
        "a self-written exemption must be ignored: {findings:#?}"
    );
}

#[test]
fn without_a_repo_no_pragma_is_honored() {
    let p = policy(&[]);
    let findings = analyze_source("x.ts", SUPPRESSED, Lang::TypeScript, &p, false);
    assert!(findings.iter().any(|f| f.rule == "prose-comment-too-long"));
}

#[test]
fn ignore_file_covers_everything_when_committed() {
    let src = "// stupid-comments: ignore-file\nexport const x = 1;\n// a\n// b\n// c\n// d\n// e\n// f\nconst y = 2;\n";
    let p = policy(&[]);
    let findings =
        stupid_comments::analyze_source_with("x.ts", src, Lang::TypeScript, &p, false, Some(src));
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn semantic_judging_is_off_unless_configured() {
    let p = policy(&[]);
    assert_eq!(p.rules.semantic, Mode::Shadow);
    let findings = analyze_source("violations.ts", &fixture("violations.ts"), Lang::TypeScript, &p, false);
    assert!(findings.iter().all(|f| f.rule != "semantic"));
}

#[test]
fn stripping_a_file_bare_raises_the_gaming_signal() {
    let dir = std::env::temp_dir().join(format!("sc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("HOME", &dir);

    let id = format!("session-{}", std::process::id());
    let mut tracker = stupid_comments::session::Tracker::load(&id).expect("tracker");
    assert!(tracker.observe("a.ts", 3).is_none(), "first sighting is a baseline");

    let signal = tracker.observe("a.ts", 0).expect("dropping to zero must be flagged");
    assert_eq!(signal.rule, "comments-removed");
    assert!(signal.message.contains("Stripping comments"));

    std::fs::remove_dir_all(&dir).ok();
}

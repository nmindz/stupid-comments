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
    for (name, lang) in [
        ("traps.ts", Lang::TypeScript),
        ("traps.go", Lang::Go),
        ("traps.yaml", Lang::Yaml),
        ("traps-templated.yaml", Lang::Yaml),
        ("traps.tf", Lang::Hcl),
        ("traps.sh", Lang::Shell),
        ("traps.mk", Lang::Make),
    ] {
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

#[test]
fn config_formats_resolve_from_their_extensions() {
    for (path, lang) in [
        ("deploy/stg/app.yaml", Lang::Yaml),
        ("ci/pipeline.yml", Lang::Yaml),
        ("infra/main.tf", Lang::Hcl),
        ("infra/stg.tfvars", Lang::Hcl),
        ("packer/build.hcl", Lang::Hcl),
        ("scripts/deploy.sh", Lang::Shell),
        ("scripts/lib.bash", Lang::Shell),
        ("Makefile", Lang::Make),
        ("GNUmakefile", Lang::Make),
        ("Makefile.local", Lang::Make),
        ("build/rules.mk", Lang::Make),
        ("tsconfig.json", Lang::Json),
        ("Cargo.toml", Lang::Toml),
    ] {
        assert_eq!(
            Lang::from_path(std::path::Path::new(path)),
            Some(lang),
            "{path} must resolve to {lang:?}"
        );
    }
}

#[test]
fn a_comment_smothered_manifest_is_caught() {
    let p = policy(&[r"\bPRDs?[- ]?\d*\b"]);
    let findings = analyze_source("violations.yaml", &fixture("violations.yaml"), Lang::Yaml, &p, false);
    let rules: Vec<&str> = findings.iter().map(|f| f.rule).collect();

    assert!(rules.contains(&"comment-ratio"), "got {rules:?}");
    assert!(rules.contains(&"prose-comment-too-long"), "got {rules:?}");
    assert!(rules.contains(&"banned-pattern"), "got {rules:?}");
}

/// The ratio rule first skipped config files outright, then measured them
/// against a looser threshold of their own. Both let a manifest carry a
/// comment load that would be flagged instantly in a .go or .ts file.
#[test]
fn config_files_answer_to_the_same_ratio_as_code() {
    // Separate blocks: minProseCommentsForRatio counts blocks, not lines.
    let yaml = "# alpha\na: 1\n# bravo\nb: 2\n# charlie\nc: 3\n# delta\nd: 4\n# echo\ne: 5\n";
    let code = "// alpha\nconst a = 1;\n// bravo\nconst b = 2;\n// charlie\nconst c = 3;\n// delta\nconst d = 4;\n// echo\nconst e = 5;\n";

    let mut p = policy(&[]);
    p.rules.max_comment_ratio = 0.35;

    for (name, src, lang) in [
        ("x.yaml", yaml, Lang::Yaml),
        ("x.ts", code, Lang::TypeScript),
    ] {
        let findings = analyze_source(name, src, lang, &p, false);
        assert!(
            findings.iter().any(|f| f.rule == "comment-ratio"),
            "{name} is half comments and must trip the ratio rule: {findings:#?}"
        );
    }

    p.rules.max_comment_ratio = 0.95;
    for (name, src, lang) in [
        ("x.yaml", yaml, Lang::Yaml),
        ("x.ts", code, Lang::TypeScript),
    ] {
        let findings = analyze_source(name, src, lang, &p, false);
        assert!(
            findings.iter().all(|f| f.rule != "comment-ratio"),
            "{name} must answer to maxCommentRatio, the single knob: {findings:#?}"
        );
    }
}

/// A file with no grammar must never be indistinguishable from a clean one.
#[test]
fn unparseable_files_are_reported_as_skipped_not_clean() {
    let dir = std::env::temp_dir().join(format!("sc-scan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.yaml"), "key: value\n").unwrap();
    std::fs::write(dir.join("notes.md"), "# heading\n").unwrap();

    let scan = stupid_comments::scan(&dir);
    assert!(scan.files.iter().any(|p| p.ends_with("app.yaml")), "{scan_files:?}", scan_files = scan.files);
    assert!(scan.skipped.iter().any(|p| p.ends_with("notes.md")), "{:?}", scan.skipped);

    let scan = stupid_comments::scan(&dir.join("notes.md"));
    assert!(scan.files.is_empty());
    assert_eq!(scan.skipped.len(), 1, "a named file with no grammar is skipped, not checked");

    std::fs::remove_dir_all(&dir).ok();
}

/// Helm templating collapses the YAML grammar to a single ERROR node, which
/// once made every templated manifest report clean.
#[test]
fn templated_config_is_recovered_by_line_scan() {
    let p = policy(&[]);
    let name = "violations-templated.yaml";
    let findings = analyze_source(name, &fixture(name), Lang::Yaml, &p, false);

    assert!(
        findings.iter().any(|f| f.rule == "prose-comment-too-long"),
        "a templated manifest must still be checked: {findings:#?}"
    );
    assert!(
        findings.iter().all(|f| f.severity == Severity::Warn),
        "line-scan recovery is less certain, so it must not block: {findings:#?}"
    );
}

/// A scripts/ directory is mostly extensionless, and skipping one silently is
/// indistinguishable from checking it and finding nothing.
#[test]
fn a_shebang_identifies_an_extensionless_script() {
    let dir = std::env::temp_dir().join(format!("sc-shebang-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let cases = [
        ("deploy", "#!/usr/bin/env bash\nset -e\n", Some(Lang::Shell)),
        ("posix", "#!/bin/sh\necho hi\n", Some(Lang::Shell)),
        ("flagged", "#!/bin/bash -e\necho hi\n", Some(Lang::Shell)),
        ("pyscript", "#!/usr/bin/env python3\nprint(1)\n", None),
        ("fishy", "#!/usr/bin/fish\necho hi\n", None),
        ("plain", "just text, no shebang\n", None),
    ];
    for (name, body, want) in cases {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        assert_eq!(Lang::from_file(&path), want, "{name}");
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Shell strings and heredoc bodies are data. Counting a `#` there would
/// invent violations in scripts that carry almost no commentary.
#[test]
fn shell_strings_and_heredocs_are_not_comments() {
    let mut p = policy(&[]);
    // Zero threshold: the rule fires on any prose at all, so the reported
    // count is what the test is actually reading.
    p.rules.max_comment_ratio = 0.0;
    p.rules.min_prose_comments_for_ratio = 1;

    // Each fixture carries exactly one real comment; every other `#` sits in a
    // string, a heredoc body, or a recipe, and must not be counted.
    for (name, lang) in [("traps.sh", Lang::Shell), ("traps.mk", Lang::Make)] {
        let findings = analyze_source(name, &fixture(name), lang, &p, false);
        let ratio = findings
            .iter()
            .find(|f| f.rule == "comment-ratio")
            .unwrap_or_else(|| panic!("{name}: expected a ratio finding: {findings:#?}"));

        let counted: usize = ratio
            .message
            .split_whitespace()
            .nth(3)
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("{name}: unparseable message {:?}", ratio.message));

        assert_eq!(
            counted, 1,
            "{name}: a `#` in a string, heredoc or recipe was counted as a comment: {:?}",
            ratio.message
        );
    }
}

/// An excluded file was folded into the "checked" count, which reports a file
/// the tool deliberately never opened as one it cleared.
#[test]
fn excluded_files_are_not_counted_as_checked() {
    use stupid_comments::excluded;
    let mut p = policy(&[]);
    p.rules.exclude = vec!["**/fixtures/**".into()];

    let inside = std::path::Path::new("crates/x/tests/fixtures/traps.yaml");
    let outside = std::path::Path::new("crates/x/src/lang.rs");

    assert!(excluded(inside, &p), "the glob must match the fixture");
    assert!(!excluded(outside, &p), "source must survive the glob");

    p.rules.exclude.clear();
    assert!(!excluded(inside, &p), "an empty exclude list matches nothing");
}

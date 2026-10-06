use stupid_comments::lang::Lang;
use stupid_comments::policy::{Mode, Policy, Rules};
use stupid_comments::rules::Severity;
use stupid_comments::{analyze_source, hook, policy::extract_section};

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

/// show_head stripped an absolute repo root off whatever path it was handed.
/// A relative path, a bare filename, or a path through a symlinked directory
/// never matched, so HEAD came back None and every pragma in the file was
/// silently dropped — including under `check .`, which is what the :fix
/// command runs.
#[test]
fn a_committed_pragma_survives_every_path_form() {
    let dir = std::env::temp_dir().join(format!("sc-vcs-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(dir.join("internal/deep")).unwrap();

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .current_dir(&dir)
            .args(args)
            .output()
            .expect("git runs")
    };
    git(&["init", "-q", "."]);
    git(&["config", "user.email", "t@t.t"]);
    git(&["config", "user.name", "t"]);

    std::fs::write(dir.join(".stupid-comments.jsonc"), "{ \"mode\": \"block\" }\n").unwrap();
    let long = "// one\n// two\n// three\n// four\n// five\n// six\nexport const x = 1;\n";
    std::fs::write(
        dir.join("internal/deep/pragma.ts"),
        format!("// stupid-comments: ignore\n{long}"),
    )
    .unwrap();
    std::fs::write(dir.join("internal/deep/plain.ts"), long).unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "init"]);

    let check = |arg: &str| -> String {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_stupid-comments"))
            .current_dir(&dir)
            .args(["check", arg])
            .output()
            .expect("binary runs");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    // Without this the test would pass on a run that checked nothing at all.
    assert!(
        check("internal/deep/plain.ts").contains("prose-comment-too-long"),
        "the control file must still be flagged"
    );

    let canonical = dir.canonicalize().unwrap();
    for arg in [
        "internal/deep/pragma.ts",
        "./internal/deep/pragma.ts",
        ".",
        canonical.join("internal/deep/pragma.ts").to_str().unwrap(),
    ] {
        let out = check(arg);
        assert!(
            !out.contains("pragma.ts"),
            "pragma dropped for {arg:?}: {out}"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

/// Isolate policy discovery from the machine running the suite. HOME is left
/// alone on purpose — another test owns it, and these overrides outrank it.
fn without_agent_homes(dir: &std::path::Path) {
    std::env::set_var("CLAUDE_CONFIG_DIR", dir.join("absent-claude"));
    std::env::set_var("DSH_HOME", dir.join("absent-dsh"));
    std::env::set_var("PI_CODING_AGENT_DIR", dir.join("absent-pi"));
    std::env::set_var("AGENTS_HOME", dir.join("absent-agents"));
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sc-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_project_agents_file_supplies_the_policy() {
    let dir = scratch("agents-md");
    without_agent_homes(&dir);
    std::fs::write(dir.join("AGENTS.md"), "# Comments Policy\n\nEarn the line.\n").unwrap();

    let resolved = stupid_comments::policy::resolve(&dir)
        .unwrap()
        .expect("AGENTS.md carries a policy");
    assert_eq!(resolved.prose, "Earn the line.");
    assert!(resolved.source.ends_with("AGENTS.md"), "{}", resolved.source);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_pi_agent_home_outranks_the_shared_one() {
    let dir = scratch("pi-home");
    let pi = dir.join("pi-agent");
    let shared = dir.join("agents");
    let project = dir.join("project");
    for d in [&pi, &shared, &project] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(pi.join("AGENTS.md"), "# Comments Policy\n\nPi's words.\n").unwrap();
    std::fs::write(shared.join("AGENTS.md"), "# Comments Policy\n\nShared words.\n").unwrap();

    // A subprocess keeps these overrides out of the tests sharing this process.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_stupid-comments"))
        .arg("policy")
        .current_dir(&project)
        .env("HOME", &dir)
        .env("CLAUDE_CONFIG_DIR", dir.join("absent-claude"))
        .env("DSH_HOME", dir.join("absent-dsh"))
        .env("PI_CODING_AGENT_DIR", &pi)
        .env("AGENTS_HOME", &shared)
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let source = pi.join("AGENTS.md");
    assert!(
        stdout.contains(&format!("source: {}", source.display())),
        "{stdout}"
    );
    assert!(stdout.contains("Pi's words."), "{stdout}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn either_harness_tool_casing_reaches_the_same_verdict() {
    let dir = scratch("tool-casing");
    without_agent_homes(&dir);
    std::fs::write(dir.join("AGENTS.md"), "# Comments Policy\n\nEarn the line.\n").unwrap();
    std::fs::write(
        dir.join(".stupid-comments.jsonc"),
        r#"{ "mode": "block", "bannedPatterns": ["obviously stupid"] }"#,
    )
    .unwrap();

    let file = dir.join("sample.ts");
    let content = "// this is an obviously stupid comment\nexport const x = 1;\n";

    // Claude Code sends `Write`, DSH sends `write`; the payload is otherwise identical.
    for tool in ["Write", "write"] {
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "cwd": dir.to_str().unwrap(),
            "tool_name": tool,
            "tool_input": { "file_path": file.to_str().unwrap(), "content": content },
        });
        let outcome = hook::run(&payload.to_string()).expect("the hook answers");
        assert!(outcome.block, "{tool} must be blocked");
        assert!(outcome.message.contains("banned-pattern"), "{}", outcome.message);
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_replace_all_edit_is_reconstructed_in_full() {
    let dir = scratch("replace-all");
    without_agent_homes(&dir);
    std::fs::write(dir.join("AGENTS.md"), "# Comments Policy\n\nEarn the line.\n").unwrap();
    std::fs::write(
        dir.join(".stupid-comments.jsonc"),
        r#"{ "mode": "block", "bannedPatterns": ["obviously stupid"] }"#,
    )
    .unwrap();

    let file = dir.join("sample.ts");
    std::fs::write(&file, "// marker\nexport const x = 1;\n// marker\nexport const y = 2;\n").unwrap();

    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "cwd": dir.to_str().unwrap(),
        "tool_name": "edit",
        "tool_input": {
            "file_path": file.to_str().unwrap(),
            "old_string": "// marker",
            "new_string": "// this is an obviously stupid comment",
            "replace_all": true,
        },
    });

    let outcome = hook::run(&payload.to_string()).expect("the hook answers");
    assert!(outcome.block, "{}", outcome.message);
    assert_eq!(
        outcome.message.matches("banned-pattern").count(),
        2,
        "every replaced occurrence is reported: {}",
        outcome.message
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_edit_answers_only_for_the_lines_it_rewrites() {
    let dir = scratch("edit-bounds");
    without_agent_homes(&dir);
    std::fs::write(dir.join("AGENTS.md"), "# Comments Policy\n\nEarn the line.\n").unwrap();
    std::fs::write(
        dir.join(".stupid-comments.jsonc"),
        r#"{ "mode": "block", "bannedPatterns": ["obviously stupid"] }"#,
    )
    .unwrap();

    let file = dir.join("sample.ts");
    let original = "// this is an obviously stupid comment\nexport const x = 1;\nexport const y = 2;\n// another obviously stupid comment\n";
    let edit = |old: &str, new: &str| {
        std::fs::write(&file, original).unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "cwd": dir.to_str().unwrap(),
            "tool_name": "edit",
            "tool_input": { "file_path": file.to_str().unwrap(), "old_string": old, "new_string": new },
        });
        hook::run(&payload.to_string()).expect("the hook answers")
    };

    // Each edit sits between two violations it did not write.
    for (old, new) in [
        ("export const x = 1;\n", "export const x = 3;\n"),
        ("export const y = 2;\n", "export const y = 4;\n"),
        ("export const x = 1;\n", ""),
    ] {
        let outcome = edit(old, new);
        assert!(!outcome.block, "{old:?} -> {new:?} was blamed for a neighbour: {}", outcome.message);
    }

    let outcome = edit("export const y = 2;\n", "// yet another obviously stupid remark\nexport const y = 2;\n");
    assert!(outcome.block, "a violation the edit did write is still caught");
    assert_eq!(outcome.message.matches("banned-pattern").count(), 1, "{}", outcome.message);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_later_edit_above_an_earlier_one_moves_its_lines() {
    let dir = scratch("edit-order");
    without_agent_homes(&dir);
    std::fs::write(dir.join("AGENTS.md"), "# Comments Policy\n\nEarn the line.\n").unwrap();
    std::fs::write(
        dir.join(".stupid-comments.jsonc"),
        r#"{ "mode": "block", "bannedPatterns": ["obviously stupid"] }"#,
    )
    .unwrap();

    let file = dir.join("sample.ts");
    std::fs::write(
        &file,
        "export const a = 1;\nexport const b = 2;\n// this is an obviously stupid comment\nexport const c = 3;\n",
    )
    .unwrap();

    // The second edit grows the file above the first, pushing the untouched
    // violation onto the line the first edit was recorded at.
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "cwd": dir.to_str().unwrap(),
        "tool_name": "MultiEdit",
        "tool_input": {
            "file_path": file.to_str().unwrap(),
            "edits": [
                { "old_string": "export const c = 3;", "new_string": "export const c = 4;" },
                { "old_string": "export const a = 1;\n", "new_string": "export const a = 1;\nexport const z = 0;\n" },
            ],
        },
    });
    let outcome = hook::run(&payload.to_string()).expect("the hook answers");
    assert!(!outcome.block, "an untouched neighbour was blamed: {}", outcome.message);

    // Sequential edits may rewrite what an earlier one wrote. Shrinking it must
    // not drag the earlier range onto the violation above.
    std::fs::write(
        &file,
        "export const a = 1;\n// this is an obviously stupid comment\nexport const b = 2;\nexport const c = 3;\n",
    )
    .unwrap();
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "cwd": dir.to_str().unwrap(),
        "tool_name": "MultiEdit",
        "tool_input": {
            "file_path": file.to_str().unwrap(),
            "edits": [
                { "old_string": "export const c = 3;\n", "new_string": "export const c = 3;\nexport const z = 0;\n" },
                {
                    "old_string": "export const b = 2;\nexport const c = 3;\nexport const z = 0;\n",
                    "new_string": "export const bcz = 5;\n",
                },
            ],
        },
    });
    let outcome = hook::run(&payload.to_string()).expect("the hook answers");
    assert!(!outcome.block, "a rewritten edit's range escaped upward: {}", outcome.message);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_blocked_write_sets_no_comment_baseline() {
    let dir = scratch("blocked-baseline");
    std::fs::write(dir.join("AGENTS.md"), "# Comments Policy\n\nEarn the line.\n").unwrap();
    std::fs::write(
        dir.join(".stupid-comments.jsonc"),
        r#"{ "mode": "block", "bannedPatterns": ["obviously stupid"] }"#,
    )
    .unwrap();
    let file = dir.join("sample.ts");

    // A subprocess keeps the session tracker's HOME away from the other tests.
    let hook = |content: &str| {
        let payload = serde_json::json!({
            "session_id": "baseline",
            "hook_event_name": "PreToolUse",
            "cwd": dir.to_str().unwrap(),
            "tool_name": "Write",
            "tool_input": { "file_path": file.to_str().unwrap(), "content": content },
        });
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_stupid-comments"))
            .args(["hook", "claude"])
            .env("HOME", &dir)
            .env("CLAUDE_CONFIG_DIR", dir.join("absent"))
            .env("DSH_HOME", dir.join("absent"))
            .env("PI_CODING_AGENT_DIR", dir.join("absent"))
            .env("AGENTS_HOME", dir.join("absent"))
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("binary runs");
        use std::io::Write;
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        String::from_utf8_lossy(&child.wait_with_output().unwrap().stderr).into_owned()
    };

    assert!(hook("// this is an obviously stupid comment\nexport const x = 1;\n").contains("banned-pattern"));
    // The blocked write never landed, so writing the file bare strips nothing.
    let second = hook("export const x = 1;\n");
    assert!(!second.contains("comments-removed"), "{second}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn deleting_whole_lines_blames_the_line_below_for_nothing() {
    let dir = scratch("line-delete");
    without_agent_homes(&dir);
    std::fs::write(dir.join("AGENTS.md"), "# Comments Policy\n\nEarn the line.\n").unwrap();
    std::fs::write(
        dir.join(".stupid-comments.jsonc"),
        r#"{ "mode": "block", "bannedPatterns": ["obviously stupid"] }"#,
    )
    .unwrap();

    let file = dir.join("sample.ts");
    let original = "export const a = 1;\nexport const b = 2;\n// this is an obviously stupid comment\nexport const c = 3;\n";
    let check = |edits: serde_json::Value| {
        std::fs::write(&file, original).unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "cwd": dir.to_str().unwrap(),
            "tool_name": "MultiEdit",
            "tool_input": { "file_path": file.to_str().unwrap(), "edits": edits },
        });
        hook::run(&payload.to_string()).expect("the hook answers")
    };

    let outcome = check(serde_json::json!([
        { "old_string": "export const b = 2;\n", "new_string": "" },
    ]));
    assert!(!outcome.block, "the line sliding up was blamed: {}", outcome.message);

    // An earlier edit's line deleted by a later one leaves no range behind.
    let outcome = check(serde_json::json!([
        { "old_string": "export const b = 2;", "new_string": "export const b = 3;" },
        { "old_string": "export const b = 3;\n", "new_string": "" },
    ]));
    assert!(!outcome.block, "a deleted edit's range outlived its line: {}", outcome.message);

    std::fs::remove_dir_all(&dir).ok();
}

/// Runs `stupid-comments policy` in `project` under `env`, with every agent
/// home not named in it pointed somewhere absent.
fn policy_source(dir: &std::path::Path, project: &std::path::Path, env: &[(&str, &std::path::Path)]) -> String {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_stupid-comments"));
    command.arg("policy").current_dir(project).env("HOME", dir);
    for var in ["CLAUDE_CONFIG_DIR", "DSH_HOME", "PI_CODING_AGENT_DIR", "AGENTS_HOME"] {
        command.env(var, dir.join(format!("absent-{var}")));
    }
    for (var, value) in env {
        command.env(var, value);
    }
    let out = command.output().expect("binary runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_pi_agent_home_answers_with_the_file_pi_loads() {
    let dir = scratch("pi-override");
    let pi = dir.join("pi-agent");
    let project = dir.join("project");
    std::fs::create_dir_all(&pi).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    std::fs::write(pi.join("CLAUDE.md"), "# Comments Policy\n\nFrom CLAUDE.md.\n").unwrap();
    let out = policy_source(&dir, &project, &[("PI_CODING_AGENT_DIR", &pi)]);
    assert!(out.contains("From CLAUDE.md."), "Pi falls back to CLAUDE.md: {out}");

    // Pi loads the override instead of the rest, so its policy is the one in force.
    std::fs::write(pi.join("AGENTS.override.md"), "# Comments Policy\n\nFrom the override.\n").unwrap();
    let out = policy_source(&dir, &project, &[("PI_CODING_AGENT_DIR", &pi)]);
    assert!(out.contains("From the override."), "{out}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_omp_agent_home_supplies_the_policy() {
    let dir = scratch("omp-home");
    let omp = dir.join(".omp/agent");
    let project = dir.join("project");
    std::fs::create_dir_all(&omp).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(omp.join("AGENTS.md"), "# Comments Policy\n\nFrom omp.\n").unwrap();

    // omp shares Pi's override variable; unset, it lives under ~/.omp/agent.
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_stupid-comments"));
    command.arg("policy").current_dir(&project).env("HOME", &dir).env_remove("PI_CODING_AGENT_DIR");
    for var in ["CLAUDE_CONFIG_DIR", "DSH_HOME", "AGENTS_HOME"] {
        command.env(var, dir.join(format!("absent-{var}")));
    }
    let out = String::from_utf8_lossy(&command.output().expect("binary runs").stdout).into_owned();
    assert!(out.contains("From omp."), "{out}");
    assert!(out.contains(&omp.join("AGENTS.md").display().to_string()), "{out}");

    std::fs::remove_dir_all(&dir).ok();
}

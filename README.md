# stupid-comments

**Runtime enforcement for your code comment policy.** A Rust CLI that parses what an LLM is about to write, checks it against *your* policy, and refuses the write when it violates.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.1.4-green.svg)](https://github.com/nmindz/stupid-comments/releases)
[![Rust](https://img.shields.io/badge/rust-2021%20edition-orange.svg)](https://www.rust-lang.org)

---

## The problem

Every model tier, at every reasoning level, eventually forgets your comment policy and starts writing `// Increment the counter` above `counter++`. That is an attention problem, and no amount of restating the rule in `CLAUDE.md` fixes it — the instruction is simply too far back in the context by the time the code gets written.

So this moves enforcement out of the prompt and into the runtime, re-injecting your policy text verbatim at the exact moment it matters.

The policy is never baked in. It is yours, it is prose, and it lives where you already keep it.

## Quick start

```sh
git clone https://github.com/nmindz/stupid-comments && cd stupid-comments
make install
```

Then, inside Claude Code:

```
/plugin marketplace add nmindz/stupid-comments
/plugin install stupid-comments@stupid-comments
```

Finally, add a `# Comments Policy` section to `~/.claude/CLAUDE.md` in your own words, and confirm it was picked up:

```sh
stupid-comments policy
```

Without that section and without a config file, the plugin stays completely silent. There is no default policy, because a default policy would be someone else's taste.

## Table of contents

- [How it works](#how-it-works)
- [Installation](#installation)
- [AI agent instructions](#ai-agent-instructions)
- [Configuration](#configuration)
- [Rules](#rules)
- [CLI usage](#cli-usage)
- [Slash commands](#slash-commands)
- [Semantic judging](#semantic-judging)
- [Escaping it](#escaping-it)
- [Detecting evasion](#detecting-evasion)
- [Languages](#languages)
- [Development](#development)
- [Known limits](#known-limits)
- [Contributing](#contributing)
- [License](#license)

## How it works

Your policy is read from the `# Comments Policy` section of `~/.claude/CLAUDE.md` (any heading level, case-insensitive). That text is quoted verbatim in every rejection, never paraphrased. If no such section and no config file exist, the plugin does nothing at all and says nothing at all.

**Enforcement is layered.** `PreToolUse` catches Write/Edit/MultiEdit early, reconstructing the post-edit file in memory so rules see whole-file context while reporting only the lines the edit introduced. `Stop` and `SubagentStop` are the real guarantee: they diff the working tree and analyze added lines only, which makes them indifferent to *how* the file was written — heredoc, `sed`, or a subagent all land in the same net.

**Nothing is judged until it is classified.** Every comment is sorted into `directive`, `license-header`, `doc-comment`, or `prose`, and only `prose` faces the ratio and redundancy rules. Lint pragmas, `go:build` lines, shebangs, SPDX headers, and JSDoc are structurally exempt rather than merely tolerated — and a pragma placed above a comment block never launders the block beneath it.

**Deletion is not compliance.** A gate you can satisfy by removing the comment trains the model to write none at all, which inverts a policy that asks for *just enough* commenting. So findings name the offending span, demand a rewrite, and say outright that removing it does not count. Bulk deletion is legitimate on a legacy codebase, but only under `/stupid-comments:fix`, where a human is present and there is nothing to game.

## Installation

Two pieces, installed separately and on purpose. The plugin never downloads or executes anything on your behalf — a marketplace plugin that silently fetches a remote binary is exactly the supply-chain pattern worth distrusting.

Requires a Rust toolchain. Get one from <https://rustup.rs> if you have none.

### 1. The CLI

**From a clone (recommended):**

```sh
make install                        # installs to ~/.local/bin
make install ROOT=$HOME/.cargo      # or wherever your PATH points
```

`make install` runs the cargo command below, then reports what `command -v` actually resolves to and its version.

**With cargo directly:**

```sh
# from a clone
cargo install --path crates/stupid-comments --root ~/.local --force

# or straight from git, without cloning
cargo install --root ~/.local --git https://github.com/nmindz/stupid-comments stupid-comments
```

Drop `--root ~/.local` to use cargo's own default of `~/.cargo/bin`.

> [!IMPORTANT]
> Pick whichever directory is already on your `PATH`. The plugin decides whether to enforce by looking the binary up on `PATH`, so installing somewhere the shell cannot resolve leaves enforcement **permanently inert**. Confirm with `command -v stupid-comments`, not by checking that the file exists.

Verify with `stupid-comments --version`.

### 2. The plugin

Inside Claude Code:

```
/plugin marketplace add nmindz/stupid-comments
/plugin install stupid-comments@stupid-comments
```

Restart the session so the hooks register. If a policy exists but the CLI is missing, the plugin says so once at session start and enforces nothing.

To upgrade later, both pieces move independently:

```sh
make install
claude plugin marketplace update stupid-comments
claude plugin update stupid-comments@stupid-comments
```

### 3. A policy

Add a `# Comments Policy` section to `~/.claude/CLAUDE.md` describing, in your own words, how you want comments written. Confirm it was picked up with `stupid-comments policy`.

To keep the policy somewhere else, point at it with the `prose` config key.

### Verify

```sh
stupid-comments check path/to/your/code
```

The CLI stands alone, so the same command works as a pre-commit hook or a CI step with `--json`.

## AI agent instructions

Paste this into a Claude Code session and it will do the setup for you:

```text
Set up the stupid-comments comment policy enforcer on this machine.

1. If `command -v stupid-comments` already resolves, it is installed and
   reachable — skip straight to step 4.

2. Pick the install root by checking my PATH FIRST. Never install into a
   directory PATH cannot resolve:
     - if ~/.local/bin is in $PATH  -> cargo install --root ~/.local --git \
         https://github.com/nmindz/stupid-comments stupid-comments
     - else if ~/.cargo/bin is in $PATH -> same command without --root
     - else STOP. Do not install. Tell me which directories cargo can target
       and ask which one I want, or give me the export line to add to my
       shell profile first.
   Check with: case ":$PATH:" in *":$HOME/.local/bin:"*) ...
   If cargo itself is missing, point me at https://rustup.rs and stop there.

3. Verify by running `command -v stupid-comments` and `stupid-comments
   --version`. If `command -v` does not resolve, the binary went somewhere
   PATH cannot see it — say so plainly instead of reporting success.

4. Tell me to run these two myself, since you cannot run slash commands:
   /plugin marketplace add nmindz/stupid-comments
   /plugin install stupid-comments@stupid-comments

5. Read ~/.claude/CLAUDE.md and look for a heading matching "Comments Policy"
   at any level, case-insensitive. If it is missing, DO NOT invent a policy.
   Show me where the section goes, ask what my rules are, and write exactly
   what I tell you.

6. Run `stupid-comments policy` and show me the resolved source, mode and rules.

7. Explain that mode defaults to `shadow` — findings reported, nothing blocked —
   and that I should stay there until the reports look right before adding a
   .stupid-comments.jsonc with "mode": "block".

8. Do not enable the `semantic` option. Tell me it exists, that it spends a
   `claude -p` call per checked file, and that turning it on is my call.
```

## Configuration

Everything here is optional. Drop a `.stupid-comments.jsonc` anywhere at or above the file being checked; the nearest one upward wins.

```jsonc
{
  "mode": "block",                    // shadow (default) | warn | block
  "bannedPatterns": ["\\bPRDs?[- ]?\\d*\\b"],
  "maxProseCommentLines": 5,
  "maxDocCommentLines": 40,
  "maxCommentRatio": 0.35,
  "minProseCommentsForRatio": 4,
  "redundancy": "warn",
  "semantic": "shadow",               // shadow (default) | warn | block
  "exclude": ["**/generated/**"]
}
```

| Key | Type | Default | Purpose |
| --- | --- | --- | --- |
| `mode` | `shadow` \| `warn` \| `block` | `shadow` | Global severity ceiling. `shadow` reports without blocking |
| `prose` | path | — | Read the policy from this file instead of `CLAUDE.md`. `~` expands |
| `maxProseCommentLines` | integer | `5` | Longest permitted prose comment block |
| `maxDocCommentLines` | integer | `40` | Longest permitted doc comment |
| `maxCommentRatio` | float | `0.35` | Share of a file that may be prose comments |
| `minProseCommentsForRatio` | integer | `4` | Comment *blocks* required before the ratio rule applies |
| `bannedPatterns` | regex list | empty | Text that may never appear in a comment |
| `redundancy` | `shadow` \| `warn` \| `block` | `warn` | Comments that restate the line below them |
| `semantic` | `shadow` \| `warn` \| `block` | `shadow` | LLM taste judgement. See [Semantic judging](#semantic-judging) |
| `semanticCommand` | string list | `["claude", "-p"]` | Command the semantic judge shells out to |
| `exclude` | glob list | empty | Paths to skip entirely |

These are *calibration*, not policy. The defaults are deliberately loose, because a threshold tight enough to be opinionated would be smuggling in someone else's taste.

> [!TIP]
> `mode` defaults to `shadow`: findings are reported, nothing is blocked. Stay there until the log convinces you the blocks would have been right, then switch to `block`.

An unparseable config is an error, not silence — `check` and `policy` print the reason and exit non-zero. Only the hook still fails open, since an unreadable config must never block a write.

## Rules

| Rule | Default severity | Fires when |
| --- | --- | --- |
| `banned-pattern` | block | A comment matches one of your `bannedPatterns` |
| `prose-comment-too-long` | block | A prose block exceeds `maxProseCommentLines` |
| `doc-comment-too-long` | warn | A doc comment exceeds `maxDocCommentLines` |
| `comment-ratio` | block | Prose comments cover more than `maxCommentRatio` of the file |
| `redundant-comment` | warn | A comment restates the code directly below it |
| `semantic` | warn | The judge decides a comment has not earned its place |
| `comments-removed` | warn | A file that had prose comments now has none |

Severities are ceilings, not floors: under `"mode": "shadow"` or `"warn"` every finding is downgraded, and findings recovered from a file whose grammar failed are always warn-only.

## CLI usage

```sh
stupid-comments check [PATH]...     # report findings, change nothing
stupid-comments check --json        # machine-readable, for CI
stupid-comments check --adjudicate  # permit deletion as a remedy
stupid-comments policy              # show the resolved policy and its source
stupid-comments hook claude         # consume a hook payload on stdin
```

Every run prints a coverage summary to **stderr**, leaving stdout clean for `--json`:

```
Checked 17 files (rust 12, json 2, toml 2, make 1).
Not checked — no grammar for 8 files: .md 5, .gitignore 1, .lock 1, LICENSE 1
Not checked — excluded by config: 10 files
```

A file with no grammar is not a passing file, so it is never folded into the checked count. The numbers add up on purpose.

| Exit code | Meaning |
| --- | --- |
| `0` | No blocking findings |
| `1` | A blocking finding, an unreadable config, or a path that does not exist |
| `2` | Hook only: block the pending write |

## Slash commands

| Command | Purpose |
| --- | --- |
| `/stupid-comments:policy` | Show the policy in force and where it came from |
| `/stupid-comments:check [path]` | Report findings, change nothing |
| `/stupid-comments:fix [path]` | Adjudicated sweep of an existing codebase; deletion permitted |
| `/stupid-comments:off` | How to disarm for a session |

## Semantic judging

Deterministic rules cannot decide whether a comment earns its place. Setting `"semantic": "warn"` (or `"block"`) sends the prose comments and your policy text to `claude -p`, using the session authentication you already have — there is no API key to configure and none is wanted. Every failure is silent: no `claude` on PATH, a timeout, unparseable output, all mean no findings.

It is off by default because it spends a model call per checked file. It is also the only rule that catches `// Adds a and b` sitting above `const sum = a + b`, which is probably the comment that made you look for this tool.

## Escaping it

Set `STUPID_COMMENTS=0` in the session environment. That is deliberately the only mid-session hatch — it lives somewhere the model cannot write, so the enforced party cannot disable its own gate.

Suppression pragmas exist, but they are anchored to git:

```ts
// stupid-comments: ignore        -> suppresses findings on the next 3 lines
// stupid-comments: ignore-file   -> suppresses the whole file
```

> [!NOTE]
> A pragma is honored **only if the identical line already exists in `HEAD`**. One introduced in the same change as the violation it silences is ignored entirely, so the model cannot write its own exemption. Outside a git repository no pragma is honored.

## Detecting evasion

A gate that counts only violations cannot tell "learned taste" from "stopped writing comments". Prose-comment counts are tracked per file for the session, and a file that had comments and now has none raises a `comments-removed` warning naming what was lost. Bulk removal is legitimate, but only under `/stupid-comments:fix`, where a human asked for it.

## Languages

JavaScript, TypeScript, TSX/JSX, Rust, Go, Kotlin, JSON/JSONC/JSON5, TOML, YAML, HCL/Terraform, shell (sh/bash/zsh/ksh), and Make, via native tree-sitter grammars.

**Not every file carries its language in its extension.** `Makefile`, `GNUmakefile`, `Makefile.*` and `*.mk` are matched by name, as are the usual shell rc files, and an extensionless file is checked for a shell shebang — a `scripts/` directory is mostly extensionless, and skipping one silently is indistinguishable from checking it and finding nothing. `#!/usr/bin/env bash` counts; `#!/usr/bin/env python3` does not, and neither does `fish`.

**A `#` inside a shell string, a heredoc body, or a Make recipe is data, not commentary.** Telling those apart is the whole reason this uses grammars rather than a regex over lines starting with `#`.

**Config formats answer to exactly the same rules as code**, `maxCommentRatio` included. They have twice been given something gentler — first an outright exemption from the ratio rule, then a looser threshold of their own — and both times the result was a manifest sitting at a comment load that would be flagged on sight in a `.go` file. A YAML at 43% comments is a YAML at 43% comments; there is no version of "just enough" that reads differently because the file ends in `.yaml`.

**Templating defeats the YAML grammar.** A Helm chart parses to a single error node with no comments in it, which would make every templated manifest in a repository look clean. When the grammar fails on a `#`-comment format, comments are recovered by a line scan instead — whole-line comments only, block scalars left alone, so the failure direction is a missed comment rather than an invented one.

Failure is otherwise open. Parse error, missing binary, unreadable config — inside the hook all of them mean *no findings*, never a blocked write.

## Development

Requires a Rust toolchain. `make help` lists every target.

| Make | Cargo equivalent | Purpose |
| --- | --- | --- |
| `make build` | `cargo build --release` | Compile the release binary |
| `make test` | `cargo test` | Run the test suite |
| `make lint` | `cargo clippy --all-targets` | Lint every target |
| `make validate` | `claude plugin validate plugins/stupid-comments` | Check the plugin manifests |
| `make check` | all three of the above | Everything CI would run |
| `make install` | `cargo install --path crates/stupid-comments --root ~/.local --force` | Install the binary |
| `make uninstall` | `cargo uninstall --root ~/.local stupid-comments` | Remove it |
| `make selfcheck` | `./target/release/stupid-comments check .` | Enforce this repo's policy on itself |
| `make clean` | `cargo clean` | Remove build artifacts |

`make selfcheck` is the one that matters: the enforcer answers to its own policy, and a change that makes this repo fail its own gate is not ready.

```
crates/stupid-comments/src/
├── lang.rs        # language detection and grammar bindings
├── comments.rs    # extraction and classification
├── rules.rs       # the deterministic rules
├── semantic.rs    # the opt-in LLM judge
├── policy.rs      # config and policy resolution
├── hook.rs        # Claude Code hook payloads
├── suppress.rs    # git-anchored pragmas
├── session.rs     # cross-turn evasion tracking
└── main.rs        # CLI
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the commit convention and a walkthrough of adding a language.

## Known limits

- Redundancy detection is warn-only. It is the most false-positive-prone rule here and has not earned blocking authority.
- Kotlin findings are warn-only while its grammar earns trust, as are findings recovered by line scan from a templated config file.
- The line-scan fallback reads whole-line comments only, so a trailing `# comment` on a value line goes unchecked in a templated file.
- Python has no grammar yet, so `.py` files are named as unchecked rather than checked.
- `minProseCommentsForRatio` counts comment *blocks*, not lines, so a file carrying fewer than four separate blocks never trips the ratio rule however much of the file they cover. Long blocks are caught by the length rule instead.
- Semantic judging costs a model call per checked file, so it is off by default.
- The `Stop` gate diffs against `HEAD`, so a tree that was already dirty before the session has those earlier changes considered too.

## Contributing

Bug reports and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

# stupid-comments

Every model tier, at every reasoning level, eventually forgets your comment policy and starts writing `// Increment the counter` above `counter++`. That is an attention problem, and no amount of restating the rule in `CLAUDE.md` fixes it — the instruction is simply too far back in the context by the time the code gets written.

So this moves enforcement out of the prompt and into the runtime. A Rust CLI parses what the model is about to write, checks it against **your** policy, and refuses the write when it violates — re-injecting your policy text verbatim at the exact moment it matters.

The policy is never baked in. It is yours, it is prose, and it lives where you already keep it.

## How it works

Your policy is read from the `# Comments Policy` section of `~/.claude/CLAUDE.md` (any heading level, case-insensitive). That text is quoted verbatim in every rejection, never paraphrased. If no such section and no config file exist, the plugin does nothing at all and says nothing at all.

Enforcement is layered. `PreToolUse` catches Write/Edit/MultiEdit early, reconstructing the post-edit file in memory so rules see whole-file context while reporting only the lines the edit introduced. `Stop` and `SubagentStop` are the real guarantee: they diff the working tree and analyze added lines only, which makes them indifferent to *how* the file was written — heredoc, `sed`, or a subagent all land in the same net.

Nothing is judged until it is classified. Every comment is sorted into `directive`, `license-header`, `doc-comment`, or `prose`, and only `prose` faces the length, ratio, and redundancy rules. Lint pragmas, `go:build` lines, shebangs, SPDX headers, and JSDoc are structurally exempt rather than merely tolerated — and a pragma placed above a comment block never launders the block beneath it.

Deletion is not compliance. A gate you can satisfy by removing the comment trains the model to write none at all, which inverts a policy that asks for *just enough* commenting. So findings name the offending span, demand a rewrite, and say outright that removing it does not count. Bulk deletion is legitimate on a legacy codebase, but only under `/stupid-comments:fix`, where a human is present and there is nothing to game.

## Install

Two pieces, installed separately and on purpose. The plugin never downloads or executes anything on your behalf — a marketplace plugin that silently fetches a remote binary is exactly the supply-chain pattern worth distrusting.

### 1. The CLI

```sh
cargo install --git https://github.com/nmindz/stupid-comments stupid-comments
```

Needs a Rust toolchain; get one from <https://rustup.rs> if you have none. Verify with `stupid-comments --version`, and make sure `~/.cargo/bin` is on your `PATH`.

### 2. The plugin

Inside Claude Code:

```
/plugin marketplace add nmindz/stupid-comments
/plugin install stupid-comments@stupid-comments
```

Then restart the session so the hooks register. If a policy exists but the CLI is missing, the plugin says so once at session start and enforces nothing.

### 3. A policy

Add a `# Comments Policy` section to `~/.claude/CLAUDE.md` describing, in your own words, how you want comments written. Confirm it was picked up with `stupid-comments policy`.

Without that section and without a config file the plugin stays completely silent. There is no default policy, because a default policy would be someone else's taste.

### Verify

```sh
stupid-comments check path/to/your/code
```

The CLI stands alone, so the same command works as a pre-commit hook or a CI step with `--json`.

## AI Agent Instructions

Paste this into a Claude Code session and it will do the setup for you:

```text
Set up the stupid-comments comment policy enforcer on this machine.

1. Check whether `stupid-comments` is already on PATH. If not, install it with
   `cargo install --git https://github.com/nmindz/stupid-comments stupid-comments`.
   If cargo is missing, point me at https://rustup.rs and stop there.
   Confirm success with `stupid-comments --version`.

2. Tell me to run these two myself, since you cannot run slash commands:
   /plugin marketplace add nmindz/stupid-comments
   /plugin install stupid-comments@stupid-comments

3. Read ~/.claude/CLAUDE.md and look for a heading matching "Comments Policy"
   at any level, case-insensitive. If it is missing, DO NOT invent a policy.
   Show me where the section goes, ask what my rules are, and write exactly
   what I tell you.

4. Run `stupid-comments policy` and show me the resolved source, mode and rules.

5. Explain that mode defaults to `shadow` — findings reported, nothing blocked —
   and that I should stay there until the reports look right before adding a
   .stupid-comments.jsonc with "mode": "block".

6. Do not enable the `semantic` option. Tell me it exists, that it spends a
   `claude -p` call per checked file, and that turning it on is my call.
```

## Configure

Everything below is optional. Drop a `.stupid-comments.jsonc` anywhere at or above the file being checked:

```jsonc
{
  "mode": "block",              // shadow (default) | warn | block
  "bannedPatterns": ["\\bPRDs?[- ]?\\d*\\b"],
  "maxProseCommentLines": 5,
  "maxCommentRatio": 0.35,
  "minProseCommentsForRatio": 4,
  "maxDocCommentLines": 40,
  "redundancy": "warn",
  "semantic": "off",           // off (default) | warn | block
  "exclude": ["**/generated/**"]
}
```

These are *calibration*, not policy. The defaults are deliberately loose, because a threshold tight enough to be opinionated would be smuggling in someone else's taste.

**`mode` defaults to `shadow`**: findings are reported, nothing is blocked. Run that way until the log convinces you the blocks would have been right, then switch to `block`.

## Commands

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

A pragma is honored **only if the identical line already exists in `HEAD`**. One introduced in the same change as the violation it silences is ignored entirely, so the model cannot write its own exemption. Outside a git repository no pragma is honored.

## Detecting evasion

A gate that counts only violations cannot tell "learned taste" from "stopped writing comments". Prose-comment counts are tracked per file for the session, and a file that had comments and now has none raises a `comments-removed` warning naming what was lost. Bulk removal is legitimate, but only under `/stupid-comments:fix`, where a human asked for it.

## Languages

JavaScript, TypeScript, TSX/JSX, Rust, Go, Kotlin, JSON/JSONC, and TOML, via native tree-sitter grammars. Anything else is ignored silently. JSON and TOML get banned-pattern and length checks only — ratio and redundancy are meaningless in a config file. Kotlin findings are warn-only for now while its grammar earns trust.

Failure is always open. Unknown language, parse error, missing binary, unreadable config — all of them mean *no findings*, never a blocked write.

## Known limits

- Redundancy detection is warn-only. It is the most false-positive-prone rule here and has not earned blocking authority.
- Kotlin findings are warn-only while its grammar earns trust.
- Semantic judging costs a model call per checked file, so it is off by default.
- The `Stop` gate diffs against `HEAD`, so a tree that was already dirty before the session has those earlier changes considered too.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

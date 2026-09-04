# Contributing

Thanks for considering a contribution. This document covers the setup, the commit convention, and the one rule that is specific to this project: **the enforcer answers to its own policy.**

## Table of contents

- [Getting started](#getting-started)
- [The self-enforcement rule](#the-self-enforcement-rule)
- [Commit convention](#commit-convention)
- [Pull requests](#pull-requests)
- [Testing](#testing)
- [Adding a language](#adding-a-language)
- [Design principles](#design-principles)
- [Releasing](#releasing)
- [License](#license)

## Getting started

You need a Rust toolchain. Get one from <https://rustup.rs>.

```sh
git clone https://github.com/nmindz/stupid-comments && cd stupid-comments
make check          # test + lint + manifest validation
```

Every target has a bare cargo equivalent if you would rather not use make:

| Make | Cargo equivalent |
| --- | --- |
| `make build` | `cargo build --release` |
| `make test` | `cargo test` |
| `make lint` | `cargo clippy --all-targets` |
| `make validate` | `claude plugin validate plugins/stupid-comments` |
| `make check` | the three above, in order |
| `make install` | `cargo install --path crates/stupid-comments --root ~/.local --force` |
| `make uninstall` | `cargo uninstall --root ~/.local stupid-comments` |
| `make selfcheck` | `./target/release/stupid-comments check .` |
| `make clean` | `cargo clean` |

`make help` lists them at any time. `make install ROOT=$HOME/.cargo` changes the destination.

### Layout

```
crates/stupid-comments/
├── src/
│   ├── lang.rs        # language detection and grammar bindings
│   ├── comments.rs    # extraction and classification
│   ├── rules.rs       # the deterministic rules
│   ├── semantic.rs    # the opt-in LLM judge
│   ├── policy.rs      # config and policy resolution
│   ├── hook.rs        # Claude Code hook payloads
│   ├── suppress.rs    # git-anchored pragmas
│   ├── session.rs     # cross-turn evasion tracking
│   ├── vcs.rs         # git plumbing
│   ├── lib.rs         # file walking and the analysis entry points
│   └── main.rs        # CLI
└── tests/
    ├── corpus.rs      # the whole suite
    └── fixtures/      # traps.* and violations.*
plugins/stupid-comments/   # the Claude Code plugin: hooks and slash commands
```

## The self-enforcement rule

```sh
make selfcheck
```

This runs the freshly built binary against this repository. It must exit `0` before you open a pull request. A tool that enforces a comment policy while violating one is not worth installing, and the repo's own `.stupid-comments.jsonc` is set to `"mode": "block"` for exactly that reason.

`tests/fixtures/**` is excluded, because those files exist to violate the policy on purpose. Nothing else is exempt.

Follow the project's comment policy in code you write: comments should be brief and few, earning their place by making the file easier to scan. Long explanations belong in documentation — this file, the README, or a doc comment — not inline.

## Commit convention

This project uses [Conventional Commits](https://www.conventionalcommits.org/).

```
<type>(<scope>): <description>

<body>

<footer>
```

**Types in use:**

| Type | For |
| --- | --- |
| `feat` | A new capability, such as a language or a rule |
| `fix` | A bug fix |
| `docs` | Documentation only |
| `test` | Tests only |
| `refactor` | A change that alters neither behaviour nor interface |
| `perf` | A performance change |
| `build` | Build system, Makefile, dependencies |
| `chore` | Releases and housekeeping |

**Scopes in use:** `lang`, `rules`, `comments`, `policy`, `cli`, `hook`, `plugin`, `release`. Omit the scope when a change genuinely spans the codebase.

Write the description in the imperative mood, lowercase, with no trailing period:

```
feat(lang): check shell scripts and Makefiles
fix(rules): hold config files to the same comment ratio as code
chore(release): 0.1.1
```

**Breaking changes** get a `BREAKING CHANGE:` footer describing what breaks and what to do about it. Removing a config key is breaking: `deny_unknown_fields` means a stale key now fails the config parse.

### Write bodies that explain why

The subject says what changed; the body says why it needed to. Wrap it at 72 columns. If a change fixes something subtle, say what the old behaviour was and how you know the new one is better — a measurement beats an assertion.

## Pull requests

1. Branch from `master`.
2. Make the change, with tests.
3. Run `make check` and `make selfcheck`. Both must pass.
4. Open the PR with a description of the problem, not just the diff.

Small, focused pull requests get reviewed faster. If you are planning something large, open an issue first so we can agree on the shape before you write it.

## Testing

Everything lives in `crates/stupid-comments/tests/corpus.rs`, driven by fixtures.

Fixtures come in two kinds, and the naming is load-bearing:

- **`traps.*`** — realistic files that must produce **zero** findings. They exist to catch false positives: a `#` inside a shell heredoc, a lint pragma above a long comment block, a legitimately documented Kubernetes manifest.
- **`violations.*`** — files that must produce specific findings, asserted by rule name.

A false positive is worse than a false negative here. The tool blocks writes, so a rule that fires on innocent code trains people to disable it. When you add a rule, add a trap fixture before you add a violation fixture.

## Adding a language

Six steps, using YAML as the worked example:

1. **Add the grammar** to `crates/stupid-comments/Cargo.toml`:
   ```sh
   cargo add --package stupid-comments tree-sitter-yaml
   ```
2. **Add the variant** in `src/lang.rs`: a `Lang` arm, an extension arm in `from_path`, the grammar in `language()`, and a string in `name()`. If the language does not carry its name in an extension, add a `from_name` arm too.
3. **Check the comment node kind.** The extractor collects any node whose kind contains `comment`. Most grammars comply; confirm yours does before assuming it.
4. **Add doc comment prefixes** to `doc_prefixes()` if the language distinguishes doc comments from prose.
5. **Write both fixtures**: `traps.<ext>` and `violations.<ext>`. Add the trap to the `traps_produce_no_findings` list, and assert the specific rules for the violation.
6. **Verify against a real codebase**, not just the fixture. Grammar surprises show up in real files: templating breaks the YAML grammar entirely, and shell heredocs look exactly like comment blocks to anything less than a parser.

> [!IMPORTANT]
> Do not mark a language `is_provisional` (warn-only) without evidence that its grammar is unreliable. Softening a rule pre-emptively is how this tool has previously ended up reporting clean on files that were three-quarters comments. If the grammar works, let it block.

## Design principles

These are the constraints the codebase is built around. A change that violates one needs a good argument.

**A file that was not checked is never reported as clean.** Every path that produces no findings has to be distinguishable from a path that produced none *because nothing was examined*. This is why `check` prints a coverage summary, why an unparseable config is a loud error, and why an excluded file is counted separately from a checked one.

**The hook fails open; everything else fails loud.** A parse error, a missing binary, an unreadable config — inside the hook, all of them mean no findings, because a broken tool must never block someone's write. Outside the hook there is no such excuse.

**Deletion is not compliance.** Findings demand a rewrite and say so. Only `--adjudicate`, which a human invokes deliberately, offers removal as a remedy.

**One rule for every language.** Config formats answer to the same thresholds as code. Carve-outs have been tried twice and produced the same bug twice.

**The enforced party cannot write its own exemption.** Suppression pragmas count only if they already exist in `HEAD`.

## Releasing

Three files carry the version and must agree:

- `Cargo.toml` (`workspace.package.version`)
- `plugins/stupid-comments/.claude-plugin/plugin.json`
- `.claude-plugin/marketplace.json`

Bump all three in one commit, `chore(release): X.Y.Z`. Confirm they agree with:

```sh
claude plugin tag plugins/stupid-comments --dry-run
```

The plugin cache is keyed by version, so shipping changed content under an unchanged version leaves users unable to tell which build they have.

## License

By contributing, you agree that your contributions will be licensed under GPL-3.0-or-later, the same terms as the project. See [LICENSE](LICENSE).

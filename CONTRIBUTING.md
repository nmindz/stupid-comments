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

You need a Rust toolchain, from <https://rustup.rs>. Node 22.13+ and pnpm are needed only to touch the DSH plugin or the release tooling.

```sh
git clone https://github.com/nmindz/stupid-comments && cd stupid-comments
pnpm install        # dev tooling only; the plugin itself has no dependencies
make check          # tests + lint + both plugin manifests
```

Every target has a bare cargo equivalent if you would rather not use make:

| Make | Cargo equivalent |
| --- | --- |
| `make build` | `cargo build --release` |
| `make test` | `cargo test` |
| `make dsh-test` | `node plugins/stupid-comments/dsh/test.mjs` |
| `make lint` | `cargo clippy --all-targets` |
| `make version VERSION=X.Y.Z` | `node scripts/sync-version.mjs X.Y.Z` |
| `make validate` | `claude plugin validate plugins/stupid-comments` + `node scripts/validate-dsh-manifest.mjs` |
| `make check` | the four above, in order |
| `make install` | `cargo install --path crates/stupid-comments --root ~/.local --force` |
| `make uninstall` | `cargo uninstall --root ~/.local stupid-comments` |
| `make dsh-install` | `dsh plugin --profile tui add $(pwd)` |
| `make dsh-uninstall` | `dsh plugin --profile tui remove stupid-comments` |
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
│   ├── hook.rs        # hook payloads, shared by every harness
│   ├── suppress.rs    # git-anchored pragmas
│   ├── session.rs     # cross-turn evasion tracking
│   ├── vcs.rs         # git plumbing
│   ├── lib.rs         # file walking and the analysis entry points
│   └── main.rs        # CLI
└── tests/
    ├── corpus.rs      # the whole suite
    └── fixtures/      # traps.* and violations.*
plugins/stupid-comments/
├── .claude-plugin/    # Claude Code manifest
├── hooks/             # Claude Code hook wiring
├── commands/          # slash command prompts, read by both harnesses
└── dsh/               # the DSH plugin: seams, payloads, commands, its own test
package.json           # the DSH bundle manifest (dsh.bundle.patch)
scripts/               # version sync and the manifest checks no compiler can do
.github/workflows/     # CI, and the release that stages the npm package
```

### Two harnesses, one engine

Rules live in the Rust crate and nowhere else. A harness plugin may only translate: build the hook payload, hand it to the binary, map the exit code back. A behavior change that has to be written twice — once per harness — belongs in the crate instead.

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

**Scopes in use:** `lang`, `rules`, `comments`, `policy`, `cli`, `hook`, `vcs`, `plugin`, `dsh`, `release`. Omit the scope when a change genuinely spans the codebase.

The convention is enforced: CI runs `commitlint` over every commit in a pull request, and the release version is derived from these messages. A `feat` is a minor bump, `fix`/`perf`/`refactor` a patch, a `BREAKING CHANGE:` footer a major; `chore`, `ci`, `style`, `test` and `build` release nothing.

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

Releases are automatic. Nothing is bumped by hand.

Merging to `master` runs CI; when CI passes, the release workflow runs `semantic-release`, which reads the Conventional Commits since the last tag and decides whether a release exists at all. A push of `docs`/`chore`/`ci` commits exits in seconds having staged nothing.

When there is a release, it:

1. computes the next version from the commit messages,
2. writes that version into every manifest that carries one (`scripts/sync-version.mjs`),
3. regenerates `CHANGELOG.md`,
4. commits `chore(release): X.Y.Z`, tags it, and cuts the GitHub release,
5. **stages** the npm package rather than publishing it.

### Five manifests, one version

`package.json`, `plugins/stupid-comments/.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`, `Cargo.toml` and `Cargo.lock` each carry the version independently. `scripts/sync-version.mjs` writes all five and `scripts/validate-dsh-manifest.mjs` fails the build on drift, which is the only reason that duplication is tolerable. To set them by hand:

```sh
make version VERSION=0.2.0
```

Confirm the Claude Code side separately with:

```sh
claude plugin tag plugins/stupid-comments --dry-run
```

### Approving a staged version

CI never holds a credential that can publish. npm mints a short-lived token from the workflow's OIDC identity ([trusted publishing](https://docs.npmjs.com/trusted-publishers)), and that token only stages. A human makes it live:

```sh
npm stage list stupid-comments
npm stage view <stage-id>       # inspect the tarball and its provenance
npm stage approve <stage-id>    # publishes, asks for 2FA
npm stage reject <stage-id>     # discards it
```

### One-time setup

- On npm: package settings -> **Trusted publisher** -> GitHub Actions, repository `nmindz/stupid-comments`, workflow `release.yaml`, environment `production`. Leave "publish directly" **unchecked** so releases stage rather than publish. npm only offers this on a package that already exists, so version 0.1.5 has to be published manually once.
- Those three values are matched against the OIDC claim verbatim. `release.yml` and `release.yaml` are different names as far as npm is concerned, and a mismatch mints no token at all: the publish fails with `E401 Unable to authenticate` *after* signing provenance, which reads like a credential problem rather than a naming one.
- On GitHub: create the `production` environment. Its name must match the OIDC claim exactly.
- Tag the current version once so semantic-release continues the `0.1.x` line instead of treating the next release as a first one: `git tag -a v0.1.5 -m v0.1.5 && git push origin v0.1.5`.

The plugin cache is keyed by version, so shipping changed content under an unchanged version leaves users unable to tell which build they have.

## License

By contributing, you agree that your contributions will be licensed under GPL-3.0-or-later, the same terms as the project. See [LICENSE](LICENSE).

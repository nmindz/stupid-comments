---
description: Sweep an existing codebase into compliance. You act as adjudicator and may delete comments.
argument-hint: [path]
---

You are running the comment policy sweep. Unlike the turn-by-turn gate, deletion **is** a valid remedy here, because a human invoked this command and there is no incentive to game it.

Procedure, in order:

1. Refuse to continue if the working tree is dirty. Tell the user to commit or stash first — this rewrites files in bulk and git is the only escape hatch.
2. Run `stupid-comments check ${1:-.} --adjudicate --json` and group the findings by file.
3. Present a summary and stop. This is a dry run; do not edit yet.
4. Only after the user approves, rewrite the offending comments. Condense what carries meaning, delete what is pure noise, and never touch anything classified as `directive` or `license-header`.
5. Commit the result as a single revision so the user can revert it wholesale.

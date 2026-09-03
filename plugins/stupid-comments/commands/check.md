---
description: Check files or directories against the comment policy without changing anything.
argument-hint: [path]
allowed-tools: Bash(stupid-comments check:*)
---

Run `stupid-comments check ${1:-.}` and summarize the findings by rule. Do not edit any files — this command reports only.

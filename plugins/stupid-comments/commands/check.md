---
description: Check files or directories against the comment policy without changing anything.
argument-hint: [path]
allowed-tools: Bash(stupid-comments check:*)
---

Run `stupid-comments check ${1:-.}` and summarize the findings by rule. Do not edit any files — this command reports only.

Report the coverage line the tool prints alongside the findings. A file it had no grammar for was not checked, and reporting it as clean is how an entire directory goes unexamined. If the extensions listed as unchecked look like they carry comments, say so plainly rather than calling the sweep complete.

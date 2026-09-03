---
description: Show the comment policy currently in force and where it came from.
allowed-tools: Bash(stupid-comments policy)
---

Run `stupid-comments policy` and report the resolved source, mode, and rule values back to the user. If it reports no policy, explain that enforcement is inert until a `# Comments Policy` section exists in `~/.claude/CLAUDE.md` or a `.stupid-comments.jsonc` is present.

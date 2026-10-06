---
description: Show the comment policy currently in force and where it came from.
allowed-tools: Bash(stupid-comments policy)
---

Run `stupid-comments policy` and report the resolved source, mode, and rule values back to the user. If it reports no policy, explain that enforcement is inert until a `# Comments Policy` section exists in the agent's memory — `~/.claude/CLAUDE.md`, `~/.dsh/AGENTS.md`, `~/.pi/agent/AGENTS.md`, `~/.omp/agent/AGENTS.md`, `~/.agents/AGENTS.md`, or a project `AGENTS.md` or `CLAUDE.md` — or a `.stupid-comments.jsonc` is present.

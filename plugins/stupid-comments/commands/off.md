---
description: Explain how to disarm enforcement for a session.
---

Tell the user that enforcement is disarmed by setting `STUPID_COMMENTS=0` in the environment of the Claude Code session, and that this is deliberately the only mid-session escape hatch: it lives outside anything you can write to a file, so you cannot disable the gate on your own behalf.

For a permanent change, point them at `mode` in `.stupid-comments.jsonc` (`shadow`, `warn`, or `block`) or `/plugin uninstall stupid-comments@stupid-comments`.

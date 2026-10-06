---
description: Explain how to disarm enforcement for a session.
---

Tell the user that enforcement is disarmed by setting `STUPID_COMMENTS=0` in the environment the harness session was started from (Claude Code, DeepSeek Harness, Pi, or omp), and that this is deliberately the only mid-session escape hatch: it lives outside anything you can write to a file, so you cannot disable the gate on your own behalf.

For a permanent change, point them at `mode` in `.stupid-comments.jsonc` (`shadow`, `warn`, or `block`), or at removing the plugin: `/plugin uninstall stupid-comments@stupid-comments` in Claude Code, `dsh plugin --profile <profile> remove stupid-comments` in DeepSeek Harness, `pi remove` with the source it was installed from in Pi, or `omp plugin uninstall stupid-comments` in omp.

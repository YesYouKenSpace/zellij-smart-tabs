---
description: Print the Codex CLI config snippet to enable automatic tab status updates for Codex
---

Run the shared setup script to print the Codex hook TOML snippet:

```bash
bash "${CLAUDE_PLUGIN_ROOT}/scripts/setup-codex.sh"
```

Show the output to the user, then tell them to:

1. Append the snippet to `~/.codex/config.toml`. If they prefer, they can run `bash "${CLAUDE_PLUGIN_ROOT}/scripts/setup-codex.sh" --apply` to append it automatically (idempotent — guarded by a marker comment, safe to re-run).
2. Restart Codex for the new hooks to load.

Caveats to mention:

1. `codex_hooks = true` is a **feature flag** in Codex CLI. It exists in current source but the flag name or hook shape may change before it ships as stable. If Codex complains about unknown fields, check the latest Codex release notes.
2. Codex plugins can't ship hooks (only MCP servers / skills / apps), which is why this is config-file surgery rather than a one-command install like we have for Claude Code.
3. `PermissionRequest` is Codex's equivalent of Claude's `Notification` — it fires when Codex asks for command approval, letting us render the `help` status.

Pure-Codex users who don't have Claude Code installed can run the same script standalone — it's in the repo at `scripts/setup-codex.sh`.

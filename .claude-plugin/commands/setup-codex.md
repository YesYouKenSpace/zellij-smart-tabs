---
description: Print the Codex CLI config snippet to enable automatic tab status updates for Codex
---

OpenAI's Codex CLI has its own hook system behind the experimental `codex_hooks` feature flag. It's analogous to Claude Code's hooks, so we can wire the same busy / help / ready lifecycle.

Print this TOML snippet and tell the user to append it to `~/.codex/config.toml`:

```toml
# Enable the experimental hooks feature. Remove this line if Codex ever
# promotes hooks out of feature-flag status.
[features]
codex_hooks = true

# Smart-tabs status hooks. Each one fires at a Codex lifecycle event and
# pipes the current pane_id + status to the zellij-smart-tabs plugin.
[[hooks.UserPromptSubmit]]
[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID busy"'

[[hooks.PreToolUse]]
[[hooks.PreToolUse.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID busy"'

[[hooks.PostToolUse]]
[[hooks.PostToolUse.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID busy"'

[[hooks.PermissionRequest]]
[[hooks.PermissionRequest.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID help"'

[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID ready"'
```

Then have the user restart Codex for the hooks to load:

```bash
# Just exit the current Codex session; next run picks up new config.
```

Caveats to mention to the user:

1. `codex_hooks = true` is a **feature flag**. It exists in current Codex CLI source but may change shape or stabilize under a different name in future versions. If Codex complains about an unknown flag or hook shape, check the latest Codex release notes.

2. Codex plugins (via `/codex plugin install`) cannot ship hooks — only MCP servers, skills, and apps. That's why this is manual config rather than a one-command install like we have for Claude Code.

3. The `PermissionRequest` event is Codex's equivalent of Claude's `Notification`. It fires when Codex asks to run a command that needs approval.

Do NOT edit `~/.codex/config.toml` directly — print the snippet and let the user paste. Merging TOML with existing `[features]` / `[hooks]` sections is fragile and a user's dotfile shouldn't be mutated without consent.

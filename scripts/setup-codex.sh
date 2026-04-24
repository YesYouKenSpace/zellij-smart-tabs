#!/usr/bin/env bash
#
# Print the Codex CLI config snippet that wires zellij-smart-tabs status
# updates to Codex's lifecycle events. Append the output to ~/.codex/config.toml.
#
# Usage:
#   ./setup-codex.sh           # print snippet to stdout
#   ./setup-codex.sh --apply   # append to ~/.codex/config.toml if not already present
#
# The script does NOT auto-merge. --apply is a naive append guarded by a marker
# comment; if your config.toml already has the marker, the script does nothing.
# For any non-trivial pre-existing [hooks] section, paste manually.

set -euo pipefail

MARKER="# zellij-smart-tabs hooks — managed block, safe to delete"

snippet() {
  cat <<'EOF'
# zellij-smart-tabs hooks — managed block, safe to delete
[features]
codex_hooks = true

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
EOF
}

if [[ "${1:-}" == "--apply" ]]; then
  config="${CODEX_CONFIG:-$HOME/.codex/config.toml}"
  if [[ ! -f "$config" ]]; then
    mkdir -p "$(dirname "$config")"
    touch "$config"
  fi
  if grep -qF "$MARKER" "$config"; then
    echo "zellij-smart-tabs block already present in $config — skipping." >&2
    exit 0
  fi
  {
    echo ""
    snippet
  } >> "$config"
  echo "Appended zellij-smart-tabs hooks to $config." >&2
  echo "Restart Codex for the new hooks to load." >&2
else
  snippet
fi

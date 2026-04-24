#!/usr/bin/env bash
#
# setup-codex.sh — install/uninstall the zellij-smart-tabs hooks in ~/.codex/config.toml
#
# The managed region is delimited by two marker comments:
#
#     # BEGIN zellij-smart-tabs managed block — do not edit inside
#     ...hooks...
#     # END zellij-smart-tabs managed block
#
# Anything you add OUTSIDE the markers (before BEGIN or after END) is never
# touched by this script. --remove deletes only lines between the markers
# inclusive. That means you can safely keep your own [hooks] or [features]
# additions next to our block.
#
# Usage:
#   ./setup-codex.sh                    # print the managed block to stdout
#   ./setup-codex.sh --apply            # append to ~/.codex/config.toml if not already there
#   ./setup-codex.sh --remove           # remove the managed block (only between markers)
#   ./setup-codex.sh --help             # show this message
#
# Env:
#   CODEX_CONFIG — override target path (default: ~/.codex/config.toml)
#
# Exit codes:
#   0  success (including "already applied" and "not present when removing")
#   1  usage error
#   2  I/O error

set -euo pipefail

MARKER_BEGIN="# BEGIN zellij-smart-tabs managed block — do not edit inside"
MARKER_END="# END zellij-smart-tabs managed block"
CONFIG="${CODEX_CONFIG:-$HOME/.codex/config.toml}"

usage() {
  sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

snippet() {
  cat <<EOF
${MARKER_BEGIN}
# Managed by zellij-smart-tabs. Run scripts/setup-codex.sh --remove to uninstall.
# See https://github.com/YesYouKenSpace/zellij-smart-tabs for details.
[features]
codex_hooks = true

[[hooks.UserPromptSubmit]]
[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "\$ZELLIJ_PANE_ID busy"'

[[hooks.PreToolUse]]
[[hooks.PreToolUse.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "\$ZELLIJ_PANE_ID busy"'

[[hooks.PostToolUse]]
[[hooks.PostToolUse.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "\$ZELLIJ_PANE_ID busy"'

[[hooks.PermissionRequest]]
[[hooks.PermissionRequest.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "\$ZELLIJ_PANE_ID help"'

[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = 'zellij pipe --plugin smart-tabs --name status -- "\$ZELLIJ_PANE_ID ready"'
${MARKER_END}
EOF
}

apply() {
  mkdir -p "$(dirname "$CONFIG")"
  [[ -f "$CONFIG" ]] || touch "$CONFIG"

  if grep -qF "$MARKER_BEGIN" "$CONFIG"; then
    echo "zellij-smart-tabs block already present in $CONFIG — no changes." >&2
    return 0
  fi

  # Ensure file ends with a newline before appending
  if [[ -s "$CONFIG" ]] && [[ $(tail -c1 "$CONFIG" | wc -l) -eq 0 ]]; then
    printf '\n' >> "$CONFIG"
  fi
  printf '\n' >> "$CONFIG"
  snippet >> "$CONFIG"
  echo "Appended zellij-smart-tabs hooks to $CONFIG." >&2
  echo "Restart Codex for the new hooks to load." >&2
}

remove() {
  if [[ ! -f "$CONFIG" ]]; then
    echo "No $CONFIG to modify." >&2
    return 0
  fi
  if ! grep -qF "$MARKER_BEGIN" "$CONFIG"; then
    echo "No zellij-smart-tabs block found in $CONFIG." >&2
    return 0
  fi

  # awk range is inclusive and matches literal text (not regex) by using index().
  awk -v b="$MARKER_BEGIN" -v e="$MARKER_END" '
    index($0, b) { skip = 1; next }
    skip && index($0, e) { skip = 0; next }
    skip { next }
    { print }
  ' "$CONFIG" > "$CONFIG.tmp" || {
    rm -f "$CONFIG.tmp"
    echo "Failed to rewrite $CONFIG." >&2
    return 2
  }
  mv "$CONFIG.tmp" "$CONFIG"
  echo "Removed zellij-smart-tabs block from $CONFIG." >&2
  echo "Restart Codex for the change to take effect." >&2
}

case "${1:-}" in
  --apply)   apply ;;
  --remove|--uninstall) remove ;;
  --help|-h) usage 0 ;;
  "")        snippet ;;
  *)         echo "Unknown flag: $1" >&2; usage 1 ;;
esac

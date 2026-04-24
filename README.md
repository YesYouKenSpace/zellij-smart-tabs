# zellij-smart-tabs

https://github.com/user-attachments/assets/e9ce05ce-677d-41ff-9707-7946323cac20

A [Zellij](https://github.com/zellij-org/zellij) plugin that manages your tabs so that you don't have to. 


## Objective

I built this because I kept losing track of which tab was which. I wanted to glance at my tab bar and instantly know what's running where - without manually renaming tabs every time I switch projects or start a new tool. Now my tabs just tell me what I need to know.

## Features
- **Smart renaming** - auto-renames tabs based on configurable Jinja2-like templates (powered by MiniJinja) with context-aware variables (`short_dir`, `short_git_root`, `program`)
- **Pane-scoped templates** - reference specific panes in templates (`pane[0].*`, `pane[-1].*`) powered by MiniJinja
- **Manual tab control** - toggle a tab to manual mode to prevent auto-renaming, then rename it yourself. Clear the tab name to restore auto-management.
- **Dashboard UI** - tabbed dashboard (Status, Tabs, Panes, Log, Help) with keyboard and mouse navigation
- **Configurable polling** - reacts to Zellij events (TabUpdate, PaneUpdate, CwdChanged) with a timer fallback

## Installation

### Download from releases

Download the latest `zellij-smart-tabs.wasm` from [GitHub Releases](https://github.com/yesyouken/zellij-smart-tabs/releases) and place it in your Zellij plugins directory:

```bash
mkdir -p ~/.config/zellij/plugins
cp zellij-smart-tabs.wasm ~/.config/zellij/plugins/
```

### Build from source

Requires Rust with the `wasm32-wasip1` target:
```bash
rustup target add wasm32-wasip1
```

```bash
git clone https://github.com/yesyouken/zellij-smart-tabs.git
cd zellij-smart-tabs
make build
make install
```

### Prerequisites
- **[Zellij](https://zellij.dev/) 0.44.0+** - requires the `CwdChanged` event and stable `tab_id` API introduced in 0.44.0
- **[Nerd Font](https://www.nerdfonts.com/)** - the default substitutions use Nerd Font icons. Install one from [nerdfonts.com](https://www.nerdfonts.com/font-downloads) and configure your terminal to use it. Without a Nerd Font, icons will appear as missing glyphs.

## Quickstart

Alias the plugin and load the plugin on startup. Replace `v0.1.0` with the latest version
```kdl
plugins {
    smart-tabs location="https://github.com/YesYouKenSpace/zellij-smart-tabs/releases/download/v0.1.0/zellij-smart-tabs.wasm" {
        // where config for the plugin should go
    }
}

load_plugins {
    smart-tabs // load smart-tabs on startup we only need 1 instance of it
}
```

## Configuration

All configuration is inline in the plugin block.

| Key | Type | Default | Description |
|---|---|---|---|
| `format` | String | See [Format Gallery](#format-gallery) | Tab name template (Jinja2-like syntax) |
| `poll_interval` | Number (seconds) | `5` | Timer fallback interval for polling |
| `debounce` | Number (seconds) | `0.2` | Delay before applying tab rename after data changes |
| `debug` | Bool | `true` | Enable debug logging to Zellij log |
| `sub` | Block | - | Substitution rules (see below) |


### Substitutions

Map program names to custom display names using the `sub` block:

```kdl
plugins {
    // Note that throughout this guide, we will refer to the plugin via "smart-tabs" alias.
    smart-tabs location="https://github.com/YesYouKenSpace/zellij-smart-tabs/releases/download/v0.0.2/zellij-smart-tabs.wasm" {
        // where config for the plugin should go
        sub {
            program {
                zsh "" // this would hide the program module if it is zsh
                nvim "nvim" // if you dont like the icon and just want it verbatim
                go "\u{e65e}" // you prefer  over the default 
            }
        }
    }
}
```

The `{{ program }}` template variable will show the substituted value. Programs not in the substitution map keep their original name. Use an empty string `""` to hide a program from the tab name.

#### Default program substitutions

| Program | Substitution | Unicode |
|---|---|---|
| `nvim` |  | `\u{e6ae}` |
| `vim` |  | `\u{37c5}` |
| `claude` |  | `\u{f069}` |
| `node` | 󰎙 | `\u{f0399}` |
| `zsh` |  | `\u{f489}` |
| `go` | | `\u{e627}` |

#### Default status substitutions

Three primary states designed for AI agent workflows (Claude Code, Codex, etc.):

| Status | Substitution | Unicode | Meaning |
|---|---|---|---|
| `busy` |  | `\u{f252}` | Agent is working |
| `help` |  | `\u{f128}` | Agent needs user input (permission, question) |
| `ready` |  | `\u{f05d}` | Agent finished, waiting for next prompt |
| `error` |  | `\u{ea87}` | Something went wrong |
| `idle` | *(empty - hidden)* | `""` | No activity |

Legacy aliases kept for backward compatibility: `running` / `pending` map to `busy`'s icon, `done` maps to `ready`'s icon.

These are [Nerd Font](https://www.nerdfonts.com/) icons. Make sure your terminal uses a Nerd Font for them to render correctly. Override any substitution in the `sub` block.

### Template variables

| Variable | Type | Description |
|---|---|---|
| `short_dir` | String | Last component of the pane's working directory |
| `cwd` | String | Full path of the pane's working directory |
| `short_git_root` | String or undefined | Last component of the git repository root path |
| `git_root` | String or undefined | Full path to the git repository root |
| `program` | String or undefined | Currently running program (e.g., `nvim`, `claude`, `opencode`) |
| `status` | String | Pane activity status (freeform, set via pipe). Defaults: `idle`, `busy`, `help`, `ready`, `error` (legacy: `running`, `pending`, `done`). |

All variables are also available scoped to specific panes:

- `pane[N].*` — Nth pane's variables (0-indexed)
- `pane[-1].*` — last pane (negative indexing supported)

Top-level variables (e.g., `{{ short_dir }}`) are aliases for `pane[0].*` (first pane).

### Format Gallery

A collection of format strings for different workflows. Copy one into your plugin config:

```kdl
// Default - IDE-style: project + file context + status
format "{% if short_git_root %}{{ short_git_root }}{% else %}{{ short_dir }}{% endif %}{% if program %} \u{eab6} {{ program }}{% endif %}{% if status %} {{ status }}{% endif %}"
// => my-repo › nvim ✅

// Minimal - just the directory name
format "{{ short_dir }}"
// => my-project

// Full path
format "{{ cwd }}"
// => /home/user/Projects/my-project

// Program-first - shows what's running, then where
format "{% if program %}{{ program }} @ {% endif %}{{ short_dir }}"
// => nvim @ my-project

// Status indicators only (great with icon substitutions)
format "{{ short_dir }}{% if status %} {{ status }}{% endif %}{% if program %} {{ program }}{% endif %}"
// => my-repo ⏳ 

// First pane's program (useful with splits)
format "{{ short_dir }}{% if program %} [{{ program }}]{% endif %}"
// => my-project [nvim]

// Multi-pane - show first and second pane directories
format "{{ short_dir }}{% if pane[1] %} | {{ pane[1].short_dir }}{% endif %}"
// => my-project | docs

```

## Manual Tab Control

By default, all tabs are auto-managed - the plugin renames them based on your format template. To manually rename a tab, first set it to manual mode, then rename it.

### Set tab to manual

Run from any terminal pane:
```bash
zellij pipe --name set_focused_to_manual --plugin smart-tabs
```

This sets the focused tab to manual mode. Manual tabs are skipped by auto-rename.

### Recommended keybinding

Add this to your Zellij config (`~/.config/zellij/config.kdl`) to override the `r` key in tab mode and the `esc` key in renametab mode.
```kdl
keybinds {
    tab {
        bind "r" {
            // It sets the tab to manual mode first, then enters rename mode.
            MessagePlugin "smart-tabs" {
                name "set_focused_to_manual"
            }
            SwitchToMode "RenameTab"
            TabNameInput 0
        }
    }
    renametab {
        bind "esc" {
            UndoRenameTab
            SwitchToMode "tab"
            // It sets the tab to managed mode.
            MessagePlugin "smart-tabs" {
                name "set_focused_to_managed"
            }
        }
    }
}
```

### Restore auto-management

Three ways to restore a manual tab to auto-management:
1. **Pipe command** - run `zellij pipe --name set_focused_to_managed --plugin smart-tabs`
2. **Clear the tab name** - rename the tab to an empty string (the plugin detects this and switches back to managed)
3. **Esc in rename mode** - if using the recommended keybinding above, pressing `Esc` cancels the rename and restores managed mode

## Pane Status

Programs can report their activity status to the plugin via pipe. The status is available as `{{ status }}` (first pane) in templates.

### Setting status

Use the `status` pipe with payload `<pane_id> <status>` (space-separated):

```bash
# From a program running inside a Zellij pane:
zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID busy"
zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID help"
zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID ready"
zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID error"
zellij pipe --plugin smart-tabs --name status -- "$ZELLIJ_PANE_ID idle"
```

Status is freeform - you can send any string (e.g. `deploying`, `testing`). The [default status substitutions](#default-status-substitutions) are applied automatically. Custom statuses without a substitution are shown as-is.

The older `pane_status` pipe with JSON payload (`{"pane_id":"...","status":"..."}`) is still supported for backward compatibility.

### Claude Code integration

The tab bar can automatically show three glanceable states for each Claude Code session:

- `busy` — Claude is actively working (processing a prompt or running tools)
- `help` — Claude sent a notification (permission request, question, or idle warning) and needs your attention
- `ready` — Claude finished and is waiting for your next prompt

#### Option A: Install as a Claude Code plugin (recommended)

This repo ships its own Claude Code plugin manifest. Installing it auto-registers all the lifecycle hooks — no manual `settings.json` editing.

In Claude Code:

```
/plugin marketplace add YesYouKenSpace/zellij-smart-tabs
/plugin install zellij-smart-tabs@zellij-smart-tabs
```

That's it. Your `~/.claude/settings.json` only gains a single `enabledPlugins` entry; the hooks live in the plugin and register automatically when it's enabled. `/plugin uninstall` removes them cleanly — nothing lingers.

If you haven't set up the Zellij-side plugin yet, run `/setup-zellij` from Claude Code to see the config snippet to add to `~/.config/zellij/config.kdl`.

#### Option B: Manual hook setup

If you prefer not to install the plugin (e.g. you're using these hooks outside Claude Code), add this to `.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "matcher": "",
        "hooks": ["zellij pipe --plugin smart-tabs --name status -- \"$ZELLIJ_PANE_ID busy\""]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "",
        "hooks": ["zellij pipe --plugin smart-tabs --name status -- \"$ZELLIJ_PANE_ID busy\""]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "",
        "hooks": ["zellij pipe --plugin smart-tabs --name status -- \"$ZELLIJ_PANE_ID busy\""]
      }
    ],
    "Notification": [
      {
        "matcher": "",
        "hooks": ["zellij pipe --plugin smart-tabs --name status -- \"$ZELLIJ_PANE_ID help\""]
      }
    ],
    "Stop": [
      {
        "matcher": "",
        "hooks": ["zellij pipe --plugin smart-tabs --name status -- \"$ZELLIJ_PANE_ID ready\""]
      }
    ],
    "StopFailure": [
      {
        "matcher": "",
        "hooks": ["zellij pipe --plugin smart-tabs --name status -- \"$ZELLIJ_PANE_ID ready\""]
      }
    ]
  }
}
```

`$ZELLIJ_PANE_ID` is set automatically by Zellij for processes running inside panes.

### Codex CLI integration

OpenAI's Codex CLI has a hook system analogous to Claude Code's, behind the experimental `codex_hooks` feature flag. Unlike Claude Code, Codex plugins cannot ship hooks (they only bundle MCP servers, skills, and apps), so Codex integration is always a one-time `~/.codex/config.toml` edit — there's no equivalent of `/plugin install`.

Three ways to set it up, pick whichever fits:

**Option 1 — standalone script (pure Codex users, no Claude Code needed):**

```bash
# Clone this repo somewhere, then:
./scripts/setup-codex.sh --apply          # append to ~/.codex/config.toml (idempotent)

# Or one-shot via curl, if you trust it:
curl -fsSL https://raw.githubusercontent.com/YesYouKenSpace/zellij-smart-tabs/main/scripts/setup-codex.sh | bash -s -- --apply
```

Drop `--apply` to just print the snippet to stdout instead of writing.

**Option 2 — Claude Code slash command (if you also use Claude):**

Install the `zellij-smart-tabs` Claude Code plugin (see [Claude Code integration](#claude-code-integration) above), then run `/setup-codex` inside Claude Code. It invokes the same script.

**Option 3 — copy-paste (trust nothing, do it yourself):**

Append this to `~/.codex/config.toml`:

```toml
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
```

Restart Codex after any of the three options.

Notes:

- `codex_hooks = true` is a **feature flag**. If Codex rejects the config, check the latest release notes — the flag name or hook shape may have changed.
- `PermissionRequest` is Codex's equivalent of Claude's `Notification` — it fires when Codex asks for approval to run a command.

For Linux desktop notifications and other integrations, see the helper scripts in [`scripts/linux/`](scripts/linux/).

## Dashboard

The plugin pane shows a tabbed dashboard with keyboard and mouse navigation.

### Views

| Key | View | Content |
|---|---|---|
| `1` | Status | Plugin version, format template, config values |
| `2` | Tabs | Table of all tabs with position, name, CWD, git root, program, status |
| `3` | Panes | Table of all panes across all tabs |
| `4` | Log | Debug log entries (enable with `debug "true"`) |
| `5` | Help | Template variables, keyboard shortcuts, config reference |

### Keyboard shortcuts

| Key | Action |
|---|---|
| `1`-`5` | Switch view |
| `Tab` / `Shift+Tab` | Next / previous view |
| `j` / `Down` | Scroll down |
| `k` / `Up` | Scroll up |
| `g` | Scroll to top |
| `G` | Scroll to bottom |
| `Esc` | Hide plugin pane |

Mouse click on the tab bar switches views. Mouse scroll works within views.


## Alternatives

| Feature | zellij-smart-tabs | zellij-tabula | zellij-tab-rename | zellij-tab-name | opencode-zellij-namer |
|---|---|---|---|---|---|
| **Type** | Rust WASM | Rust WASM + zsh hook | Rust WASM | Rust WASM | TypeScript (OpenCode) |
| **Renames** | Tabs | Tabs | Tabs | Tabs | Sessions |
| **CWD detection** | Events + timer | zsh hook | Events | Shell hook (pipe) | OpenCode events |
| **Shell support** | Any | zsh only | Any | Any (via pipe) | N/A |
| **Configurable name format** | Yes | No | Yes | Yes | No |
| **Manual rename detection** | Yes | No | No | No | No |
| **Dashboard UI** | Yes | No | No | No | No |
| **Standalone** | Yes | Yes (zsh required) | Yes | Yes | No |

## References

- [zellij-tabula](https://github.com/bezbac/zellij-tabula) - Tab renaming via zsh shell hook
- [zellij-tab-rename](https://github.com/vmaerten/zellij-tab-rename) - Tab renaming with CwdChanged event
- [zellij-tab-name](https://github.com/Cynary/zellij-tab-name) - Pipe-based tab renaming with format strings
- [opencode-zellij-namer](https://github.com/24601/opencode-zellij-namer) - AI-powered session naming for OpenCode
- [Zellij Plugin API](https://zellij.dev/documentation/plugin-api)
- [MiniJinja](https://github.com/mitsuhiko/minijinja) - Runtime template engine

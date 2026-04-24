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
| `poll_interval` | Number (seconds) | `2` | Timer fallback interval for polling |
| `debounce` | Number (seconds) | `0.2` | Delay before applying tab rename after data changes |
| `debug` | Bool | `false` | Enable debug logging to Zellij log |
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
| `vim` |  | `\u{e7c5}` |
| `claude` |  | `\u{f069}` |
| `codex` | 󰒰 | `\u{eac4}` |
| `node` | 󰎙 | `\u{f0399}` |
| `zsh` |  | `\u{f489}` |
| `go` | | `\u{e627}` |

#### Screen activity status

The plugin reads each pane viewport through Zellij's pane-content API and compares it on each poll.

| Screen state | `screen_status` | Unicode | Meaning |
|---|---|---|---|
| `unknown` |  | `\u{f128}` | No viewport baseline has been captured yet |
| `changed` |  | `\u{f252}` | Viewport text changed on the last poll |
| `stable` | *(empty - hidden)* | `""` | Viewport text did not change |

These are [Nerd Font](https://www.nerdfonts.com/) icons. Make sure your terminal uses a Nerd Font for them to render correctly.

### Template variables

| Variable | Type | Description |
|---|---|---|
| `short_dir` | String | Last component of the pane's working directory |
| `cwd` | String | Full path of the pane's working directory |
| `short_git_root` | String or undefined | Last component of the git repository root path |
| `git_root` | String or undefined | Full path to the git repository root |
| `program` | String or undefined | Currently running program (e.g., `nvim`, `claude`, `opencode`) |
| `screen_state` | String | Viewport state: `unknown`, `changed`, or `stable` |
| `screen_status` | String | Icon for the viewport state |
| `screen_changed` | Boolean | `true` when viewport text changed on the last poll |
| `screen_quiet_ticks` | Number | Completed polls since the last viewport change |

All variables are also available scoped to specific panes:

- `pane[N].*` — Nth pane's variables (0-indexed)
- `pane[-1].*` — last pane (negative indexing supported)

Top-level variables (e.g., `{{ short_dir }}`) are aliases for `pane[0].*` (first pane).

### Format Gallery

A collection of format strings for different workflows. Copy one into your plugin config:

```kdl
// Default - IDE-style: project + file context + screen activity
format "{% if short_git_root %}{{ short_git_root }}{% else %}{{ short_dir }}{% endif %}{% if program %} {{ program }}{% endif %}{% if screen_status %} {{ screen_status }}{% endif %}"
// => my-repo nvim 

// Minimal - just the directory name
format "{{ short_dir }}"
// => my-project

// Full path
format "{{ cwd }}"
// => /home/user/Projects/my-project

// Program-first - shows what's running, then where
format "{% if program %}{{ program }} @ {% endif %}{{ short_dir }}"
// => nvim @ my-project

// Screen activity indicator first
format "{{ short_dir }}{% if screen_status %} {{ screen_status }}{% endif %}{% if program %} {{ program }}{% endif %}"
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

## Screen Activity

Screen activity is detected inside the Zellij plugin. No Codex, Claude, or opencode hook setup is required.

The plugin captures the visible viewport text for each pane, hashes it, and compares it on every poll:

- `unknown` — no baseline has been captured yet
- `changed` — viewport text changed on the last poll
- `stable` — viewport text did not change

This is intentionally heuristic. It can tell whether visible output moved, but it cannot prove that an agent is done, blocked on permission, or silently thinking.

## Dashboard

The plugin pane shows a tabbed dashboard with keyboard and mouse navigation.

### Views

| Key | View | Content |
|---|---|---|
| `1` | Status | Plugin version, format template, config values |
| `2` | Tabs | Table of all tabs with position, name, CWD, git root, and program |
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

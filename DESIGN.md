# Design

## Overview

A Zellij WASM plugin (Rust) that automatically manages tab names using configurable templates with per-pane context. Inspired by Dia and Arc by The Browser Company.

## Architecture

Single Rust WASM plugin with two data stores, a MiniJinja template engine, and a dashboard UI.

```
┌──────────────────────────────────────────────────────┐
│              zellij-smart-tabs (WASM)                │
│                                                      │
│  ┌────────────┐  ┌────────────┐  ┌────────────────┐ │
│  │  Event      │  │  Template  │  │  Dashboard UI  │ │
│  │  Handler    │→ │  Engine    │  │  (5 views)     │ │
│  │            │  │ (MiniJinja)│  │                │ │
│  │ TabUpdate   │  └────────────┘  └────────────────┘ │
│  │ PaneUpdate  │                                     │
│  │ CwdChanged  │  ┌────────────┐  ┌────────────────┐ │
│  │ Timer       │  │  TabStore  │  │  PaneStore     │ │
│  │ Key/Mouse   │  │ (by tab_id)│  │  (by pane_id)  │ │
│  │ Pipe        │  └────────────┘  └────────────────┘ │
│  └────────────┘                                      │
└──────────────────────────────────────────────────────┘
```

## Data Model

### Two Stores

**TabStore** — keyed by `tab_id` (stable, from `TabInfo.tab_id`). Holds tab metadata only:

```rust
struct TabState {
    tab_id: usize,
    position: usize,
    name: String,
    is_managed: bool,   // true = auto-rename, false = user controls
}
```

**PaneStore** — keyed by `pane_id`. Holds all per-pane context:

```rust
struct PaneState {
    pane_id: u32,
    tab_id: usize,              // links to owning tab
    position: usize,            // visual position (sorted by pane_x, pane_y)
    cwd: Option<String>,
    short_dir: Option<String>,  // last component of cwd
    git_root: Option<String>,
    short_git_root: Option<String>,
    program: Option<String>,    // running program (after substitution)
    terminal_command: Option<String>, // set for command panes only
    status: String,             // freeform, default "idle"
}
```

Panes link to tabs via `tab_id`. Template context is built by filtering `PaneStore` by `tab_id` and sorting by `position`.

### Tab Identification

Uses `tab_id` from `TabInfo` (stable across tab closures/reordering) instead of position. Renames use `rename_tab_with_id(tab_id, name)` to avoid Zellij bug #3535.

## Template System

### Engine

MiniJinja only. Askama was removed to simplify the codebase. Templates are rendered from a format string + a nested context value.

### Template Context

Built per-tab from both stores:

```json
{
  "short_dir": "my-project",
  "cwd": "/home/user/my-project",
  "short_git_root": "my-project",
  "git_root": "/home/user/my-project",
  "program": "nvim",
  "status": "running",

  "pane": [
    { "short_dir": "...", "program": "...", "status": "...", ... },
    { "short_dir": "...", ... }
  ]
}
```

- **Top-level aliases** resolve to `pane[0].*` (first pane)
- **`pane[-1]`** — the last pane (negative indexing)
- **`pane[N]`** — pane at visual position N
- Undefined values are falsy in `{% if %}` conditionals

### Substitutions

Program and status values are mapped through `Substitutions` before entering the template context. Default substitutions provide Nerd Font icons. User config merges on top via KDL `sub` block.

## Event Flow

1. **`TabUpdate`** — sync `TabStore` (new tabs, closed tabs, position changes). New tabs are scheduled for rename.
2. **`PaneUpdate`** — sync `PaneStore` (positions, terminal_command for command panes). Panes removed from manifest are cleaned up.
3. **`CwdChanged`** — update `PaneState.cwd` and `short_dir`, request git info via `run_command`.
4. **`RunCommandResult`** — update `PaneState.git_root` and `short_git_root` from `git rev-parse --show-toplevel` result.
5. **`Timer`** — debounce tick (0.2s): fire pending renames. Poll tick (5s): refresh CWD, program, and git info for all panes.
6. **`Key`/`Mouse`** — dashboard navigation.
7. **`Pipe`** — `set_focused_to_manual`, `set_focused_to_managed`, `pane_status`.

## Manual Tab Control

Tabs have an `is_managed` flag (default `true`). Unmanaged tabs are skipped by auto-rename.

- **`set_focused_to_manual`** pipe — sets `is_managed = false`
- **`set_focused_to_managed`** pipe — sets `is_managed = true` and schedules rename
- **Empty tab name** — detected in `sync_tabs`, restores `is_managed = true`

No automatic detection of manual renames — the user explicitly opts out via pipe command.

## Debounce & Polling

The timer fires at `debounce` interval (default 0.2s). Two mechanisms ride on it:

1. **Rename debounce** — `pending_renames: HashSet<usize>` collects tab IDs. Each tick drains the set and renames all pending tabs. Multiple events within one tick coalesce.

2. **Poll cycle** — `poll_ticks` counter increments each tick. When it reaches `poll_interval / debounce`, a full poll runs: refresh CWD (via `get_pane_cwd`), program (via `get_pane_running_command`), and git info (via `run_command`) for all panes.

## Dashboard UI

Five views rendered in the plugin pane via Zellij's `ui_components` API:

| View | Content |
|---|---|
| Status | Plugin version, format, poll interval, debug |
| Tabs | Table of all tabs: position, name, CWD, git root, program, managed |
| Panes | Table of all panes: tab, position, CWD, git root, program, status |
| Log | Debug log ring buffer (100 entries, `debug "true"` required) |
| Help | Template variables, keyboard shortcuts, config reference |

Navigation: `1-5` jump, `Tab`/`Shift+Tab` cycle, `j/k` scroll, `g/G` top/bottom, `Esc` hide, mouse click/scroll.

## Testability

Zellij host calls are abstracted behind a trait (`src/host.rs`) and mocked in tests via `mockall`. The `ZellijPlugin` trait impl is `#[cfg(not(test))]` because WASM host functions don't link on the host target. Tests call `handle_event()` directly.


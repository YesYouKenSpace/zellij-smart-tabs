# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.2] - 2026-04-07

### Added
- Auto-rename tabs using configurable Jinja2-like templates (MiniJinja)
- Per-pane template variables: `short_dir`, `cwd`, `short_git_root`, `git_root`, `program`, `status`
- Pane-scoped access via array indexing: `pane[0]`, `pane[1]`, `pane[-1]`
- Top-level aliases resolve to first pane (`pane[0]`)
- Program substitutions with Nerd Font icon defaults
- Status substitutions with Nerd Font icon defaults
- Freeform pane status via `pane_status` pipe command
- Manual tab control via `set_focused_to_manual` / `set_focused_to_managed` pipe commands
- Empty tab name restores auto-management
- Dashboard UI with 5 views: Status, Tabs, Panes, Log, Help
- Keyboard navigation (1-5, Tab, j/k, g/G, Esc)
- Mouse click and scroll support in dashboard
- Debounced tab renaming (default 0.2s)
- Configurable poll interval for CWD/program/git refresh (default 5s)
- Git root detection via `git rev-parse --show-toplevel`
- Program detection via `get_pane_running_command` API
- Command pane program detection via `terminal_command`
- KDL-based substitution config with nested blocks
- State persistence to `/data` (tab store + pane store)
- Tab identification via stable `tab_id` (resolves Zellij #3535)
- Debug logging to Zellij log (stderr) and in-plugin Log view
- GitHub Actions CI (test, lint, build) and release workflows
- Gitleaks secret scanning in CI and pre-commit
- MIT License

### Architecture
- Two-store model: `TabStore` (by tab_id) + `PaneStore` (by pane_id)
- `ZellijHost` trait for testable Zellij API abstraction
- MiniJinja template engine (Askama removed)
- Module system removed in favor of direct pane state

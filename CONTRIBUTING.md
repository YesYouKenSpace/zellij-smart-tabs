# Development Guide

For architecture and design decisions, see [DESIGN.md](DESIGN.md).

## Prerequisites

- Rust with `wasm32-wasip1` target: `rustup target add wasm32-wasip1`
- Zellij 0.44+
- [Nerd Font](https://www.nerdfonts.com/font-downloads) in your terminal
- (Optional) [pre-commit](https://pre-commit.com/): `pip install pre-commit && pre-commit install`

## Quick Start

```bash
make dev          # clean cache, build debug, launch dev session
make dev-reload   # rebuild and restart dev session
```

## Make Targets

| Target | Description |
|---|---|
| `make build` | Release build (WASM) |
| `make build-dev` | Debug build (WASM) |
| `make test` | Run unit tests on host target |
| `make lint` | Run clippy with `-D warnings` |
| `make test-all` | Test + lint + release build |
| `make dev` | Clean cache, debug build, launch dev Zellij session |
| `make dev-reload` | Kill dev session, clean cache, rebuild, relaunch |
| `make install` | Release build + copy to `~/.config/zellij/plugins/` |
| `make clean` | `cargo clean` |
| `make clean-cache` | Remove `~/.cache/zellij` (forces plugin recompilation) |

## Dev Workflow

1. `make dev` — launches Zellij with `dev-layout.kdl` in a session named `smart-tabs-dev`
2. The layout has a terminal pane (80%) and the plugin pane (20%) side by side
3. Edit code, then `make dev-reload` from another terminal to rebuild and restart
4. Check debug logs in the plugin's Log view (press `4`) — requires `debug "true"` in the dev layout
5. Grant permissions when prompted on first load

### Why `clean-cache`?

Zellij aggressively caches compiled WASM plugins in `~/.cache/zellij`. Without clearing it, the old binary keeps running even after a rebuild. Both `make dev` and `make dev-reload` clear the cache automatically.

### Why `--target` for tests?

`.cargo/config.toml` sets the default build target to `wasm32-wasip1`. Tests can't run in WASM, so `make test` overrides the target to the host platform.

## Source Files

```
src/
  main.rs          Plugin entry point, event handling, template context building
  config.rs        Configuration parsing, substitutions
  tab_state.rs     TabState, TabStore, PaneState, PaneStore
  template.rs      MiniJinja template rendering
  ui.rs            Dashboard UI (Status, Tabs, Panes, Log, Help views)
  host.rs          Host trait — abstracts Zellij API calls for testability
  utils.rs         Utility functions (short_path, extract_program, parse_git_*)
```


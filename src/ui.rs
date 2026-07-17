use crate::config::Config;
use crate::tab_state::{PaneStore, TabStore};
use zellij_tile::prelude::*;

pub const VIEW_COUNT: usize = 4;
/// Index of the Help view — the only view whose content scrolls.
pub const HELP_VIEW: usize = 3;

fn nonempty(s: &str) -> &str {
    if s.is_empty() {
        " "
    } else {
        s
    }
}

pub const APPROX_TAB_WIDTH: usize = 12;
const VIEW_NAMES: [&str; VIEW_COUNT] = ["Status", "Tabs", "Panes", "Help"];

pub struct DashboardContext<'a> {
    pub active_view: usize,
    pub scroll_offsets: &'a [usize; VIEW_COUNT],
    pub config: &'a Config,
    pub tab_store: &'a TabStore,
    pub pane_store: &'a PaneStore,
    pub last_rename: &'a Option<String>,
    pub confirm_reload: bool,
}

pub fn render_dashboard(rows: usize, cols: usize, ctx: &DashboardContext) {
    if rows < 3 || cols < 10 {
        return;
    }
    render_tab_bar(ctx.active_view);
    let content_rows = rows.saturating_sub(2);
    let scroll = ctx.scroll_offsets[ctx.active_view];

    match ctx.active_view {
        0 => render_status(content_rows, cols, ctx.config),
        1 => render_tabs(content_rows, cols, ctx.tab_store, ctx.pane_store, ctx.last_rename),
        2 => render_panes(content_rows, cols, ctx.tab_store, ctx.pane_store),
        3 => render_help(content_rows, cols, scroll, ctx.config),
        _ => {}
    }

    if ctx.confirm_reload {
        render_confirm_reload(rows, cols);
    } else {
        render_shortcuts(rows, cols);
    }
}

fn render_tab_bar(active_view: usize) {
    let ribbons: Vec<Text> = VIEW_NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let label = format!(" {} {} ", i + 1, name);
            if i == active_view {
                Text::new(label).selected()
            } else {
                Text::new(label)
            }
        })
        .collect();
    println!(
        "{}",
        serialize_ribbon_line_with_coordinates(ribbons.iter(), 0, 0, None, None)
    );
}

fn render_status(rows: usize, cols: usize, config: &Config) {
    let lines = [
        format!("zellij-smart-tabs v{}", env!("CARGO_PKG_VERSION")),
        " ".to_string(),
        format!("Format:     {}", config.format),
        format!("Poll:       {}s", config.poll_interval),
        format!("Debug:      {}", if config.debug { "on" } else { "off" }),
    ];
    for (i, line) in lines.iter().take(rows).enumerate() {
        let text = if i == 0 {
            Text::new(line).color_all(0)
        } else {
            Text::new(line)
        };
        print_text_with_coordinates(text, 0, 1 + i, Some(cols), None);
    }
}

fn render_tabs(
    rows: usize,
    cols: usize,
    tab_store: &TabStore,
    pane_store: &PaneStore,
    last_rename: &Option<String>,
) {
    let mut tabs: Vec<_> = tab_store.tabs.values().collect();
    tabs.sort_by_key(|t| t.position);

    let mut table =
        Table::new().add_row(vec!["Pos", "Name", "CWD", "Git Root", "Program", "Managed"]);

    for tab in &tabs {
        let panes = pane_store.panes_for_tab(tab.tab_id);
        let first = panes.first();
        let short_dir = first.and_then(|p| p.short_dir.as_deref()).unwrap_or("-");
        let git_root = first
            .and_then(|p| p.short_git_root.as_deref())
            .unwrap_or("-");
        let program = first.and_then(|p| p.program.as_deref()).unwrap_or("-");
        let managed = if tab.is_managed { "true" } else { "false" };
        table = table.add_row(vec![
            &tab.position.to_string(),
            nonempty(&tab.name),
            nonempty(short_dir),
            nonempty(git_root),
            nonempty(program),
            managed,
        ]);
    }

    if let Some(rename) = last_rename {
        let avail = rows.saturating_sub(2);
        print_table_with_coordinates(table, 0, 1, Some(cols), Some(avail));
        print_text_with_coordinates(
            Text::new(format!("Last: {}", rename)).dim_all(),
            0,
            1 + avail,
            Some(cols),
            None,
        );
    } else {
        print_table_with_coordinates(table, 0, 1, Some(cols), Some(rows));
    };
}

fn render_panes(rows: usize, cols: usize, tab_store: &TabStore, pane_store: &PaneStore) {
    let mut all_tabs: Vec<_> = tab_store.tabs.values().collect();
    all_tabs.sort_by_key(|t| t.position);

    let mut table = Table::new().add_row(vec![
        "ID",
        "Tab",
        "Pos",
        "CWD",
        "Git Root",
        "Program",
        "Terminal Cmd",
        "Running Cmd",
        "Status",
        "On Focus",
    ]);

    for tab in &all_tabs {
        let panes = pane_store.panes_for_tab(tab.tab_id);
        let tab_idx = tab.position.to_string();
        for p in &panes {
            let id = p.pane_id.to_string();
            let pos = p.position.to_string();
            table = table.add_row(vec![
                id.as_str(),
                tab_idx.as_str(),
                pos.as_str(),
                nonempty(p.short_dir.as_deref().unwrap_or("-")),
                nonempty(p.short_git_root.as_deref().unwrap_or("-")),
                nonempty(p.program.as_deref().unwrap_or("-")),
                nonempty(p.terminal_command.as_deref().unwrap_or("-")),
                nonempty(p.running_command.as_deref().unwrap_or("-")),
                nonempty(p.status.as_str()),
                nonempty(p.on_focus.as_deref().unwrap_or("-")),
            ]);
        }
    }

    print_table_with_coordinates(table, 0, 1, Some(cols), Some(rows));
}

/// Build the Help view content. Kept separate from rendering so the scroll
/// bound can be derived from the same line set (see [`help_line_count`]).
fn help_lines(config: &Config) -> Vec<Text> {
    let mut lines: Vec<Text> = Vec::new();

    lines.push(Text::new("Template Variables").color_all(0));
    lines.push(Text::new("Top-level (aliases for pane[0].*)").dim_all());
    lines.push(Text::new("  {{ short_dir }}       Last component of CWD"));
    lines.push(Text::new(
        "  {{ cwd }}             Full working directory path",
    ));
    lines.push(Text::new(
        "  {{ git_root }}        Git repository root path",
    ));
    lines.push(Text::new(
        "  {{ short_git_root }}  Last component of git root",
    ));
    lines.push(Text::new("  {{ program }}         Running program name"));
    lines.push(Text::new(" "));
    lines.push(Text::new("Pane-scoped access:").dim_all());
    lines.push(Text::new("  {{ pane[0].short_dir }}       First pane"));
    lines.push(Text::new("  {{ pane[1].short_dir }}       Second pane"));
    lines.push(Text::new("  {{ pane[-1].program }}       Last pane"));
    lines.push(Text::new(" "));
    lines.push(Text::new("Keyboard Shortcuts").color_all(0));
    lines.push(Text::new("  1-4         Switch view"));
    lines.push(Text::new("  Tab         Next view"));
    lines.push(Text::new("  j / Down    Scroll down"));
    lines.push(Text::new("  k / Up      Scroll up"));
    lines.push(Text::new("  g           Scroll to top"));
    lines.push(Text::new("  G           Scroll to bottom"));
    lines.push(Text::new("  Esc         Hide plugin pane"));
    lines.push(Text::new(" "));
    lines.push(Text::new("Config Reference").color_all(0));
    lines.push(Text::new(format!(
        "  format:        Tab name template (current: {})",
        config.format
    )));
    lines.push(Text::new(format!(
        "  poll_interval: Timer interval in seconds (current: {}s)",
        config.poll_interval
    )));
    lines.push(Text::new(format!(
        "  debug:         Enable debug logging (current: {})",
        config.debug
    )));
    lines
}

/// Number of lines in the Help view, used to bound scrolling.
pub fn help_line_count(config: &Config) -> usize {
    help_lines(config).len()
}

fn render_help(rows: usize, cols: usize, scroll: usize, config: &Config) {
    let lines = help_lines(config);
    // Defensive guard: `clamp_active_scroll` in main.rs already bounds the
    // stored offset against the viewport before this is called, so the normal
    // path is a no-op here. Tighten against the bare line count so direct
    // callers (tests) cannot render a blank page.
    let scroll = scroll.min(lines.len().saturating_sub(1));
    for (i, line) in lines.iter().skip(scroll).take(rows).enumerate() {
        print_text_with_coordinates(line.clone(), 0, 1 + i, Some(cols), None);
    }
}

pub fn render_version_error(rows: usize, cols: usize, error: &str) {
    if rows < 3 || cols < 10 {
        return;
    }
    let (major, minor, patch) = crate::MIN_ZELLIJ_VERSION;
    let upgrade_line = format!("Please upgrade Zellij to {major}.{minor}.{patch} or later.");
    let lines = [
        "zellij-smart-tabs",
        "",
        error,
        "",
        upgrade_line.as_str(),
        "https://zellij.dev/documentation/installation",
    ];
    for (i, line) in lines.iter().take(rows).enumerate() {
        let text = if i == 0 {
            Text::new(*line).color_all(0)
        } else if i == 2 {
            Text::new(*line).color_all(1)
        } else {
            Text::new(*line)
        };
        print_text_with_coordinates(text, 0, i, Some(cols), None);
    }
}

fn render_shortcuts(rows: usize, cols: usize) {
    let shortcuts = "1-4:View  Tab:Next  j/k:Scroll  g/G:Top/Bot  R:Reload  Esc:Hide";
    print_text_with_coordinates(
        Text::new(shortcuts).dim_all(),
        0,
        rows.saturating_sub(1),
        Some(cols),
        None,
    );
}

fn render_confirm_reload(rows: usize, cols: usize) {
    let prompt = "Reload plugin? (R/y to confirm, any other key to cancel)";
    print_text_with_coordinates(
        Text::new(prompt).color_range(3, 0..prompt.len()),
        0,
        rows.saturating_sub(1),
        Some(cols),
        None,
    );
}

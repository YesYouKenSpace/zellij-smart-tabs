use crate::config::Config;
use crate::tab_state::{PaneStore, TabStore};
use std::collections::VecDeque;
use zellij_tile::prelude::*;

pub const VIEW_COUNT: usize = 5;

fn nonempty(s: &str) -> &str {
    if s.is_empty() { " " } else { s }
}

pub const APPROX_TAB_WIDTH: usize = 12;
const VIEW_NAMES: [&str; VIEW_COUNT] = ["Status", "Tabs", "Panes", "Log", "Help"];

#[allow(clippy::too_many_arguments)]
pub fn render_dashboard(
    rows: usize,
    cols: usize,
    active_view: usize,
    scroll_offsets: &[usize; VIEW_COUNT],
    config: &Config,
    tab_store: &TabStore,
    pane_store: &PaneStore,
    log_buffer: &VecDeque<String>,
    last_rename: &Option<String>,
) {
    if rows < 3 || cols < 10 {
        return;
    }
    render_tab_bar(active_view);
    let content_rows = rows.saturating_sub(2);
    let scroll = scroll_offsets[active_view];

    match active_view {
        0 => render_status(content_rows, cols, config),
        1 => render_tabs(content_rows, cols, tab_store, pane_store, last_rename),
        2 => render_panes(content_rows, cols, tab_store, pane_store),
        3 => render_log(content_rows, cols, scroll, log_buffer),
        4 => render_help(content_rows, cols, scroll, config),
        _ => {}
    }

    render_shortcuts(rows, cols);
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
        String::new(),
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

fn render_panes(
    rows: usize,
    cols: usize,
    tab_store: &TabStore,
    pane_store: &PaneStore,
) {
    let mut all_tabs: Vec<_> = tab_store.tabs.values().collect();
    all_tabs.sort_by_key(|t| t.position);

    let mut table = Table::new().add_row(vec![
        "ID", "Tab", "Pos", "CWD", "Git Root", "Program", "Terminal Cmd", "Running Cmd", "Status",
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
            ]);
        }
    }

    print_table_with_coordinates(table, 0, 1, Some(cols), Some(rows));
}

fn render_log(rows: usize, cols: usize, scroll: usize, log_buffer: &VecDeque<String>) {
    if log_buffer.is_empty() {
        print_text_with_coordinates(
            Text::new("No log entries. Enable debug=\"true\" in plugin config.").dim_all(),
            0,
            1,
            Some(cols),
            None,
        );
        return;
    }

    let total = log_buffer.len();
    let end = total.saturating_sub(scroll);
    let start = end.saturating_sub(rows);

    for (i, entry) in log_buffer.iter().skip(start).take(end - start).enumerate() {
        print_text_with_coordinates(Text::new(entry), 0, 1 + i, Some(cols), None);
    }
}

fn render_help(rows: usize, cols: usize, scroll: usize, config: &Config) {
    let mut lines: Vec<Text> = Vec::new();

    lines.push(Text::new("Template Variables").color_all(0));
    lines.push(Text::new("Top-level (aliases for pane['0'].*)").dim_all());
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
    lines.push(Text::new(""));
    lines.push(Text::new("Pane-scoped access:").dim_all());
    lines.push(Text::new("  {{ pane.last.program }}         Last pane"));
    lines.push(Text::new("  {{ pane['0'].short_dir }}       First pane"));
    lines.push(Text::new("  {{ pane['1'].short_dir }}       Second pane"));
    lines.push(Text::new(""));
    lines.push(Text::new("Keyboard Shortcuts").color_all(0));
    lines.push(Text::new("  1-5         Switch view"));
    lines.push(Text::new("  Tab         Next view"));
    lines.push(Text::new("  j / Down    Scroll down"));
    lines.push(Text::new("  k / Up      Scroll up"));
    lines.push(Text::new("  g           Scroll to top"));
    lines.push(Text::new("  G           Scroll to bottom"));
    lines.push(Text::new("  Esc         Hide plugin pane"));
    lines.push(Text::new(""));
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

    for (i, line) in lines.iter().skip(scroll).take(rows).enumerate() {
        print_text_with_coordinates(line.clone(), 0, 1 + i, Some(cols), None);
    }
}

pub fn render_version_error(rows: usize, cols: usize, error: &str) {
    if rows < 3 || cols < 10 {
        return;
    }
    let lines = [
        "zellij-smart-tabs",
        "",
        error,
        "",
        "Please upgrade Zellij to 0.44.0 or later.",
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
    let shortcuts = "1-5:View  Tab:Next  j/k:Scroll  g/G:Top/Bot  Esc:Hide";
    print_text_with_coordinates(
        Text::new(shortcuts).dim_all(),
        0, rows.saturating_sub(1), Some(cols), None,
    );
}

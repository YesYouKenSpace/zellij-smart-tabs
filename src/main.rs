mod config;
mod host;
mod logging;
mod tab_state;
mod template;
mod ui;
mod utils;

use std::collections::{BTreeMap, HashSet};
use zellij_tile::prelude::*;

use config::Config;
#[cfg(not(test))]
use host::RealZellijHost;
use host::ZellijHost;

use log::{debug, error, warn};
use tab_state::{PaneState, PaneStore, TabStore};
use utils::{extract_program, parse_git_root};

const CTX_PANE_ID: &str = "pane_id";
const CTX_COMMAND_TYPE: &str = "command_type";
const CMD_GIT_ROOT: &str = "git_root";
const PIPE_SET_MANUAL: &str = "set_focused_to_manual";
const PIPE_SET_MANAGED: &str = "set_focused_to_managed";
const PIPE_PANE_STATUS: &str = "pane_status";

struct ZellijSmartTabsPlugin {
    host: Box<dyn ZellijHost>,
    config: Option<Config>,
    tab_store: TabStore,
    pane_store: PaneStore,
    permissions_granted: bool,
    /// Tabs scheduled for rename on the next timer tick.
    /// Acts as a debounce — multiple events within one tick coalesce into a single rename.
    pending_renames: HashSet<usize>,
    /// Count of Zellij timers currently in flight. Zellij's `set_timeout` cannot
    /// be cancelled or deduplicated — each call yields exactly one future Timer
    /// event — so the Timer handler re-arms only when this reaches zero. That
    /// collapses a piled-up heartbeat plus debounce timer back into a single
    /// chain instead of each spawning a successor, which would leak timer chains
    /// for the life of the session. Bounded at 2 (one heartbeat + one debounce).
    outstanding_timers: u32,
    active_view: usize,
    scroll_offsets: [usize; ui::VIEW_COUNT],
    last_rename: Option<String>,
    version_error: Option<String>,
    /// Cached $HOME for tilde-replacing display paths. None if env unavailable.
    home_dir: Option<String>,
    confirm_reload: bool,
}

#[cfg(not(test))]
impl Default for ZellijSmartTabsPlugin {
    fn default() -> Self {
        Self {
            host: Box::new(RealZellijHost),
            config: None,
            tab_store: TabStore::default(),
            pane_store: PaneStore::default(),
            permissions_granted: false,
            pending_renames: HashSet::new(),
            outstanding_timers: 0,
            active_view: 0,
            scroll_offsets: [0; ui::VIEW_COUNT],
            last_rename: None,
            version_error: None,
            home_dir: std::env::var("HOME").ok(),
            confirm_reload: false,
        }
    }
}

const MIN_ZELLIJ_VERSION: (u32, u32, u32) = (0, 44, 2);

fn parse_semver(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()
        .and_then(|p| p.split('-').next().unwrap_or(p).parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

#[cfg(not(test))]
fn check_zellij_version() -> Option<String> {
    let version = get_zellij_version();
    match parse_semver(&version) {
        Some(v) if v >= MIN_ZELLIJ_VERSION => None,
        Some(_) => Some(format!(
            "zellij-smart-tabs requires Zellij {}.{}.{} or later, but found {}",
            MIN_ZELLIJ_VERSION.0, MIN_ZELLIJ_VERSION.1, MIN_ZELLIJ_VERSION.2, version
        )),
        None => Some(format!(
            "zellij-smart-tabs could not parse Zellij version: {:?}",
            version
        )),
    }
}

#[cfg(not(test))]
register_plugin!(ZellijSmartTabsPlugin);

impl ZellijSmartTabsPlugin {
    fn sub_prog(&self, program: String) -> String {
        self.config()
            .substitutions
            .program
            .get(&program)
            .cloned()
            .unwrap_or(program)
    }

    /// Resolve the display program from command tokens: pick the meaningful
    /// program (skipping wrappers via `skip_programs`) then apply substitutions.
    fn resolve_program(&self, tokens: &[&str]) -> Option<String> {
        extract_program(tokens, &self.config().skip_programs).map(|p| self.sub_prog(p))
    }

    /// Record a pane's latest command: store the raw `running_command` and refresh
    /// the derived `program`. Returns the owning tab_id when `program` actually
    /// changed, so the caller can queue a rename. `reason` labels the update
    /// source in logs (`"event"` / `"poll"`).
    fn update_pane_program(
        &mut self,
        pane_id: u32,
        command: &[String],
        reason: &str,
    ) -> Option<usize> {
        let tokens: Vec<&str> = command.iter().map(|s| s.as_str()).collect();
        let new_program = self.resolve_program(&tokens);
        let pane = self.pane_store.panes.get_mut(&pane_id)?;
        pane.running_command = Some(command.join(" "));
        if pane.program == new_program {
            return None;
        }
        debug!(
            pane_id = pane_id,
            program = new_program.as_deref().unwrap_or("-"),
            source = reason;
            "program updated"
        );
        let tab_id = pane.tab_id;
        pane.program = new_program;
        Some(tab_id)
    }

    fn warn_format_error(&self) {
        if let Some(err) = &self.config().format_error {
            warn!(err = err.as_str(); "invalid format template, using default");
        }
    }

    fn initialize(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Some(Config::from_map(&configuration));
        logging::init();
        logging::set_debug(self.config().debug);
        self.warn_format_error();
        debug!(format = self.config().format.as_str(), poll_interval = self.config().poll_interval; "initialized");
    }

    fn config(&self) -> &Config {
        self.config.as_ref().expect("config not initialized")
    }

    /// Arm one Zellij timer and count it. Callers gate this so timers don't
    /// accumulate — see [`Self::outstanding_timers`].
    fn arm(&mut self, interval: f64) {
        self.outstanding_timers += 1;
        self.host.set_timeout(interval);
    }

    /// Ensure the low-frequency poll heartbeat is running. No-op if any timer is
    /// already in flight (it will re-arm on fire) — used after permission grant.
    fn ensure_heartbeat(&mut self) {
        if self.permissions_granted && self.outstanding_timers == 0 {
            self.arm(self.config().poll_interval);
        }
    }

    fn request_git_info(&self, pane_id: u32, cwd: &str) {
        let mut ctx = BTreeMap::new();
        ctx.insert(CTX_PANE_ID.into(), pane_id.to_string());
        ctx.insert(CTX_COMMAND_TYPE.into(), CMD_GIT_ROOT.into());
        self.host.run_command(
            vec!["git".into(), "rev-parse".into(), "--show-toplevel".into()],
            BTreeMap::new(),
            std::path::PathBuf::from(cwd),
            ctx,
        );
    }

    fn build_template_context(&self, tab_id: usize) -> minijinja::Value {
        let panes = self.pane_store.panes_for_tab(tab_id);
        let status_subs = &self.config().substitutions.status;

        let pane_to_json = |p: &PaneState| -> serde_json::Value {
            let status = status_subs
                .get(p.status.as_str())
                .cloned()
                .unwrap_or_else(|| p.status.as_str().to_string());
            serde_json::json!({
                "cwd": p.cwd,
                "short_dir": p.short_dir,
                "git_root": p.git_root,
                "short_git_root": p.short_git_root,
                "program": p.program,
                "status": status,
            })
        };

        let pane_array: Vec<serde_json::Value> = panes.iter().map(|p| pane_to_json(p)).collect();

        // Top-level aliases from first pane
        let mut ctx = match pane_array.first() {
            Some(serde_json::Value::Object(first)) => first.clone(),
            _ => serde_json::Map::new(),
        };
        ctx.insert("pane".into(), serde_json::Value::Array(pane_array));

        minijinja::Value::from_serialize(&ctx)
    }

    fn rename_tab_for(&mut self, tab_id: usize) {
        let state = match self.tab_store.tabs.get(&tab_id) {
            Some(s) if s.is_managed => s,
            _ => return,
        };
        let has_cwd = self
            .pane_store
            .panes_for_tab(tab_id)
            .iter()
            .any(|p| p.cwd.is_some());
        if !has_cwd {
            return;
        }

        let ctx = self.build_template_context(tab_id);
        let name = template::render(&self.config().format, &ctx);
        if !name.is_empty() && state.name != name {
            debug!(tab_id = tab_id, name = name.as_str(); "rename tab");
            self.host.rename_tab(tab_id as u64, name.clone());
            self.last_rename = Some(format!("tab {} -> {:?}", tab_id, name));
            if let Some(state) = self.tab_store.tabs.get_mut(&tab_id) {
                state.name = name;
            }
        }
    }

    /// Queue a tab for rename without arming a timer. Used inside the Timer
    /// handler, which re-arms once at the end; arming here would stack timers.
    fn mark_pending(&mut self, tab_id: usize) {
        self.pending_renames.insert(tab_id);
    }

    /// Queue a tab for rename from an event context, arming a debounce timer if
    /// none is chasing this batch yet. The `was_empty` guard means a burst of
    /// events coalesces into a single debounce timer per drain cycle.
    fn schedule_rename(&mut self, tab_id: usize) {
        let was_empty = self.pending_renames.is_empty();
        self.pending_renames.insert(tab_id);
        if was_empty && self.permissions_granted {
            self.arm(self.config().debounce);
        }
    }

    fn schedule_rename_all(&mut self) {
        for tab_id in self.tab_store.auto_renameable() {
            self.schedule_rename(tab_id);
        }
    }

    fn tick_pending_renames(&mut self) {
        let tab_ids: Vec<usize> = self.pending_renames.drain().collect();
        for tab_id in tab_ids {
            self.rename_tab_for(tab_id);
        }
    }

    #[cfg(test)]
    fn flush_pending_renames(&mut self) {
        self.tick_pending_renames();
    }

    fn handle_tab_update(&mut self, tabs: Vec<TabInfo>) {
        let tab_infos: Vec<(usize, usize, String, bool)> = tabs
            .iter()
            .map(|t| (t.tab_id, t.position, t.name.clone(), t.active))
            .collect();

        let needs_rename = self.tab_store.sync_tabs(&tab_infos);

        self.pane_store
            .panes
            .retain(|_, p| self.tab_store.tabs.contains_key(&p.tab_id));

        for tab_id in needs_rename {
            self.rename_tab_for(tab_id);
        }
    }

    fn handle_pane_update(&mut self, manifest: PaneManifest) {
        let mut seen_pane_ids = HashSet::new();
        let mut changed_tabs = HashSet::new();

        for (tab_position, panes) in &manifest.panes {
            let tab_id = match self.tab_store.tab_id_at_position(*tab_position) {
                Some(id) => id,
                None => continue,
            };
            let tab = match self.tab_store.tabs.get(&tab_id) {
                Some(t) => t,
                None => continue,
            };

            // Sort by visual position. Floating panes are excluded: they are
            // transient overlays whose coordinates overlap the tiled grid, so a
            // floating scratch terminal could otherwise sort into position 0 and
            // hijack the tab name.
            let mut terminal_panes: Vec<&PaneInfo> = panes
                .iter()
                .filter(|p| !p.is_plugin && !p.is_suppressed && !p.is_floating)
                .collect();
            terminal_panes.sort_by(|a, b| a.pane_x.cmp(&b.pane_x).then(a.pane_y.cmp(&b.pane_y)));

            for (pos, pane) in terminal_panes.iter().enumerate() {
                seen_pane_ids.insert(pane.id);

                let is_command_pane = pane.terminal_command.is_some();
                let program = if is_command_pane {
                    let tokens: Vec<&str> = pane
                        .terminal_command
                        .as_deref()
                        .map(|s| s.split_whitespace().collect())
                        .unwrap_or_default();
                    self.resolve_program(&tokens)
                } else {
                    None
                };

                if let Some(existing) = self.pane_store.panes.get_mut(&pane.id) {
                    let mut changed = false;
                    if existing.tab_id != tab.tab_id {
                        // The tab the pane left must also re-render (it lost a pane).
                        changed_tabs.insert(existing.tab_id);
                        existing.tab_id = tab.tab_id;
                        changed = true;
                    }
                    if existing.position != pos {
                        existing.position = pos;
                        changed = true;
                    }
                    if existing.terminal_command != pane.terminal_command {
                        existing.terminal_command = pane.terminal_command.clone();
                        changed = true;
                    }
                    if is_command_pane && existing.program != program {
                        existing.program = program;
                        changed = true;
                    }
                    // Apply on_focus when both tab and pane are focused (one-shot: take clears it)
                    if pane.is_focused && tab.is_active
                        && let Some(new_status) = existing.on_focus.take()
                    {
                        debug!(pane_id = pane.id, from = existing.status.as_str(), to = new_status.as_str(); "on_focus applied");
                        existing.status = new_status;
                        changed = true;
                    }
                    if changed {
                        changed_tabs.insert(tab.tab_id);
                    }
                } else {
                    self.pane_store.panes.insert(
                        pane.id,
                        PaneState {
                            pane_id: pane.id,
                            tab_id: tab.tab_id,
                            position: pos,
                            cwd: None,
                            short_dir: None,
                            git_root: None,
                            short_git_root: None,
                            raw_cwd: None,
                            raw_git_root: None,
                            program,
                            terminal_command: pane.terminal_command.clone(),
                            running_command: None,
                            status: tab_state::DEFAULT_STATUS.to_string(),
                            on_focus: None,
                        },
                    );
                    changed_tabs.insert(tab.tab_id);
                }
            }
        }

        // Tabs that lost a pane (closed pane) must re-render too — a stale
        // segment (e.g. a format referencing the closed pane) would otherwise
        // persist until an unrelated event.
        for (&id, pane) in self.pane_store.panes.iter() {
            if !seen_pane_ids.contains(&id) {
                changed_tabs.insert(pane.tab_id);
            }
        }

        self.pane_store
            .panes
            .retain(|id, _| seen_pane_ids.contains(id));
        for tab_id in changed_tabs {
            self.schedule_rename(tab_id);
        }
    }

    fn handle_cwd_changed(&mut self, pane_id: u32, cwd: std::path::PathBuf) {
        let cwd_str = cwd.to_string_lossy().to_string();
        let tab_id = match self.pane_store.panes.get(&pane_id) {
            Some(p) => p.tab_id,
            None => return,
        };

        let changed = if let Some(pane) = self.pane_store.panes.get_mut(&pane_id) {
            if pane.raw_cwd.as_ref() != Some(&cwd_str) {
                debug!(pane_id = pane_id, tab_id = tab_id, cwd = cwd_str.as_str(); "cwd changed");
                pane.set_cwd(cwd_str.clone(), self.home_dir.as_deref());
                self.request_git_info(pane_id, &cwd_str);
                true
            } else {
                false
            }
        } else {
            false
        };

        if changed {
            self.schedule_rename(tab_id);
        }
    }

    fn handle_run_command_result(
        &mut self,
        exit_code: Option<i32>,
        stdout: Vec<u8>,
        _stderr: Vec<u8>,
        context: BTreeMap<String, String>,
    ) {
        let pane_id = match context.get(CTX_PANE_ID).and_then(|s| s.parse::<u32>().ok()) {
            Some(id) => id,
            None => return,
        };
        let cmd_type = match context.get(CTX_COMMAND_TYPE) {
            Some(t) => t.as_str(),
            None => return,
        };
        let success = exit_code == Some(0);

        let tab_id = match self.pane_store.panes.get(&pane_id) {
            Some(p) => p.tab_id,
            None => return,
        };

        let changed = if let Some(pane) = self.pane_store.panes.get_mut(&pane_id) {
            match cmd_type {
                CMD_GIT_ROOT => {
                    if success {
                        if let Some(root) = parse_git_root(&stdout) {
                            if pane.raw_git_root.as_ref() != Some(&root) {
                                debug!(pane_id = pane_id, tab_id = tab_id, git_root = root.as_str(); "git_root changed");
                                pane.set_git_root(root, self.home_dir.as_deref());
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else if pane.git_root.is_some() {
                        debug!(pane_id = pane_id, tab_id = tab_id; "git_root cleared");
                        pane.clear_git();
                        true
                    } else {
                        false
                    }
                }
                _ => return,
            }
        } else {
            return;
        };

        if changed {
            self.schedule_rename(tab_id);
        }
    }

    /// Polls panes missing CWD, program, or git data.
    /// Fallback for panes that existed before the plugin loaded and never received
    /// CwdChanged/CommandChanged events, and for git repos created after the last
    /// CWD change (no Zellij event covers `git init` in place).
    fn poll_missing_pane_data(&mut self) {
        let mut changed_tabs = HashSet::new();

        // Snapshot before the CWD loop below: panes whose CWD gets filled in
        // this tick already request git info there.
        let panes_missing_git: Vec<(u32, String)> = self
            .pane_store
            .panes
            .iter()
            .filter(|(_, p)| p.raw_git_root.is_none())
            .filter_map(|(&id, p)| p.raw_cwd.clone().map(|cwd| (id, cwd)))
            .collect();
        for (pane_id, cwd) in panes_missing_git {
            self.request_git_info(pane_id, &cwd);
        }

        let panes_missing_cwd: Vec<u32> = self
            .pane_store
            .panes
            .iter()
            .filter(|(_, p)| p.raw_cwd.is_none())
            .map(|(&id, _)| id)
            .collect();
        for pane_id in panes_missing_cwd {
            if let Ok(cwd) = self.host.get_pane_cwd(pane_id) {
                let cwd_str = cwd.to_string_lossy().to_string();
                if !cwd_str.is_empty() {
                    if let Some(pane) = self.pane_store.panes.get_mut(&pane_id) {
                        debug!(pane_id = pane_id, cwd = cwd_str.as_str(); "cwd polled");
                        let tab_id = pane.tab_id;
                        pane.set_cwd(cwd_str.clone(), self.home_dir.as_deref());
                        changed_tabs.insert(tab_id);
                    }
                    self.request_git_info(pane_id, &cwd_str);
                }
            }
        }

        let panes_missing_program: Vec<u32> = self
            .pane_store
            .panes
            .iter()
            .filter(|(_, p)| p.terminal_command.is_none() && p.program.is_none())
            .map(|(&id, _)| id)
            .collect();
        for pane_id in panes_missing_program {
            if let Ok(cmd) = self.host.get_pane_running_command(pane_id)
                && let Some(tab_id) = self.update_pane_program(pane_id, &cmd, "poll")
            {
                changed_tabs.insert(tab_id);
            }
        }

        // Runs inside the Timer handler, which re-arms once afterward — queue
        // without arming so poll-discovered renames don't stack extra timers.
        for tab_id in changed_tabs {
            self.mark_pending(tab_id);
        }
    }

    fn handle_cmd_changed(&mut self, pane_id: u32, command: Vec<String>) {
        if let Some(tab_id) = self.update_pane_program(pane_id, &command, "event") {
            self.schedule_rename(tab_id);
        }
    }

    fn handle_key(&mut self, key: KeyWithModifier) {
        if self.confirm_reload {
            self.confirm_reload = false;
            if key.has_no_modifiers()
                && matches!(key.bare_key, BareKey::Char('R') | BareKey::Char('y'))
            {
                self.host.reload_self();
            }
            return;
        }
        if key.has_no_modifiers() {
            match key.bare_key {
                BareKey::Char('1') => self.active_view = 0,
                BareKey::Char('2') => self.active_view = 1,
                BareKey::Char('3') => self.active_view = 2,
                BareKey::Char('4') => self.active_view = 3,
                BareKey::Tab => {
                    self.active_view = (self.active_view + 1) % ui::VIEW_COUNT;
                }
                BareKey::Char('j') | BareKey::Down => {
                    self.scroll_offsets[self.active_view] += 1;
                }
                BareKey::Char('k') | BareKey::Up => {
                    let offset = &mut self.scroll_offsets[self.active_view];
                    *offset = offset.saturating_sub(1);
                }
                BareKey::Char('g') => {
                    self.scroll_offsets[self.active_view] = 0;
                }
                BareKey::Char('G') => {
                    self.scroll_offsets[self.active_view] = 10000;
                }
                BareKey::Char('R') => {
                    self.confirm_reload = true;
                }
                BareKey::Esc => {
                    self.host.hide_self();
                }
                _ => {}
            }
        } else if key.bare_key == BareKey::Tab && key.has_modifiers(&[KeyModifier::Shift]) {
            self.active_view = (self.active_view + ui::VIEW_COUNT - 1) % ui::VIEW_COUNT;
        }
    }

    /// Bound the Help view's scroll offset to its content for the given viewport
    /// height. Prevents an over-scroll (e.g. the `G` sentinel) from rendering a
    /// blank page and keeps the stored offset in step so `k` responds at once.
    fn clamp_active_scroll(&mut self, rows: usize) {
        if self.active_view == ui::HELP_VIEW {
            let content_rows = rows.saturating_sub(2);
            let max_scroll = ui::help_line_count(self.config()).saturating_sub(content_rows);
            let offset = &mut self.scroll_offsets[self.active_view];
            *offset = (*offset).min(max_scroll);
        }
    }

    fn handle_mouse(&mut self, mouse: Mouse) {
        match mouse {
            Mouse::ScrollUp(_) => {
                let offset = &mut self.scroll_offsets[self.active_view];
                *offset = offset.saturating_sub(3);
            }
            Mouse::ScrollDown(_) => {
                self.scroll_offsets[self.active_view] += 3;
            }
            Mouse::LeftClick(0, col) => {
                // Each tab label is roughly " N Name " ≈ 12 chars
                let approx_view = col / ui::APPROX_TAB_WIDTH;
                if approx_view < ui::VIEW_COUNT {
                    self.active_view = approx_view;
                }
            }
            _ => {}
        }
    }
}

impl ZellijSmartTabsPlugin {
    fn handle_event(&mut self, event: Event) -> bool {
        if self.version_error.is_some() {
            if let Event::Key(key) = event {
                self.handle_key(key);
            }
            return true;
        }
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                debug!("permissions granted");
                self.permissions_granted = true;
                self.host.hide_self();
                self.schedule_rename_all();
                self.ensure_heartbeat();
                true
            }
            Event::TabUpdate(tabs) => {
                self.handle_tab_update(tabs);
                true
            }
            Event::PaneUpdate(manifest) => {
                self.handle_pane_update(manifest);
                true
            }
            Event::CwdChanged(PaneId::Terminal(pane_id), cwd, _) => {
                self.handle_cwd_changed(pane_id, cwd);
                true
            }
            // is_foreground=false carries the shell command after the
            // foreground command exits — handle both so the program clears.
            Event::CommandChanged(PaneId::Terminal(pane_id), command, _, _) => {
                self.handle_cmd_changed(pane_id, command);
                true
            }
            Event::RunCommandResult(exit_code, stdout, stderr, context) => {
                self.handle_run_command_result(exit_code, stdout, stderr, context);
                true
            }
            Event::Timer(_) => {
                // Account for the timer that just fired.
                self.outstanding_timers = self.outstanding_timers.saturating_sub(1);
                if self.permissions_granted {
                    self.poll_missing_pane_data();
                    self.tick_pending_renames();
                    // Re-arm only when this was the last timer in flight, so a
                    // heartbeat and a debounce that piled up collapse into one
                    // chain instead of each spawning a successor. `tick_pending_renames`
                    // drained the set, so the heart of this branch is exactly
                    // `ensure_heartbeat`.
                    self.ensure_heartbeat();
                }
                true
            }
            Event::PluginConfigurationChanged(configuration) => {
                self.config = Some(Config::from_map(&configuration));
                logging::set_debug(self.config().debug);
                debug!("config reloaded");
                self.warn_format_error();
                if self.permissions_granted {
                    self.schedule_rename_all();
                }
                true
            }
            Event::Key(key) => {
                self.handle_key(key);
                true
            }
            Event::Mouse(mouse) => {
                self.handle_mouse(mouse);
                true
            }
            _ => false,
        }
    }

    fn set_focused_managed(&mut self, managed: bool) {
        if let Some(tab_id) = self.host.get_focused_tab_id()
            && let Some(state) = self.tab_store.tabs.get_mut(&tab_id)
            && state.is_managed != managed
        {
            state.is_managed = managed;
            debug!(tab_id = tab_id, managed = managed; "tab management changed");
            if managed {
                self.schedule_rename(tab_id);
            }
        }
    }

    fn handle_pane_status(&mut self, payload: &str) {
        #[derive(serde::Deserialize)]
        struct StatusPayload {
            pane_id: String,
            status: String,
            on_focus: Option<String>,
        }

        let parsed: StatusPayload = match serde_json::from_str(payload) {
            Ok(p) => p,
            Err(e) => {
                error!(err = format!("{}", e).as_str(), payload = payload; "invalid pane_status payload");
                return;
            }
        };

        let pane_id: u32 = match parsed.pane_id.parse() {
            Ok(id) => id,
            Err(_) => {
                debug!(pane_id = parsed.pane_id.as_str(); "invalid pane_id in pane_status");
                return;
            }
        };

        let new_status = parsed.status;

        if let Some(pane) = self.pane_store.panes.get_mut(&pane_id) {
            let status_changed = pane.status != new_status;
            let on_focus_changed = pane.on_focus != parsed.on_focus;

            if status_changed || on_focus_changed {
                debug!(
                    pane_id = pane_id,
                    from = pane.status.as_str(),
                    to = new_status.as_str(),
                    on_focus_from = format!("{:?}", pane.on_focus).as_str(),
                    on_focus_to = format!("{:?}", parsed.on_focus).as_str();
                    "pane status changed"
                );
                let tab_id = pane.tab_id;
                pane.status = new_status;
                pane.on_focus = parsed.on_focus;
                if status_changed {
                    self.schedule_rename(tab_id);
                }
            }
        } else {
            debug!(pane_id = pane_id; "pane not found for pane_status");
        }
    }

    fn handle_pipe(&mut self, message: PipeMessage) -> bool {
        if !message.is_private {
            return false;
        }
        match message.name.as_str() {
            PIPE_SET_MANUAL => {
                self.set_focused_managed(false);
                true
            }
            PIPE_SET_MANAGED => {
                self.set_focused_managed(true);
                true
            }
            PIPE_PANE_STATUS => {
                if let Some(payload) = &message.payload {
                    self.handle_pane_status(payload);
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(not(test))]
impl ZellijPlugin for ZellijSmartTabsPlugin {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.version_error = check_zellij_version();
        if self.version_error.is_some() {
            show_self(true);
            subscribe(&[EventType::Key]);
            return;
        }

        show_self(true);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::RunCommands,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::CwdChanged,
            EventType::CommandChanged,
            EventType::Timer,
            EventType::PermissionRequestResult,
            EventType::RunCommandResult,
            EventType::PluginConfigurationChanged,
            EventType::Key,
            EventType::Mouse,
        ]);
        self.initialize(configuration);
    }

    // Delegates to handle_event() so tests can call the logic directly
    // without the ZellijPlugin trait (which requires WASM host functions).
    fn update(&mut self, event: Event) -> bool {
        self.handle_event(event)
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        self.handle_pipe(message)
    }

    fn render(&mut self, rows: usize, cols: usize) {
        if let Some(error) = &self.version_error {
            ui::render_version_error(rows, cols, error);
            return;
        }
        self.clamp_active_scroll(rows);
        ui::render_dashboard(
            rows,
            cols,
            &ui::DashboardContext {
                active_view: self.active_view,
                scroll_offsets: &self.scroll_offsets,
                config: self.config(),
                tab_store: &self.tab_store,
                pane_store: &self.pane_store,
                last_rename: &self.last_rename,
                confirm_reload: self.confirm_reload,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use host::MockZellijHost;
    use mockall::predicate::*;
    fn default_config() -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert("format".into(), "{{ short_dir }}".into());
        m
    }

    fn make_plugin(mock: MockZellijHost) -> ZellijSmartTabsPlugin {
        ZellijSmartTabsPlugin {
            host: Box::new(mock),
            config: None,
            tab_store: TabStore::default(),
            pane_store: PaneStore::default(),
            permissions_granted: false,
            pending_renames: HashSet::new(),
            outstanding_timers: 0,
            active_view: 0,
            scroll_offsets: [0; ui::VIEW_COUNT],
            last_rename: None,
            version_error: None,
            home_dir: None,
            confirm_reload: false,
        }
    }

    fn tab_info(tab_id: usize, position: usize, name: &str) -> TabInfo {
        TabInfo {
            tab_id,
            position,
            name: name.into(),
            active: true,
            ..Default::default()
        }
    }

    fn pane_info(id: u32, pane_x: usize, pane_y: usize) -> PaneInfo {
        PaneInfo {
            id,
            pane_x,
            pane_y,
            ..Default::default()
        }
    }

    fn pane_manifest(entries: Vec<(usize, Vec<PaneInfo>)>) -> PaneManifest {
        PaneManifest {
            panes: entries.into_iter().collect(),
        }
    }

    #[test]
    fn test_tab_rename_on_cwd_change() {
        let mut mock = MockZellijHost::new();

        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab()
            .with(eq(1u64), eq("my-project".to_string()))
            .times(1)
            .returning(|_, _| ());
        // git info requests
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        // 1. TabUpdate: register the tab
        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));

        // 2. PaneUpdate: register a pane in tab position 0
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));

        // 3. CwdChanged: set the pane's CWD, schedules rename
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/my-project"),
            vec![],
        ));

        // 4. Flush debounced renames
        plugin.flush_pending_renames();

        assert_eq!(plugin.tab_store.tabs.get(&1).unwrap().name, "my-project");
    }

    #[test]
    fn test_manual_tab_skips_auto_rename() {
        let mut mock = MockZellijHost::new();

        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().times(1).returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        // Setup: tab + pane + CWD → auto rename fires once
        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/my-project"),
            vec![],
        ));
        plugin.flush_pending_renames();

        // Set tab to unmanaged (manual)
        plugin.tab_store.tabs.get_mut(&1).unwrap().is_managed = false;

        // CWD change should NOT trigger another rename
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/other-project"),
            vec![],
        ));
        plugin.flush_pending_renames();
    }

    #[test]
    fn test_empty_name_restores_auto_management() {
        let mut mock = MockZellijHost::new();

        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        // Setup tab + pane + CWD
        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/my-project"),
            vec![],
        ));
        plugin.flush_pending_renames();

        // Set unmanaged (manual)
        plugin.tab_store.tabs.get_mut(&1).unwrap().is_managed = false;
        assert!(!plugin.tab_store.tabs.get(&1).unwrap().is_managed);

        // User clears tab name (empty string) → restores managed
        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "")]));
        assert!(plugin.tab_store.tabs.get(&1).unwrap().is_managed);
    }

    #[test]
    fn test_timer_polls_missing_cwd() {
        let mut mock = MockZellijHost::new();

        mock.expect_rename_tab()
            .with(eq(1u64), eq("fetched-dir".to_string()))
            .times(1)
            .returning(|_, _| ());
        mock.expect_get_pane_cwd()
            .with(eq(10u32))
            .returning(|_| Ok(std::path::PathBuf::from("/home/user/fetched-dir")));
        mock.expect_get_pane_running_command()
            .returning(|_| Ok(vec![]));
        mock.expect_run_command().returning(|_, _, _, _| ());
        mock.expect_set_timeout().returning(|_| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));
        assert!(plugin.pane_store.panes.get(&10).unwrap().cwd.is_none());

        plugin.handle_event(Event::Timer(0.0));
        plugin.flush_pending_renames();

        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.cwd.as_deref(), Some("/home/user/fetched-dir"));
    }

    #[test]
    fn test_program_tracks_command_changed_foreground_and_exit() {
        struct Step {
            label: &'static str,
            command: Vec<&'static str>,
            is_foreground: bool,
            expected_program: Option<&'static str>,
        }

        // Zellij emits (foreground_cmd, true) when a command starts and
        // (shell_cmd, false) when it exits back to the shell.
        let steps = vec![
            Step {
                label: "command starts",
                command: vec!["nvim", "src/main.rs"],
                is_foreground: true,
                expected_program: Some("\u{e6ae}"), // default nvim substitution
            },
            Step {
                label: "command exits back to shell",
                command: vec!["zsh"],
                is_foreground: false,
                expected_program: Some("\u{f489}"), // default zsh substitution
            },
        ];

        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));

        for step in steps {
            plugin.handle_event(Event::CommandChanged(
                PaneId::Terminal(10),
                step.command.iter().map(|s| s.to_string()).collect(),
                step.is_foreground,
                vec![],
            ));
            assert_eq!(
                plugin.pane_store.panes.get(&10).unwrap().program.as_deref(),
                step.expected_program,
                "{}",
                step.label
            );
        }
    }

    #[test]
    fn test_timer_polls_git_for_pane_with_cwd_but_no_git_root() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_get_pane_running_command()
            .returning(|_| Ok(vec![]));
        // Once from CwdChanged, once from the timer poll after the first
        // lookup came back negative (e.g. `git init` ran after the cd).
        mock.expect_run_command().times(2).returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/not-a-repo"),
            vec![],
        ));

        // git rev-parse failed: not a repo (yet)
        let mut ctx = BTreeMap::new();
        ctx.insert(CTX_PANE_ID.into(), "10".into());
        ctx.insert(CTX_COMMAND_TYPE.into(), CMD_GIT_ROOT.into());
        plugin.handle_event(Event::RunCommandResult(Some(128), vec![], vec![], ctx));
        assert!(plugin.pane_store.panes.get(&10).unwrap().git_root.is_none());

        // Timer poll must retry git detection for panes with cwd but no git root
        plugin.handle_event(Event::Timer(0.0));
    }

    #[test]
    fn test_tab_renames_when_referenced_pane_closes() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        let mut cfg = default_config();
        cfg.insert(
            "format".into(),
            "{{ short_dir }}{% if pane[1] %} | {{ pane[1].short_dir }}{% endif %}".into(),
        );
        plugin.config = Some(Config::from_map(&cfg));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0), pane_info(11, 1, 0)],
        )])));
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/a"),
            vec![],
        ));
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(11),
            std::path::PathBuf::from("/home/user/b"),
            vec![],
        ));
        plugin.flush_pending_renames();
        assert_eq!(plugin.tab_store.tabs.get(&1).unwrap().name, "a | b");

        // Closing the second pane must drop the stale " | b" segment.
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));
        plugin.flush_pending_renames();
        assert_eq!(plugin.tab_store.tabs.get(&1).unwrap().name, "a");
    }

    #[test]
    fn test_floating_panes_excluded_from_naming() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));

        // Floating pane sorts first by x but must not be tracked or named.
        let tiled = pane_info(10, 5, 0);
        let mut floating = pane_info(20, 0, 0);
        floating.is_floating = true;
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![floating, tiled],
        )])));

        let panes = plugin.pane_store.panes_for_tab(1);
        assert_eq!(panes.len(), 1, "only the tiled pane is tracked");
        assert_eq!(panes[0].pane_id, 10);
        assert!(
            plugin.pane_store.panes.get(&20).is_none(),
            "floating pane is not stored"
        );
    }

    #[test]
    fn test_help_scroll_clamps_and_stays_navigable() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.active_view = ui::HELP_VIEW;

        let rows = 10;
        let content_rows = rows - 2;
        let expected_max = ui::help_line_count(plugin.config()).saturating_sub(content_rows);
        assert!(expected_max > 0, "help content must overflow a 10-row pane");

        // `G` sets a large sentinel; clamp must bound it to the real maximum so
        // the page is never blank.
        plugin.handle_event(Event::Key(KeyWithModifier::new(BareKey::Char('G'))));
        assert_eq!(plugin.scroll_offsets[ui::HELP_VIEW], 10000);
        plugin.clamp_active_scroll(rows);
        assert_eq!(
            plugin.scroll_offsets[ui::HELP_VIEW], expected_max,
            "G clamps to the last full page"
        );

        // `k` from the clamped bottom moves up exactly one line (not stuck).
        plugin.handle_event(Event::Key(KeyWithModifier::new(BareKey::Char('k'))));
        assert_eq!(plugin.scroll_offsets[ui::HELP_VIEW], expected_max - 1);

        // `g` returns to the top.
        plugin.handle_event(Event::Key(KeyWithModifier::new(BareKey::Char('g'))));
        assert_eq!(plugin.scroll_offsets[ui::HELP_VIEW], 0);
    }

    #[test]
    fn test_permission_grant_arms_single_timer() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let count = Arc::new(AtomicUsize::new(0));
        let mut mock = MockZellijHost::new();
        mock.expect_hide_self().returning(|| ());
        let c = count.clone();
        mock.expect_set_timeout().returning(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));

        // A managed tab exists but nothing is pending or armed yet.
        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "no timers armed before permission grant"
        );

        // The grant must arm exactly one timer, not one per source (rename batch
        // + heartbeat). The old code armed two.
        plugin.handle_event(Event::PermissionRequestResult(PermissionStatus::Granted));
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "permission grant arms exactly one timer"
        );
        assert_eq!(plugin.outstanding_timers, 1);
    }

    #[test]
    fn test_idle_event_arms_debounce_not_poll_interval() {
        use std::sync::{Arc, Mutex};

        let intervals = Arc::new(Mutex::new(Vec::<f64>::new()));
        let mut mock = MockZellijHost::new();
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());
        mock.expect_get_pane_running_command()
            .returning(|_| Ok(vec![]));
        let iv = intervals.clone();
        mock.expect_set_timeout().returning(move |secs| {
            iv.lock().unwrap().push(secs);
        });

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/p"),
            vec![],
        ));
        // Drain to the idle state: a poll-interval heartbeat is outstanding.
        plugin.handle_event(Event::Timer(0.0));
        assert_eq!(
            *intervals.lock().unwrap().last().unwrap(),
            5.0,
            "idle heartbeat uses poll_interval"
        );

        // An event while idle must arm a debounce immediately so the rename
        // applies in ~0.2s, not up to poll_interval later.
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/moved"),
            vec![],
        ));
        assert_eq!(
            *intervals.lock().unwrap().last().unwrap(),
            0.2,
            "event while idle arms a debounce timer, not another poll_interval"
        );
    }

    #[test]
    fn test_timers_stay_bounded_under_event_storm() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let count = Arc::new(AtomicUsize::new(0));
        let mut mock = MockZellijHost::new();
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());
        mock.expect_get_pane_cwd()
            .returning(|_| Ok(std::path::PathBuf::from("/home/user/p")));
        mock.expect_get_pane_running_command()
            .returning(|_| Ok(vec![]));
        let c = count.clone();
        mock.expect_set_timeout().returning(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
        });

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));

        // Alternate an event and a timer tick many times. Each event may arm a
        // debounce while a heartbeat is mid-flight, but the two must collapse —
        // the count can never grow with the number of events.
        for i in 0..50 {
            plugin.handle_event(Event::CwdChanged(
                PaneId::Terminal(10),
                std::path::PathBuf::from(format!("/home/user/d{i}")),
                vec![],
            ));
            assert!(
                plugin.outstanding_timers <= 2,
                "timers bounded after event (iter {i}): {}",
                plugin.outstanding_timers
            );
            plugin.handle_event(Event::Timer(0.0));
            assert!(
                plugin.outstanding_timers <= 2,
                "timers bounded after tick (iter {i}): {}",
                plugin.outstanding_timers
            );
        }

        // A final idle tick collapses back to a single heartbeat timer.
        plugin.handle_event(Event::Timer(0.0));
        assert_eq!(
            plugin.outstanding_timers, 1,
            "collapses to one timer when idle"
        );
        // Total arms scale ~linearly with events (one debounce each), not with
        // an ever-growing set of self-sustaining chains.
        assert!(
            count.load(Ordering::SeqCst) <= 60,
            "no runaway timer growth: {}",
            count.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn test_set_focused_managed_targets_by_tab_id_not_position() {
        // tab_id and position diverge once tabs are closed/reordered.
        // The focused-tab lookup must use the stable tab_id returned by
        // get_focused_pane_info, not treat it as a position.
        struct Case {
            label: &'static str,
            pipe: &'static str,
            focused_tab_id: usize,
            initial_managed: bool,
            expected_managed: bool,
        }

        let cases = vec![
            Case {
                label: "manual on tab whose id != position",
                pipe: PIPE_SET_MANUAL,
                focused_tab_id: 6,
                initial_managed: true,
                expected_managed: false,
            },
            Case {
                label: "managed restore on tab whose id != position",
                pipe: PIPE_SET_MANAGED,
                focused_tab_id: 6,
                initial_managed: false,
                expected_managed: true,
            },
        ];

        for case in cases {
            let mut mock = MockZellijHost::new();
            mock.expect_set_timeout().returning(|_| ());
            mock.expect_rename_tab().returning(|_, _| ());
            mock.expect_run_command().returning(|_, _, _, _| ());
            let focused = case.focused_tab_id;
            mock.expect_get_focused_tab_id()
                .returning(move || Some(focused));

            let mut plugin = make_plugin(mock);
            plugin.config = Some(Config::from_map(&default_config()));
            plugin.permissions_granted = true;

            // Tabs 5 and 6 sit at positions 0 and 1 — ids never equal positions.
            plugin.handle_event(Event::TabUpdate(vec![
                tab_info(5, 0, "Tab #1"),
                tab_info(6, 1, "Tab #2"),
            ]));
            plugin
                .tab_store
                .tabs
                .get_mut(&case.focused_tab_id)
                .unwrap()
                .is_managed = case.initial_managed;

            plugin.handle_pipe(PipeMessage {
                source: PipeSource::Cli(String::new()),
                name: case.pipe.into(),
                payload: None,
                args: std::collections::BTreeMap::new(),
                is_private: true,
            });

            assert_eq!(
                plugin.tab_store.tabs.get(&6).unwrap().is_managed,
                case.expected_managed,
                "{}: focused tab",
                case.label
            );
            // The non-focused tab must be untouched.
            assert!(
                plugin.tab_store.tabs.get(&5).unwrap().is_managed,
                "{}: non-focused tab unchanged",
                case.label
            );
        }
    }

    #[test]
    fn test_esc_hides_plugin() {
        let mut mock = MockZellijHost::new();
        mock.expect_hide_self().times(1).returning(|| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));

        plugin.handle_event(Event::Key(KeyWithModifier::new(BareKey::Esc)));
    }

    #[test]
    fn test_permissions_granted_triggers_rename() {
        let mut mock = MockZellijHost::new();

        mock.expect_hide_self().times(1).returning(|| ());
        mock.expect_rename_tab()
            .with(eq(1u64), eq("my-project".to_string()))
            .times(1)
            .returning(|_, _| ());
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));

        // Events arrive before permissions — data stored, renames scheduled
        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/my-project"),
            vec![],
        ));

        // Permissions granted → schedules rename for all tabs
        plugin.handle_event(Event::PermissionRequestResult(PermissionStatus::Granted));
        assert!(plugin.permissions_granted);
        plugin.flush_pending_renames();
    }

    #[test]
    fn test_parse_semver() {
        assert_eq!(parse_semver("0.44.0"), Some((0, 44, 0)));
        assert_eq!(parse_semver("0.44.1"), Some((0, 44, 1)));
        assert_eq!(parse_semver("1.0.0"), Some((1, 0, 0)));
        assert_eq!(parse_semver("0.44.0-beta"), Some((0, 44, 0)));
        assert_eq!(parse_semver("0.43"), Some((0, 43, 0)));
        assert_eq!(parse_semver(""), None);
        assert_eq!(parse_semver("abc"), None);
    }

    #[test]
    fn test_version_check_passes() {
        assert!(parse_semver("0.44.2").unwrap() >= MIN_ZELLIJ_VERSION);
        assert!(parse_semver("0.45.0").unwrap() >= MIN_ZELLIJ_VERSION);
        assert!(parse_semver("1.0.0").unwrap() >= MIN_ZELLIJ_VERSION);
    }

    #[test]
    fn test_version_check_fails() {
        assert!(parse_semver("0.43.0").unwrap() < MIN_ZELLIJ_VERSION);
        assert!(parse_semver("0.43.9").unwrap() < MIN_ZELLIJ_VERSION);
    }

    #[test]
    fn test_pane_status_on_focus_stored() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));

        let payload = r#"{"pane_id":"10","status":"🔔 new","on_focus":"idle"}"#;
        plugin.handle_pipe(PipeMessage {
            source: PipeSource::Cli(String::new()),
            name: "pane_status".into(),
            payload: Some(payload.into()),
            args: std::collections::BTreeMap::new(),
            is_private: true,
        });

        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.status, "🔔 new");
        assert_eq!(pane.on_focus, Some("idle".into()));
    }

    #[test]
    fn test_pane_status_without_on_focus_is_noop() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));

        let payload = r#"{"pane_id":"10","status":"running"}"#;
        plugin.handle_pipe(PipeMessage {
            source: PipeSource::Cli(String::new()),
            name: "pane_status".into(),
            payload: Some(payload.into()),
            args: std::collections::BTreeMap::new(),
            is_private: true,
        });

        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.status, "running");
        assert_eq!(pane.on_focus, None);
    }

    #[test]
    fn test_pane_status_on_focus_overwritten_by_second_call() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));

        let payload1 = r#"{"pane_id":"10","status":"🔔 new","on_focus":"idle"}"#;
        plugin.handle_pipe(PipeMessage {
            source: PipeSource::Cli(String::new()),
            name: "pane_status".into(),
            payload: Some(payload1.into()),
            args: std::collections::BTreeMap::new(),
            is_private: true,
        });

        let payload2 = r#"{"pane_id":"10","status":"🔔 urgent","on_focus":"read"}"#;
        plugin.handle_pipe(PipeMessage {
            source: PipeSource::Cli(String::new()),
            name: "pane_status".into(),
            payload: Some(payload2.into()),
            args: std::collections::BTreeMap::new(),
            is_private: true,
        });

        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.status, "🔔 urgent");
        assert_eq!(pane.on_focus, Some("read".into()));
    }

    #[test]
    fn test_on_focus_status_applied_when_pane_focused() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));

        let payload = r#"{"pane_id":"10","status":"🔔 new","on_focus":"idle"}"#;
        plugin.handle_pipe(PipeMessage {
            source: PipeSource::Cli(String::new()),
            name: "pane_status".into(),
            payload: Some(payload.into()),
            args: std::collections::BTreeMap::new(),
            is_private: true,
        });

        // Simulate pane gaining focus via PaneUpdate
        let mut focused_pane = pane_info(10, 0, 0);
        focused_pane.is_focused = true;
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![focused_pane],
        )])));

        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.status, "idle");
        assert_eq!(pane.on_focus, None);
    }

    #[test]
    fn test_on_focus_status_is_one_shot() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));

        let payload = r#"{"pane_id":"10","status":"🔔 new","on_focus":"idle"}"#;
        plugin.handle_pipe(PipeMessage {
            source: PipeSource::Cli(String::new()),
            name: "pane_status".into(),
            payload: Some(payload.into()),
            args: std::collections::BTreeMap::new(),
            is_private: true,
        });

        // First focus — triggers on_focus
        let mut focused_pane = pane_info(10, 0, 0);
        focused_pane.is_focused = true;
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![focused_pane.clone()],
        )])));
        assert_eq!(plugin.pane_store.panes.get(&10).unwrap().status, "idle");

        // Manually set status to something else (simulating external update)
        plugin.pane_store.panes.get_mut(&10).unwrap().status = "custom".into();

        // Second focus — should NOT change status (on_focus already consumed)
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![focused_pane],
        )])));
        assert_eq!(plugin.pane_store.panes.get(&10).unwrap().status, "custom");
        assert_eq!(plugin.pane_store.panes.get(&10).unwrap().on_focus, None);
    }

    #[test]
    fn test_on_focus_not_applied_when_tab_inactive() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        // Tab is active initially
        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));

        // Set pane_status with on_focus
        let payload = r#"{"pane_id":"10","status":"🔔 new","on_focus":"idle"}"#;
        plugin.handle_pipe(PipeMessage {
            source: PipeSource::Cli(String::new()),
            name: "pane_status".into(),
            payload: Some(payload.into()),
            args: std::collections::BTreeMap::new(),
            is_private: true,
        });

        // Switch tab to inactive
        let mut inactive_tab = tab_info(1, 0, "Tab #1");
        inactive_tab.active = false;
        plugin.handle_event(Event::TabUpdate(vec![inactive_tab]));

        // Pane is focused but tab is inactive — on_focus should NOT trigger
        let mut focused_pane = pane_info(10, 0, 0);
        focused_pane.is_focused = true;
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![focused_pane],
        )])));

        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.status, "🔔 new");
        assert_eq!(pane.on_focus, Some("idle".into()));
    }

    fn make_plugin_with_home(
        mock: MockZellijHost,
        home_dir: Option<String>,
    ) -> ZellijSmartTabsPlugin {
        let mut plugin = make_plugin(mock);
        plugin.home_dir = home_dir;
        plugin
    }

    #[test]
    fn test_tilde_replacement_git_root() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout().returning(|_| ());
        mock.expect_rename_tab().returning(|_, _| ());
        mock.expect_run_command().returning(|_, _, _, _| ());

        let mut plugin = make_plugin_with_home(mock, Some("/home/user".into()));
        let mut cfg = default_config();
        cfg.insert("format".into(), "{{ short_git_root }}".into());
        plugin.config = Some(Config::from_map(&cfg));
        plugin.permissions_granted = true;

        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));
        plugin.handle_event(Event::CwdChanged(
            PaneId::Terminal(10),
            std::path::PathBuf::from("/home/user/project"),
            vec![],
        ));

        let mut ctx = BTreeMap::new();
        ctx.insert(CTX_PANE_ID.into(), "10".into());
        ctx.insert(CTX_COMMAND_TYPE.into(), CMD_GIT_ROOT.into());
        plugin.handle_event(Event::RunCommandResult(
            Some(0),
            b"/home/user/project\n".to_vec(),
            vec![],
            ctx,
        ));

        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(
            pane.git_root.as_deref(),
            Some("~/project"),
            "git_root display"
        );
        assert_eq!(
            pane.raw_git_root.as_deref(),
            Some("/home/user/project"),
            "raw_git_root"
        );
        assert_eq!(
            pane.short_git_root.as_deref(),
            Some("project"),
            "short_git_root"
        );
    }

    #[test]
    fn test_tilde_replacement_in_tab_rename() {
        struct Case {
            label: &'static str,
            cwd: &'static str,
            home: Option<String>,
            expected_rename: &'static str,
            expected_cwd: &'static str,
            expected_raw_cwd: &'static str,
            expected_short_dir: &'static str,
        }

        let cases = vec![
            Case {
                label: "home dir itself becomes ~",
                cwd: "/home/user",
                home: Some("/home/user".into()),
                expected_rename: "~",
                expected_cwd: "~",
                expected_raw_cwd: "/home/user",
                expected_short_dir: "~",
            },
            Case {
                label: "subdirectory keeps last component as short_dir",
                cwd: "/home/user/project",
                home: Some("/home/user".into()),
                expected_rename: "project",
                expected_cwd: "~/project",
                expected_raw_cwd: "/home/user/project",
                expected_short_dir: "project",
            },
            Case {
                label: "no home_dir leaves paths unchanged",
                cwd: "/home/user/project",
                home: None,
                expected_rename: "project",
                expected_cwd: "/home/user/project",
                expected_raw_cwd: "/home/user/project",
                expected_short_dir: "project",
            },
        ];

        for case in cases {
            let mut mock = MockZellijHost::new();
            mock.expect_set_timeout().returning(|_| ());
            mock.expect_rename_tab()
                .withf({
                    let expected = case.expected_rename.to_string();
                    move |_, name| name == &expected
                })
                .times(1)
                .returning(|_, _| ());
            mock.expect_run_command().returning(|_, _, _, _| ());

            let mut plugin = make_plugin_with_home(mock, case.home);
            plugin.config = Some(Config::from_map(&default_config()));
            plugin.permissions_granted = true;

            plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
            plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
                0,
                vec![pane_info(10, 0, 0)],
            )])));
            plugin.handle_event(Event::CwdChanged(
                PaneId::Terminal(10),
                std::path::PathBuf::from(case.cwd),
                vec![],
            ));
            plugin.flush_pending_renames();

            let pane = plugin.pane_store.panes.get(&10).unwrap();
            assert_eq!(
                pane.cwd.as_deref(),
                Some(case.expected_cwd),
                "{}: cwd",
                case.label
            );
            assert_eq!(
                pane.raw_cwd.as_deref(),
                Some(case.expected_raw_cwd),
                "{}: raw_cwd",
                case.label
            );
            assert_eq!(
                pane.short_dir.as_deref(),
                Some(case.expected_short_dir),
                "{}: short_dir",
                case.label
            );
        }
    }
}

mod config;
mod host;
mod tab_state;
mod template;
mod ui;
mod utils;

use std::collections::{hash_map::DefaultHasher, BTreeMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use zellij_tile::prelude::*;

use config::Config;
#[cfg(not(test))]
use host::RealZellijHost;
use host::ZellijHost;
use tab_state::{PaneState, PaneStore, TabStore};
use utils::{extract_program, parse_git_root};

const MAX_LOG_ENTRIES: usize = 100;

macro_rules! debug_log {
    ($self:expr, $($arg:tt)*) => {
        if $self.config.as_ref().map_or(false, |c| c.debug) {
            let msg = format!($($arg)*);
            eprintln!("[smart-tabs] {}", msg);
            $self.log_buffer.push_back(msg);
            if $self.log_buffer.len() > MAX_LOG_ENTRIES {
                $self.log_buffer.pop_front();
            }
        }
    };
}

fn pane_label(pane_store: &PaneStore, pane_id: u32) -> String {
    match pane_store.panes.get(&pane_id) {
        Some(p) => format!("pane {} of tab {}", p.position, p.tab_id),
        None => format!("pane ?{}", pane_id),
    }
}

fn viewport_hash(viewport: &[String]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for line in viewport {
        line.trim_end().hash(&mut hasher);
        '\n'.hash(&mut hasher);
    }
    hasher.finish()
}

const CTX_PANE_ID: &str = "pane_id";
const CTX_COMMAND_TYPE: &str = "command_type";
const CMD_GIT_ROOT: &str = "git_root";
const PIPE_SET_MANUAL: &str = "set_focused_to_manual";
const PIPE_SET_MANAGED: &str = "set_focused_to_managed";

struct ZellijSmartTabsPlugin {
    host: Box<dyn ZellijHost>,
    config: Option<Config>,
    tab_store: TabStore,
    pane_store: PaneStore,
    permissions_granted: bool,
    /// Tabs scheduled for rename on the next timer tick.
    /// Acts as a debounce — multiple events within one tick coalesce into a single rename.
    pending_renames: HashSet<usize>,
    /// Counts timer ticks between full polls. Poll-interval timers run a full
    /// poll immediately; debounce timers use this as a guard against over-polling.
    poll_ticks: u32,
    active_view: usize,
    scroll_offsets: [usize; 5],
    log_buffer: VecDeque<String>,
    last_rename: Option<String>,
    version_error: Option<String>,
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
            poll_ticks: 0,
            active_view: 0,
            scroll_offsets: [0; 5],
            log_buffer: VecDeque::new(),
            last_rename: None,
            version_error: None,
        }
    }
}

const MIN_ZELLIJ_VERSION: (u32, u32, u32) = (0, 44, 0);

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
    fn substitute_program(&self, program: Option<String>) -> (Option<String>, bool) {
        match program {
            Some(p) => match self.config().substitutions.program.get(&p) {
                Some(substitution) => (Some(substitution.clone()), true),
                None => (Some(p), false),
            },
            None => (None, false),
        }
    }

    fn initialize(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Some(Config::from_map(&configuration));
        debug_log!(
            self,
            "initialized: format={:?} poll={}s",
            self.config().format,
            self.config().poll_interval
        );
    }

    fn config(&self) -> &Config {
        self.config.as_ref().expect("config not initialized")
    }

    fn schedule_next_timer(&self) {
        if self.pending_renames.is_empty() {
            self.host.set_timeout(self.config().poll_interval);
        } else {
            self.host.set_timeout(self.config().debounce);
        }
    }

    fn poll_tick_threshold(&self) -> u32 {
        (self.config().poll_interval / self.config().debounce).ceil() as u32
    }

    fn needs_initial_poll(&self) -> bool {
        self.pane_store
            .panes
            .values()
            .any(|p| p.cwd.is_none() || p.screen_hash.is_none())
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

        let pane_to_json = |p: &PaneState| -> serde_json::Value {
            serde_json::json!({
                "cwd": p.cwd,
                "short_dir": p.short_dir,
                "git_root": p.git_root,
                "short_git_root": p.short_git_root,
                "program": p.program,
                "program_substituted": p.program_substituted,
                "screen_state": p.screen_state(),
                "screen_status": p.screen_status(),
                "screen_changed": p.screen_changed,
                "screen_quiet_ticks": p.screen_quiet_ticks,
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
            debug_log!(self, "rename tab {} -> {:?}", tab_id, name);
            self.host.rename_tab(tab_id as u64, name.clone());
            self.last_rename = Some(format!("tab {} -> {:?}", tab_id, name));
            if let Some(state) = self.tab_store.tabs.get_mut(&tab_id) {
                state.name = name;
            }
        }
    }

    fn schedule_rename(&mut self, tab_id: usize) {
        self.pending_renames.insert(tab_id);
    }

    fn schedule_rename_all(&mut self) {
        for tab_id in self.tab_store.auto_renameable() {
            self.schedule_rename(tab_id);
        }
    }

    /// Tick per-tab debounce counters. Tabs reaching 0 get renamed.
    /// Tabs that were re-scheduled keep waiting.
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
        let tab_infos: Vec<(usize, usize, String)> = tabs
            .iter()
            .map(|t| (t.tab_id, t.position, t.name.clone()))
            .collect();

        let needs_rename = self.tab_store.sync_tabs(&tab_infos);

        for &tab_id in &needs_rename {
            debug_log!(self, "new tab {}", tab_id);
        }

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

            // Sort by visual position
            let mut terminal_panes: Vec<&PaneInfo> = panes
                .iter()
                .filter(|p| !p.is_plugin && !p.is_suppressed)
                .collect();
            terminal_panes.sort_by(|a, b| a.pane_x.cmp(&b.pane_x).then(a.pane_y.cmp(&b.pane_y)));

            for (pos, pane) in terminal_panes.iter().enumerate() {
                seen_pane_ids.insert(pane.id);

                // For command panes, terminal_command is the definitive program source.
                // For regular terminal panes, program is polled via get_pane_running_command in the timer.
                let is_command_pane = pane.terminal_command.is_some();
                let (program, program_substituted) = if is_command_pane {
                    let tokens: Vec<&str> = pane
                        .terminal_command
                        .as_deref()
                        .map(|s| s.split_whitespace().collect())
                        .unwrap_or_default();
                    self.substitute_program(extract_program(&tokens, &self.config().skip_programs))
                } else {
                    (None, false)
                };

                if let Some(existing) = self.pane_store.panes.get_mut(&pane.id) {
                    let mut changed = false;
                    if existing.tab_id != tab.tab_id {
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
                    if is_command_pane
                        && (existing.program != program
                            || existing.program_substituted != program_substituted)
                    {
                        existing.program = program;
                        existing.program_substituted = program_substituted;
                        changed = true;
                    }
                    if changed {
                        changed_tabs.insert(tab.tab_id);
                    }
                } else {
                    self.pane_store.panes.insert(
                        pane.id,
                        PaneState::new(
                            pane.id,
                            tab.tab_id,
                            pos,
                            program,
                            program_substituted,
                            pane.terminal_command.clone(),
                        ),
                    );
                    changed_tabs.insert(tab.tab_id);
                }
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

        let label = pane_label(&self.pane_store, pane_id);
        let changed = if let Some(pane) = self.pane_store.panes.get_mut(&pane_id) {
            if pane.cwd.as_ref() != Some(&cwd_str) {
                debug_log!(self, "{} cwd -> {:?}", label, cwd_str);
                pane.set_cwd(cwd_str.clone());
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

        let label = pane_label(&self.pane_store, pane_id);
        let changed = if let Some(pane) = self.pane_store.panes.get_mut(&pane_id) {
            match cmd_type {
                CMD_GIT_ROOT => {
                    if success {
                        if let Some(root) = parse_git_root(&stdout) {
                            if pane.git_root.as_ref() != Some(&root) {
                                debug_log!(self, "{} git_root -> {:?}", label, root);
                                pane.set_git_root(root);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else if pane.git_root.is_some() {
                        debug_log!(self, "{} git_root cleared", label);
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

    fn handle_timer(&mut self) {
        let mut changed_tabs = HashSet::new();

        let panes_missing_cwd: Vec<u32> = self
            .pane_store
            .panes
            .iter()
            .filter(|(_, p)| p.cwd.is_none())
            .map(|(&id, _)| id)
            .collect();
        for pane_id in panes_missing_cwd {
            let label = pane_label(&self.pane_store, pane_id);
            if let Ok(cwd) = self.host.get_pane_cwd(pane_id) {
                let cwd_str = cwd.to_string_lossy().to_string();
                if !cwd_str.is_empty() {
                    let tab_id = self.pane_store.panes.get(&pane_id).map(|p| p.tab_id);
                    if let Some(pane) = self.pane_store.panes.get_mut(&pane_id) {
                        debug_log!(self, "{} cwd -> {:?}", label, cwd_str);
                        pane.set_cwd(cwd_str.clone());
                    }
                    if let Some(tab_id) = tab_id {
                        changed_tabs.insert(tab_id);
                    }
                    self.request_git_info(pane_id, &cwd_str);
                }
            }
        }

        // Only poll running command for non-command panes.
        // Command panes have a fixed program from terminal_command.
        let pane_ids: Vec<u32> = self
            .pane_store
            .panes
            .iter()
            .filter(|(_, p)| p.terminal_command.is_none())
            .map(|(&id, _)| id)
            .collect();
        for pane_id in pane_ids {
            let raw_cmd = self.host.get_pane_running_command(pane_id).ok();
            let running_command = raw_cmd.as_ref().map(|cmd| cmd.join(" "));
            let raw_program = raw_cmd.and_then(|cmd| {
                let tokens: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
                extract_program(&tokens, &self.config().skip_programs)
            });
            let (new_program, new_program_substituted) = self.substitute_program(raw_program);
            let label = pane_label(&self.pane_store, pane_id);
            if let Some(pane) = self.pane_store.panes.get_mut(&pane_id) {
                pane.running_command = running_command;
                if pane.program != new_program
                    || pane.program_substituted != new_program_substituted
                {
                    debug_log!(self, "{} program -> {:?}", label, new_program);
                    changed_tabs.insert(pane.tab_id);
                    pane.program = new_program;
                    pane.program_substituted = new_program_substituted;
                }
            }

            // Fill git info for panes with CWD on auto-managed tabs. CWD changes
            // already trigger a fresh git lookup, so avoid re-running git every poll
            // once the root is known.
            let should_refresh_git = self.pane_store.panes.get(&pane_id).and_then(|p| {
                if p.cwd.is_some() && p.git_root.is_none() {
                    self.tab_store
                        .tabs
                        .get(&p.tab_id)
                        .filter(|t| t.is_managed)
                        .map(|_| p.cwd.clone().unwrap())
                } else {
                    None
                }
            });
            if let Some(cwd) = should_refresh_git {
                self.request_git_info(pane_id, &cwd);
            }
        }

        let pane_ids: Vec<u32> = self.pane_store.panes.keys().copied().collect();
        for pane_id in pane_ids {
            let viewport = match self.host.get_pane_viewport(pane_id) {
                Ok(viewport) => viewport,
                Err(e) => {
                    debug_log!(
                        self,
                        "{} screen read failed: {}",
                        pane_label(&self.pane_store, pane_id),
                        e
                    );
                    continue;
                }
            };
            let new_hash = viewport_hash(&viewport);
            let label = pane_label(&self.pane_store, pane_id);
            if let Some(pane) = self.pane_store.panes.get_mut(&pane_id) {
                let was_changed = pane.screen_changed;
                match pane.screen_hash {
                    None => {
                        pane.screen_hash = Some(new_hash);
                        pane.screen_changed = false;
                        pane.screen_quiet_ticks = 0;
                        debug_log!(self, "{} screen baseline captured", label);
                    }
                    Some(old_hash) if old_hash != new_hash => {
                        pane.screen_hash = Some(new_hash);
                        pane.screen_changed = true;
                        pane.screen_quiet_ticks = 0;
                        changed_tabs.insert(pane.tab_id);
                        debug_log!(self, "{} screen changed", label);
                    }
                    Some(_) => {
                        pane.screen_quiet_ticks = pane.screen_quiet_ticks.saturating_add(1);
                        pane.screen_changed = false;
                        if was_changed {
                            changed_tabs.insert(pane.tab_id);
                            debug_log!(self, "{} screen stable", label);
                        }
                    }
                }
            }
        }

        for tab_id in changed_tabs {
            self.schedule_rename(tab_id);
        }
    }

    fn handle_key(&mut self, key: KeyWithModifier) {
        if key.has_no_modifiers() {
            match key.bare_key {
                BareKey::Char('1') => self.active_view = 0,
                BareKey::Char('2') => self.active_view = 1,
                BareKey::Char('3') => self.active_view = 2,
                BareKey::Char('4') => self.active_view = 3,
                BareKey::Char('5') => self.active_view = 4,
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
                BareKey::Esc => {
                    self.host.hide_self();
                }
                _ => {}
            }
        } else if key.bare_key == BareKey::Tab && key.has_modifiers(&[KeyModifier::Shift]) {
            self.active_view = (self.active_view + ui::VIEW_COUNT - 1) % ui::VIEW_COUNT;
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
                debug_log!(self, "permissions granted");
                self.permissions_granted = true;
                self.host.hide_self();
                self.schedule_rename_all();
                self.schedule_next_timer();
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
            Event::CwdChanged(pane_id, cwd, _) => {
                if let PaneId::Terminal(id) = pane_id {
                    self.handle_cwd_changed(id, cwd);
                }
                true
            }
            Event::RunCommandResult(exit_code, stdout, stderr, context) => {
                self.handle_run_command_result(exit_code, stdout, stderr, context);
                true
            }
            Event::Timer(_) => {
                if self.permissions_granted {
                    let was_debounce_tick = !self.pending_renames.is_empty();
                    self.tick_pending_renames();
                    self.poll_ticks += 1;
                    if self.needs_initial_poll()
                        || !was_debounce_tick
                        || self.poll_ticks >= self.poll_tick_threshold()
                    {
                        self.poll_ticks = 0;
                        self.handle_timer();
                    }
                    self.schedule_next_timer();
                }
                true
            }
            Event::PluginConfigurationChanged(configuration) => {
                debug_log!(self, "config reloaded");
                self.config = Some(Config::from_map(&configuration));
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
        if let Some(tab_pos) = self.host.get_focused_tab_position() {
            if let Some(tab_id) = self.tab_store.tab_id_at_position(tab_pos) {
                if let Some(state) = self.tab_store.tabs.get_mut(&tab_id) {
                    if state.is_managed != managed {
                        state.is_managed = managed;
                        debug_log!(self, "tab {} managed={}", tab_id, managed);
                        if managed {
                            self.schedule_rename(tab_id);
                        }
                    }
                }
            }
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
            PermissionType::ReadPaneContents,
        ]);
        subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::CwdChanged,
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
        ui::render_dashboard(
            rows,
            cols,
            self.active_view,
            &self.scroll_offsets,
            self.config(),
            &self.tab_store,
            &self.pane_store,
            &self.log_buffer,
            &self.last_rename,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::Substitutions;
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
            poll_ticks: 0,
            active_view: 0,
            scroll_offsets: [0; 5],
            log_buffer: VecDeque::new(),
            last_rename: None,
            version_error: None,
        }
    }

    fn tab_info(tab_id: usize, position: usize, name: &str) -> TabInfo {
        TabInfo {
            tab_id,
            position,
            name: name.into(),
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
    fn test_timer_fetches_missing_cwd() {
        let mut mock = MockZellijHost::new();

        mock.expect_rename_tab()
            .with(eq(1u64), eq("fetched-dir".to_string()))
            .times(1)
            .returning(|_, _| ());
        mock.expect_get_pane_cwd()
            .with(eq(10u32))
            .returning(|_| Ok(std::path::PathBuf::from("/home/user/fetched-dir")));
        mock.expect_get_pane_running_command()
            .returning(|_| Ok(vec!["nvim".into(), "src/main.rs".into()]));
        mock.expect_get_pane_viewport()
            .with(eq(10u32))
            .returning(|_| Ok(vec!["editing src/main.rs".into()]));
        mock.expect_run_command().returning(|_, _, _, _| ());
        mock.expect_set_timeout().returning(|_| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;

        // Tab + pane registered but no CWD yet
        plugin.handle_event(Event::TabUpdate(vec![tab_info(1, 0, "Tab #1")]));
        plugin.handle_event(Event::PaneUpdate(pane_manifest(vec![(
            0,
            vec![pane_info(10, 0, 0)],
        )])));
        assert!(plugin.pane_store.panes.get(&10).unwrap().cwd.is_none());

        // Timer should fetch CWD and program, scheduling a rename
        plugin.handle_event(Event::Timer(0.0));
        plugin.flush_pending_renames();

        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.cwd, Some("/home/user/fetched-dir".into()));
        let expected_program = Substitutions::default().program.get("nvim").cloned();
        assert_eq!(pane.program, expected_program);
        assert_eq!(pane.screen_state(), "stable");
    }

    #[test]
    fn test_timer_detects_screen_changes() {
        let mut mock = MockZellijHost::new();

        mock.expect_get_pane_running_command()
            .returning(|_| Ok(vec!["codex".into()]));
        mock.expect_get_pane_viewport()
            .with(eq(10u32))
            .times(3)
            .returning(|_| {
                static CALLS: std::sync::atomic::AtomicUsize =
                    std::sync::atomic::AtomicUsize::new(0);
                let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if call < 2 {
                    Ok(vec!["thinking".into()])
                } else {
                    Ok(vec!["thinking".into(), "new output".into()])
                }
            });
        mock.expect_run_command().returning(|_, _, _, _| ());

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
            std::path::PathBuf::from("/home/user/project"),
            vec![],
        ));

        plugin.handle_timer();
        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.screen_state(), "stable");
        assert_eq!(pane.screen_quiet_ticks, 0);

        plugin.handle_timer();
        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.screen_state(), "stable");
        assert_eq!(pane.screen_quiet_ticks, 1);

        plugin.handle_timer();
        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(pane.screen_state(), "changed");
        assert_eq!(pane.screen_quiet_ticks, 0);
    }

    #[test]
    fn test_debounce_timer_does_not_run_full_poll() {
        let mut mock = MockZellijHost::new();
        mock.expect_set_timeout()
            .with(eq(2.0))
            .times(1)
            .returning(|_| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;
        plugin.tab_store.tabs.insert(
            1,
            tab_state::TabState {
                tab_id: 1,
                position: 0,
                name: "project".into(),
                is_managed: true,
            },
        );
        let mut pane = PaneState::new(10, 1, 0, Some("nvim".into()), false, None);
        pane.cwd = Some("/home/user/project".into());
        pane.short_dir = Some("project".into());
        pane.screen_hash = Some(1);
        plugin.pane_store.panes.insert(10, pane);
        plugin.pending_renames.insert(1);

        plugin.handle_event(Event::Timer(0.0));

        assert_eq!(plugin.poll_ticks, 1);
    }

    #[test]
    fn test_poll_timer_runs_full_poll() {
        let mut mock = MockZellijHost::new();
        mock.expect_get_pane_running_command()
            .with(eq(10u32))
            .times(1)
            .returning(|_| Ok(vec!["nvim".into()]));
        mock.expect_get_pane_viewport()
            .with(eq(10u32))
            .times(1)
            .returning(|_| Ok(vec!["same output".into()]));
        mock.expect_run_command()
            .times(1)
            .returning(|_, _, _, _| ());
        mock.expect_set_timeout()
            .with(eq(2.0))
            .times(1)
            .returning(|_| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;
        plugin.tab_store.tabs.insert(
            1,
            tab_state::TabState {
                tab_id: 1,
                position: 0,
                name: "project".into(),
                is_managed: true,
            },
        );
        let mut pane = PaneState::new(
            10,
            1,
            0,
            Substitutions::default().program.get("nvim").cloned(),
            true,
            None,
        );
        pane.cwd = Some("/home/user/project".into());
        pane.short_dir = Some("project".into());
        pane.screen_hash = Some(viewport_hash(&["same output".into()]));
        plugin.pane_store.panes.insert(10, pane);

        plugin.handle_event(Event::Timer(0.0));

        let pane = plugin.pane_store.panes.get(&10).unwrap();
        assert_eq!(plugin.poll_ticks, 0);
        assert_eq!(pane.screen_state(), "stable");
        assert_eq!(pane.screen_quiet_ticks, 1);
    }

    #[test]
    fn test_poll_timer_skips_known_git_root() {
        let mut mock = MockZellijHost::new();
        mock.expect_get_pane_running_command()
            .with(eq(10u32))
            .times(1)
            .returning(|_| Ok(vec!["nvim".into()]));
        mock.expect_get_pane_viewport()
            .with(eq(10u32))
            .times(1)
            .returning(|_| Ok(vec!["same output".into()]));
        mock.expect_set_timeout()
            .with(eq(2.0))
            .times(1)
            .returning(|_| ());

        let mut plugin = make_plugin(mock);
        plugin.config = Some(Config::from_map(&default_config()));
        plugin.permissions_granted = true;
        plugin.tab_store.tabs.insert(
            1,
            tab_state::TabState {
                tab_id: 1,
                position: 0,
                name: "project".into(),
                is_managed: true,
            },
        );
        let mut pane = PaneState::new(
            10,
            1,
            0,
            Substitutions::default().program.get("nvim").cloned(),
            true,
            None,
        );
        pane.cwd = Some("/home/user/project".into());
        pane.short_dir = Some("project".into());
        pane.git_root = Some("/home/user/project".into());
        pane.short_git_root = Some("project".into());
        pane.screen_hash = Some(viewport_hash(&["same output".into()]));
        plugin.pane_store.panes.insert(10, pane);

        plugin.handle_event(Event::Timer(0.0));

        assert_eq!(plugin.poll_ticks, 0);
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
        assert!(parse_semver("0.44.0").unwrap() >= MIN_ZELLIJ_VERSION);
        assert!(parse_semver("0.45.0").unwrap() >= MIN_ZELLIJ_VERSION);
        assert!(parse_semver("1.0.0").unwrap() >= MIN_ZELLIJ_VERSION);
    }

    #[test]
    fn test_version_check_fails() {
        assert!(parse_semver("0.43.0").unwrap() < MIN_ZELLIJ_VERSION);
        assert!(parse_semver("0.43.9").unwrap() < MIN_ZELLIJ_VERSION);
    }
}

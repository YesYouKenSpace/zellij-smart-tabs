use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::tab_state::{PaneStore, TabStore};

#[cfg(test)]
use mockall::automock;

#[cfg_attr(test, automock)]
pub trait ZellijHost {
    fn rename_tab(&self, tab_id: u64, name: String);
    fn run_command(
        &self,
        cmd: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: PathBuf,
        ctx: BTreeMap<String, String>,
    );
    fn set_timeout(&self, secs: f64);
    fn get_pane_cwd(&self, pane_id: u32) -> Result<PathBuf, String>;
    fn get_pane_running_command(&self, pane_id: u32) -> Result<Vec<String>, String>;
    fn hide_self(&self);
    fn get_focused_tab_position(&self) -> Option<usize>;
    fn save_state(&self, tab_store: &TabStore, pane_store: &PaneStore);
    fn load_state(&self) -> (TabStore, PaneStore);
}

#[cfg(not(test))]
pub struct RealZellijHost;

#[cfg(not(test))]
impl ZellijHost for RealZellijHost {
    fn rename_tab(&self, tab_id: u64, name: String) {
        zellij_tile::prelude::rename_tab_with_id(tab_id, &name);
    }

    fn run_command(
        &self,
        cmd: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: PathBuf,
        ctx: BTreeMap<String, String>,
    ) {
        let refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
        zellij_tile::prelude::run_command_with_env_variables_and_cwd(&refs, env, cwd, ctx);
    }

    fn set_timeout(&self, secs: f64) {
        zellij_tile::prelude::set_timeout(secs);
    }

    fn get_pane_cwd(&self, pane_id: u32) -> Result<PathBuf, String> {
        zellij_tile::prelude::get_pane_cwd(zellij_tile::prelude::PaneId::Terminal(pane_id))
    }

    fn get_pane_running_command(&self, pane_id: u32) -> Result<Vec<String>, String> {
        zellij_tile::prelude::get_pane_running_command(zellij_tile::prelude::PaneId::Terminal(pane_id))
    }

    fn hide_self(&self) {
        zellij_tile::prelude::hide_self();
    }

    fn get_focused_tab_position(&self) -> Option<usize> {
        zellij_tile::prelude::get_focused_pane_info()
            .ok()
            .map(|(tab_index, _)| tab_index)
    }

    fn save_state(&self, tab_store: &TabStore, pane_store: &PaneStore) {
        let _ = crate::persistence::save(tab_store, pane_store);
    }

    fn load_state(&self) -> (TabStore, PaneStore) {
        crate::persistence::load()
    }
}

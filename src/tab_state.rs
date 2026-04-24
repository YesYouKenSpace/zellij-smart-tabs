use crate::utils::short_path;
use std::collections::HashMap;

const SCREEN_UNKNOWN_ICON: &str = "\u{f128}";
const SCREEN_CHANGED_ICON: &str = "\u{f252}";
const SCREEN_STABLE_ICON: &str = "";

#[derive(Debug, Clone)]
pub struct PaneState {
    #[allow(dead_code)] // Redundant with HashMap key; kept for test assertions and debugging
    pub pane_id: u32,
    pub tab_id: usize,
    pub position: usize,
    pub cwd: Option<String>,
    pub short_dir: Option<String>,
    pub git_root: Option<String>,
    pub short_git_root: Option<String>,
    pub program: Option<String>,
    pub program_substituted: bool,
    /// Set when the pane is a command pane (started with `zellij run`).
    /// When set, `program` comes from this and we skip polling `get_pane_running_command`.
    pub terminal_command: Option<String>,
    /// Raw output from `get_pane_running_command` for non-command panes.
    pub running_command: Option<String>,
    /// Hash of the last viewport read through Zellij's pane-content API.
    pub screen_hash: Option<u64>,
    /// True only for the most recent poll where viewport content changed.
    pub screen_changed: bool,
    /// Number of completed polls since the last observed viewport change.
    pub screen_quiet_ticks: u32,
}

impl PaneState {
    pub fn new(
        pane_id: u32,
        tab_id: usize,
        position: usize,
        program: Option<String>,
        program_substituted: bool,
        terminal_command: Option<String>,
    ) -> Self {
        Self {
            pane_id,
            tab_id,
            position,
            cwd: None,
            short_dir: None,
            git_root: None,
            short_git_root: None,
            program,
            program_substituted,
            terminal_command,
            running_command: None,
            screen_hash: None,
            screen_changed: false,
            screen_quiet_ticks: 0,
        }
    }

    pub fn screen_state(&self) -> &'static str {
        if self.screen_hash.is_none() {
            "unknown"
        } else if self.screen_changed {
            "changed"
        } else {
            "stable"
        }
    }

    pub fn screen_status(&self) -> &'static str {
        match self.screen_state() {
            "unknown" => SCREEN_UNKNOWN_ICON,
            "changed" => SCREEN_CHANGED_ICON,
            "stable" => SCREEN_STABLE_ICON,
            _ => "",
        }
    }

    pub fn set_cwd(&mut self, cwd: String) {
        self.short_dir = Some(short_path(&cwd));
        self.cwd = Some(cwd);
    }

    pub fn set_git_root(&mut self, root: String) {
        self.short_git_root = Some(short_path(&root));
        self.git_root = Some(root);
    }

    pub fn clear_git(&mut self) {
        self.git_root = None;
        self.short_git_root = None;
    }
}

#[derive(Debug, Default)]
pub struct PaneStore {
    pub panes: HashMap<u32, PaneState>,
}

impl PaneStore {
    pub fn panes_for_tab(&self, tab_id: usize) -> Vec<&PaneState> {
        let mut panes: Vec<&PaneState> =
            self.panes.values().filter(|p| p.tab_id == tab_id).collect();
        panes.sort_by_key(|p| p.position);
        panes
    }
}

#[derive(Debug, Clone)]
pub struct TabState {
    pub tab_id: usize,
    pub position: usize,
    pub name: String,
    pub is_managed: bool,
}

impl TabState {
    pub fn new(tab_id: usize, position: usize, name: String) -> Self {
        Self {
            tab_id,
            position,
            name,
            is_managed: true,
        }
    }
}

#[derive(Debug, Default)]
pub struct TabStore {
    pub tabs: HashMap<usize, TabState>,
}

impl TabStore {
    /// Sync with Zellij's tab info. Returns tab_ids that need renaming (new tabs
    /// or tabs where the user cleared the name to restore auto-management).
    pub fn sync_tabs(
        &mut self,
        tab_infos: &[(usize, usize, String)], // (tab_id, position, name)
    ) -> Vec<usize> {
        let mut needs_rename = Vec::new();
        let current_ids: std::collections::HashSet<usize> =
            tab_infos.iter().map(|(id, _, _)| *id).collect();

        self.tabs.retain(|id, _| current_ids.contains(id));

        for (tab_id, position, name) in tab_infos {
            if let Some(state) = self.tabs.get_mut(tab_id) {
                state.position = *position;
                // Empty name = user wants to restore auto-management
                if name.trim().is_empty() && !state.is_managed {
                    state.is_managed = true;
                    needs_rename.push(*tab_id);
                }
                state.name = name.clone();
            } else {
                self.tabs
                    .insert(*tab_id, TabState::new(*tab_id, *position, name.clone()));
                needs_rename.push(*tab_id);
            }
        }

        needs_rename
    }

    pub fn auto_renameable(&self) -> Vec<usize> {
        self.tabs
            .iter()
            .filter(|(_, s)| s.is_managed)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Find tab_id by tab position.
    pub fn tab_id_at_position(&self, position: usize) -> Option<usize> {
        self.tabs
            .values()
            .find(|t| t.position == position)
            .map(|t| t.tab_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_tabs_need_renaming() {
        let mut store = TabStore::default();
        let needs = store.sync_tabs(&[(1, 0, "Tab #1".into()), (2, 1, "Tab #2".into())]);
        assert_eq!(needs.len(), 2);
    }

    #[test]
    fn test_existing_tabs_dont_need_renaming() {
        let mut store = TabStore::default();
        store.sync_tabs(&[(1, 0, "Tab #1".into())]);
        let needs = store.sync_tabs(&[(1, 0, "Tab #1".into())]);
        assert!(needs.is_empty());
    }

    #[test]
    fn test_unmanaged_tab_excluded_from_auto_renameable() {
        let mut store = TabStore::default();
        store.sync_tabs(&[(1, 0, "Tab #1".into())]);
        store.tabs.get_mut(&1).unwrap().is_managed = false;
        assert!(store.auto_renameable().is_empty());
    }

    #[test]
    fn test_restore_managed() {
        let mut store = TabStore::default();
        store.sync_tabs(&[(1, 0, "Tab #1".into())]);
        store.tabs.get_mut(&1).unwrap().is_managed = false;
        assert!(store.auto_renameable().is_empty());
        store.tabs.get_mut(&1).unwrap().is_managed = true;
        assert_eq!(store.auto_renameable(), vec![1]);
    }

    #[test]
    fn test_closed_tab_removed() {
        let mut store = TabStore::default();
        store.sync_tabs(&[(1, 0, "Tab #1".into()), (2, 1, "Tab #2".into())]);
        store.sync_tabs(&[(1, 0, "Tab #1".into())]);
        assert_eq!(store.tabs.len(), 1);
    }

    #[test]
    fn test_tab_id_at_position() {
        let mut store = TabStore::default();
        store.sync_tabs(&[(10, 0, "Tab #1".into()), (20, 1, "Tab #2".into())]);
        assert_eq!(store.tab_id_at_position(0), Some(10));
        assert_eq!(store.tab_id_at_position(1), Some(20));
        assert_eq!(store.tab_id_at_position(99), None);
    }

    #[test]
    fn test_pane_store_queries() {
        let mut pane_store = PaneStore::default();
        let mut pane_a = PaneState::new(10, 1, 0, Some("nvim".into()), false, None);
        pane_a.cwd = Some("/home/user/a".into());
        pane_a.short_dir = Some("a".into());
        pane_store.panes.insert(10, pane_a);
        let mut pane_b = PaneState::new(11, 1, 1, None, false, None);
        pane_b.cwd = Some("/home/user/b".into());
        pane_b.short_dir = Some("b".into());
        pane_store.panes.insert(11, pane_b);

        let tab1_panes = pane_store.panes_for_tab(1);
        assert_eq!(tab1_panes.len(), 2);
        assert_eq!(tab1_panes[0].pane_id, 10);
        assert_eq!(tab1_panes[1].pane_id, 11);
        assert_eq!(pane_store.panes_for_tab(99).len(), 0);
    }

    #[test]
    fn test_pane_set_cwd_updates_short_dir() {
        let mut pane = PaneState::new(1, 1, 0, None, false, None);
        pane.set_cwd("/home/user/Projects/my-project".into());
        assert_eq!(pane.short_dir, Some("my-project".into()));
    }

    #[test]
    fn test_pane_set_git_root_updates_short() {
        let mut pane = PaneState::new(1, 1, 0, None, false, None);
        pane.set_git_root("/home/user/Projects/my-project".into());
        assert_eq!(pane.short_git_root, Some("my-project".into()));
    }
}

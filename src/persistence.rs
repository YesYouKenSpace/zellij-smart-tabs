use crate::tab_state::{TabStore, PaneStore};

const TAB_STATE_PATH: &str = "/data/tab_store.json";
const PANE_STATE_PATH: &str = "/data/pane_store.json";

pub fn save(tab_store: &TabStore, pane_store: &PaneStore) -> Result<(), String> {
    let tab_json = serde_json::to_string(tab_store).map_err(|e| e.to_string())?;
    std::fs::write(TAB_STATE_PATH, tab_json).map_err(|e| e.to_string())?;
    let pane_json = serde_json::to_string(pane_store).map_err(|e| e.to_string())?;
    std::fs::write(PANE_STATE_PATH, pane_json).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load() -> (TabStore, PaneStore) {
    let tab_store = std::fs::read_to_string(TAB_STATE_PATH)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let pane_store = std::fs::read_to_string(PANE_STATE_PATH)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    (tab_store, pane_store)
}

#[cfg(test)]
mod tests {
    use crate::tab_state::{TabState, TabStore, PaneState, PaneStore};

    #[test]
    fn test_roundtrip_serialization() {
        let mut tab_store = TabStore::default();
        tab_store.tabs.insert(1, TabState::new(1, 0, "my-project".into()));

        let mut pane_store = PaneStore::default();
        pane_store.panes.insert(10, PaneState {
            pane_id: 10, tab_id: 1, position: 0,
            cwd: Some("/home/user/project".into()), short_dir: Some("project".into()),
            git_root: None, short_git_root: None,
            program: Some("nvim".into()), terminal_command: None, status: crate::tab_state::DEFAULT_STATUS.to_string(),
        });

        let tab_json = serde_json::to_string(&tab_store).unwrap();
        let pane_json = serde_json::to_string(&pane_store).unwrap();
        let loaded_tabs: TabStore = serde_json::from_str(&tab_json).unwrap();
        let loaded_panes: PaneStore = serde_json::from_str(&pane_json).unwrap();

        assert_eq!(loaded_tabs.tabs.len(), 1);
        let pane = loaded_panes.panes.get(&10).unwrap();
        assert_eq!(pane.short_dir, Some("project".into()));
        assert_eq!(pane.program, Some("nvim".into()));
    }
}

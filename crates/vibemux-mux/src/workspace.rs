use crate::pane::PaneId;
use crate::split::SplitTree;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type WorkspaceId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMetadata {
    #[serde(default)]
    pub status_entries: Vec<StatusEntry>,
    pub progress: Option<ProgressState>,
    #[serde(default)]
    pub log_entries: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEntry {
    pub key: String,
    pub value: String,
    pub icon: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressState {
    pub value: f32,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub level: String,
    pub source: Option<String>,
    pub message: String,
}

impl Default for WorkspaceMetadata {
    fn default() -> Self {
        Self {
            status_entries: Vec::new(),
            progress: None,
            log_entries: Vec::new(),
        }
    }
}

/// One shell session (WezTerm-style tab) inside a workspace.
pub struct WorkspaceTab {
    pub id: Uuid,
    pub split_tree: SplitTree,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub title: Option<String>,
}

impl WorkspaceTab {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            split_tree: SplitTree::empty(),
            cwd: None,
            git_branch: None,
            title: None,
        }
    }

    pub fn label(&self, index: usize) -> String {
        let n = index + 1;
        match &self.title {
            Some(t) if !t.is_empty() => format!("{n}: {t}"),
            _ => format!("{n}: shell"),
        }
    }
}

pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub tabs: Vec<WorkspaceTab>,
    pub active_tab_index: usize,
    pub metadata: WorkspaceMetadata,
    pub has_unread: bool,
    pub pinned: bool,
}

impl Workspace {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            tabs: vec![WorkspaceTab::new()],
            active_tab_index: 0,
            metadata: WorkspaceMetadata::default(),
            has_unread: false,
            pinned: false,
        }
    }

    pub fn active_tab(&self) -> &WorkspaceTab {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut WorkspaceTab {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn split_tree(&self) -> &SplitTree {
        &self.active_tab().split_tree
    }

    pub fn split_tree_mut(&mut self) -> &mut SplitTree {
        &mut self.active_tab_mut().split_tree
    }

    pub fn all_pane_ids(&self) -> Vec<PaneId> {
        self.tabs
            .iter()
            .flat_map(|t| t.split_tree.pane_ids())
            .collect()
    }
}

pub struct WorkspaceManager {
    workspaces: Vec<Workspace>,
    active_index: usize,
}

impl WorkspaceManager {
    pub fn new() -> Self {
        let initial = Workspace::new("Workspace 1");
        Self {
            workspaces: vec![initial],
            active_index: 0,
        }
    }

    pub fn active(&self) -> &Workspace {
        &self.workspaces[self.active_index]
    }

    pub fn active_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_index]
    }

    pub fn active_index(&self) -> usize {
        self.active_index
    }

    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn workspaces_mut(&mut self) -> &mut Vec<Workspace> {
        &mut self.workspaces
    }

    /// Returns `(workspace_index, tab_index)` for the pane, if any.
    pub fn locate_pane(&self, pane_id: PaneId) -> Option<(usize, usize)> {
        for (wi, ws) in self.workspaces.iter().enumerate() {
            for (ti, tab) in ws.tabs.iter().enumerate() {
                if let Some(root) = &tab.split_tree.root {
                    if root.find_pane(pane_id) {
                        return Some((wi, ti));
                    }
                }
            }
        }
        None
    }

    pub fn create_workspace(&mut self, name: impl Into<String>) -> WorkspaceId {
        let ws = Workspace::new(name);
        let id = ws.id;
        self.workspaces.push(ws);
        self.active_index = self.workspaces.len() - 1;
        id
    }

    pub fn close_workspace(&mut self, id: WorkspaceId) -> bool {
        if self.workspaces.len() <= 1 {
            return false;
        }
        if let Some(idx) = self.workspaces.iter().position(|w| w.id == id) {
            self.workspaces.remove(idx);
            if self.active_index >= self.workspaces.len() {
                self.active_index = self.workspaces.len() - 1;
            }
            true
        } else {
            false
        }
    }

    pub fn select_workspace(&mut self, id: WorkspaceId) -> bool {
        if let Some(idx) = self.workspaces.iter().position(|w| w.id == id) {
            self.active_index = idx;
            true
        } else {
            false
        }
    }

    pub fn select_workspace_by_index(&mut self, index: usize) -> bool {
        if index < self.workspaces.len() {
            self.active_index = index;
            true
        } else {
            false
        }
    }

    pub fn next_workspace(&mut self) {
        if !self.workspaces.is_empty() {
            self.active_index = (self.active_index + 1) % self.workspaces.len();
        }
    }

    pub fn prev_workspace(&mut self) {
        if !self.workspaces.is_empty() {
            self.active_index = if self.active_index == 0 {
                self.workspaces.len() - 1
            } else {
                self.active_index - 1
            };
        }
    }

    pub fn rename_workspace(&mut self, id: WorkspaceId, name: impl Into<String>) -> bool {
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == id) {
            ws.name = name.into();
            true
        } else {
            false
        }
    }

    pub fn workspace_count(&self) -> usize {
        self.workspaces.len()
    }
}

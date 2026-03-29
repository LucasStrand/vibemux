use crate::split::SplitTree;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type WorkspaceId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMetadata {
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub title: Option<String>,
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
            cwd: None,
            git_branch: None,
            title: None,
            status_entries: Vec::new(),
            progress: None,
            log_entries: Vec::new(),
        }
    }
}

pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub split_tree: SplitTree,
    pub metadata: WorkspaceMetadata,
    pub has_unread: bool,
    pub pinned: bool,
}

impl Workspace {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            split_tree: SplitTree::empty(),
            metadata: WorkspaceMetadata::default(),
            has_unread: false,
            pinned: false,
        }
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

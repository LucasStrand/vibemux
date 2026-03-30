use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use vibemux_mux::{SplitDirection, SplitNode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub workspaces: Vec<WorkspaceState>,
    pub active_workspace_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub name: String,
    pub pinned: bool,
    pub tabs: Vec<TabState>,
    pub active_tab_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabState {
    pub cwd: Option<String>,
    pub split_layout: SplitLayoutState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitLayoutState {
    Single { cwd: Option<String> },
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<SplitLayoutState>,
        second: Box<SplitLayoutState>,
    },
}

impl SessionState {
    pub fn session_path() -> PathBuf {
        let dir = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        dir.join("vibemux").join("session.json")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::session_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::session_path();
        if !path.exists() {
            anyhow::bail!("No session file");
        }
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }
}

pub fn capture_split_layout(
    node: &SplitNode,
    cwd_for_pane: &dyn Fn(uuid::Uuid) -> Option<String>,
) -> SplitLayoutState {
    match node {
        SplitNode::Leaf { pane_id } => SplitLayoutState::Single {
            cwd: cwd_for_pane(*pane_id),
        },
        SplitNode::Split {
            direction,
            ratio,
            first,
            second,
            ..
        } => SplitLayoutState::Split {
            direction: *direction,
            ratio: *ratio,
            first: Box::new(capture_split_layout(first, cwd_for_pane)),
            second: Box::new(capture_split_layout(second, cwd_for_pane)),
        },
    }
}

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type PaneId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneMetadata {
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub has_notification: bool,
}

impl Default for PaneMetadata {
    fn default() -> Self {
        Self {
            cwd: None,
            title: None,
            has_notification: false,
        }
    }
}

pub struct Pane {
    pub id: PaneId,
    pub metadata: PaneMetadata,
}

impl Pane {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            metadata: PaneMetadata::default(),
        }
    }
}

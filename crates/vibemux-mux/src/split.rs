use crate::pane::PaneId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitNode {
    Leaf {
        pane_id: PaneId,
    },
    Split {
        id: Uuid,
        direction: SplitDirection,
        ratio: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

impl SplitNode {
    pub fn leaf(pane_id: PaneId) -> Self {
        Self::Leaf { pane_id }
    }

    pub fn find_pane(&self, target: PaneId) -> bool {
        match self {
            Self::Leaf { pane_id } => *pane_id == target,
            Self::Split { first, second, .. } => {
                first.find_pane(target) || second.find_pane(target)
            }
        }
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        match self {
            Self::Leaf { pane_id } => vec![*pane_id],
            Self::Split { first, second, .. } => {
                let mut ids = first.pane_ids();
                ids.extend(second.pane_ids());
                ids
            }
        }
    }

    pub fn split_at(
        &mut self,
        target_pane: PaneId,
        new_pane: PaneId,
        direction: SplitDirection,
    ) -> bool {
        match self {
            Self::Leaf { pane_id } if *pane_id == target_pane => {
                let old = Self::leaf(*pane_id);
                let new = Self::leaf(new_pane);
                *self = Self::Split {
                    id: Uuid::new_v4(),
                    direction,
                    ratio: 0.5,
                    first: Box::new(old),
                    second: Box::new(new),
                };
                true
            }
            Self::Split { first, second, .. } => {
                first.split_at(target_pane, new_pane, direction)
                    || second.split_at(target_pane, new_pane, direction)
            }
            _ => false,
        }
    }

    pub fn remove_pane(&mut self, target: PaneId) -> Option<PaneId> {
        match self {
            Self::Leaf { pane_id } if *pane_id == target => None,
            Self::Split { first, second, .. } => {
                if let Self::Leaf { pane_id } = first.as_ref() {
                    if *pane_id == target {
                        let replacement = second.as_ref().clone();
                        *self = replacement;
                        return Some(target);
                    }
                }
                if let Self::Leaf { pane_id } = second.as_ref() {
                    if *pane_id == target {
                        let replacement = first.as_ref().clone();
                        *self = replacement;
                        return Some(target);
                    }
                }
                if first.remove_pane(target).is_some() {
                    return Some(target);
                }
                second.remove_pane(target)
            }
            _ => None,
        }
    }
}

pub struct SplitTree {
    pub root: Option<SplitNode>,
    pub focused_pane: Option<PaneId>,
}

impl SplitTree {
    pub fn empty() -> Self {
        Self {
            root: None,
            focused_pane: None,
        }
    }

    pub fn with_pane(pane_id: PaneId) -> Self {
        Self {
            root: Some(SplitNode::leaf(pane_id)),
            focused_pane: Some(pane_id),
        }
    }

    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.root.as_ref().map_or(Vec::new(), |r| r.pane_ids())
    }

    pub fn split(
        &mut self,
        new_pane: PaneId,
        direction: SplitDirection,
    ) -> bool {
        if let Some(focused) = self.focused_pane {
            if let Some(root) = &mut self.root {
                if root.split_at(focused, new_pane, direction) {
                    self.focused_pane = Some(new_pane);
                    return true;
                }
            }
        }
        false
    }

    pub fn remove_pane(&mut self, target: PaneId) -> bool {
        if let Some(root) = &mut self.root {
            let pane_ids = root.pane_ids();
            if pane_ids.len() <= 1 {
                return false;
            }
            if root.remove_pane(target).is_some() {
                if self.focused_pane == Some(target) {
                    self.focused_pane = root.pane_ids().first().copied();
                }
                return true;
            }
        }
        false
    }
}

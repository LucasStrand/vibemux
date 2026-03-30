//! Map split-tree layout to pixel sizes (matches `split_view` equal `Fill` splits).
use vibemux_mux::{PaneId, SplitDirection, SplitNode};

const DIVIDER_V: f32 = 2.0;
const DIVIDER_H: f32 = 2.0;

pub fn pane_content_sizes(node: &SplitNode, w: f32, h: f32) -> Vec<(PaneId, f32, f32)> {
    match node {
        SplitNode::Leaf { pane_id } => {
            vec![(*pane_id, w.max(0.0), h.max(0.0))]
        }
        SplitNode::Split {
            direction,
            first,
            second,
            ..
        } => match direction {
            SplitDirection::Vertical => {
                let inner = ((w - DIVIDER_V).max(0.0)) * 0.5;
                let mut a = pane_content_sizes(first, inner, h);
                let mut b = pane_content_sizes(second, inner, h);
                a.append(&mut b);
                a
            }
            SplitDirection::Horizontal => {
                let inner = ((h - DIVIDER_H).max(0.0)) * 0.5;
                let mut a = pane_content_sizes(first, w, inner);
                let mut b = pane_content_sizes(second, w, inner);
                a.append(&mut b);
                a
            }
        },
    }
}

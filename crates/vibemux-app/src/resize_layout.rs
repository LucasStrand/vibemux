//! Map split-tree layout to pixel sizes (matches `split_view` ratio-based splits).
use vibemux_mux::{PaneId, SplitDirection, SplitNode};

const DIVIDER_V: f32 = 4.0;
const DIVIDER_H: f32 = 4.0;

pub fn pane_content_sizes(node: &SplitNode, w: f32, h: f32) -> Vec<(PaneId, f32, f32)> {
    match node {
        SplitNode::Leaf { pane_id } => {
            vec![(*pane_id, w.max(0.0), h.max(0.0))]
        }
        SplitNode::Split {
            direction,
            ratio,
            first,
            second,
            ..
        } => match direction {
            SplitDirection::Vertical => {
                let inner = (w - DIVIDER_V).max(0.0);
                let first_w = inner * ratio;
                let second_w = inner * (1.0 - ratio);
                let mut a = pane_content_sizes(first, first_w, h);
                let mut b = pane_content_sizes(second, second_w, h);
                a.append(&mut b);
                a
            }
            SplitDirection::Horizontal => {
                let inner = (h - DIVIDER_H).max(0.0);
                let first_h = inner * ratio;
                let second_h = inner * (1.0 - ratio);
                let mut a = pane_content_sizes(first, w, first_h);
                let mut b = pane_content_sizes(second, w, second_h);
                a.append(&mut b);
                a
            }
        },
    }
}

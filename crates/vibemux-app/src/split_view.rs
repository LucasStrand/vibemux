use crate::app::Message;
use crate::theme;
use crate::term_view;
use iced::widget::{container, text, Column, Row};
use iced::{Border, Element, Fill, Length, Theme};
use std::collections::HashMap;
use vibemux_mux::{PaneId, SplitDirection, SplitNode};
use vibemux_term::Terminal;

pub fn render_split_tree<'a>(
    node: &SplitNode,
    terminals: &'a HashMap<PaneId, Terminal>,
    focused_pane: Option<PaneId>,
    bytes_received: usize,
) -> Element<'a, Message> {
    match node {
        SplitNode::Leaf { pane_id } => {
            let is_focused = focused_pane == Some(*pane_id);
            let pane_id = *pane_id;

            let content = if let Some(terminal) = terminals.get(&pane_id) {
                term_view::view(&terminal.grid, bytes_received)
            } else {
                container(text("No terminal").size(14).color(theme::FG_DIM))
                    .width(Fill)
                    .height(Fill)
                    .center(Fill)
                    .style(|_t: &Theme| container::Style {
                        background: Some(theme::BG_PRIMARY.into()),
                        ..Default::default()
                    })
                    .into()
            };

            let border_color = if is_focused {
                theme::ACCENT
            } else {
                theme::BORDER
            };

            container(content)
                .width(Fill)
                .height(Fill)
                .style(move |_t: &Theme| container::Style {
                    background: Some(theme::BG_PRIMARY.into()),
                    border: Border {
                        color: border_color,
                        width: if is_focused { 1.0 } else { 0.0 },
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                })
                .into()
        }
        SplitNode::Split {
            direction,
            first,
            second,
            ratio: _,
            ..
        } => {
            let first_el =
                render_split_tree(first, terminals, focused_pane, bytes_received);
            let second_el =
                render_split_tree(second, terminals, focused_pane, bytes_received);

            let divider_style = |_t: &Theme| container::Style {
                background: Some(theme::BORDER.into()),
                ..Default::default()
            };

            match direction {
                SplitDirection::Vertical => {
                    let divider = container(text(""))
                        .width(Length::Fixed(2.0))
                        .height(Fill)
                        .style(divider_style);

                    Row::new()
                        .push(container(first_el).width(Fill).height(Fill))
                        .push(divider)
                        .push(container(second_el).width(Fill).height(Fill))
                        .width(Fill)
                        .height(Fill)
                        .into()
                }
                SplitDirection::Horizontal => {
                    let divider = container(text(""))
                        .width(Fill)
                        .height(Length::Fixed(2.0))
                        .style(divider_style);

                    Column::new()
                        .push(container(first_el).width(Fill).height(Fill))
                        .push(divider)
                        .push(container(second_el).width(Fill).height(Fill))
                        .width(Fill)
                        .height(Fill)
                        .into()
                }
            }
        }
    }
}

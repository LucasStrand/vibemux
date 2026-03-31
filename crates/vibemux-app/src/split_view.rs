use crate::app::Message;
use crate::term_selection::TerminalSelection;
use crate::theme;
use crate::term_view;
use iced::widget::{container, mouse_area, text, Column, Row};
use iced::mouse::Interaction;
use iced::{Border, Element, Fill, Font, Length, Theme};
use std::collections::HashMap;
use vibemux_mux::{PaneId, SplitDirection, SplitNode};
use vibemux_term::Terminal;

pub fn render_split_tree<'a>(
    node: &SplitNode,
    terminals: &'a HashMap<PaneId, Terminal>,
    focused_pane: Option<PaneId>,
    bytes_received: usize,
    selections: &'a HashMap<PaneId, Option<TerminalSelection>>,
    font: Font,
    font_size: f32,
) -> Element<'a, Message> {
    match node {
        SplitNode::Leaf { pane_id } => {
            let is_focused = focused_pane == Some(*pane_id);
            let pane_id = *pane_id;

            let content = if let Some(terminal) = terminals.get(&pane_id) {
                let sel = selections
                    .get(&pane_id)
                    .and_then(|s| s.as_ref());
                term_view::view(&terminal.grid, bytes_received, pane_id, sel, font, font_size)
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
            id,
            direction,
            first,
            second,
            ratio,
            ..
        } => {
            let first_el = render_split_tree(
                first,
                terminals,
                focused_pane,
                bytes_received,
                selections,
                font,
                font_size,
            );
            let second_el = render_split_tree(
                second,
                terminals,
                focused_pane,
                bytes_received,
                selections,
                font,
                font_size,
            );

            let divider_style = |_t: &Theme| container::Style {
                background: Some(theme::BORDER.into()),
                ..Default::default()
            };
            let split_id = *id;
            let dir = *direction;
            let r = *ratio;
            let _ = (r, split_id); // ratio is used in layout; drag events will adjust it

            match direction {
                SplitDirection::Vertical => {
                    // Divider that can be dragged horizontally.
                    let divider_inner = container(text(""))
                        .width(Length::Fixed(4.0))
                        .height(Fill)
                        .style(divider_style);
                    let divider = mouse_area(divider_inner)
                        .interaction(Interaction::ResizingHorizontally)
                        .on_press(Message::SplitDragStart(split_id, dir))
                        .on_move(move |p| Message::SplitDragMove(split_id, dir, p));

                    // Use ratio-based sizing via Length::FillPortion.
                    let p1 = (r * 1000.0) as u16;
                    let p2 = ((1.0 - r) * 1000.0) as u16;

                    Row::new()
                        .push(container(first_el).width(Length::FillPortion(p1)).height(Fill))
                        .push(divider)
                        .push(container(second_el).width(Length::FillPortion(p2)).height(Fill))
                        .width(Fill)
                        .height(Fill)
                        .into()
                }
                SplitDirection::Horizontal => {
                    let divider_inner = container(text(""))
                        .width(Fill)
                        .height(Length::Fixed(4.0))
                        .style(divider_style);
                    let divider = mouse_area(divider_inner)
                        .interaction(Interaction::ResizingVertically)
                        .on_press(Message::SplitDragStart(split_id, dir))
                        .on_move(move |p| Message::SplitDragMove(split_id, dir, p));

                    let p1 = (r * 1000.0) as u16;
                    let p2 = ((1.0 - r) * 1000.0) as u16;

                    Column::new()
                        .push(container(first_el).width(Fill).height(Length::FillPortion(p1)))
                        .push(divider)
                        .push(container(second_el).width(Fill).height(Length::FillPortion(p2)))
                        .width(Fill)
                        .height(Fill)
                        .into()
                }
            }
        }
    }
}

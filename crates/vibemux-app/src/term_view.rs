use crate::app::Message;
use crate::theme;
use iced::widget::{column, container, text, Column};
use iced::{Color, Element, Fill, Font, Length, Theme};
use vibemux_term::grid::TerminalGrid;

const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 18.0;

pub fn view<'a>(grid: &'a TerminalGrid, bytes_received: usize) -> Element<'a, Message> {
    let mut lines = Column::new().spacing(0);

    let cell_rows = grid.visible_cells();
    for (row_idx, row_cells) in cell_rows.iter().enumerate() {
        let is_cursor_row = row_idx == grid.cursor_row;

        let mut line_text = String::with_capacity(grid.cols);
        for (col_idx, cell) in row_cells.iter().enumerate() {
            let is_cursor =
                is_cursor_row && col_idx == grid.cursor_col && grid.cursor_visible;

            if is_cursor {
                line_text.push('\u{2588}');
            } else {
                line_text.push(cell.c);
            }
        }

        let fg = if is_cursor_row && grid.cursor_visible {
            theme::FG_PRIMARY
        } else {
            Color::from_rgb(
                row_cells[0].attrs.fg.r as f32 / 255.0,
                row_cells[0].attrs.fg.g as f32 / 255.0,
                row_cells[0].attrs.fg.b as f32 / 255.0,
            )
        };

        let line = text(line_text)
            .size(FONT_SIZE)
            .font(Font::MONOSPACE)
            .color(fg);

        lines = lines.push(
            container(line).height(Length::Fixed(LINE_HEIGHT)),
        );
    }

    let status = text(format!(
        " Ln {}, Col {} | {} bytes | {}x{}",
        grid.cursor_row + 1,
        grid.cursor_col + 1,
        bytes_received,
        grid.cols,
        grid.rows,
    ))
    .size(11)
    .color(theme::FG_DIM)
    .font(Font::MONOSPACE);

    let status_bar = container(status)
        .width(Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(theme::BG_SIDEBAR.into()),
            ..Default::default()
        });

    column![
        container(lines)
            .width(Fill)
            .height(Fill)
            .padding(4)
            .style(|_t: &Theme| container::Style {
                background: Some(theme::BG_PRIMARY.into()),
                ..Default::default()
            }),
        status_bar,
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

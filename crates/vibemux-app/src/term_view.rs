use crate::app::Message;
use crate::term_selection::{TerminalSelection, TERM_FONT_SIZE, TERM_LINE_HEIGHT};
use crate::theme;
use iced::widget::text::{LineHeight, Span};
use iced::widget::{column, container, mouse_area, rich_text, scrollable, span, Column, Id};
use iced::{Color, Element, Fill, Font, Length, Theme};
use iced::mouse::Interaction;
use vibemux_mux::PaneId;
use vibemux_term::grid::TerminalGrid;

fn scroll_id(pane_id: PaneId) -> Id {
    Id::from(format!("term-scroll-{pane_id}"))
}

const SELECTION_BG: Color = Color::from_rgb(0.35, 0.42, 0.65);

/// Block cursor and status line use this: follows selection **head** while selecting,
/// otherwise the PTY cursor (Shift+arrows only move our selection, not the shell).
fn visual_cursor(grid: &TerminalGrid, selection: Option<&TerminalSelection>) -> (usize, usize) {
    if let Some(s) = selection {
        if !s.collapsed() {
            return s.head;
        }
    }
    (grid.display_cursor_row(), grid.cursor_col)
}

fn line_element<'a>(
    row_idx: usize,
    row_cells: &[vibemux_term::grid::Cell],
    grid: &TerminalGrid,
    selection: Option<&TerminalSelection>,
    v_row: usize,
    v_col: usize,
) -> Element<'a, Message> {
    let is_cursor_row = row_idx == v_row;
    let cols = grid.cols;

    // Collapsed selections still exist in state (e.g. after clamping) but must not
    // paint a highlight: visual_cursor follows the PTY, so a 1-cell "range" looks
    // like a stuck band beside the real cursor.
    let range_sel = selection.filter(|s| !s.collapsed());

    let mut spans: Vec<Span<'a, (), Font>> = Vec::new();
    let mut i = 0;
    while i < row_cells.len().min(cols) {
        let selected = range_sel
            .map(|s| s.contains_cell(row_idx, i, cols))
            .unwrap_or(false);

        let start = i;
        while i < row_cells.len().min(cols) {
            let sel = range_sel
                .map(|s| s.contains_cell(row_idx, i, cols))
                .unwrap_or(false);
            if sel != selected {
                break;
            }
            i += 1;
        }

        let mut frag = String::new();
        for c in start..i {
            let is_cursor = is_cursor_row && c == v_col && grid.cursor_visible;
            if is_cursor {
                frag.push('\u{2588}');
            } else {
                frag.push(row_cells[c].c);
            }
        }

        let fg = if is_cursor_row && grid.cursor_visible {
            theme::FG_PRIMARY
        } else {
            Color::from_rgb(
                row_cells[start].attrs.fg.r as f32 / 255.0,
                row_cells[start].attrs.fg.g as f32 / 255.0,
                row_cells[start].attrs.fg.b as f32 / 255.0,
            )
        };

        let mut s = span(frag)
            .size(TERM_FONT_SIZE)
            .font(Font::MONOSPACE)
            .color(fg);
        if selected {
            s = s.background(SELECTION_BG);
        }
        spans.push(s);
    }

    let rt = rich_text(spans)
        .size(TERM_FONT_SIZE)
        .line_height(LineHeight::Absolute(TERM_LINE_HEIGHT.into()))
        .font(Font::MONOSPACE)
        .width(Fill);

    container(rt)
        .height(Length::Fixed(TERM_LINE_HEIGHT))
        .into()
}

pub fn view<'a>(
    grid: &'a TerminalGrid,
    bytes_received: usize,
    pane_id: PaneId,
    selection: Option<&'a TerminalSelection>,
) -> Element<'a, Message> {
    let mut lines = Column::new().spacing(0);

    let (v_row, v_col) = visual_cursor(grid, selection);
    let n_lines = grid.display_line_count();
    for row_idx in 0..n_lines {
        let Some(row_cells) = grid.display_line_cells(row_idx) else {
            continue;
        };
        lines = lines.push(line_element(
            row_idx,
            row_cells,
            grid,
            selection,
            v_row,
            v_col,
        ));
    }

    let status = text_status(grid, bytes_received, v_row, v_col);

    let status_bar = container(status)
        .width(Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(theme::BG_SIDEBAR.into()),
            ..Default::default()
        });

    let lines_pane = container(lines)
        .width(Fill)
        .padding(4)
        .style(|_t: &Theme| container::Style {
            background: Some(theme::BG_PRIMARY.into()),
            ..Default::default()
        });

    let pane_id_move = pane_id;
    let interactive = mouse_area(lines_pane)
        .interaction(Interaction::Text)
        .on_move(move |p| Message::TerminalMouseMove(pane_id_move, p))
        .on_press(Message::TerminalMouseDown(pane_id));

    let sc = scrollable(interactive)
        .id(scroll_id(pane_id))
        .width(Fill)
        .height(Fill)
        .on_scroll(move |vp| {
            let cb = vp.content_bounds().height;
            let b = vp.bounds().height;
            let stick = if cb <= b + 1.0 {
                true
            } else {
                let ry = vp.relative_offset().y;
                ry.is_finite() && ry >= 0.99
            };
            Message::TerminalViewportChanged(pane_id, stick)
        });

    column![sc, status_bar]
        .width(Fill)
        .height(Fill)
        .into()
}

fn text_status(
    grid: &TerminalGrid,
    bytes_received: usize,
    v_row: usize,
    v_col: usize,
) -> iced::widget::Text<'_, Theme> {
    iced::widget::text(format!(
        " Ln {}, Col {} | {} bytes | {}x{} (+{} scrollback)",
        v_row + 1,
        v_col + 1,
        bytes_received,
        grid.cols,
        grid.rows,
        grid.scrollback_len(),
    ))
    .size(11)
    .color(theme::FG_DIM)
    .font(Font::MONOSPACE)
}

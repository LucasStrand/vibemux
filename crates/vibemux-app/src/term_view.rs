use crate::app::Message;
use crate::term_selection::{TerminalSelection, TERM_LINE_HEIGHT};
use crate::theme;
use iced::widget::text::{LineHeight, Span};
use iced::widget::{column, container, mouse_area, rich_text, scrollable, span, Column, Id};
use iced::{Color, Element, Fill, Font, Length, Theme};
use iced::mouse::Interaction;
use vibemux_mux::PaneId;
use vibemux_term::grid::{CellAttributes, Color as TermColor, TerminalGrid};

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

/// Resolved effective colors for a cell, accounting for inverse attribute.
fn resolve_colors(attrs: &CellAttributes) -> (Color, Color) {
    let (fg_t, bg_t) = if attrs.inverse {
        (attrs.bg, attrs.fg)
    } else {
        (attrs.fg, attrs.bg)
    };
    let fg = Color::from_rgb(
        fg_t.r as f32 / 255.0,
        fg_t.g as f32 / 255.0,
        fg_t.b as f32 / 255.0,
    );
    let bg = Color::from_rgb(
        bg_t.r as f32 / 255.0,
        bg_t.g as f32 / 255.0,
        bg_t.b as f32 / 255.0,
    );
    (fg, bg)
}

fn has_non_default_bg(attrs: &CellAttributes) -> bool {
    let bg = if attrs.inverse { attrs.fg } else { attrs.bg };
    bg != TermColor::DEFAULT_BG
}

/// A contiguous run of cells sharing the same visual attributes.
struct SpanRun {
    text: String,
    fg: Color,
    bg: Option<Color>,
    bold: bool,
    italic: bool,
    underline: bool,
}

fn line_element<'a>(
    row_idx: usize,
    row_cells: &[vibemux_term::grid::Cell],
    grid: &TerminalGrid,
    selection: Option<&TerminalSelection>,
    v_row: usize,
    v_col: usize,
    font: Font,
    font_size: f32,
) -> Element<'a, Message> {
    let is_cursor_row = row_idx == v_row;
    let cols = grid.cols;

    // Collapsed selections must not paint a highlight.
    let range_sel = selection.filter(|s| !s.collapsed());

    // Build runs of contiguous cells with identical visual properties.
    let mut runs: Vec<SpanRun> = Vec::new();

    for c in 0..row_cells.len().min(cols) {
        // Skip wide-continuation cells: the primary cell's character already accounts for width.
        if row_cells[c].wide_continuation {
            continue;
        }

        let selected = range_sel
            .map(|s| s.contains_cell(row_idx, c, cols))
            .unwrap_or(false);
        let is_cursor = is_cursor_row && c == v_col && grid.cursor_visible;
        let ch = if is_cursor { '\u{2588}' } else { row_cells[c].c };

        let (fg, cell_bg) = resolve_colors(&row_cells[c].attrs);
        let bg = if selected {
            Some(SELECTION_BG)
        } else if has_non_default_bg(&row_cells[c].attrs) {
            Some(cell_bg)
        } else {
            None
        };

        let bold = row_cells[c].attrs.bold;
        let italic = row_cells[c].attrs.italic;
        let underline = row_cells[c].attrs.underline;

        // Try to extend the current run.
        let can_extend = if let Some(last) = runs.last() {
            last.fg == fg && last.bg == bg && last.bold == bold
                && last.italic == italic && last.underline == underline
        } else {
            false
        };

        if can_extend {
            runs.last_mut().unwrap().text.push(ch);
        } else {
            runs.push(SpanRun {
                text: ch.to_string(),
                fg,
                bg,
                bold,
                italic,
                underline,
            });
        }
    }

    let spans: Vec<Span<'a, (), Font>> = runs
        .into_iter()
        .map(|run| {
            let mut s = span(run.text)
                .size(font_size)
                .font(font)
                .color(run.fg);
            if let Some(bg) = run.bg {
                s = s.background(bg);
            }
            if run.bold {
                s = s.font(Font {
                    weight: iced::font::Weight::Bold,
                    ..font
                });
            }
            if run.italic {
                s = s.font(Font {
                    style: iced::font::Style::Italic,
                    ..if run.bold {
                        Font {
                            weight: iced::font::Weight::Bold,
                            ..font
                        }
                    } else {
                        font
                    }
                });
            }
            if run.underline {
                s = s.underline(true);
            }
            s
        })
        .collect();

    let rt = rich_text(spans)
        .size(font_size)
        .line_height(LineHeight::Absolute(TERM_LINE_HEIGHT.into()))
        .font(font)
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
    font: Font,
    font_size: f32,
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
            font,
            font_size,
        ));
    }

    let status = text_status(grid, bytes_received, v_row, v_col, font);

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
    font: Font,
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
    .font(font)
}

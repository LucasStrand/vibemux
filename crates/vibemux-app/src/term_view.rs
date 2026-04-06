use crate::app::Message;
use crate::box_drawing::{self, BoxDrawCell, BoxDrawingOverlay};
use crate::term_selection::{
    term_char_width, term_line_height, TerminalSelection,
};
use crate::theme;
use iced::widget::canvas;
use iced::widget::text::{LineHeight, Span, Wrapping};
use iced::widget::{column, container, mouse_area, rich_text, scrollable, span, stack, Column, Id};
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

/// Bold/italic must not change the monospace advance: the box-drawing canvas is on a fixed grid.
fn terminal_fg_for_cell(fg: Color, bold: bool, italic: bool, dim: bool) -> Color {
    let mut c = fg;
    if dim {
        c = Color::from_rgba(c.r, c.g, c.b, c.a * 0.5);
    }
    if bold {
        c = Color::from_rgba(
            (c.r * 1.2 + 0.04).min(1.0),
            (c.g * 1.2 + 0.04).min(1.0),
            (c.b * 1.2 + 0.04).min(1.0),
            c.a,
        );
    }
    if italic {
        // No italic face: slight cool shift so it stays distinguishable without changing metrics.
        c = Color::from_rgba((c.r * 0.92).max(0.0), (c.g * 0.96).max(0.0), c.b.min(1.0), c.a);
    }
    c
}

/// A contiguous run of cells sharing the same visual attributes.
struct SpanRun {
    text: String,
    fg: Color,
    bg: Option<Color>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
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
    char_width: f32,
    line_height: f32,
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
        let raw_ch = if is_cursor { '\u{2588}' } else { row_cells[c].c };
        // Replace box-drawing chars with spaces – they'll be drawn on the canvas overlay.
        // Keep the cursor block char (is_cursor) so it renders via the text span.
        let ch = if !is_cursor && box_drawing::is_box_drawing(raw_ch) { ' ' } else { raw_ch };

        let (fg, cell_bg) = resolve_colors(&row_cells[c].attrs);
        let bg = if selected {
            Some(SELECTION_BG)
        } else if has_non_default_bg(&row_cells[c].attrs) {
            Some(cell_bg)
        } else {
            None
        };

        let bold = row_cells[c].attrs.bold;
        let dim = row_cells[c].attrs.dim;
        let italic = row_cells[c].attrs.italic;
        let underline = row_cells[c].attrs.underline;
        let strikethrough = row_cells[c].attrs.strikethrough;

        // Try to extend the current run.
        let can_extend = if let Some(last) = runs.last() {
            last.fg == fg && last.bg == bg && last.bold == bold
                && last.dim == dim && last.italic == italic
                && last.underline == underline && last.strikethrough == strikethrough
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
                dim,
                italic,
                underline,
                strikethrough,
            });
        }
    }

    let spans: Vec<Span<'a, (), Font>> = runs
        .into_iter()
        .map(|run| {
            let fg = terminal_fg_for_cell(run.fg, run.bold, run.italic, run.dim);
            let mut s = span(run.text)
                .size(font_size)
                .font(font)
                .color(fg);
            if let Some(bg) = run.bg {
                s = s.background(bg);
            }
            if run.underline {
                s = s.underline(true);
            }
            if run.strikethrough {
                s = s.strikethrough(true);
            }
            s
        })
        .collect();

    let line_w = cols as f32 * char_width;
    let rt = rich_text(spans)
        .size(font_size)
        .line_height(LineHeight::Absolute(line_height.into()))
        .font(font)
        .width(Length::Fixed(line_w))
        .wrapping(Wrapping::None);

    container(rt)
        .width(Length::Fixed(line_w))
        .height(Length::Fixed(line_height))
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
    let char_width = term_char_width(font_size);
    let line_height = term_line_height(font_size);
    let mut lines = Column::new().spacing(0);
    let mut box_cells: Vec<BoxDrawCell> = Vec::new();

    let (v_row, v_col) = visual_cursor(grid, selection);
    let n_lines = grid.display_line_count();
    for row_idx in 0..n_lines {
        let Some(row_cells) = grid.display_line_cells(row_idx) else {
            continue;
        };

        // Collect box-drawing characters for the canvas overlay.
        for c in 0..row_cells.len().min(grid.cols) {
            if row_cells[c].wide_continuation {
                continue;
            }
            let ch = row_cells[c].c;
            if box_drawing::is_box_drawing(ch) {
                let (fg, bg) = resolve_colors(&row_cells[c].attrs);
                box_cells.push(BoxDrawCell {
                    row: row_idx,
                    col: c,
                    ch,
                    fg,
                    bg,
                });
            }
        }

        lines = lines.push(line_element(
            row_idx,
            row_cells,
            grid,
            selection,
            v_row,
            v_col,
            font,
            font_size,
            char_width,
            line_height,
        ));
    }

    let status = text_status(grid, bytes_received, v_row, v_col, font);

    let status_bar = container(status)
        .width(Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(theme::BG_SIDEBAR.into()),
            ..Default::default()
        });

    let content_height = n_lines as f32 * line_height + 8.0; // +padding

    let lines_pane = container(lines)
        .width(Fill)
        .padding(4)
        .style(|_t: &Theme| container::Style {
            background: Some(theme::BG_PRIMARY.into()),
            ..Default::default()
        });

    // Overlay a canvas that draws box-drawing characters as geometric primitives.
    let lines_pane: Element<'a, Message> = if box_cells.is_empty() {
        lines_pane.into()
    } else {
        let overlay = BoxDrawingOverlay {
            cells: box_cells,
            cell_width: char_width,
            cell_height: line_height,
            padding: 4.0,
        };
        stack![
            lines_pane,
            canvas(overlay)
                .width(Fill)
                .height(Length::Fixed(content_height))
        ]
        .width(Fill)
        .into()
    };

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

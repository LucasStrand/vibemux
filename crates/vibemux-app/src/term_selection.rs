//! Terminal text selection in display (scrollback + screen) coordinates.
use vibemux_term::grid::TerminalGrid;

pub const TERM_FONT_SIZE: f32 = 16.0;
pub const TERM_LINE_HEIGHT: f32 = 20.0;
/// Approximate monospace advance; keep in sync with `TERM_FONT_SIZE`.
pub const TERM_CHAR_WIDTH: f32 = TERM_FONT_SIZE * 0.6;

const PADDING: f32 = 4.0;

#[derive(Clone, Debug)]
pub struct TerminalSelection {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl TerminalSelection {
    pub fn normalized(&self) -> ((usize, usize), (usize, usize)) {
        norm_pair(self.anchor, self.head)
    }

    /// Whether this cell is part of the selection (inclusive, row-major range).
    pub fn contains_cell(&self, row: usize, col: usize, cols: usize) -> bool {
        let ((sr, sc), (er, ec)) = self.normalized();
        if row < sr || row > er {
            return false;
        }
        if sr == er {
            return col >= sc && col <= ec;
        }
        if row == sr {
            return col >= sc;
        }
        if row == er {
            return col <= ec;
        }
        col < cols
    }

    pub fn collapsed(&self) -> bool {
        self.anchor == self.head
    }
}

fn norm_pair(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    if a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Move `(row, col)` by `delta_*`, clamped to the display grid.
/// First column of user-editable input on the **current shell line** (PTY cursor row).
/// Stays 0 for scrollback lines or unrecognized prompts.
pub fn input_start_column(grid: &TerminalGrid, display_row: usize) -> usize {
    if display_row != grid.display_cursor_row() {
        return 0;
    }
    let Some(row) = grid.display_line_cells(display_row) else {
        return 0;
    };
    let chars: Vec<char> = row.iter().map(|c| c.c).collect();
    if chars.is_empty() {
        return 0;
    }
    let mut i = 0usize;
    // Skip leading whitespace.
    while i < chars.len() && chars[i] == ' ' {
        i += 1;
    }

    // --- Windows prompts ---
    // PowerShell: "PS " … ">"
    let looks_like_ps =
        i + 3 <= chars.len() && chars[i] == 'P' && chars[i + 1] == 'S' && chars[i + 2].is_whitespace();
    // cmd / path style: "C:\..."
    let looks_like_path_prompt = i + 2 < chars.len()
        && chars[i].is_ascii_alphanumeric()
        && chars.get(i + 1) == Some(&':');

    if looks_like_ps || looks_like_path_prompt {
        // First `>` after the prefix: the prompt terminator.
        for (idx, &ch) in chars.iter().enumerate().skip(i) {
            if ch == '>' {
                let mut col = idx + 1;
                while col < chars.len() && chars[col] == ' ' {
                    col += 1;
                }
                return col.min(grid.cols.saturating_sub(1));
            }
        }
    }

    // --- Unix-style prompts ---
    // Common patterns: "user@host:~$ ", "$ ", "% ", "# ", ">>> "
    // Look for common prompt terminators: "$ ", "% ", "# ", "> "
    // Scan for the *last* prompt-like suffix before the cursor.
    let line_str: String = chars[i..].iter().collect();

    // Check for "$ " or "% " or "# " pattern (common bash/zsh/root prompts).
    for suffix in &["$ ", "% ", "# "] {
        if let Some(pos) = line_str.rfind(suffix) {
            let abs = i + pos + suffix.len();
            if abs <= grid.cursor_col + 1 {
                return abs.min(grid.cols.saturating_sub(1));
            }
        }
    }
    // Also check terminal `$`/`%`/`#` at end without trailing space (cursor is right after).
    for ch in &['$', '%', '#'] {
        if let Some(pos) = line_str.rfind(*ch) {
            let abs = i + pos + 1;
            // Skip any spaces after the prompt char.
            let mut col = abs;
            while col < chars.len() && chars[col] == ' ' {
                col += 1;
            }
            if col <= grid.cursor_col + 1 && col > i {
                return col.min(grid.cols.saturating_sub(1));
            }
        }
    }
    // Python REPL: ">>> "
    if line_str.starts_with(">>> ") {
        return (i + 4).min(grid.cols.saturating_sub(1));
    }

    0
}

/// Clamp column on the PTY cursor row so selection cannot enter the prompt.
pub fn clamp_cell_for_input_line(
    grid: &TerminalGrid,
    row: usize,
    col: usize,
) -> (usize, usize) {
    let min_c = input_start_column(grid, row);
    (row, col.max(min_c))
}

pub fn clamp_selection_to_input(grid: &TerminalGrid, sel: &mut TerminalSelection) {
    let dr = grid.display_cursor_row();
    let min = input_start_column(grid, dr);
    if sel.anchor.0 == dr {
        sel.anchor.1 = sel.anchor.1.max(min);
    }
    if sel.head.0 == dr {
        sel.head.1 = sel.head.1.max(min);
    }
}

/// Rightmost column that belongs to the editable buffer on this line — avoids treating
/// the rest of the fixed-width grid as selectable/copyable padding.
pub fn logical_line_end_col(grid: &TerminalGrid, display_row: usize) -> usize {
    let cols = grid.cols;
    let cap = cols.saturating_sub(1);
    let Some(row) = grid.display_line_cells(display_row) else {
        return cap;
    };
    let len = row.len().min(cols);
    let dr = grid.display_cursor_row();
    let input_start = if display_row == dr {
        input_start_column(grid, display_row)
    } else {
        0
    };
    let scan_from = input_start.min(len.saturating_sub(1));
    let mut last_non_space: Option<usize> = None;
    for i in scan_from..len {
        if row[i].c != ' ' {
            last_non_space = Some(i);
        }
    }
    if display_row == dr {
        match last_non_space {
            Some(ln) => ln.max(grid.cursor_col).max(scan_from).min(cap),
            None => grid.cursor_col.max(scan_from).min(cap),
        }
    } else {
        match last_non_space {
            Some(ln) => ln.max(scan_from).min(cap),
            None => scan_from.min(cap),
        }
    }
}

/// Cap `ec` on the PTY cursor row so copy/cut/delete ignore empty grid past the buffer.
fn clamp_ec_for_export(grid: &TerminalGrid, display_row: usize, ec: usize) -> usize {
    if display_row != grid.display_cursor_row() {
        return ec;
    }
    ec.min(logical_line_end_col(grid, display_row))
}

/// Keystrokes to delete the selected range on the **current input line** in the shell
/// (PSReadLine / readline). Shift+arrows only move our overlay; the PTY cursor stays put,
/// so we move with arrows to column `sc` then send Delete `n` times.
pub fn delete_selection_via_pty(grid: &TerminalGrid, sel: &TerminalSelection) -> Option<Vec<u8>> {
    let ((sr, sc), (er, mut ec)) = sel.normalized();
    let dr = grid.display_cursor_row();
    if sr != dr || er != dr {
        return None;
    }
    ec = clamp_ec_for_export(grid, dr, ec);
    if sc > ec {
        return None;
    }
    let n = ec.saturating_sub(sc).saturating_add(1);
    if n == 0 {
        return None;
    }
    let pty = grid.cursor_col;
    let mut buf = Vec::new();
    if pty < sc {
        for _ in 0..(sc - pty) {
            buf.extend_from_slice(b"\x1b[C");
        }
    } else if pty > sc {
        for _ in 0..(pty - sc) {
            buf.extend_from_slice(b"\x1b[D");
        }
    }
    for _ in 0..n {
        buf.extend_from_slice(b"\x1b[3~");
    }
    Some(buf)
}

pub fn move_cell(
    pos: (usize, usize),
    delta_row: i32,
    delta_col: i32,
    n_lines: usize,
    cols: usize,
) -> (usize, usize) {
    if n_lines == 0 || cols == 0 {
        return (0, 0);
    }
    let max_r = (n_lines - 1) as i32;
    let max_c = (cols - 1) as i32;
    let nr = (pos.0 as i32 + delta_row).clamp(0, max_r) as usize;
    let nc = (pos.1 as i32 + delta_col).clamp(0, max_c) as usize;
    (nr, nc)
}

pub fn point_to_cell(x: f32, y: f32, cols: usize, n_lines: usize) -> (usize, usize) {
    if n_lines == 0 || cols == 0 {
        return (0, 0);
    }
    let row = ((y - PADDING) / TERM_LINE_HEIGHT).floor().max(0.0) as usize;
    let col = ((x - PADDING) / TERM_CHAR_WIDTH).floor().max(0.0) as usize;
    (
        row.min(n_lines.saturating_sub(1)),
        col.min(cols.saturating_sub(1)),
    )
}

pub fn selection_text(grid: &TerminalGrid, sel: &TerminalSelection) -> String {
    let ((sr, sc), (er, ec)) = sel.normalized();
    let dr = grid.display_cursor_row();
    let mut out = String::new();
    for r in sr..=er {
        let Some(row) = grid.display_line_cells(r) else {
            continue;
        };
        let start_c = if r == sr { sc } else { 0 };
        let mut end_c = if r == er {
            ec
        } else {
            grid.cols.saturating_sub(1)
        };
        if r == er && r == dr {
            end_c = clamp_ec_for_export(grid, r, end_c);
        }
        let last_i = row.len().saturating_sub(1);
        let end_c = end_c.min(last_i);
        if start_c > end_c {
            if r < er {
                out.push('\n');
            }
            continue;
        }
        for c in start_c..=end_c {
            // Use the real cell character. The block cursor is only visual; substituting
            // a space here made cut/copy/paste gain bogus spaces.
            out.push(row[c].c);
        }
        if r < er {
            out.push('\n');
        }
    }
    out
}

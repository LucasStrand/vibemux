use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const WHITE: Self = Self::new(255, 255, 255);
    pub const BLACK: Self = Self::new(0, 0, 0);

    pub const DEFAULT_FG: Self = Self::new(204, 204, 204);
    pub const DEFAULT_BG: Self = Self::new(30, 30, 46);
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CellAttributes {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

impl Default for CellAttributes {
    fn default() -> Self {
        Self {
            fg: Color::DEFAULT_FG,
            bg: Color::DEFAULT_BG,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub c: char,
    pub attrs: CellAttributes,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            attrs: CellAttributes::default(),
        }
    }
}

pub struct TerminalGrid {
    cells: Vec<Vec<Cell>>,
    pub rows: usize,
    pub cols: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub cursor_visible: bool,
    scrollback: Vec<Vec<Cell>>,
    pub scroll_offset: usize,
    current_attrs: CellAttributes,
    /// Pending notification parsed from OSC sequences
    pub pending_notification: Option<Notification>,
    /// Current working directory from OSC 7
    pub osc_cwd: Option<String>,
    /// Title set by OSC 0/2
    pub title: Option<String>,
    /// Queued responses to send back to the PTY
    pub response_queue: Vec<Vec<u8>>,
    using_alt_screen: bool,
    saved_primary_screen: Option<SavedScreen>,
    saved_cursor: Option<(usize, usize)>,
    wrap_pending: bool,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub subtitle: Option<String>,
}

#[derive(Debug, Clone)]
struct SavedScreen {
    cells: Vec<Vec<Cell>>,
    scrollback: Vec<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
    cursor_visible: bool,
    wrap_pending: bool,
}

impl TerminalGrid {
    pub fn new(rows: usize, cols: usize) -> Self {
        let cells = blank_cells(rows, cols);
        Self {
            cells,
            rows,
            cols,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            scrollback: Vec::new(),
            scroll_offset: 0,
            current_attrs: CellAttributes::default(),
            pending_notification: None,
            osc_cwd: None,
            title: None,
            response_queue: Vec::new(),
            using_alt_screen: false,
            saved_primary_screen: None,
            saved_cursor: None,
            wrap_pending: false,
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.rows = rows;
        self.cols = cols;
        resize_screen(
            &mut self.cells,
            rows,
            cols,
            &mut self.cursor_row,
            &mut self.cursor_col,
        );
        if let Some(saved) = &mut self.saved_primary_screen {
            resize_screen(
                &mut saved.cells,
                rows,
                cols,
                &mut saved.cursor_row,
                &mut saved.cursor_col,
            );
        }
        self.wrap_pending = false;
    }

    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.cells[row][col]
    }

    pub fn visible_cells(&self) -> &[Vec<Cell>] {
        &self.cells
    }

    /// Lines from top (oldest scrollback) through the active screen (bottom).
    pub fn display_line_count(&self) -> usize {
        self.scrollback.len() + self.rows
    }

    pub fn display_line_cells(&self, display_row: usize) -> Option<&[Cell]> {
        let sb = self.scrollback.len();
        if display_row < sb {
            Some(self.scrollback[display_row].as_slice())
        } else {
            let r = display_row - sb;
            if r < self.rows {
                Some(self.cells[r].as_slice())
            } else {
                None
            }
        }
    }

    /// Cursor row in the combined scrollback + screen coordinates.
    pub fn display_cursor_row(&self) -> usize {
        self.scrollback.len() + self.cursor_row
    }

    fn scroll_up(&mut self) {
        if !self.cells.is_empty() {
            let line = self.cells.remove(0);
            self.scrollback.push(line);
            self.cells.push(blank_row(self.cols));
            if self.scrollback.len() > 10_000 {
                self.scrollback.remove(0);
            }
        }
    }

    fn put_char(&mut self, c: char) {
        if self.wrap_pending {
            self.wrap_to_next_line();
        }
        if self.cursor_row < self.rows && self.cursor_col < self.cols {
            self.cells[self.cursor_row][self.cursor_col] = Cell {
                c,
                attrs: self.current_attrs,
            };
            if self.cursor_col + 1 >= self.cols {
                self.wrap_pending = true;
            } else {
                self.cursor_col += 1;
            }
        }
    }

    fn newline(&mut self) {
        self.wrap_pending = false;
        self.cursor_row += 1;
        if self.cursor_row >= self.rows {
            self.scroll_up();
            self.cursor_row = self.rows - 1;
        }
    }

    fn carriage_return(&mut self) {
        self.cursor_col = 0;
        self.wrap_pending = false;
    }

    /// Backspace is cursor motion only; erasing should be performed by explicit
    /// delete/erase control sequences emitted by the host.
    fn backspace(&mut self) {
        self.wrap_pending = false;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    fn wrap_to_next_line(&mut self) {
        self.wrap_pending = false;
        self.cursor_col = 0;
        self.cursor_row += 1;
        if self.cursor_row >= self.rows {
            self.scroll_up();
            self.cursor_row = self.rows.saturating_sub(1);
        }
    }

    fn erase_in_display(&mut self, mode: u16) {
        if self.rows == 0 {
            return;
        }
        self.wrap_pending = false;
        let r = self.cursor_row.min(self.rows - 1);
        match mode {
            0 => {
                for col in self.cursor_col..self.cols {
                    self.cells[r][col] = Cell::default();
                }
                for row in (r + 1)..self.rows {
                    for col in 0..self.cols {
                        self.cells[row][col] = Cell::default();
                    }
                }
            }
            1 => {
                for row in 0..r {
                    for col in 0..self.cols {
                        self.cells[row][col] = Cell::default();
                    }
                }
                for col in 0..=self.cursor_col.min(self.cols - 1) {
                    self.cells[r][col] = Cell::default();
                }
            }
            2 | 3 => {
                for row in 0..self.rows {
                    for col in 0..self.cols {
                        self.cells[row][col] = Cell::default();
                    }
                }
            }
            _ => {}
        }
    }

    fn erase_in_line(&mut self, mode: u16) {
        if self.rows == 0 {
            return;
        }
        self.wrap_pending = false;
        let r = self.cursor_row.min(self.rows - 1);
        match mode {
            0 => {
                for col in self.cursor_col..self.cols {
                    self.cells[r][col] = Cell::default();
                }
            }
            1 => {
                for col in 0..=self.cursor_col.min(self.cols - 1) {
                    self.cells[r][col] = Cell::default();
                }
            }
            2 => {
                for col in 0..self.cols {
                    self.cells[r][col] = Cell::default();
                }
            }
            _ => {}
        }
    }

    /// ECMA-48 ECH — erase `n` cells with blanks without moving the cursor (CSI n X).
    fn erase_chars(&mut self, n: usize) {
        if self.rows == 0 || n == 0 {
            return;
        }
        self.wrap_pending = false;
        let r = self.cursor_row.min(self.rows - 1);
        let c0 = self.cursor_col.min(self.cols.saturating_sub(1));
        for i in 0..n {
            let c = c0 + i;
            if c < self.cols {
                self.cells[r][c] = Cell::default();
            }
        }
    }

    fn set_sgr(&mut self, params: &[u16]) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.current_attrs = CellAttributes::default(),
                1 => self.current_attrs.bold = true,
                3 => self.current_attrs.italic = true,
                4 => self.current_attrs.underline = true,
                7 => self.current_attrs.inverse = true,
                22 => self.current_attrs.bold = false,
                23 => self.current_attrs.italic = false,
                24 => self.current_attrs.underline = false,
                27 => self.current_attrs.inverse = false,
                30..=37 => {
                    self.current_attrs.fg = ansi_color(params[i] - 30);
                }
                38 => {
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.current_attrs.fg = xterm_256_color(params[i + 2]);
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        self.current_attrs.fg = Color::new(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                39 => self.current_attrs.fg = Color::DEFAULT_FG,
                40..=47 => {
                    self.current_attrs.bg = ansi_color(params[i] - 40);
                }
                48 => {
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.current_attrs.bg = xterm_256_color(params[i + 2]);
                        i += 2;
                    } else if i + 4 < params.len() && params[i + 1] == 2 {
                        self.current_attrs.bg = Color::new(
                            params[i + 2] as u8,
                            params[i + 3] as u8,
                            params[i + 4] as u8,
                        );
                        i += 4;
                    }
                }
                49 => self.current_attrs.bg = Color::DEFAULT_BG,
                90..=97 => {
                    self.current_attrs.fg = ansi_bright_color(params[i] - 90);
                }
                100..=107 => {
                    self.current_attrs.bg = ansi_bright_color(params[i] - 100);
                }
                _ => {}
            }
            i += 1;
        }
    }

    pub fn take_notification(&mut self) -> Option<Notification> {
        self.pending_notification.take()
    }

    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }

    pub fn drain_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.response_queue)
    }

    fn queue_response(&mut self, data: Vec<u8>) {
        self.response_queue.push(data);
    }

    fn save_cursor(&mut self) {
        self.saved_cursor = Some((self.cursor_row, self.cursor_col));
    }

    fn restore_cursor(&mut self) {
        if let Some((row, col)) = self.saved_cursor {
            self.cursor_row = row.min(self.rows.saturating_sub(1));
            self.cursor_col = col.min(self.cols.saturating_sub(1));
        }
        self.wrap_pending = false;
    }

    fn enter_alternate_screen(&mut self) {
        if self.using_alt_screen {
            return;
        }

        self.saved_primary_screen = Some(SavedScreen {
            cells: std::mem::take(&mut self.cells),
            scrollback: std::mem::take(&mut self.scrollback),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            cursor_visible: self.cursor_visible,
            wrap_pending: self.wrap_pending,
        });

        self.cells = blank_cells(self.rows, self.cols);
        self.scrollback.clear();
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.cursor_visible = true;
        self.wrap_pending = false;
        self.using_alt_screen = true;
    }

    fn exit_alternate_screen(&mut self) {
        let Some(saved) = self.saved_primary_screen.take() else {
            return;
        };

        self.cells = saved.cells;
        self.scrollback = saved.scrollback;
        self.cursor_row = saved.cursor_row.min(self.rows.saturating_sub(1));
        self.cursor_col = saved.cursor_col.min(self.cols.saturating_sub(1));
        self.cursor_visible = saved.cursor_visible;
        self.wrap_pending = saved.wrap_pending;
        self.using_alt_screen = false;
    }
}

fn blank_row(cols: usize) -> Vec<Cell> {
    vec![Cell::default(); cols]
}

fn blank_cells(rows: usize, cols: usize) -> Vec<Vec<Cell>> {
    vec![blank_row(cols); rows]
}

fn resize_screen(
    cells: &mut Vec<Vec<Cell>>,
    rows: usize,
    cols: usize,
    cursor_row: &mut usize,
    cursor_col: &mut usize,
) {
    cells.resize(rows, blank_row(cols));
    for row in cells {
        row.resize(cols, Cell::default());
    }
    if *cursor_row >= rows {
        *cursor_row = rows.saturating_sub(1);
    }
    if *cursor_col >= cols {
        *cursor_col = cols.saturating_sub(1);
    }
}

fn ansi_color(idx: u16) -> Color {
    match idx {
        0 => Color::new(0, 0, 0),
        1 => Color::new(205, 49, 49),
        2 => Color::new(13, 188, 121),
        3 => Color::new(229, 229, 16),
        4 => Color::new(36, 114, 200),
        5 => Color::new(188, 63, 188),
        6 => Color::new(17, 168, 205),
        7 => Color::new(204, 204, 204),
        _ => Color::DEFAULT_FG,
    }
}

fn ansi_bright_color(idx: u16) -> Color {
    match idx {
        0 => Color::new(118, 118, 118),
        1 => Color::new(241, 76, 76),
        2 => Color::new(35, 209, 139),
        3 => Color::new(245, 245, 67),
        4 => Color::new(59, 142, 234),
        5 => Color::new(214, 112, 214),
        6 => Color::new(41, 184, 219),
        7 => Color::new(242, 242, 242),
        _ => Color::DEFAULT_FG,
    }
}

fn xterm_256_color(idx: u16) -> Color {
    match idx {
        0..=7 => ansi_color(idx),
        8..=15 => ansi_bright_color(idx - 8),
        16..=231 => {
            let idx = idx - 16;
            let r = (idx / 36) as u8;
            let g = ((idx % 36) / 6) as u8;
            let b = (idx % 6) as u8;
            let to_val = |c: u8| if c == 0 { 0u8 } else { 55 + 40 * c };
            Color::new(to_val(r), to_val(g), to_val(b))
        }
        232..=255 => {
            let v = (8 + 10 * (idx - 232)) as u8;
            Color::new(v, v, v)
        }
        _ => Color::DEFAULT_FG,
    }
}

pub struct VteHandler<'a> {
    grid: &'a mut TerminalGrid,
}

impl<'a> VteHandler<'a> {
    pub fn new(grid: &'a mut TerminalGrid) -> Self {
        Self { grid }
    }
}

impl vte::Perform for VteHandler<'_> {
    fn print(&mut self, c: char) {
        self.grid.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.grid.newline(),
            b'\r' => self.grid.carriage_return(),
            b'\x08' => self.grid.backspace(),
            // Windows / xterm often send DEL for backspace in the echo stream too.
            b'\x7f' => self.grid.backspace(),
            b'\t' => {
                let next_tab = (self.grid.cursor_col + 8) & !7;
                self.grid.cursor_col = next_tab.min(self.grid.cols - 1);
            }
            b'\x07' => {} // bell
            _ => {}
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        let _ = bell_terminated;
        if params.is_empty() {
            return;
        }

        let cmd = std::str::from_utf8(params[0]).unwrap_or("");
        match cmd {
            "0" | "2" => {
                if params.len() > 1 {
                    let title = std::str::from_utf8(params[1]).unwrap_or("").to_string();
                    self.grid.title = Some(title);
                }
            }
            "7" => {
                if params.len() > 1 {
                    let cwd = std::str::from_utf8(params[1]).unwrap_or("").to_string();
                    self.grid.osc_cwd = Some(cwd);
                }
            }
            "9" => {
                if params.len() > 1 {
                    let body = std::str::from_utf8(params[1]).unwrap_or("").to_string();
                    self.grid.pending_notification = Some(Notification {
                        title: "Notification".into(),
                        body,
                        subtitle: None,
                    });
                }
            }
            "777" => {
                if params.len() >= 4 {
                    let action = std::str::from_utf8(params[1]).unwrap_or("");
                    if action == "notify" {
                        let title = std::str::from_utf8(params[2]).unwrap_or("").to_string();
                        let body = std::str::from_utf8(params[3]).unwrap_or("").to_string();
                        self.grid.pending_notification = Some(Notification {
                            title,
                            body,
                            subtitle: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if ignore {
            return;
        }
        let params_vec: Vec<u16> = params.iter().flat_map(|p| p.iter().copied()).collect();
        let p0 = params_vec.first().copied().unwrap_or(0);
        let p1 = if params_vec.len() > 1 { params_vec[1] } else { 0 };
        let is_private = intermediates.contains(&b'?');
        let is_gt = intermediates.contains(&b'>');

        match action {
            'c' if is_gt => {
                // Secondary Device Attributes - respond as VT100
                self.grid.queue_response(b"\x1b[>0;0;0c".to_vec());
                return;
            }
            'c' => {
                // Primary Device Attributes - respond as VT102
                self.grid.queue_response(b"\x1b[?6c".to_vec());
                return;
            }
            'n' => {
                match p0 {
                    5 => {
                        // Device Status Report - respond OK
                        self.grid.queue_response(b"\x1b[0n".to_vec());
                        return;
                    }
                    6 => {
                        // Cursor Position Report
                        let response = format!(
                            "\x1b[{};{}R",
                            self.grid.cursor_row + 1,
                            self.grid.cursor_col + 1
                        );
                        self.grid.queue_response(response.into_bytes());
                        return;
                    }
                    _ => {}
                }
            }
            'A' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                self.grid.cursor_row = self.grid.cursor_row.saturating_sub(n);
            }
            'B' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                self.grid.cursor_row =
                    (self.grid.cursor_row + n).min(self.grid.rows.saturating_sub(1));
            }
            'C' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                self.grid.cursor_col =
                    (self.grid.cursor_col + n).min(self.grid.cols.saturating_sub(1));
            }
            'D' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                self.grid.cursor_col = self.grid.cursor_col.saturating_sub(n);
            }
            'H' | 'f' => {
                self.grid.wrap_pending = false;
                let row = (p0.max(1) - 1) as usize;
                let col = (p1.max(1) - 1) as usize;
                self.grid.cursor_row = row.min(self.grid.rows.saturating_sub(1));
                self.grid.cursor_col = col.min(self.grid.cols.saturating_sub(1));
            }
            'J' => self.grid.erase_in_display(p0),
            'K' => self.grid.erase_in_line(p0),
            // Erase Character (ECH): used by ConPTY/readline-style redraws.
            'X' if intermediates.is_empty() => {
                let n = p0.max(1) as usize;
                self.grid.erase_chars(n);
            }
            'm' => {
                if params_vec.is_empty() {
                    self.grid.set_sgr(&[0]);
                } else {
                    self.grid.set_sgr(&params_vec);
                }
            }
            'h' => {
                if is_private {
                    for &p in &params_vec {
                        match p {
                            1 => {} // Application Cursor Keys - accept silently
                            7 => {} // Auto-wrap mode
                            12 => {} // Cursor blink
                            25 => self.grid.cursor_visible = true,
                            1000 | 1002 | 1003 | 1006 => {} // Mouse tracking
                            1049 => self.grid.enter_alternate_screen(),
                            2004 => {} // Bracketed paste
                            _ => {}
                        }
                    }
                }
            }
            'l' => {
                if is_private {
                    for &p in &params_vec {
                        match p {
                            1 => {}
                            7 => {}
                            12 => {}
                            25 => self.grid.cursor_visible = false,
                            1000 | 1002 | 1003 | 1006 => {}
                            1049 => self.grid.exit_alternate_screen(),
                            2004 => {}
                            _ => {}
                        }
                    }
                }
            }
            'G' => {
                self.grid.wrap_pending = false;
                let col = (p0.max(1) - 1) as usize;
                self.grid.cursor_col = col.min(self.grid.cols.saturating_sub(1));
            }
            'd' => {
                self.grid.wrap_pending = false;
                let row = (p0.max(1) - 1) as usize;
                self.grid.cursor_row = row.min(self.grid.rows.saturating_sub(1));
            }
            's' => self.grid.save_cursor(),
            'u' => self.grid.restore_cursor(),
            'L' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                for _ in 0..n {
                    if self.grid.cursor_row < self.grid.rows {
                        self.grid
                            .cells
                            .insert(self.grid.cursor_row, blank_row(self.grid.cols));
                        if self.grid.cells.len() > self.grid.rows {
                            self.grid.cells.pop();
                        }
                    }
                }
            }
            'M' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                for _ in 0..n {
                    if self.grid.cursor_row < self.grid.cells.len() {
                        self.grid.cells.remove(self.grid.cursor_row);
                        self.grid.cells.push(blank_row(self.grid.cols));
                    }
                }
            }
            'P' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                let row = self.grid.cursor_row;
                let col = self.grid.cursor_col;
                if row < self.grid.rows {
                    for _ in 0..n {
                        if col < self.grid.cells[row].len() {
                            self.grid.cells[row].remove(col);
                            self.grid.cells[row].push(Cell::default());
                        }
                    }
                }
            }
            '@' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                let row = self.grid.cursor_row;
                let col = self.grid.cursor_col;
                if row < self.grid.rows {
                    for _ in 0..n {
                        if self.grid.cells[row].len() >= self.grid.cols {
                            self.grid.cells[row].pop();
                        }
                        self.grid.cells[row].insert(col, Cell::default());
                    }
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'7' => self.grid.save_cursor(),
            b'8' => self.grid.restore_cursor(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TerminalGrid, VteHandler};

    fn feed(grid: &mut TerminalGrid, bytes: &[u8]) {
        let mut parser = vte::Parser::new();
        let mut handler = VteHandler::new(grid);
        parser.advance(&mut handler, bytes);
    }

    #[test]
    fn wraps_only_before_next_print() {
        let mut grid = TerminalGrid::new(2, 3);
        feed(&mut grid, b"abc");
        assert_eq!((grid.cursor_row, grid.cursor_col), (0, 2));
        assert_eq!(grid.cell(0, 0).c, 'a');
        assert_eq!(grid.cell(0, 1).c, 'b');
        assert_eq!(grid.cell(0, 2).c, 'c');

        feed(&mut grid, b"d");
        assert_eq!((grid.cursor_row, grid.cursor_col), (1, 1));
        assert_eq!(grid.cell(1, 0).c, 'd');
    }

    #[test]
    fn alternate_screen_restores_primary_content() {
        let mut grid = TerminalGrid::new(3, 8);
        feed(&mut grid, b"main");
        feed(&mut grid, b"\x1b[?1049halt");
        assert_eq!(grid.cell(0, 0).c, 'a');

        feed(&mut grid, b"\x1b[?1049l");
        assert_eq!(grid.cell(0, 0).c, 'm');
        assert_eq!(grid.cell(0, 1).c, 'a');
        assert_eq!(grid.cell(0, 2).c, 'i');
        assert_eq!(grid.cell(0, 3).c, 'n');
    }
}

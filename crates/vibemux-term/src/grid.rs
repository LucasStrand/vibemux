use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use unicode_width::UnicodeWidthChar;

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
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub strikethrough: bool,
}

impl Default for CellAttributes {
    fn default() -> Self {
        Self {
            fg: Color::DEFAULT_FG,
            bg: Color::DEFAULT_BG,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
            strikethrough: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub c: char,
    pub attrs: CellAttributes,
    /// True for the second cell of a double-width character.
    pub wide_continuation: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            attrs: CellAttributes::default(),
            wide_continuation: false,
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
    scrollback: VecDeque<Vec<Cell>>,
    scrollback_limit: usize,
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
    /// Mouse tracking modes enabled by the application.
    pub mouse_tracking: MouseTracking,
    /// Whether SGR (1006) extended mouse mode is active.
    pub mouse_sgr_mode: bool,
    /// Dirty flag: set when grid content changes, cleared by the renderer.
    pub dirty: bool,
    /// Scroll region: top margin (0-based, inclusive).
    scroll_top: usize,
    /// Scroll region: bottom margin (0-based, inclusive).
    scroll_bottom: usize,
}

/// Which mouse events the terminal application wants reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseTracking {
    Off,
    /// Mode 1000: report button press/release.
    Normal,
    /// Mode 1002: report button press/release + drag with button held.
    ButtonEvent,
    /// Mode 1003: report all motion (even without button).
    AnyEvent,
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
    scrollback: VecDeque<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
    cursor_visible: bool,
    wrap_pending: bool,
    current_attrs: CellAttributes,
}

impl TerminalGrid {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self::with_scrollback_limit(rows, cols, 10_000)
    }

    pub fn with_scrollback_limit(rows: usize, cols: usize, scrollback_limit: usize) -> Self {
        let cells = blank_cells(rows, cols);
        Self {
            cells,
            rows,
            cols,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            scrollback: VecDeque::new(),
            scrollback_limit,
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
            mouse_tracking: MouseTracking::Off,
            mouse_sgr_mode: false,
            dirty: true,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.rows = rows;
        self.cols = cols;
        // Reset scroll region to full screen on resize.
        self.scroll_top = 0;
        self.scroll_bottom = rows.saturating_sub(1);
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
        self.dirty = true;
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

    /// Scroll the content inside the scroll region up by one line.
    /// The top line of the region is removed and a blank line is inserted at the bottom.
    /// If the scroll region covers the whole screen and cursor is at row 0,
    /// the removed line goes into scrollback.
    fn scroll_region_up(&mut self) {
        let top = self.scroll_top;
        let bot = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if top > bot || bot >= self.rows {
            return;
        }
        let line = self.cells.remove(top);
        // Only push to scrollback if this is a full-screen scroll (top == 0).
        if top == 0 && !self.using_alt_screen {
            self.scrollback.push_back(line);
            while self.scrollback.len() > self.scrollback_limit {
                self.scrollback.pop_front();
            }
        }
        // Insert blank line at the bottom of the scroll region.
        self.cells.insert(bot, blank_row(self.cols));
        // Ensure we still have exactly `self.rows` rows (should already be the case).
        self.cells.truncate(self.rows);
        while self.cells.len() < self.rows {
            self.cells.push(blank_row(self.cols));
        }
    }

    /// Scroll the content inside the scroll region down by one line.
    /// A blank line is inserted at the top of the region and the bottom line is removed.
    fn scroll_region_down(&mut self) {
        let top = self.scroll_top;
        let bot = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if top > bot || bot >= self.rows {
            return;
        }
        self.cells.remove(bot);
        self.cells.insert(top, blank_row(self.cols));
        self.cells.truncate(self.rows);
        while self.cells.len() < self.rows {
            self.cells.push(blank_row(self.cols));
        }
    }

    fn put_char(&mut self, c: char) {
        if self.wrap_pending {
            self.wrap_to_next_line();
        }
        let w = c.width().unwrap_or(0);
        if w == 0 {
            return;
        }
        if self.cursor_row < self.rows && self.cursor_col < self.cols {
            if w == 2 && self.cursor_col + 1 >= self.cols {
                self.cells[self.cursor_row][self.cursor_col] = Cell::default();
                self.wrap_to_next_line();
            }

            self.cells[self.cursor_row][self.cursor_col] = Cell {
                c,
                attrs: self.current_attrs,
                wide_continuation: false,
            };

            if w == 2 && self.cursor_col + 1 < self.cols {
                self.cells[self.cursor_row][self.cursor_col + 1] = Cell {
                    c: ' ',
                    attrs: self.current_attrs,
                    wide_continuation: true,
                };
                self.cursor_col += 1;
            }

            if self.cursor_col + 1 >= self.cols {
                self.wrap_pending = true;
            } else {
                self.cursor_col += 1;
            }
        }
        self.dirty = true;
    }

    /// Linefeed / newline: move cursor down. If at the bottom of the scroll region,
    /// scroll the region up instead.
    fn newline(&mut self) {
        self.wrap_pending = false;
        let bot = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if self.cursor_row == bot {
            // At the bottom margin — scroll the region up.
            self.scroll_region_up();
        } else if self.cursor_row < self.rows.saturating_sub(1) {
            self.cursor_row += 1;
        }
        self.dirty = true;
    }

    /// Reverse index (ESC M): move cursor up. If at the top of the scroll region,
    /// scroll the region down instead.
    fn reverse_index(&mut self) {
        self.wrap_pending = false;
        if self.cursor_row == self.scroll_top {
            self.scroll_region_down();
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
        }
        self.dirty = true;
    }

    fn carriage_return(&mut self) {
        self.cursor_col = 0;
        self.wrap_pending = false;
    }

    fn backspace(&mut self) {
        self.wrap_pending = false;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    fn wrap_to_next_line(&mut self) {
        self.wrap_pending = false;
        self.cursor_col = 0;
        let bot = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if self.cursor_row == bot {
            self.scroll_region_up();
        } else if self.cursor_row < self.rows.saturating_sub(1) {
            self.cursor_row += 1;
        }
    }

    /// Set scroll region (DECSTBM). Parameters are 1-based; 0 means default.
    fn set_scroll_region(&mut self, top_1: u16, bottom_1: u16) {
        let top = if top_1 == 0 { 0 } else { (top_1 - 1) as usize };
        let bottom = if bottom_1 == 0 {
            self.rows.saturating_sub(1)
        } else {
            ((bottom_1 - 1) as usize).min(self.rows.saturating_sub(1))
        };
        if top < bottom && bottom < self.rows {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        }
        // DECSTBM also homes the cursor.
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.wrap_pending = false;
        self.dirty = true;
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
            2 => {
                for row in 0..self.rows {
                    for col in 0..self.cols {
                        self.cells[row][col] = Cell::default();
                    }
                }
            }
            3 => {
                for row in 0..self.rows {
                    for col in 0..self.cols {
                        self.cells[row][col] = Cell::default();
                    }
                }
                self.scrollback.clear();
            }
            _ => {}
        }
        self.dirty = true;
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
        self.dirty = true;
    }

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
        self.dirty = true;
    }

    /// Insert `n` blank lines at the cursor row, within the scroll region.
    fn insert_lines(&mut self, n: usize) {
        self.wrap_pending = false;
        let bot = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if self.cursor_row > bot {
            return;
        }
        for _ in 0..n {
            if self.cursor_row <= bot && bot < self.cells.len() {
                self.cells.remove(bot);
                self.cells.insert(self.cursor_row, blank_row(self.cols));
            }
        }
        // Ensure grid stays the right size.
        self.cells.truncate(self.rows);
        while self.cells.len() < self.rows {
            self.cells.push(blank_row(self.cols));
        }
        self.dirty = true;
    }

    /// Delete `n` lines at the cursor row, within the scroll region.
    fn delete_lines(&mut self, n: usize) {
        self.wrap_pending = false;
        let bot = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if self.cursor_row > bot {
            return;
        }
        for _ in 0..n {
            if self.cursor_row < self.cells.len() && self.cursor_row <= bot {
                self.cells.remove(self.cursor_row);
                // Insert blank at the bottom of the scroll region.
                let insert_at = bot.min(self.cells.len());
                self.cells.insert(insert_at, blank_row(self.cols));
            }
        }
        self.cells.truncate(self.rows);
        while self.cells.len() < self.rows {
            self.cells.push(blank_row(self.cols));
        }
        self.dirty = true;
    }

    /// CSI S: Scroll up `n` lines within the scroll region.
    fn scroll_up_n(&mut self, n: usize) {
        for _ in 0..n {
            self.scroll_region_up();
        }
        self.dirty = true;
    }

    /// CSI T: Scroll down `n` lines within the scroll region.
    fn scroll_down_n(&mut self, n: usize) {
        for _ in 0..n {
            self.scroll_region_down();
        }
        self.dirty = true;
    }

    fn set_sgr(&mut self, params: &[u16]) {
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.current_attrs = CellAttributes::default(),
                1 => self.current_attrs.bold = true,
                2 => self.current_attrs.dim = true,
                3 => self.current_attrs.italic = true,
                4 => self.current_attrs.underline = true,
                7 => self.current_attrs.inverse = true,
                9 => self.current_attrs.strikethrough = true,
                22 => {
                    self.current_attrs.bold = false;
                    self.current_attrs.dim = false;
                }
                23 => self.current_attrs.italic = false,
                24 => self.current_attrs.underline = false,
                27 => self.current_attrs.inverse = false,
                29 => self.current_attrs.strikethrough = false,
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
            current_attrs: self.current_attrs,
        });

        self.cells = blank_cells(self.rows, self.cols);
        self.scrollback.clear();
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.cursor_visible = true;
        self.wrap_pending = false;
        self.using_alt_screen = true;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
        self.dirty = true;
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
        self.current_attrs = saved.current_attrs;
        self.using_alt_screen = false;
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
        self.dirty = true;
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
            b'\x7f' => self.grid.backspace(),
            b'\t' => {
                let next_tab = (self.grid.cursor_col + 8) & !7;
                self.grid.cursor_col = next_tab.min(self.grid.cols.saturating_sub(1));
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
                self.grid.queue_response(b"\x1b[>0;0;0c".to_vec());
                return;
            }
            'c' => {
                self.grid.queue_response(b"\x1b[?6c".to_vec());
                return;
            }
            'n' => {
                match p0 {
                    5 => {
                        self.grid.queue_response(b"\x1b[0n".to_vec());
                        return;
                    }
                    6 => {
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
                self.grid.dirty = true;
            }
            'B' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                self.grid.cursor_row =
                    (self.grid.cursor_row + n).min(self.grid.rows.saturating_sub(1));
                self.grid.dirty = true;
            }
            'C' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                self.grid.cursor_col =
                    (self.grid.cursor_col + n).min(self.grid.cols.saturating_sub(1));
                self.grid.dirty = true;
            }
            'D' => {
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                self.grid.cursor_col = self.grid.cursor_col.saturating_sub(n);
                self.grid.dirty = true;
            }
            'E' => {
                // Cursor Next Line: move down N lines, to column 0.
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                self.grid.cursor_row =
                    (self.grid.cursor_row + n).min(self.grid.rows.saturating_sub(1));
                self.grid.cursor_col = 0;
                self.grid.dirty = true;
            }
            'F' => {
                // Cursor Previous Line: move up N lines, to column 0.
                self.grid.wrap_pending = false;
                let n = p0.max(1) as usize;
                self.grid.cursor_row = self.grid.cursor_row.saturating_sub(n);
                self.grid.cursor_col = 0;
                self.grid.dirty = true;
            }
            'H' | 'f' => {
                self.grid.wrap_pending = false;
                let row = (p0.max(1) - 1) as usize;
                let col = (p1.max(1) - 1) as usize;
                self.grid.cursor_row = row.min(self.grid.rows.saturating_sub(1));
                self.grid.cursor_col = col.min(self.grid.cols.saturating_sub(1));
                self.grid.dirty = true;
            }
            'J' => self.grid.erase_in_display(p0),
            'K' => self.grid.erase_in_line(p0),
            'X' if intermediates.is_empty() => {
                let n = p0.max(1) as usize;
                self.grid.erase_chars(n);
            }
            'L' => {
                let n = p0.max(1) as usize;
                self.grid.insert_lines(n);
            }
            'M' => {
                let n = p0.max(1) as usize;
                self.grid.delete_lines(n);
            }
            'S' if !is_private => {
                // Scroll Up: scroll content up N lines within scroll region.
                let n = p0.max(1) as usize;
                self.grid.scroll_up_n(n);
            }
            'T' if !is_private => {
                // Scroll Down: scroll content down N lines within scroll region.
                let n = p0.max(1) as usize;
                self.grid.scroll_down_n(n);
            }
            'r' if !is_private => {
                // DECSTBM: Set Top and Bottom Margins (scroll region).
                self.grid.set_scroll_region(p0, p1);
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
                            1 => {}
                            7 => {}
                            12 => {}
                            25 => self.grid.cursor_visible = true,
                            1000 => self.grid.mouse_tracking = MouseTracking::Normal,
                            1002 => self.grid.mouse_tracking = MouseTracking::ButtonEvent,
                            1003 => self.grid.mouse_tracking = MouseTracking::AnyEvent,
                            1006 => self.grid.mouse_sgr_mode = true,
                            1049 => self.grid.enter_alternate_screen(),
                            2004 => {}
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
                            1000 | 1002 | 1003 => self.grid.mouse_tracking = MouseTracking::Off,
                            1006 => self.grid.mouse_sgr_mode = false,
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
                self.grid.dirty = true;
            }
            'd' => {
                self.grid.wrap_pending = false;
                let row = (p0.max(1) - 1) as usize;
                self.grid.cursor_row = row.min(self.grid.rows.saturating_sub(1));
                self.grid.dirty = true;
            }
            's' => self.grid.save_cursor(),
            'u' => self.grid.restore_cursor(),
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
                self.grid.dirty = true;
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
                self.grid.dirty = true;
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'7' => self.grid.save_cursor(),
            b'8' => self.grid.restore_cursor(),
            // ESC M: Reverse Index — move cursor up, scroll region down if at top.
            b'M' => self.grid.reverse_index(),
            // ESC D: Index — move cursor down, scroll region up if at bottom.
            b'D' => self.grid.newline(),
            // ESC E: Next Line — CR + LF.
            b'E' => {
                self.grid.carriage_return();
                self.grid.newline();
            }
            // ESC c: Full Reset (RIS).
            b'c' => {
                let rows = self.grid.rows;
                let cols = self.grid.cols;
                let limit = self.grid.scrollback_limit;
                *self.grid = TerminalGrid::with_scrollback_limit(rows, cols, limit);
            }
            // ESC ( 0 / ESC ( B: character set selection — silently ignore.
            b'(' => {}
            _ => {
                // ESC ( 0, ESC ( B etc. come through intermediates.
                if intermediates == b"(" {
                    // Silently accept character set designations.
                }
            }
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

    #[test]
    fn wide_char_occupies_two_cells() {
        let mut grid = TerminalGrid::new(2, 10);
        let mut parser = vte::Parser::new();
        let mut handler = VteHandler::new(&mut grid);
        parser.advance(&mut handler, "世".as_bytes());
        assert_eq!(grid.cell(0, 0).c, '世');
        assert!(!grid.cell(0, 0).wide_continuation);
        assert!(grid.cell(0, 1).wide_continuation);
        assert_eq!(grid.cursor_col, 2);
    }

    #[test]
    fn scroll_region_keeps_header_footer() {
        // Simulate a TUI: set scroll region to middle rows (2-4 of a 6-row terminal).
        let mut grid = TerminalGrid::new(6, 10);
        let mut parser = vte::Parser::new();

        // Write header on row 0.
        {
            let mut h = VteHandler::new(&mut grid);
            parser.advance(&mut h, b"\x1b[1;1H"); // Move to row 1, col 1
            parser.advance(&mut h, b"HEADER");
        }
        // Write footer on row 5 (last row).
        {
            let mut h = VteHandler::new(&mut grid);
            parser.advance(&mut h, b"\x1b[6;1H"); // Move to row 6
            parser.advance(&mut h, b"FOOTER");
        }
        // Set scroll region to rows 2-5 (1-based).
        {
            let mut h = VteHandler::new(&mut grid);
            parser.advance(&mut h, b"\x1b[2;5r");
        }
        // Move to bottom of scroll region and print lines to trigger scrolling.
        {
            let mut h = VteHandler::new(&mut grid);
            parser.advance(&mut h, b"\x1b[5;1H"); // row 5 (bottom of region)
            parser.advance(&mut h, b"line1\n");
            parser.advance(&mut h, b"line2\n");
            parser.advance(&mut h, b"line3");
        }

        // Header (row 0) and footer (row 5) should be unchanged.
        let header: String = grid.cells[0].iter().take(6).map(|c| c.c).collect();
        let footer: String = grid.cells[5].iter().take(6).map(|c| c.c).collect();
        assert_eq!(header, "HEADER");
        assert_eq!(footer, "FOOTER");
    }

    #[test]
    fn reverse_index_scrolls_region_down() {
        let mut grid = TerminalGrid::new(5, 10);
        let mut parser = vte::Parser::new();

        // Set scroll region to rows 2-4.
        {
            let mut h = VteHandler::new(&mut grid);
            parser.advance(&mut h, b"\x1b[2;4r");
        }
        // Move cursor to top of scroll region (row 2, 1-based = row 1, 0-based).
        {
            let mut h = VteHandler::new(&mut grid);
            parser.advance(&mut h, b"\x1b[2;1H");
            parser.advance(&mut h, b"TOP");
        }
        // Reverse index at the top of scroll region should push content down.
        {
            let mut h = VteHandler::new(&mut grid);
            parser.advance(&mut h, b"\x1b[2;1H");
            parser.advance(&mut h, b"\x1bM"); // ESC M = reverse index
        }
        // Row 1 (0-based) should now be blank (new line inserted).
        let row1: String = grid.cells[1].iter().take(3).map(|c| c.c).collect();
        assert_eq!(row1, "   ");
        // "TOP" should have moved down to row 2.
        let row2: String = grid.cells[2].iter().take(3).map(|c| c.c).collect();
        assert_eq!(row2, "TOP");
    }
}

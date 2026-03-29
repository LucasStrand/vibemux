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
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub subtitle: Option<String>,
}

impl TerminalGrid {
    pub fn new(rows: usize, cols: usize) -> Self {
        let cells = vec![vec![Cell::default(); cols]; rows];
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
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.rows = rows;
        self.cols = cols;
        self.cells.resize(rows, vec![Cell::default(); cols]);
        for row in &mut self.cells {
            row.resize(cols, Cell::default());
        }
        if self.cursor_row >= rows {
            self.cursor_row = rows.saturating_sub(1);
        }
        if self.cursor_col >= cols {
            self.cursor_col = cols.saturating_sub(1);
        }
    }

    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.cells[row][col]
    }

    pub fn visible_cells(&self) -> &[Vec<Cell>] {
        &self.cells
    }

    fn scroll_up(&mut self) {
        if !self.cells.is_empty() {
            let line = self.cells.remove(0);
            self.scrollback.push(line);
            self.cells.push(vec![Cell::default(); self.cols]);
            if self.scrollback.len() > 10_000 {
                self.scrollback.remove(0);
            }
        }
    }

    fn put_char(&mut self, c: char) {
        if self.cursor_row < self.rows && self.cursor_col < self.cols {
            self.cells[self.cursor_row][self.cursor_col] = Cell {
                c,
                attrs: self.current_attrs,
            };
            self.cursor_col += 1;
            if self.cursor_col >= self.cols {
                self.cursor_col = 0;
                self.cursor_row += 1;
                if self.cursor_row >= self.rows {
                    self.scroll_up();
                    self.cursor_row = self.rows - 1;
                }
            }
        }
    }

    fn newline(&mut self) {
        self.cursor_row += 1;
        if self.cursor_row >= self.rows {
            self.scroll_up();
            self.cursor_row = self.rows - 1;
        }
    }

    fn carriage_return(&mut self) {
        self.cursor_col = 0;
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    fn erase_in_display(&mut self, mode: u16) {
        match mode {
            0 => {
                for col in self.cursor_col..self.cols {
                    self.cells[self.cursor_row][col] = Cell::default();
                }
                for row in (self.cursor_row + 1)..self.rows {
                    for col in 0..self.cols {
                        self.cells[row][col] = Cell::default();
                    }
                }
            }
            1 => {
                for row in 0..self.cursor_row {
                    for col in 0..self.cols {
                        self.cells[row][col] = Cell::default();
                    }
                }
                for col in 0..=self.cursor_col.min(self.cols - 1) {
                    self.cells[self.cursor_row][col] = Cell::default();
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
        match mode {
            0 => {
                for col in self.cursor_col..self.cols {
                    self.cells[self.cursor_row][col] = Cell::default();
                }
            }
            1 => {
                for col in 0..=self.cursor_col.min(self.cols - 1) {
                    self.cells[self.cursor_row][col] = Cell::default();
                }
            }
            2 => {
                for col in 0..self.cols {
                    self.cells[self.cursor_row][col] = Cell::default();
                }
            }
            _ => {}
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
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let params_vec: Vec<u16> = params.iter().flat_map(|p| p.iter().copied()).collect();
        let p0 = params_vec.first().copied().unwrap_or(0);
        let p1 = if params_vec.len() > 1 { params_vec[1] } else { 0 };

        match action {
            'A' => {
                let n = p0.max(1) as usize;
                self.grid.cursor_row = self.grid.cursor_row.saturating_sub(n);
            }
            'B' => {
                let n = p0.max(1) as usize;
                self.grid.cursor_row =
                    (self.grid.cursor_row + n).min(self.grid.rows.saturating_sub(1));
            }
            'C' => {
                let n = p0.max(1) as usize;
                self.grid.cursor_col =
                    (self.grid.cursor_col + n).min(self.grid.cols.saturating_sub(1));
            }
            'D' => {
                let n = p0.max(1) as usize;
                self.grid.cursor_col = self.grid.cursor_col.saturating_sub(n);
            }
            'H' | 'f' => {
                let row = (p0.max(1) - 1) as usize;
                let col = (p1.max(1) - 1) as usize;
                self.grid.cursor_row = row.min(self.grid.rows.saturating_sub(1));
                self.grid.cursor_col = col.min(self.grid.cols.saturating_sub(1));
            }
            'J' => self.grid.erase_in_display(p0),
            'K' => self.grid.erase_in_line(p0),
            'm' => {
                if params_vec.is_empty() {
                    self.grid.set_sgr(&[0]);
                } else {
                    self.grid.set_sgr(&params_vec);
                }
            }
            'h' => {
                if p0 == 25 {
                    self.grid.cursor_visible = true;
                }
            }
            'l' => {
                if p0 == 25 {
                    self.grid.cursor_visible = false;
                }
            }
            'G' => {
                let col = (p0.max(1) - 1) as usize;
                self.grid.cursor_col = col.min(self.grid.cols.saturating_sub(1));
            }
            'd' => {
                let row = (p0.max(1) - 1) as usize;
                self.grid.cursor_row = row.min(self.grid.rows.saturating_sub(1));
            }
            'L' => {
                let n = p0.max(1) as usize;
                for _ in 0..n {
                    if self.grid.cursor_row < self.grid.rows {
                        self.grid
                            .cells
                            .insert(self.grid.cursor_row, vec![Cell::default(); self.grid.cols]);
                        if self.grid.cells.len() > self.grid.rows {
                            self.grid.cells.pop();
                        }
                    }
                }
            }
            'M' => {
                let n = p0.max(1) as usize;
                for _ in 0..n {
                    if self.grid.cursor_row < self.grid.cells.len() {
                        self.grid.cells.remove(self.grid.cursor_row);
                        self.grid.cells.push(vec![Cell::default(); self.grid.cols]);
                    }
                }
            }
            'P' => {
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

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

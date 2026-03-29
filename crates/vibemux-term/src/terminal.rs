use crate::grid::{Notification, TerminalGrid, VteHandler};
use crate::pty_backend::PtyBackend;
use anyhow::Result;
use uuid::Uuid;

pub struct Terminal {
    pub id: Uuid,
    pub grid: TerminalGrid,
    pub pty: PtyBackend,
    parser: vte::Parser,
    pub alive: bool,
}

impl Terminal {
    pub fn spawn(rows: u16, cols: u16, shell: Option<&str>) -> Result<Self> {
        let pty = PtyBackend::spawn(rows, cols, shell)?;
        Ok(Self {
            id: Uuid::new_v4(),
            grid: TerminalGrid::new(rows as usize, cols as usize),
            pty,
            parser: vte::Parser::new(),
            alive: true,
        })
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.pty.resize(rows, cols)?;
        self.grid.resize(rows as usize, cols as usize);
        Ok(())
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.pty.write_all(data)
    }

    pub fn process_output(&mut self, data: &[u8]) {
        let mut handler = VteHandler::new(&mut self.grid);
        self.parser.advance(&mut handler, data);
    }

    pub fn take_notification(&mut self) -> Option<Notification> {
        self.grid.take_notification()
    }
}

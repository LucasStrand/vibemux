use crate::grid::{MouseTracking, Notification, TerminalGrid, VteHandler};
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
        Self::spawn_with_scrollback(rows, cols, shell, 10_000)
    }

    pub fn spawn_with_scrollback(
        rows: u16,
        cols: u16,
        shell: Option<&str>,
        scrollback_limit: usize,
    ) -> Result<Self> {
        let pty = PtyBackend::spawn(rows, cols, shell)?;
        Ok(Self {
            id: Uuid::new_v4(),
            grid: TerminalGrid::with_scrollback_limit(
                rows as usize,
                cols as usize,
                scrollback_limit,
            ),
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

        let responses = self.grid.drain_responses();
        for response in responses {
            let _ = self.pty.write_all(&response);
        }
    }

    pub fn take_notification(&mut self) -> Option<Notification> {
        self.grid.take_notification()
    }

    /// Encode a mouse event and write it to the PTY if the application has
    /// requested mouse tracking. Returns true if the event was consumed.
    pub fn send_mouse_event(&mut self, event: MouseEvent) -> bool {
        if self.grid.mouse_tracking == MouseTracking::Off {
            return false;
        }
        let should_report = match self.grid.mouse_tracking {
            MouseTracking::Off => false,
            MouseTracking::Normal => {
                matches!(event.kind, MouseEventKind::Press | MouseEventKind::Release)
            }
            MouseTracking::ButtonEvent => {
                matches!(
                    event.kind,
                    MouseEventKind::Press | MouseEventKind::Release | MouseEventKind::Drag
                )
            }
            MouseTracking::AnyEvent => true,
        };
        if !should_report {
            return false;
        }
        let encoded = if self.grid.mouse_sgr_mode {
            encode_mouse_sgr(&event)
        } else {
            encode_mouse_x10(&event)
        };
        if let Some(bytes) = encoded {
            let _ = self.pty.write_all(&bytes);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug, Clone, Copy)]
pub enum MouseEventKind {
    Press,
    Release,
    Drag,
    Move,
}

#[derive(Debug, Clone, Copy)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub button: MouseButton,
    /// 0-based column.
    pub col: u16,
    /// 0-based row.
    pub row: u16,
}

fn encode_mouse_sgr(ev: &MouseEvent) -> Option<Vec<u8>> {
    let btn = match ev.button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::ScrollUp => 64,
        MouseButton::ScrollDown => 65,
    };
    let cb = match ev.kind {
        MouseEventKind::Drag | MouseEventKind::Move => btn + 32,
        _ => btn,
    };
    let suffix = if matches!(ev.kind, MouseEventKind::Release) {
        'm'
    } else {
        'M'
    };
    // SGR uses 1-based coordinates.
    Some(
        format!(
            "\x1b[<{};{};{}{}",
            cb,
            ev.col as u32 + 1,
            ev.row as u32 + 1,
            suffix
        )
        .into_bytes(),
    )
}

fn encode_mouse_x10(ev: &MouseEvent) -> Option<Vec<u8>> {
    let btn = match ev.kind {
        MouseEventKind::Release => 3,
        _ => match ev.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::ScrollUp => 64,
            MouseButton::ScrollDown => 65,
        },
    };
    let cb = match ev.kind {
        MouseEventKind::Drag | MouseEventKind::Move => btn + 32,
        _ => btn,
    };
    // X10 encoding: values + 32, max 223 (fits in a byte).
    let cx = (ev.col as u32 + 1).min(223) + 32;
    let cy = (ev.row as u32 + 1).min(223) + 32;
    Some(vec![b'\x1b', b'[', b'M', (cb + 32) as u8, cx as u8, cy as u8])
}

use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

pub struct PtyBackend {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    reader: Arc<Mutex<Box<dyn Read + Send>>>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtyBackend {
    pub fn spawn(rows: u16, cols: u16, shell: Option<&str>) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell_cmd = shell
            .map(String::from)
            .unwrap_or_else(|| detect_shell());

        let mut cmd = CommandBuilder::new(&shell_cmd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("VIBEMUX", "1");

        let child = pair.slave.spawn_command(cmd)?;
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        Ok(Self {
            master: pair.master,
            writer,
            reader: Arc::new(Mutex::new(reader)),
            _child: child,
        })
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn write_all(&mut self, data: &[u8]) -> Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn try_read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut reader = self.reader.lock().unwrap();
        match reader.read(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(e.into()),
        }
    }

    pub fn get_reader(&self) -> Arc<Mutex<Box<dyn Read + Send>>> {
        Arc::clone(&self.reader)
    }
}

fn detect_shell() -> String {
    for candidate in &["pwsh.exe", "powershell.exe", "cmd.exe"] {
        if which::which(candidate).is_ok() {
            return candidate.to_string();
        }
    }
    "cmd.exe".to_string()
}

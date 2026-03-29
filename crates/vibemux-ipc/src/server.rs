use crate::protocol::{Request, Response};
use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::mpsc;

pub const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\vibemux";

pub enum IpcCommand {
    Request {
        request: Request,
        reply: tokio::sync::oneshot::Sender<Response>,
    },
}

pub struct IpcServer {
    pipe_name: String,
}

impl IpcServer {
    pub fn new(pipe_name: Option<String>) -> Self {
        Self {
            pipe_name: pipe_name.unwrap_or_else(|| DEFAULT_PIPE_NAME.to_string()),
        }
    }

    pub async fn run(self, cmd_tx: mpsc::UnboundedSender<IpcCommand>) -> Result<()> {
        loop {
            let server = ServerOptions::new()
                .first_pipe_instance(false)
                .create(&self.pipe_name)?;

            server.connect().await?;
            let cmd_tx = cmd_tx.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_client(server, cmd_tx).await {
                    log::error!("IPC client error: {e}");
                }
            });
        }
    }
}

async fn handle_client(
    pipe: NamedPipeServer,
    cmd_tx: mpsc::UnboundedSender<IpcCommand>,
) -> Result<()> {
    let (reader, mut writer) = tokio::io::split(pipe);
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::error("unknown", format!("Invalid JSON: {e}"));
                let mut out = serde_json::to_vec(&resp)?;
                out.push(b'\n');
                writer.write_all(&out).await?;
                continue;
            }
        };

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let _ = cmd_tx.send(IpcCommand::Request {
            request,
            reply: reply_tx,
        });

        if let Ok(response) = reply_rx.await {
            let mut out = serde_json::to_vec(&response)?;
            out.push(b'\n');
            writer.write_all(&out).await?;
        }
    }

    Ok(())
}

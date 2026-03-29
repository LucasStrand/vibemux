use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::ClientOptions;
use vibemux_ipc::protocol::{Request, Response};

#[derive(Parser)]
#[command(name = "vibemux", about = "VibeMux terminal multiplexer CLI")]
struct Cli {
    #[arg(long, default_value = r"\\.\pipe\vibemux")]
    pipe: String,

    #[arg(long)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Ping,
    ListWorkspaces,
    NewWorkspace {
        #[arg(long)]
        name: Option<String>,
    },
    CloseWorkspace {
        #[arg(long)]
        workspace: String,
    },
    SelectWorkspace {
        #[arg(long)]
        workspace: String,
    },
    Notify {
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: String,
        #[arg(long)]
        subtitle: Option<String>,
    },
    Send {
        text: String,
    },
    SendKey {
        key: String,
    },
    ListSurfaces,
    Capabilities,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let (method, params) = match &cli.command {
        Commands::Ping => ("system.ping", serde_json::json!({})),
        Commands::ListWorkspaces => ("workspace.list", serde_json::json!({})),
        Commands::NewWorkspace { name } => (
            "workspace.create",
            serde_json::json!({ "name": name }),
        ),
        Commands::CloseWorkspace { workspace } => (
            "workspace.close",
            serde_json::json!({ "workspace_id": workspace }),
        ),
        Commands::SelectWorkspace { workspace } => (
            "workspace.select",
            serde_json::json!({ "workspace_id": workspace }),
        ),
        Commands::Notify { title, body, subtitle } => (
            "notification.create",
            serde_json::json!({
                "title": title,
                "body": body,
                "subtitle": subtitle,
            }),
        ),
        Commands::Send { text } => (
            "surface.send_text",
            serde_json::json!({ "text": text }),
        ),
        Commands::SendKey { key } => (
            "surface.send_key",
            serde_json::json!({ "key": key }),
        ),
        Commands::ListSurfaces => ("surface.list", serde_json::json!({})),
        Commands::Capabilities => ("system.capabilities", serde_json::json!({})),
    };

    let request = Request {
        id: "cli-1".to_string(),
        method: method.to_string(),
        params,
    };

    let response = send_request(&cli.pipe, &request).await?;

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else if response.ok {
        if let Some(result) = &response.result {
            println!("{}", serde_json::to_string_pretty(result)?);
        } else {
            println!("OK");
        }
    } else {
        eprintln!("Error: {}", response.error.as_deref().unwrap_or("unknown"));
        std::process::exit(1);
    }

    Ok(())
}

async fn send_request(pipe_name: &str, request: &Request) -> Result<Response> {
    let pipe = ClientOptions::new().open(pipe_name)?;
    let (reader, mut writer) = tokio::io::split(pipe);

    let mut payload = serde_json::to_vec(request)?;
    payload.push(b'\n');
    writer.write_all(&payload).await?;

    let mut lines = BufReader::new(reader).lines();
    if let Some(line) = lines.next_line().await? {
        Ok(serde_json::from_str(&line)?)
    } else {
        anyhow::bail!("No response from server");
    }
}

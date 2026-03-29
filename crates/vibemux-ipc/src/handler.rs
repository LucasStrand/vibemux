use crate::protocol::{Request, Response};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};

pub enum AppCommand {
    ListWorkspaces {
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    CreateWorkspace {
        name: Option<String>,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    CloseWorkspace {
        workspace_id: String,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    SelectWorkspace {
        workspace_id: String,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    CurrentWorkspace {
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    Notify {
        title: String,
        body: String,
        subtitle: Option<String>,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    SendText {
        text: String,
        surface_id: Option<String>,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    SendKey {
        key: String,
        surface_id: Option<String>,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    ListSurfaces {
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    SetStatus {
        key: String,
        value: String,
        icon: Option<String>,
        color: Option<String>,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    ClearStatus {
        key: String,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    SetProgress {
        value: f32,
        label: Option<String>,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    ClearProgress {
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    Log {
        level: String,
        source: Option<String>,
        message: String,
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    ClearLog {
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    Ping {
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
    Capabilities {
        reply: oneshot::Sender<Response>,
        req_id: String,
    },
}

pub fn parse_request(
    request: Request,
    reply: oneshot::Sender<Response>,
) -> Option<AppCommand> {
    let req_id = request.id.clone();
    let params = request.params;

    match request.method.as_str() {
        "system.ping" => Some(AppCommand::Ping { reply, req_id }),
        "system.capabilities" => {
            Some(AppCommand::Capabilities { reply, req_id })
        }
        "workspace.list" => {
            Some(AppCommand::ListWorkspaces { reply, req_id })
        }
        "workspace.create" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(AppCommand::CreateWorkspace {
                name,
                reply,
                req_id,
            })
        }
        "workspace.close" => {
            let workspace_id = params
                .get("workspace_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(AppCommand::CloseWorkspace {
                workspace_id,
                reply,
                req_id,
            })
        }
        "workspace.select" => {
            let workspace_id = params
                .get("workspace_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(AppCommand::SelectWorkspace {
                workspace_id,
                reply,
                req_id,
            })
        }
        "workspace.current" => {
            Some(AppCommand::CurrentWorkspace { reply, req_id })
        }
        "notification.create" => {
            let title = params
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Notification")
                .to_string();
            let body = params
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let subtitle = params
                .get("subtitle")
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(AppCommand::Notify {
                title,
                body,
                subtitle,
                reply,
                req_id,
            })
        }
        "surface.send_text" => {
            let text = params
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let surface_id = params
                .get("surface_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(AppCommand::SendText {
                text,
                surface_id,
                reply,
                req_id,
            })
        }
        "surface.send_key" => {
            let key = params
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let surface_id = params
                .get("surface_id")
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(AppCommand::SendKey {
                key,
                surface_id,
                reply,
                req_id,
            })
        }
        "surface.list" => {
            Some(AppCommand::ListSurfaces { reply, req_id })
        }
        "sidebar.set_status" => {
            let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let value = params.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let icon = params.get("icon").and_then(|v| v.as_str()).map(String::from);
            let color = params.get("color").and_then(|v| v.as_str()).map(String::from);
            Some(AppCommand::SetStatus { key, value, icon, color, reply, req_id })
        }
        "sidebar.clear_status" => {
            let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some(AppCommand::ClearStatus { key, reply, req_id })
        }
        "sidebar.set_progress" => {
            let value = params.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let label = params.get("label").and_then(|v| v.as_str()).map(String::from);
            Some(AppCommand::SetProgress { value, label, reply, req_id })
        }
        "sidebar.clear_progress" => {
            Some(AppCommand::ClearProgress { reply, req_id })
        }
        "sidebar.log" => {
            let level = params.get("level").and_then(|v| v.as_str()).unwrap_or("info").to_string();
            let source = params.get("source").and_then(|v| v.as_str()).map(String::from);
            let message = params.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Some(AppCommand::Log { level, source, message, reply, req_id })
        }
        "sidebar.clear_log" => {
            Some(AppCommand::ClearLog { reply, req_id })
        }
        _ => {
            let _ = reply.send(Response::error(
                req_id,
                format!("Unknown method: {}", request.method),
            ));
            None
        }
    }
}

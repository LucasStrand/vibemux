use crate::command_palette::CommandPalette;
use crate::find_bar::FindBar;
use crate::git_info;
use crate::notifications::NotificationManager;
use crate::pty_stream::PtyReader;
use crate::{sidebar, split_view, theme};
use iced::keyboard;
use iced::widget::{container, row, text};
use iced::{event, Element, Fill, Length, Subscription, Task, Theme};
use std::collections::HashMap;
use uuid::Uuid;
use vibemux_mux::{Pane, PaneId, SplitDirection, SplitTree, WorkspaceManager};
use vibemux_term::Terminal;

pub struct VibeMux {
    workspace_manager: WorkspaceManager,
    terminals: HashMap<PaneId, Terminal>,
    pty_readers: HashMap<PaneId, PtyReader>,
    notification_manager: NotificationManager,
    command_palette: CommandPalette,
    find_bar: FindBar,
    ipc_rx: std::sync::mpsc::Receiver<vibemux_ipc::AppCommand>,
    next_workspace_num: usize,
    show_notification_panel: bool,
    last_session_save: std::time::Instant,
    bytes_received: usize,
}

#[derive(Debug, Clone)]
pub enum Message {
    CreateWorkspace,
    CloseWorkspace(Uuid),
    SelectWorkspace(Uuid),
    NextWorkspace,
    PrevWorkspace,
    SplitRight,
    SplitDown,
    TerminalOutput(PaneId, Vec<u8>),
    KeyboardInput(keyboard::Key, keyboard::Modifiers),
    FocusPane(PaneId),
    FocusNextPane,
    CloseFocusedPane,
    ToggleNotificationPanel,
    ToggleCommandPalette,
    CommandPaletteInput(String),
    CommandPaletteUp,
    CommandPaletteDown,
    CommandPaletteConfirm,
    ToggleFindBar,
    FindBarInput(String),
    SaveSession,
    Tick,
}

impl VibeMux {
    pub fn new() -> (Self, Task<Message>) {
        let mut manager = WorkspaceManager::new();

        let pane = Pane::new();
        let pane_id = pane.id;
        manager.active_mut().split_tree = SplitTree::with_pane(pane_id);

        if let Ok(cwd) = std::env::current_dir() {
            let cwd_str = cwd.to_string_lossy().to_string();
            manager.active_mut().metadata.cwd = Some(cwd_str.clone());
            manager.active_mut().metadata.git_branch =
                git_info::detect_git_branch(&cwd_str);
        }

        eprintln!("[init] spawning terminal...");
        let terminal =
            Terminal::spawn(30, 120, None).expect("Failed to spawn terminal");
        eprintln!("[init] terminal spawned, starting PTY reader...");
        let pty_reader = PtyReader::spawn(terminal.pty.get_reader());
        eprintln!("[init] PTY reader started");

        let mut terminals = HashMap::new();
        terminals.insert(pane_id, terminal);

        let mut pty_readers = HashMap::new();
        pty_readers.insert(pane_id, pty_reader);

        let (ipc_tx, ipc_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let server = vibemux_ipc::IpcServer::new(None);
                let (cmd_tx, mut cmd_rx) =
                    tokio::sync::mpsc::unbounded_channel();

                tokio::spawn(async move {
                    if let Err(e) = server.run(cmd_tx).await {
                        log::error!("IPC server error: {e}");
                    }
                });

                while let Some(ipc_cmd) = cmd_rx.recv().await {
                    match ipc_cmd {
                        vibemux_ipc::server::IpcCommand::Request {
                            request,
                            reply,
                        } => {
                            if let Some(app_cmd) =
                                vibemux_ipc::parse_request(request, reply)
                            {
                                let _ = ipc_tx.send(app_cmd);
                            }
                        }
                    }
                }
            });
        });

        (
            Self {
                workspace_manager: manager,
                terminals,
                pty_readers,
                notification_manager: NotificationManager::new(),
                command_palette: CommandPalette::new(),
                find_bar: FindBar::new(),
                ipc_rx,
                next_workspace_num: 2,
                show_notification_panel: false,
                last_session_save: std::time::Instant::now(),
                bytes_received: 0,
            },
            Task::none(),
        )
    }

    fn spawn_terminal(&mut self, pane_id: PaneId) {
        if let Ok(terminal) = Terminal::spawn(30, 120, None) {
            let reader = PtyReader::spawn(terminal.pty.get_reader());
            self.terminals.insert(pane_id, terminal);
            self.pty_readers.insert(pane_id, reader);
        }
    }

    fn remove_terminal(&mut self, pane_id: PaneId) {
        self.terminals.remove(&pane_id);
        self.pty_readers.remove(&pane_id);
    }

    pub fn title(&self) -> String {
        let ws = self.workspace_manager.active();
        if let Some(ref title) = ws.metadata.title {
            format!("VibeMux - {title}")
        } else {
            format!("VibeMux - {}", ws.name)
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::CatppuccinMocha
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CreateWorkspace => {
                let name = format!("Workspace {}", self.next_workspace_num);
                self.next_workspace_num += 1;
                self.workspace_manager.create_workspace(&name);

                let pane = Pane::new();
                let pane_id = pane.id;
                self.workspace_manager.active_mut().split_tree =
                    SplitTree::with_pane(pane_id);
                self.spawn_terminal(pane_id);
            }
            Message::CloseWorkspace(id) => {
                let pane_ids: Vec<PaneId> = self
                    .workspace_manager
                    .workspaces()
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| w.split_tree.pane_ids())
                    .unwrap_or_default();

                for pid in pane_ids {
                    self.remove_terminal(pid);
                }
                self.workspace_manager.close_workspace(id);
            }
            Message::SelectWorkspace(id) => {
                self.workspace_manager.select_workspace(id);
                self.notification_manager.mark_workspace_read(id);
                if let Some(ws) = self
                    .workspace_manager
                    .workspaces()
                    .iter()
                    .find(|w| w.id == id)
                {
                    // has_unread is updated below in tick
                }
            }
            Message::NextWorkspace => {
                self.workspace_manager.next_workspace();
            }
            Message::PrevWorkspace => {
                self.workspace_manager.prev_workspace();
            }
            Message::SplitRight => {
                let pane = Pane::new();
                let pane_id = pane.id;
                let tree = &mut self.workspace_manager.active_mut().split_tree;
                tree.split(pane_id, vibemux_mux::SplitDirection::Vertical);
                self.spawn_terminal(pane_id);
            }
            Message::SplitDown => {
                let pane = Pane::new();
                let pane_id = pane.id;
                let tree = &mut self.workspace_manager.active_mut().split_tree;
                tree.split(pane_id, vibemux_mux::SplitDirection::Horizontal);
                self.spawn_terminal(pane_id);
            }
            Message::TerminalOutput(pane_id, data) => {
                if let Some(terminal) = self.terminals.get_mut(&pane_id) {
                    terminal.process_output(&data);

                    if let Some(cwd) = terminal.grid.osc_cwd.clone() {
                        self.workspace_manager.active_mut().metadata.cwd =
                            Some(cwd);
                    }
                    if let Some(title) = terminal.grid.title.clone() {
                        self.workspace_manager.active_mut().metadata.title =
                            Some(title);
                    }
                }
            }
            Message::FocusPane(pane_id) => {
                self.workspace_manager.active_mut().split_tree.focused_pane =
                    Some(pane_id);
            }
            Message::FocusNextPane => {
                let tree =
                    &mut self.workspace_manager.active_mut().split_tree;
                let panes = tree.pane_ids();
                if let Some(focused) = tree.focused_pane {
                    if let Some(idx) =
                        panes.iter().position(|&id| id == focused)
                    {
                        let next = (idx + 1) % panes.len();
                        tree.focused_pane = Some(panes[next]);
                    }
                }
            }
            Message::CloseFocusedPane => {
                if let Some(focused) = self.focused_pane_id() {
                    let tree =
                        &mut self.workspace_manager.active_mut().split_tree;
                    if tree.pane_ids().len() > 1 {
                        tree.remove_pane(focused);
                        self.remove_terminal(focused);
                    }
                }
            }
            Message::KeyboardInput(key, modifiers) => {
                if self.command_palette.visible {
                    match &key {
                        keyboard::Key::Named(
                            keyboard::key::Named::Escape,
                        ) => {
                            self.command_palette.close();
                            return Task::none();
                        }
                        keyboard::Key::Named(
                            keyboard::key::Named::Enter,
                        ) => {
                            return self
                                .update(Message::CommandPaletteConfirm);
                        }
                        keyboard::Key::Named(
                            keyboard::key::Named::ArrowUp,
                        ) => {
                            self.command_palette.select_up();
                            return Task::none();
                        }
                        keyboard::Key::Named(
                            keyboard::key::Named::ArrowDown,
                        ) => {
                            self.command_palette.select_down();
                            return Task::none();
                        }
                        _ => return Task::none(),
                    }
                }

                if modifiers.control() && modifiers.shift() {
                    match &key {
                        keyboard::Key::Character(c)
                            if c.as_str() == "n" || c.as_str() == "N" =>
                        {
                            return self.update(Message::CreateWorkspace);
                        }
                        keyboard::Key::Character(c)
                            if c.as_str() == "d" || c.as_str() == "D" =>
                        {
                            return self.update(Message::SplitRight);
                        }
                        keyboard::Key::Character(c)
                            if c.as_str() == "e" || c.as_str() == "E" =>
                        {
                            return self.update(Message::SplitDown);
                        }
                        keyboard::Key::Character(c)
                            if c.as_str() == "w" || c.as_str() == "W" =>
                        {
                            let id = self.workspace_manager.active().id;
                            return self.update(Message::CloseWorkspace(id));
                        }
                        keyboard::Key::Character(c)
                            if c.as_str() == "q" || c.as_str() == "Q" =>
                        {
                            return self
                                .update(Message::CloseFocusedPane);
                        }
                        keyboard::Key::Character(c)
                            if c.as_str() == "i" || c.as_str() == "I" =>
                        {
                            return self
                                .update(Message::ToggleNotificationPanel);
                        }
                        keyboard::Key::Character(c)
                            if c.as_str() == "p" || c.as_str() == "P" =>
                        {
                            return self
                                .update(Message::ToggleCommandPalette);
                        }
                        _ => {}
                    }
                }

                if modifiers.control() && !modifiers.shift() {
                    match &key {
                        keyboard::Key::Named(
                            keyboard::key::Named::Tab,
                        ) => {
                            return self
                                .update(Message::NextWorkspace);
                        }
                        keyboard::Key::Character(c)
                            if c.as_str() == "f" =>
                        {
                            return self
                                .update(Message::ToggleFindBar);
                        }
                        _ => {}
                    }
                }

                if modifiers.alt() && !modifiers.control() {
                    match &key {
                        keyboard::Key::Named(
                            keyboard::key::Named::Tab,
                        ) => {
                            return self
                                .update(Message::FocusNextPane);
                        }
                        _ => {}
                    }
                }

                let bytes = key_to_bytes(&key, &modifiers);
                if let Some(bytes) = bytes {
                    if let Some(focused) = self.focused_pane_id() {
                        if let Some(terminal) =
                            self.terminals.get_mut(&focused)
                        {
                            let _ = terminal.write(&bytes);
                        }
                    }
                }
            }
            Message::ToggleNotificationPanel => {
                self.show_notification_panel = !self.show_notification_panel;
            }
            Message::ToggleFindBar => {
                self.find_bar.toggle();
            }
            Message::FindBarInput(query) => {
                self.find_bar.set_query(query.clone());
                if let Some(focused) = self.focused_pane_id() {
                    if let Some(terminal) = self.terminals.get(&focused) {
                        let matches = crate::find_bar::search_grid(
                            &terminal.grid,
                            &query,
                        );
                        self.find_bar.match_count = matches.len();
                        self.find_bar.current_match = 0;
                    }
                }
            }
            Message::SaveSession => {
                self.save_session();
            }
            Message::ToggleCommandPalette => {
                self.command_palette.toggle();
            }
            Message::CommandPaletteInput(query) => {
                self.command_palette.set_query(query);
            }
            Message::CommandPaletteUp => {
                self.command_palette.select_up();
            }
            Message::CommandPaletteDown => {
                self.command_palette.select_down();
            }
            Message::CommandPaletteConfirm => {
                if let Some(msg) = self.command_palette.confirm() {
                    return self.update(msg);
                }
            }
            Message::Tick => {
                let pane_ids: Vec<PaneId> =
                    self.pty_readers.keys().copied().collect();
                for pane_id in pane_ids {
                    let data =
                        if let Some(reader) = self.pty_readers.get(&pane_id) {
                            let d = reader.drain();
                            if !d.is_empty() {
                                Some(d)
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                    if let Some(data) = data {
                        self.bytes_received += data.len();
                        eprintln!(
                            "[tick] processing {} bytes for pane (total: {}), cursor at ({},{})",
                            data.len(),
                            self.bytes_received,
                            self.terminals.get(&pane_id).map_or(0, |t| t.grid.cursor_row),
                            self.terminals.get(&pane_id).map_or(0, |t| t.grid.cursor_col),
                        );
                        if let Some(terminal) =
                            self.terminals.get_mut(&pane_id)
                        {
                            terminal.process_output(&data);

                            if let Some(cwd) =
                                terminal.grid.osc_cwd.clone()
                            {
                                let ws =
                                    self.workspace_manager.active_mut();
                                let old_cwd = ws.metadata.cwd.clone();
                                ws.metadata.cwd = Some(cwd.clone());

                                if old_cwd.as_deref() != Some(&cwd) {
                                    ws.metadata.git_branch =
                                        git_info::detect_git_branch(&cwd);
                                }
                            }
                            if let Some(title) =
                                terminal.grid.title.clone()
                            {
                                self.workspace_manager
                                    .active_mut()
                                    .metadata
                                    .title = Some(title);
                            }

                            if let Some(notif) =
                                terminal.take_notification()
                            {
                                let ws_id =
                                    self.workspace_manager.active().id;
                                self.notification_manager.add(
                                    ws_id,
                                    notif.title,
                                    notif.body,
                                    notif.subtitle,
                                );
                            }
                        }
                    }
                }

                while let Ok(cmd) = self.ipc_rx.try_recv() {
                    self.handle_ipc_command(cmd);
                }

                let ws_ids: Vec<Uuid> = self
                    .workspace_manager
                    .workspaces()
                    .iter()
                    .map(|w| w.id)
                    .collect();
                for ws_id in ws_ids {
                    let has_unread =
                        self.notification_manager.has_unread(ws_id);
                    if let Some(ws) = self
                        .workspace_manager
                        .workspaces_mut()
                        .iter_mut()
                        .find(|w| w.id == ws_id)
                    {
                        ws.has_unread = has_unread;
                    }
                }

                if self.last_session_save.elapsed()
                    > std::time::Duration::from_secs(30)
                {
                    self.save_session();
                    self.last_session_save = std::time::Instant::now();
                }
            }
        }

        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let sidebar = sidebar::view(&self.workspace_manager);

        let active_ws = self.workspace_manager.active();
        let focused = active_ws.split_tree.focused_pane;

        let content = if let Some(ref root) = active_ws.split_tree.root {
            split_view::render_split_tree(
                root,
                &self.terminals,
                focused,
                self.bytes_received,
            )
        } else {
            empty_pane()
        };

        let divider = container(text(""))
            .width(Length::Fixed(1.0))
            .height(Fill)
            .style(|_t: &Theme| iced::widget::container::Style {
                background: Some(theme::BORDER.into()),
                ..Default::default()
            });

        let content_with_find: Element<'_, Message> = if self.find_bar.visible
        {
            iced::widget::column![content, self.find_bar.view()]
                .width(Fill)
                .height(Fill)
                .into()
        } else {
            content
        };

        let main_layout: Element<'_, Message> =
            row![sidebar, divider, content_with_find]
                .width(Fill)
                .height(Fill)
                .into();

        if self.command_palette.visible {
            let overlay = self.command_palette.view();
            iced::widget::stack![main_layout, overlay]
                .width(Fill)
                .height(Fill)
                .into()
        } else {
            main_layout
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let tick =
            iced::time::every(std::time::Duration::from_millis(8))
                .map(|_| Message::Tick);

        let keys =
            event::listen_with(|event, _status, _id| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    ..
                }) => Some(Message::KeyboardInput(key, modifiers)),
                _ => None,
            });

        Subscription::batch([tick, keys])
    }

    fn focused_pane_id(&self) -> Option<PaneId> {
        self.workspace_manager.active().split_tree.focused_pane
    }

    fn save_session(&self) {
        use crate::session::{
            capture_split_layout, SessionState, WorkspaceState,
        };

        let workspaces: Vec<WorkspaceState> = self
            .workspace_manager
            .workspaces()
            .iter()
            .map(|ws| {
                let layout = if let Some(ref root) = ws.split_tree.root {
                    capture_split_layout(root, &|_pane_id| {
                        ws.metadata.cwd.clone()
                    })
                } else {
                    crate::session::SplitLayoutState::Single {
                        cwd: ws.metadata.cwd.clone(),
                    }
                };
                WorkspaceState {
                    name: ws.name.clone(),
                    cwd: ws.metadata.cwd.clone(),
                    split_layout: layout,
                    pinned: ws.pinned,
                }
            })
            .collect();

        let state = SessionState {
            workspaces,
            active_workspace_index: self.workspace_manager.active_index(),
        };

        if let Err(e) = state.save() {
            log::error!("Failed to save session: {e}");
        }
    }

    fn handle_ipc_command(&mut self, cmd: vibemux_ipc::AppCommand) {
        use serde_json::json;
        use vibemux_ipc::{AppCommand, Response};

        match cmd {
            AppCommand::Ping { reply, req_id } => {
                let _ = reply
                    .send(Response::success(req_id, json!({"pong": true})));
            }
            AppCommand::Capabilities { reply, req_id } => {
                let methods = vec![
                    "system.ping",
                    "system.capabilities",
                    "workspace.list",
                    "workspace.create",
                    "workspace.close",
                    "workspace.select",
                    "workspace.current",
                    "notification.create",
                    "surface.send_text",
                    "surface.send_key",
                    "surface.list",
                ];
                let _ = reply.send(Response::success(
                    req_id,
                    json!({"methods": methods}),
                ));
            }
            AppCommand::ListWorkspaces { reply, req_id } => {
                let workspaces: Vec<serde_json::Value> = self
                    .workspace_manager
                    .workspaces()
                    .iter()
                    .map(|ws| {
                        json!({
                            "id": ws.id.to_string(),
                            "name": ws.name,
                            "cwd": ws.metadata.cwd,
                            "git_branch": ws.metadata.git_branch,
                        })
                    })
                    .collect();
                let _ = reply.send(Response::success(
                    req_id,
                    json!({"workspaces": workspaces}),
                ));
            }
            AppCommand::CreateWorkspace {
                name,
                reply,
                req_id,
            } => {
                let ws_name = name.unwrap_or_else(|| {
                    let n = format!(
                        "Workspace {}",
                        self.next_workspace_num
                    );
                    self.next_workspace_num += 1;
                    n
                });
                let ws_id = self
                    .workspace_manager
                    .create_workspace(&ws_name);

                let pane = Pane::new();
                let pane_id = pane.id;
                self.workspace_manager.active_mut().split_tree =
                    SplitTree::with_pane(pane_id);
                self.spawn_terminal(pane_id);

                let _ = reply.send(Response::success(
                    req_id,
                    json!({"workspace_id": ws_id.to_string()}),
                ));
            }
            AppCommand::CloseWorkspace {
                workspace_id,
                reply,
                req_id,
            } => {
                if let Ok(id) = workspace_id.parse::<Uuid>() {
                    let pane_ids: Vec<PaneId> = self
                        .workspace_manager
                        .workspaces()
                        .iter()
                        .find(|w| w.id == id)
                        .map(|w| w.split_tree.pane_ids())
                        .unwrap_or_default();
                    for pid in pane_ids {
                        self.remove_terminal(pid);
                    }
                    let ok =
                        self.workspace_manager.close_workspace(id);
                    let _ = reply.send(Response::success(
                        req_id,
                        json!({"closed": ok}),
                    ));
                } else {
                    let _ = reply.send(Response::error(
                        req_id,
                        "Invalid workspace_id",
                    ));
                }
            }
            AppCommand::SelectWorkspace {
                workspace_id,
                reply,
                req_id,
            } => {
                if let Ok(id) = workspace_id.parse::<Uuid>() {
                    let ok =
                        self.workspace_manager.select_workspace(id);
                    let _ = reply.send(Response::success(
                        req_id,
                        json!({"selected": ok}),
                    ));
                } else {
                    let _ = reply.send(Response::error(
                        req_id,
                        "Invalid workspace_id",
                    ));
                }
            }
            AppCommand::CurrentWorkspace { reply, req_id } => {
                let ws = self.workspace_manager.active();
                let _ = reply.send(Response::success(
                    req_id,
                    json!({
                        "id": ws.id.to_string(),
                        "name": ws.name,
                        "cwd": ws.metadata.cwd,
                        "git_branch": ws.metadata.git_branch,
                    }),
                ));
            }
            AppCommand::Notify {
                title,
                body,
                subtitle,
                reply,
                req_id,
            } => {
                let ws_id = self.workspace_manager.active().id;
                let notif_id = self.notification_manager.add(
                    ws_id,
                    title,
                    body,
                    subtitle,
                );
                let _ = reply.send(Response::success(
                    req_id,
                    json!({"notification_id": notif_id.to_string()}),
                ));
            }
            AppCommand::SendText {
                text,
                surface_id: _,
                reply,
                req_id,
            } => {
                if let Some(focused) = self.focused_pane_id() {
                    if let Some(terminal) =
                        self.terminals.get_mut(&focused)
                    {
                        let _ = terminal.write(text.as_bytes());
                        let _ = reply.send(Response::success(
                            req_id,
                            json!({"sent": true}),
                        ));
                    } else {
                        let _ = reply.send(Response::error(
                            req_id,
                            "No active terminal",
                        ));
                    }
                } else {
                    let _ = reply.send(Response::error(
                        req_id,
                        "No focused pane",
                    ));
                }
            }
            AppCommand::SendKey {
                key,
                surface_id: _,
                reply,
                req_id,
            } => {
                let bytes = match key.as_str() {
                    "enter" => Some(b"\r".to_vec()),
                    "tab" => Some(b"\t".to_vec()),
                    "escape" => Some(b"\x1b".to_vec()),
                    "backspace" => Some(b"\x7f".to_vec()),
                    "up" => Some(b"\x1b[A".to_vec()),
                    "down" => Some(b"\x1b[B".to_vec()),
                    "right" => Some(b"\x1b[C".to_vec()),
                    "left" => Some(b"\x1b[D".to_vec()),
                    "delete" => Some(b"\x1b[3~".to_vec()),
                    _ => None,
                };
                if let Some(bytes) = bytes {
                    if let Some(focused) = self.focused_pane_id() {
                        if let Some(terminal) =
                            self.terminals.get_mut(&focused)
                        {
                            let _ = terminal.write(&bytes);
                            let _ = reply.send(Response::success(
                                req_id,
                                json!({"sent": true}),
                            ));
                        } else {
                            let _ = reply.send(Response::error(
                                req_id,
                                "No active terminal",
                            ));
                        }
                    } else {
                        let _ = reply.send(Response::error(
                            req_id,
                            "No focused pane",
                        ));
                    }
                } else {
                    let _ = reply.send(Response::error(
                        req_id,
                        format!("Unknown key: {key}"),
                    ));
                }
            }
            AppCommand::SetStatus {
                key,
                value,
                icon,
                color,
                reply,
                req_id,
            } => {
                let ws = self.workspace_manager.active_mut();
                ws.metadata.status_entries.retain(|e| e.key != key);
                ws.metadata.status_entries.push(
                    vibemux_mux::workspace::StatusEntry {
                        key,
                        value,
                        icon,
                        color,
                    },
                );
                let _ = reply
                    .send(Response::success(req_id, json!({"ok": true})));
            }
            AppCommand::ClearStatus { key, reply, req_id } => {
                let ws = self.workspace_manager.active_mut();
                ws.metadata.status_entries.retain(|e| e.key != key);
                let _ = reply
                    .send(Response::success(req_id, json!({"ok": true})));
            }
            AppCommand::SetProgress {
                value,
                label,
                reply,
                req_id,
            } => {
                let ws = self.workspace_manager.active_mut();
                ws.metadata.progress =
                    Some(vibemux_mux::workspace::ProgressState {
                        value,
                        label,
                    });
                let _ = reply
                    .send(Response::success(req_id, json!({"ok": true})));
            }
            AppCommand::ClearProgress { reply, req_id } => {
                let ws = self.workspace_manager.active_mut();
                ws.metadata.progress = None;
                let _ = reply
                    .send(Response::success(req_id, json!({"ok": true})));
            }
            AppCommand::Log {
                level,
                source,
                message,
                reply,
                req_id,
            } => {
                let ws = self.workspace_manager.active_mut();
                ws.metadata.log_entries.push(
                    vibemux_mux::workspace::LogEntry {
                        level,
                        source,
                        message,
                    },
                );
                if ws.metadata.log_entries.len() > 100 {
                    ws.metadata.log_entries.remove(0);
                }
                let _ = reply
                    .send(Response::success(req_id, json!({"ok": true})));
            }
            AppCommand::ClearLog { reply, req_id } => {
                let ws = self.workspace_manager.active_mut();
                ws.metadata.log_entries.clear();
                let _ = reply
                    .send(Response::success(req_id, json!({"ok": true})));
            }
            AppCommand::ListSurfaces { reply, req_id } => {
                let ws = self.workspace_manager.active();
                let surfaces: Vec<serde_json::Value> = ws
                    .split_tree
                    .pane_ids()
                    .iter()
                    .map(|pid| {
                        json!({
                            "id": pid.to_string(),
                            "focused": ws.split_tree.focused_pane == Some(*pid),
                        })
                    })
                    .collect();
                let _ = reply.send(Response::success(
                    req_id,
                    json!({"surfaces": surfaces}),
                ));
            }
        }
    }
}

fn empty_pane<'a>() -> Element<'a, Message> {
    container(text("No terminal").size(16).color(theme::FG_DIM))
        .width(Fill)
        .height(Fill)
        .center(Fill)
        .style(|_t: &Theme| iced::widget::container::Style {
            background: Some(theme::BG_PRIMARY.into()),
            ..Default::default()
        })
        .into()
}

fn key_to_bytes(
    key: &keyboard::Key,
    modifiers: &keyboard::Modifiers,
) -> Option<Vec<u8>> {
    use keyboard::key::Named;

    if modifiers.control() && !modifiers.shift() {
        if let keyboard::Key::Character(c) = key {
            let ch = c.chars().next()?;
            if ch.is_ascii_alphabetic() {
                let ctrl = (ch.to_ascii_lowercase() as u8) - b'a' + 1;
                return Some(vec![ctrl]);
            }
        }
        return None;
    }

    if modifiers.control() || modifiers.alt() {
        return None;
    }

    match key {
        keyboard::Key::Named(named) => match named {
            Named::Enter => Some(b"\r".to_vec()),
            Named::Backspace => Some(b"\x7f".to_vec()),
            Named::Tab => Some(b"\t".to_vec()),
            Named::Escape => Some(b"\x1b".to_vec()),
            Named::ArrowUp => Some(b"\x1b[A".to_vec()),
            Named::ArrowDown => Some(b"\x1b[B".to_vec()),
            Named::ArrowRight => Some(b"\x1b[C".to_vec()),
            Named::ArrowLeft => Some(b"\x1b[D".to_vec()),
            Named::Home => Some(b"\x1b[H".to_vec()),
            Named::End => Some(b"\x1b[F".to_vec()),
            Named::PageUp => Some(b"\x1b[5~".to_vec()),
            Named::PageDown => Some(b"\x1b[6~".to_vec()),
            Named::Delete => Some(b"\x1b[3~".to_vec()),
            Named::Insert => Some(b"\x1b[2~".to_vec()),
            Named::F1 => Some(b"\x1bOP".to_vec()),
            Named::F2 => Some(b"\x1bOQ".to_vec()),
            Named::F3 => Some(b"\x1bOR".to_vec()),
            Named::F4 => Some(b"\x1bOS".to_vec()),
            Named::F5 => Some(b"\x1b[15~".to_vec()),
            Named::F6 => Some(b"\x1b[17~".to_vec()),
            Named::F7 => Some(b"\x1b[18~".to_vec()),
            Named::F8 => Some(b"\x1b[19~".to_vec()),
            Named::F9 => Some(b"\x1b[20~".to_vec()),
            Named::F10 => Some(b"\x1b[21~".to_vec()),
            Named::F11 => Some(b"\x1b[23~".to_vec()),
            Named::F12 => Some(b"\x1b[24~".to_vec()),
            Named::Space => Some(b" ".to_vec()),
            _ => None,
        },
        keyboard::Key::Character(c) => Some(c.as_bytes().to_vec()),
        _ => None,
    }
}

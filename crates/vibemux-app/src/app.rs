use crate::command_palette::CommandPalette;
use crate::find_bar::FindBar;
use crate::git_info;
use crate::notifications::NotificationManager;
use crate::pty_stream::PtyReader;
use crate::term_selection::{
    clamp_cell_for_input_line, clamp_selection_to_input, delete_selection_via_pty,
    input_start_column, logical_line_end_col, move_cell as sel_move_cell, point_to_cell,
    selection_text, TerminalSelection,
};
use crate::term_selection::{term_char_width, term_line_height};
use crate::{notification_panel, resize_layout, sidebar, split_view, tab_bar, theme};
use iced::clipboard;
use iced::keyboard;
use iced::mouse;
use iced::widget::operation;
use iced::widget::{column, container, row, text};
use iced::{event, Element, Fill, Font, Length, Point, Size, Subscription, Task, Theme};
use iced::window;
use std::collections::HashMap;
use uuid::Uuid;
use vibemux_mux::{Pane, PaneId, SplitDirection, SplitTree, WorkspaceManager, WorkspaceTab};
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
    /// Last reported window size; used to resize PTYs when the layout changes.
    last_window_size: Option<Size>,
    last_session_save: std::time::Instant,
    bytes_received: usize,
    /// When true, new PTY output snaps the terminal scroll view to the bottom.
    terminal_stick_to_bottom: HashMap<PaneId, bool>,
    terminal_selection: HashMap<PaneId, Option<TerminalSelection>>,
    /// Last pointer position inside each pane's terminal (local coords).
    terminal_pointer_local: HashMap<PaneId, Point>,
    /// Pane where the user pressed the mouse for drag-select (`None` after release).
    selection_drag_pane: Option<PaneId>,
    /// Anchor cell for an in-progress selection (shown only after pointer moves).
    selection_pending_anchor: Option<(PaneId, (usize, usize))>,
    selection_drag_moved: bool,
    /// Loaded config.
    config: vibemux_config::Config,
    /// Resolved terminal font.
    term_font: Font,
    term_font_size: f32,
    /// Split divider drag state.
    split_drag_active: Option<Uuid>,
}

// Variants are handled in `update`; some are reserved for subscriptions / shortcuts not wired yet.
#[allow(dead_code)]
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
    FindBarNext,
    FindBarPrev,
    SaveSession,
    Tick,
    NewTab,
    CloseTab(Uuid),
    CloseActiveTab,
    SelectTab(Uuid),
    NextTab,
    PrevTab,
    TerminalViewportChanged(PaneId, bool),
    TerminalMouseMove(PaneId, Point),
    TerminalMouseDown(PaneId),
    /// Left button released anywhere (finish drag / click selection).
    TerminalMouseUpAnywhere,
    CopyTerminalSelection,
    /// Read clipboard and paste into the focused PTY (command palette).
    RequestClipboardPaste,
    /// Select editable text on the current shell line (command palette).
    SelectAllTerminalInput,
    /// Cut non-collapsed terminal selection (command palette).
    CutTerminalSelection,
    /// Clipboard paste into the focused PTY (Ctrl+Shift+V / Shift+Insert).
    TerminalPaste(Option<String>),
    /// Shift+arrows / Shift+PgUp/PgDn: extend selection (`delta` in display cells).
    TerminalExtendSelection { delta_row: i32, delta_col: i32 },
    TerminalExtendSelectionLineStart,
    TerminalExtendSelectionLineEnd,
    WindowResized(Size),
    /// Split divider drag events.
    SplitDragStart(Uuid, SplitDirection),
    SplitDragMove(Uuid, SplitDirection, Point),
    SplitDragEnd,
}

impl VibeMux {
    pub fn new() -> (Self, Task<Message>) {
        let config = vibemux_config::Config::load().unwrap_or_default();
        let scrollback_limit = config.terminal.scrollback_limit;

        // Try to load a custom font by family name. Iced will use MONOSPACE as
        // fallback if the font is not found at runtime.
        let term_font = Font::MONOSPACE;
        let term_font_size = config.font.size;

        let mut manager = WorkspaceManager::new();

        // Try to restore previous session.
        let restored = crate::session::SessionState::load().ok();

        let mut terminals = HashMap::new();
        let mut pty_readers = HashMap::new();
        let mut stick = HashMap::new();

        // Pre-calculate terminal dimensions from the known initial window size
        // so that PTYs start with correct dimensions from the very first spawn.
        let initial_window = Size::new(1200.0, 800.0);
        let (init_rows, init_cols) =
            Self::default_term_size(initial_window, term_font_size);

        if let Some(session) = &restored {
            // Rebuild workspaces from session state.
            // First, remove the default workspace that WorkspaceManager::new() created.
            // We'll build fresh ones from the session.
            // We can't close the last workspace, so we create session ones first.
            let mut first = true;
            for ws_state in &session.workspaces {
                if first {
                    // Reuse the existing workspace.
                    manager.active_mut().name = ws_state.name.clone();
                    manager.active_mut().pinned = ws_state.pinned;
                    // Clear default tab.
                    manager.active_mut().tabs.clear();
                    first = false;
                } else {
                    manager.create_workspace(&ws_state.name);
                    manager.active_mut().pinned = ws_state.pinned;
                    manager.active_mut().tabs.clear();
                }

                for tab_state in &ws_state.tabs {
                    let (root_node, pane_list) =
                        crate::session::restore_split_layout(&tab_state.split_layout);
                    let first_pane = pane_list.first().map(|(id, _)| *id);
                    let mut tab = WorkspaceTab::new();
                    tab.split_tree = SplitTree {
                        root: Some(root_node),
                        focused_pane: first_pane,
                    };
                    tab.cwd = tab_state.cwd.clone();
                    if let Some(ref cwd) = tab_state.cwd {
                        tab.git_branch = git_info::detect_git_branch(cwd);
                    }
                    manager.active_mut().tabs.push(tab);

                    for (pane_id, cwd) in &pane_list {
                        if let Ok(term) = Terminal::spawn_with_scrollback(
                            init_rows,
                            init_cols,
                            config.terminal.shell.as_deref(),
                            scrollback_limit,
                        ) {
                            let reader = PtyReader::spawn(term.pty.get_reader());
                            terminals.insert(*pane_id, term);
                            pty_readers.insert(*pane_id, reader);
                            stick.insert(*pane_id, true);
                            // If there's a CWD, send a `cd` command.
                            if let Some(cwd) = cwd {
                                if let Some(t) = terminals.get_mut(pane_id) {
                                    let cd_cmd = format!("cd \"{}\"\r", cwd.replace('"', "\\\""));
                                    let _ = t.write(cd_cmd.as_bytes());
                                }
                            }
                        }
                    }
                }

                manager.active_mut().active_tab_index =
                    ws_state.active_tab_index.min(manager.active().tabs.len().saturating_sub(1));
            }

            if session.active_workspace_index < manager.workspaces().len() {
                let target_id = manager.workspaces()[session.active_workspace_index].id;
                manager.select_workspace(target_id);
            }
        }

        // If no session was restored (or it was empty), set up defaults.
        if terminals.is_empty() {
            let pane = Pane::new();
            let pane_id = pane.id;
            {
                let tab = manager.active_mut().active_tab_mut();
                tab.split_tree = SplitTree::with_pane(pane_id);
                if let Ok(cwd) = std::env::current_dir() {
                    let cwd_str = cwd.to_string_lossy().to_string();
                    tab.cwd = Some(cwd_str.clone());
                    tab.git_branch = git_info::detect_git_branch(&cwd_str);
                }
            }

            let terminal = Terminal::spawn_with_scrollback(
                init_rows,
                init_cols,
                config.terminal.shell.as_deref(),
                scrollback_limit,
            )
            .expect("Failed to spawn terminal");
            let pty_reader = PtyReader::spawn(terminal.pty.get_reader());

            terminals.insert(pane_id, terminal);
            pty_readers.insert(pane_id, pty_reader);
            stick.insert(pane_id, true);
        }

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

        let mut app = Self {
            workspace_manager: manager,
            terminals,
            pty_readers,
            notification_manager: NotificationManager::new(),
            command_palette: CommandPalette::new(),
            find_bar: FindBar::new(),
            ipc_rx,
            next_workspace_num: 2,
            show_notification_panel: false,
            last_window_size: Some(initial_window),
            last_session_save: std::time::Instant::now(),
            bytes_received: 0,
            terminal_stick_to_bottom: stick,
            terminal_selection: HashMap::new(),
            terminal_pointer_local: HashMap::new(),
            selection_drag_pane: None,
            selection_pending_anchor: None,
            selection_drag_moved: false,
            config,
            term_font,
            term_font_size,
            split_drag_active: None,
        };
        // Resize terminals to match the initial window size immediately so the
        // PTY dimensions are correct before any output arrives.
        app.resize_terminals_from_window();
        let tabs_snap = app.snap_shell_tabs_task();
        // Query the actual window size so we can resize terminals to match.
        // resize_events() only fires on *changes*, so we need this initial query.
        let get_size = window::latest()
            .and_then(|id| window::size(id))
            .map(Message::WindowResized);
        (app, Task::batch([tabs_snap, get_size]))
    }

    #[inline]
    fn term_metrics(&self) -> (f32, f32) {
        (
            term_char_width(self.term_font_size),
            term_line_height(self.term_font_size),
        )
    }

    fn sync_notification_badges(&mut self) {
        let ws_ids: Vec<Uuid> = self
            .workspace_manager
            .workspaces()
            .iter()
            .map(|w| w.id)
            .collect();
        for ws_id in ws_ids {
            let has_unread = self.notification_manager.has_unread(ws_id);
            if let Some(ws) = self
                .workspace_manager
                .workspaces_mut()
                .iter_mut()
                .find(|w| w.id == ws_id)
            {
                ws.has_unread = has_unread;
            }
        }
    }

    /// Calculate default terminal dimensions (rows, cols) from a window size.
    /// Used to give PTYs the correct size from the very first spawn.
    fn default_term_size(window_size: Size, font_size: f32) -> (u16, u16) {
        const SIDEBAR_W: f32 = 220.0;
        const MAIN_DIVIDER: f32 = 1.0;
        const TAB_BAR_H: f32 = 44.0;
        const TERM_SCROLL_PAD: f32 = 8.0;
        const STATUS_BAR_H: f32 = 24.0;

        let cw = term_char_width(font_size);
        let lh = term_line_height(font_size);
        let content_w = window_size.width - SIDEBAR_W - MAIN_DIVIDER;
        let content_h = window_size.height - TAB_BAR_H;
        let scroll_h = (content_h - STATUS_BAR_H).max(lh);
        let cols = ((content_w - TERM_SCROLL_PAD) / cw)
            .floor()
            .clamp(1.0, 512.0) as u16;
        let rows = ((scroll_h - TERM_SCROLL_PAD) / lh)
            .floor()
            .clamp(1.0, 256.0) as u16;
        (rows, cols)
    }

    fn resize_terminals_from_window(&mut self) {
        const SIDEBAR_W: f32 = 220.0;
        const MAIN_DIVIDER: f32 = 1.0;
        const TAB_BAR_H: f32 = 44.0;
        const FIND_BAR_H: f32 = 48.0;
        const NOTIF_PANEL_W: f32 = 280.0;
        const NOTIF_DIVIDER: f32 = 1.0;
        const TERM_SCROLL_PAD: f32 = 8.0;
        const STATUS_BAR_H: f32 = 24.0;

        let Some(size) = self.last_window_size else {
            return;
        };

        let mut content_w = size.width - SIDEBAR_W - MAIN_DIVIDER;
        if self.show_notification_panel {
            content_w -= NOTIF_PANEL_W + NOTIF_DIVIDER;
        }
        let mut content_h = size.height - TAB_BAR_H;
        if self.find_bar.visible {
            content_h -= FIND_BAR_H;
        }
        if content_w < 80.0 || content_h < 80.0 {
            return;
        }

        let mut work: Vec<(PaneId, f32, f32)> = Vec::new();
        for ws in self.workspace_manager.workspaces() {
            for tab in &ws.tabs {
                if let Some(root) = &tab.split_tree.root {
                    work.extend(resize_layout::pane_content_sizes(
                        root,
                        content_w,
                        content_h,
                    ));
                }
            }
        }

        let cw = term_char_width(self.term_font_size);
        let lh = term_line_height(self.term_font_size);
        for (pane_id, pw, ph) in work {
            let scroll_h = (ph - STATUS_BAR_H).max(lh);
            let cols_f = ((pw - TERM_SCROLL_PAD) / cw).floor();
            let rows_f = ((scroll_h - TERM_SCROLL_PAD) / lh).floor();
            let cols = cols_f.clamp(1.0, 512.0) as u16;
            let rows = rows_f.clamp(1.0, 256.0) as u16;

            if let Some(term) = self.terminals.get_mut(&pane_id) {
                if term.grid.cols != cols as usize || term.grid.rows != rows as usize {
                    match term.resize(rows, cols) {
                        Ok(()) => {
                            if let Some(reader) = self.pty_readers.get(&pane_id) {
                                reader.discard_queue();
                            }
                        }
                        Err(e) => {
                            log::warn!("terminal resize failed for pane {pane_id:?}: {e}");
                        }
                    }
                }
            }
        }
    }

    fn spawn_terminal(&mut self, pane_id: PaneId) {
        let (rows, cols) = self
            .last_window_size
            .map(|s| Self::default_term_size(s, self.term_font_size))
            .unwrap_or((40, 120));
        if let Ok(terminal) = Terminal::spawn_with_scrollback(
            rows,
            cols,
            self.config.terminal.shell.as_deref(),
            self.config.terminal.scrollback_limit,
        ) {
            let reader = PtyReader::spawn(terminal.pty.get_reader());
            self.terminals.insert(pane_id, terminal);
            self.pty_readers.insert(pane_id, reader);
            self.terminal_stick_to_bottom.insert(pane_id, true);
        }
    }

    fn remove_terminal(&mut self, pane_id: PaneId) {
        // Signal the reader thread to stop.
        if let Some(reader) = self.pty_readers.get(&pane_id) {
            reader.shutdown();
        }
        self.terminals.remove(&pane_id);
        self.pty_readers.remove(&pane_id);
        self.terminal_stick_to_bottom.remove(&pane_id);
        self.terminal_selection.remove(&pane_id);
        self.terminal_pointer_local.remove(&pane_id);
        if self.selection_drag_pane == Some(pane_id) {
            self.selection_drag_pane = None;
        }
        if self
            .selection_pending_anchor
            .map(|(p, _)| p == pane_id)
            .unwrap_or(false)
        {
            self.selection_pending_anchor = None;
        }
    }

    fn snap_shell_tabs_task(&self) -> Task<Message> {
        tab_bar::snap_active_tab_scroll_task(self.workspace_manager.active())
    }

    pub fn title(&self) -> String {
        let ws = self.workspace_manager.active();
        let tab = ws.active_tab();
        if let Some(ref title) = tab.title {
            format!("VibeMux - {title}")
        } else {
            format!("VibeMux - {}", ws.name)
        }
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowResized(size) => {
                self.last_window_size = Some(size);
                self.resize_terminals_from_window();
            }
            Message::CreateWorkspace => {
                let name = format!("Workspace {}", self.next_workspace_num);
                self.next_workspace_num += 1;
                self.workspace_manager.create_workspace(&name);

                let pane = Pane::new();
                let pane_id = pane.id;
                self.workspace_manager
                    .active_mut()
                    .active_tab_mut()
                    .split_tree = SplitTree::with_pane(pane_id);
                self.spawn_terminal(pane_id);
                self.resize_terminals_from_window();
                return self.snap_shell_tabs_task();
            }
            Message::CloseWorkspace(id) => {
                let pane_ids: Vec<PaneId> = self
                    .workspace_manager
                    .workspaces()
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| w.all_pane_ids())
                    .unwrap_or_default();

                for pid in pane_ids {
                    self.remove_terminal(pid);
                }
                self.workspace_manager.close_workspace(id);
                self.resize_terminals_from_window();
                return self.snap_shell_tabs_task();
            }
            Message::SelectWorkspace(id) => {
                self.workspace_manager.select_workspace(id);
                self.notification_manager.mark_workspace_read(id);
                return self.snap_shell_tabs_task();
            }
            Message::NextWorkspace => {
                self.workspace_manager.next_workspace();
                return self.snap_shell_tabs_task();
            }
            Message::PrevWorkspace => {
                self.workspace_manager.prev_workspace();
                return self.snap_shell_tabs_task();
            }
            Message::SplitRight => {
                let pane = Pane::new();
                let pane_id = pane.id;
                let tree = self.workspace_manager.active_mut().split_tree_mut();
                tree.split(pane_id, vibemux_mux::SplitDirection::Vertical);
                self.spawn_terminal(pane_id);
                self.resize_terminals_from_window();
            }
            Message::SplitDown => {
                let pane = Pane::new();
                let pane_id = pane.id;
                let tree = self.workspace_manager.active_mut().split_tree_mut();
                tree.split(pane_id, vibemux_mux::SplitDirection::Horizontal);
                self.spawn_terminal(pane_id);
                self.resize_terminals_from_window();
            }
            Message::TerminalOutput(pane_id, data) => {
                if let Some(terminal) = self.terminals.get_mut(&pane_id) {
                    terminal.process_output(&data);

                    if let Some((wi, ti)) =
                        self.workspace_manager.locate_pane(pane_id)
                    {
                        let ws = &mut self.workspace_manager.workspaces_mut()[wi];
                        let tab = &mut ws.tabs[ti];
                        if let Some(cwd) = terminal.grid.osc_cwd.clone() {
                            let old = tab.cwd.clone();
                            tab.cwd = Some(cwd.clone());
                            if old.as_deref() != Some(&cwd) {
                                tab.git_branch = git_info::detect_git_branch(&cwd);
                            }
                        }
                        if let Some(title) = terminal.grid.title.clone() {
                            tab.title = Some(title);
                        }
                    }
                }
            }
            Message::FocusPane(pane_id) => {
                self.workspace_manager.active_mut().split_tree_mut().focused_pane =
                    Some(pane_id);
            }
            Message::FocusNextPane => {
                let tree = self.workspace_manager.active_mut().split_tree_mut();
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
                    let tree = self.workspace_manager.active_mut().split_tree_mut();
                    if tree.pane_ids().len() > 1 {
                        tree.remove_pane(focused);
                        self.remove_terminal(focused);
                        self.resize_terminals_from_window();
                    }
                }
            }
            Message::NewTab => {
                let pane = Pane::new();
                let pane_id = pane.id;
                let mut tab = WorkspaceTab::new();
                tab.split_tree = SplitTree::with_pane(pane_id);
                let ws = self.workspace_manager.active_mut();
                ws.tabs.push(tab);
                ws.active_tab_index = ws.tabs.len() - 1;
                self.spawn_terminal(pane_id);
                self.resize_terminals_from_window();
                return self.snap_shell_tabs_task();
            }
            Message::CloseTab(tab_id) => {
                let pane_ids = {
                    let ws = self.workspace_manager.active_mut();
                    if ws.tabs.len() <= 1 {
                        return Task::none();
                    }
                    let Some(ti) = ws.tabs.iter().position(|t| t.id == tab_id) else {
                        return Task::none();
                    };
                    let pane_ids = ws.tabs[ti].split_tree.pane_ids();
                    let old_active = ws.active_tab_index;
                    ws.tabs.remove(ti);
                    let new_len = ws.tabs.len();
                    if old_active > ti {
                        ws.active_tab_index = old_active - 1;
                    } else if old_active == ti {
                        ws.active_tab_index =
                            old_active.min(new_len.saturating_sub(1));
                    }
                    pane_ids
                };
                for pid in pane_ids {
                    self.remove_terminal(pid);
                }
                self.resize_terminals_from_window();
                return self.snap_shell_tabs_task();
            }
            Message::CloseActiveTab => {
                let tid = self.workspace_manager.active().active_tab().id;
                return self.update(Message::CloseTab(tid));
            }
            Message::SelectTab(tab_id) => {
                let ws = self.workspace_manager.active_mut();
                if let Some(ti) = ws.tabs.iter().position(|t| t.id == tab_id) {
                    ws.active_tab_index = ti;
                }
                return self.snap_shell_tabs_task();
            }
            Message::NextTab => {
                let ws = self.workspace_manager.active_mut();
                if !ws.tabs.is_empty() {
                    ws.active_tab_index = (ws.active_tab_index + 1) % ws.tabs.len();
                }
                return self.snap_shell_tabs_task();
            }
            Message::PrevTab => {
                let ws = self.workspace_manager.active_mut();
                if !ws.tabs.is_empty() {
                    ws.active_tab_index = if ws.active_tab_index == 0 {
                        ws.tabs.len() - 1
                    } else {
                        ws.active_tab_index - 1
                    };
                }
                return self.snap_shell_tabs_task();
            }
            Message::TerminalViewportChanged(pane_id, stick_to_bottom) => {
                self.terminal_stick_to_bottom
                    .insert(pane_id, stick_to_bottom);
            }
            Message::TerminalMouseMove(pane_id, pt) => {
                let (cw, lh) = self.term_metrics();
                self.terminal_pointer_local.insert(pane_id, pt);

                // Forward mouse move to terminal app if it wants tracking.
                if self.selection_drag_pane.is_none() {
                    if let Some(terminal) = self.terminals.get_mut(&pane_id) {
                        if terminal.grid.mouse_tracking == vibemux_term::MouseTracking::AnyEvent {
                            let grid = &terminal.grid;
                            let (r, c) =
                                point_to_cell(pt.x, pt.y, grid.cols, grid.rows, cw, lh);
                            terminal.send_mouse_event(vibemux_term::MouseEvent {
                                kind: vibemux_term::MouseEventKind::Move,
                                button: vibemux_term::MouseButton::Left,
                                col: c as u16,
                                row: r as u16,
                            });
                        }
                    }
                }

                if self.selection_drag_pane == Some(pane_id) {
                    if let Some(terminal) = self.terminals.get(&pane_id) {
                        let grid = &terminal.grid;
                        let n = grid.display_line_count();
                        let cols = grid.cols;
                        let (r, c) = point_to_cell(pt.x, pt.y, cols, n, cw, lh);
                        let cell = clamp_cell_for_input_line(grid, r, c);
                        if let Some((p, anchor)) = self.selection_pending_anchor {
                            if p == pane_id {
                                if cell != anchor {
                                    self.selection_drag_moved = true;
                                }
                                let mut sel = TerminalSelection {
                                    anchor,
                                    head: cell,
                                };
                                clamp_selection_to_input(grid, &mut sel);
                                self.terminal_selection.insert(pane_id, Some(sel));
                            }
                        }
                    }
                }
            }
            Message::TerminalMouseDown(pane_id) => {
                let (cw, lh) = self.term_metrics();
                // Check if the terminal app wants mouse events.
                if let Some(terminal) = self.terminals.get_mut(&pane_id) {
                    if terminal.grid.mouse_tracking != vibemux_term::MouseTracking::Off {
                        let p = self
                            .terminal_pointer_local
                            .get(&pane_id)
                            .copied()
                            .unwrap_or_else(|| Point::new(5.0, 5.0));
                        let grid = &terminal.grid;
                        let (r, c) = point_to_cell(p.x, p.y, grid.cols, grid.rows, cw, lh);
                        let consumed = terminal.send_mouse_event(vibemux_term::MouseEvent {
                            kind: vibemux_term::MouseEventKind::Press,
                            button: vibemux_term::MouseButton::Left,
                            col: c as u16,
                            row: r as u16,
                        });
                        if consumed {
                            // Focus the pane but don't start selection.
                            self.workspace_manager
                                .active_mut()
                                .split_tree_mut()
                                .focused_pane = Some(pane_id);
                            return Task::none();
                        }
                    }
                }

                self.workspace_manager
                    .active_mut()
                    .split_tree_mut()
                    .focused_pane = Some(pane_id);
                self.selection_drag_pane = Some(pane_id);
                self.selection_drag_moved = false;
                for pid in self.terminal_selection.keys().copied().collect::<Vec<_>>() {
                    if pid != pane_id {
                        self.terminal_selection.insert(pid, None);
                    }
                }
                let Some(terminal) = self.terminals.get(&pane_id) else {
                    return Task::none();
                };
                let grid = &terminal.grid;
                let n = grid.display_line_count();
                let cols = grid.cols;
                let p = self
                    .terminal_pointer_local
                    .get(&pane_id)
                    .copied()
                    .unwrap_or_else(|| Point::new(5.0, 5.0));
                let (r, c) = point_to_cell(p.x, p.y, cols, n, cw, lh);
                let cell = clamp_cell_for_input_line(grid, r, c);
                self.selection_pending_anchor = Some((pane_id, cell));
                self.terminal_selection.insert(pane_id, None);
            }
            Message::TerminalMouseUpAnywhere => {
                let (cw, lh) = self.term_metrics();
                // Send release to terminal app if mouse tracking.
                if let Some(focused) = self.focused_pane_id() {
                    if let Some(terminal) = self.terminals.get_mut(&focused) {
                        if terminal.grid.mouse_tracking != vibemux_term::MouseTracking::Off {
                            let p = self
                                .terminal_pointer_local
                                .get(&focused)
                                .copied()
                                .unwrap_or_else(|| Point::new(5.0, 5.0));
                            let grid = &terminal.grid;
                            let (r, c) = point_to_cell(p.x, p.y, grid.cols, grid.rows, cw, lh);
                            terminal.send_mouse_event(vibemux_term::MouseEvent {
                                kind: vibemux_term::MouseEventKind::Release,
                                button: vibemux_term::MouseButton::Left,
                                col: c as u16,
                                row: r as u16,
                            });
                        }
                    }
                }

                let pane_opt = self.selection_drag_pane.take();
                self.selection_pending_anchor = None;
                self.split_drag_active = None;
                if !self.selection_drag_moved {
                    if let Some(pid) = pane_opt {
                        self.terminal_selection.insert(pid, None);
                    }
                }
            }
            Message::RequestClipboardPaste => {
                return clipboard::read().map(Message::TerminalPaste);
            }
            Message::SelectAllTerminalInput => {
                self.select_all_terminal_input();
            }
            Message::CutTerminalSelection => {
                return self.terminal_apply_selection_delete(true);
            }
            Message::TerminalPaste(text_opt) => {
                let Some(raw) = text_opt else {
                    return Task::none();
                };
                let text = sanitize_clipboard_for_shell(&raw);
                if !text.is_empty() {
                    if let Some(focused) = self.focused_pane_id() {
                        if let Some(terminal) = self.terminals.get_mut(&focused) {
                            self.terminal_selection.insert(focused, None);
                            let _ = terminal.write(text.as_bytes());
                        }
                    }
                }
            }
            Message::CopyTerminalSelection => {
                let Some(focused) = self.focused_pane_id() else {
                    return Task::none();
                };
                let Some(term) = self.terminals.get(&focused) else {
                    return Task::none();
                };
                let Some(sel) =
                    self.terminal_selection.get(&focused).and_then(|s| s.as_ref())
                else {
                    return Task::none();
                };
                if sel.collapsed() {
                    return Task::none();
                }
                let s = selection_text(&term.grid, sel);
                return clipboard::write::<Message>(s);
            }
            Message::TerminalExtendSelection {
                delta_row,
                delta_col,
            } => {
                self.extend_terminal_selection_keyboard(delta_row, delta_col);
            }
            Message::TerminalExtendSelectionLineStart => {
                self.extend_terminal_selection_line_start();
            }
            Message::TerminalExtendSelectionLineEnd => {
                self.extend_terminal_selection_line_end();
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
                            return self
                                .command_palette
                                .scroll_list_to_selection_task(false);
                        }
                        keyboard::Key::Named(
                            keyboard::key::Named::ArrowDown,
                        ) => {
                            self.command_palette.select_down();
                            return self
                                .command_palette
                                .scroll_list_to_selection_task(false);
                        }
                        _ => return Task::none(),
                    }
                }

                // Find bar keyboard shortcuts.
                if self.find_bar.visible {
                    match &key {
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            return self.update(Message::ToggleFindBar);
                        }
                        keyboard::Key::Named(keyboard::key::Named::Enter) => {
                            if modifiers.shift() {
                                return self.update(Message::FindBarPrev);
                            } else {
                                return self.update(Message::FindBarNext);
                            }
                        }
                        _ => {}
                    }
                }

                if !self.command_palette.visible && !self.find_bar.visible {
                    if let keyboard::Key::Named(keyboard::key::Named::Escape) =
                        &key
                    {
                        if !modifiers.control() && !modifiers.alt() {
                            if let Some(f) = self.focused_pane_id() {
                                let had_range = self
                                    .terminal_selection
                                    .get(&f)
                                    .and_then(|o| o.as_ref())
                                    .is_some_and(|s| !s.collapsed());
                                self.terminal_selection.insert(f, None);
                                if had_range {
                                    return Task::none();
                                }
                            }
                        }
                    }
                }

                if !self.command_palette.visible
                    && !self.find_bar.visible
                    && modifiers.shift()
                    && !modifiers.control()
                    && !modifiers.alt()
                {
                    use keyboard::key::Named;
                    match &key {
                        keyboard::Key::Named(Named::ArrowLeft) => {
                            return self.update(Message::TerminalExtendSelection {
                                delta_row: 0,
                                delta_col: -1,
                            });
                        }
                        keyboard::Key::Named(Named::ArrowRight) => {
                            return self.update(Message::TerminalExtendSelection {
                                delta_row: 0,
                                delta_col: 1,
                            });
                        }
                        keyboard::Key::Named(Named::ArrowUp) => {
                            return self.update(Message::TerminalExtendSelection {
                                delta_row: -1,
                                delta_col: 0,
                            });
                        }
                        keyboard::Key::Named(Named::ArrowDown) => {
                            return self.update(Message::TerminalExtendSelection {
                                delta_row: 1,
                                delta_col: 0,
                            });
                        }
                        keyboard::Key::Named(Named::Home) => {
                            return self
                                .update(Message::TerminalExtendSelectionLineStart);
                        }
                        keyboard::Key::Named(Named::End) => {
                            return self
                                .update(Message::TerminalExtendSelectionLineEnd);
                        }
                        keyboard::Key::Named(Named::PageUp) => {
                            let rows = self
                                .focused_pane_id()
                                .and_then(|p| self.terminals.get(&p))
                                .map(|t| t.grid.rows as i32)
                                .unwrap_or(1);
                            return self.update(Message::TerminalExtendSelection {
                                delta_row: -rows.max(1),
                                delta_col: 0,
                            });
                        }
                        keyboard::Key::Named(Named::PageDown) => {
                            let rows = self
                                .focused_pane_id()
                                .and_then(|p| self.terminals.get(&p))
                                .map(|t| t.grid.rows as i32)
                                .unwrap_or(1);
                            return self.update(Message::TerminalExtendSelection {
                                delta_row: rows.max(1),
                                delta_col: 0,
                            });
                        }
                        keyboard::Key::Named(Named::Insert) => {
                            return clipboard::read().map(Message::TerminalPaste);
                        }
                        _ => {}
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
                        keyboard::Key::Character(c)
                            if c.as_str() == "t" || c.as_str() == "T" =>
                        {
                            return self.update(Message::NewTab);
                        }
                        keyboard::Key::Character(c)
                            if c.as_str() == "c" || c.as_str() == "C" =>
                        {
                            return self.update(Message::CopyTerminalSelection);
                        }
                        keyboard::Key::Character(c)
                            if c.as_str().eq_ignore_ascii_case("v")
                                && !self.command_palette.visible
                                && !self.find_bar.visible =>
                        {
                            return clipboard::read().map(Message::TerminalPaste);
                        }
                        keyboard::Key::Character(c)
                            if c.as_str().eq_ignore_ascii_case("a")
                                && !self.command_palette.visible
                                && !self.find_bar.visible =>
                        {
                            self.select_all_terminal_input();
                            return Task::none();
                        }
                        keyboard::Key::Character(c)
                            if c.as_str() == "f" || c.as_str() == "F" =>
                        {
                            return self.update(Message::ToggleFindBar);
                        }
                        _ => {}
                    }
                }

                if modifiers.control() && !modifiers.shift() {
                    match &key {
                        keyboard::Key::Character(c)
                            if c.as_str().eq_ignore_ascii_case("a")
                                && !self.command_palette.visible
                                && !self.find_bar.visible =>
                        {
                            self.select_all_terminal_input();
                            return Task::none();
                        }
                        keyboard::Key::Named(
                            keyboard::key::Named::Tab,
                        ) => {
                            return self
                                .update(Message::NextWorkspace);
                        }
                        keyboard::Key::Named(
                            keyboard::key::Named::F4,
                        ) => {
                            return self.update(Message::CloseActiveTab);
                        }
                        keyboard::Key::Named(
                            keyboard::key::Named::PageDown,
                        ) => {
                            return self.update(Message::NextTab);
                        }
                        keyboard::Key::Named(
                            keyboard::key::Named::PageUp,
                        ) => {
                            return self.update(Message::PrevTab);
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

                // Backspace / Ctrl+Shift+X: delete or cut the in-app selection.
                if !self.command_palette.visible && !self.find_bar.visible {
                    use keyboard::key::Named;
                    if let Some(focused) = self.focused_pane_id() {
                        let do_cut = modifiers.control()
                            && modifiers.shift()
                            && !modifiers.alt()
                            && matches!(
                                &key,
                                keyboard::Key::Character(c)
                                    if c.as_str().eq_ignore_ascii_case("x")
                            );
                        let do_bs = !modifiers.control()
                            && !modifiers.alt()
                            && matches!(&key, keyboard::Key::Named(Named::Backspace));
                        if (do_cut || do_bs)
                            && self
                                .terminal_selection
                                .get(&focused)
                                .and_then(|o| o.as_ref())
                                .is_some_and(|s| !s.collapsed())
                        {
                            return self.terminal_apply_selection_delete(do_cut);
                        }
                    }
                }

                let bytes = key_to_bytes(&key, &modifiers);
                if let Some(bytes) = bytes {
                    if let Some(focused) = self.focused_pane_id() {
                        if let Some(terminal) =
                            self.terminals.get_mut(&focused)
                        {
                            self.terminal_selection.insert(focused, None);
                            let _ = terminal.write(&bytes);
                        }
                    }
                }
            }
            Message::ToggleNotificationPanel => {
                self.show_notification_panel = !self.show_notification_panel;
                if self.show_notification_panel {
                    let id = self.workspace_manager.active().id;
                    self.notification_manager.mark_workspace_read(id);
                    self.sync_notification_badges();
                }
                self.resize_terminals_from_window();
            }
            Message::ToggleFindBar => {
                self.find_bar.toggle();
                self.resize_terminals_from_window();
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
                        self.find_bar.matches = matches;
                    }
                }
            }
            Message::FindBarNext => {
                self.find_bar.next_match();
            }
            Message::FindBarPrev => {
                self.find_bar.prev_match();
            }
            Message::SaveSession => {
                self.save_session();
            }
            Message::ToggleCommandPalette => {
                self.command_palette.toggle();
                if self.command_palette.visible {
                    return self
                        .command_palette
                        .scroll_list_to_selection_task(true);
                }
            }
            Message::CommandPaletteInput(query) => {
                self.command_palette.set_query(query);
                return self.command_palette.scroll_list_to_selection_task(true);
            }
            Message::CommandPaletteUp => {
                self.command_palette.select_up();
                return self
                    .command_palette
                    .scroll_list_to_selection_task(false);
            }
            Message::CommandPaletteDown => {
                self.command_palette.select_down();
                return self
                    .command_palette
                    .scroll_list_to_selection_task(false);
            }
            Message::CommandPaletteConfirm => {
                if let Some(msg) = self.command_palette.confirm() {
                    return self.update(msg);
                }
            }
            Message::SplitDragStart(split_id, _dir) => {
                self.split_drag_active = Some(split_id);
            }
            Message::SplitDragMove(split_id, dir, point) => {
                if self.split_drag_active == Some(split_id) {
                    // Calculate the ratio based on pointer position relative to window.
                    // This is approximate since we get local coords within the divider area.
                    // We'll use the window size to compute the ratio.
                    if let Some(size) = self.last_window_size {
                        const SIDEBAR_W: f32 = 220.0;
                        const MAIN_DIVIDER: f32 = 1.0;
                        const TAB_BAR_H: f32 = 44.0;

                        let ratio = match dir {
                            SplitDirection::Vertical => {
                                let content_w = size.width - SIDEBAR_W - MAIN_DIVIDER;
                                // point.x is in local coords of the mouse_area (divider)
                                // We need to use a different approach: the ratio should
                                // be computed from the fraction of the available space.
                                // For now, use a simple approach: shift ratio by the
                                // delta from center.
                                // Since we can't get absolute coords easily, just use
                                // relative movement from the point's x position.
                                // point.x will be negative when dragging left of the divider.
                                let current_ratio = self.get_split_ratio(split_id).unwrap_or(0.5);
                                let delta = point.x / content_w.max(1.0);
                                (current_ratio + delta).clamp(0.1, 0.9)
                            }
                            SplitDirection::Horizontal => {
                                let content_h = size.height - TAB_BAR_H;
                                let current_ratio = self.get_split_ratio(split_id).unwrap_or(0.5);
                                let delta = point.y / content_h.max(1.0);
                                (current_ratio + delta).clamp(0.1, 0.9)
                            }
                        };
                        self.set_split_ratio(split_id, ratio);
                        self.resize_terminals_from_window();
                    }
                }
            }
            Message::SplitDragEnd => {
                self.split_drag_active = None;
            }
            Message::Tick => {
                let pane_ids: Vec<PaneId> =
                    self.pty_readers.keys().copied().collect();
                let mut snap_tasks: Vec<Task<Message>> = Vec::new();
                for pane_id in pane_ids {
                    let has_data = self
                        .pty_readers
                        .get(&pane_id)
                        .map(|r| r.has_data())
                        .unwrap_or(false);

                    if !has_data {
                        continue;
                    }

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
                        if let Some(terminal) =
                            self.terminals.get_mut(&pane_id)
                        {
                            terminal.process_output(&data);

                            if let Some((wi, ti)) =
                                self.workspace_manager.locate_pane(pane_id)
                            {
                                let ws =
                                    &mut self.workspace_manager.workspaces_mut()
                                        [wi];
                                let tab = &mut ws.tabs[ti];
                                if let Some(cwd) =
                                    terminal.grid.osc_cwd.clone()
                                {
                                    let old_cwd = tab.cwd.clone();
                                    tab.cwd = Some(cwd.clone());

                                    if old_cwd.as_deref() != Some(&cwd) {
                                        tab.git_branch =
                                            git_info::detect_git_branch(&cwd);
                                    }
                                }
                                if let Some(title) =
                                    terminal.grid.title.clone()
                                {
                                    tab.title = Some(title);
                                }
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

                            if self
                                .terminal_stick_to_bottom
                                .get(&pane_id)
                                .copied()
                                .unwrap_or(true)
                            {
                                snap_tasks.push(operation::snap_to_end(
                                    iced::widget::Id::from(format!(
                                        "term-scroll-{pane_id}"
                                    )),
                                ));
                            }
                        }
                    }
                }

                while let Ok(cmd) = self.ipc_rx.try_recv() {
                    self.handle_ipc_command(cmd);
                }

                self.sync_notification_badges();

                if self.last_session_save.elapsed()
                    > std::time::Duration::from_secs(30)
                {
                    self.save_session();
                    self.last_session_save = std::time::Instant::now();
                }

                if !snap_tasks.is_empty() {
                    return Task::batch(snap_tasks);
                }
            }
        }

        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let sidebar = sidebar::view(&self.workspace_manager);

        let active_ws = self.workspace_manager.active();
        let focused = active_ws.split_tree().focused_pane;

        let term_area = if let Some(ref root) = active_ws.split_tree().root {
            split_view::render_split_tree(
                root,
                &self.terminals,
                focused,
                self.bytes_received,
                &self.terminal_selection,
                self.term_font,
                self.term_font_size,
            )
        } else {
            empty_pane()
        };

        let content = column![
            tab_bar::view(active_ws),
            container(term_area).width(Fill).height(Fill),
        ]
        .width(Fill)
        .height(Fill)
        .into();

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

        let panel_divider = container(text(""))
            .width(Length::Fixed(1.0))
            .height(Fill)
            .style(|_t: &Theme| iced::widget::container::Style {
                background: Some(theme::BORDER.into()),
                ..Default::default()
            });

        let main_layout: Element<'_, Message> = if self.show_notification_panel
        {
            row![
                sidebar,
                divider,
                content_with_find,
                panel_divider,
                notification_panel::view(&self.notification_manager),
            ]
            .width(Fill)
            .height(Fill)
            .into()
        } else {
            row![sidebar, divider, content_with_find]
                .width(Fill)
                .height(Fill)
                .into()
        };

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
                iced::Event::Mouse(mouse::Event::ButtonReleased(
                    mouse::Button::Left,
                )) => Some(Message::TerminalMouseUpAnywhere),
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    modified_key,
                    modifiers,
                    ..
                }) => Some(Message::KeyboardInput(modified_key, modifiers)),
                _ => None,
            });

        let resize = window::resize_events().map(|(_, size)| Message::WindowResized(size));

        Subscription::batch([tick, keys, resize])
    }

    /// Delete the non-collapsed terminal selection; optionally copy to clipboard (cut).
    fn terminal_apply_selection_delete(&mut self, copy_to_clipboard: bool) -> Task<Message> {
        let Some(focused) = self.focused_pane_id() else {
            return Task::none();
        };
        let Some(sel) = self
            .terminal_selection
            .get(&focused)
            .and_then(|o| o.as_ref())
            .filter(|s| !s.collapsed())
        else {
            return Task::none();
        };
        let Some(term) = self.terminals.get(&focused) else {
            return Task::none();
        };
        if let Some(bytes) = delete_selection_via_pty(&term.grid, sel) {
            let cut_text = if copy_to_clipboard {
                Some(selection_text(&term.grid, sel))
            } else {
                None
            };
            self.terminal_selection.insert(focused, None);
            if let Some(terminal) = self.terminals.get_mut(&focused) {
                let _ = terminal.write(&bytes);
            }
            if let Some(t) = cut_text {
                return clipboard::write::<Message>(t);
            }
            return Task::none();
        }
        self.terminal_selection.insert(focused, None);
        Task::none()
    }

    /// Select editable text on the current shell line (after the prompt through logical EOL).
    fn select_all_terminal_input(&mut self) {
        let Some(focused) = self.focused_pane_id() else {
            return;
        };
        let Some(term) = self.terminals.get(&focused) else {
            return;
        };
        let grid = &term.grid;
        if grid.display_line_count() == 0 || grid.cols == 0 {
            return;
        }
        let dr = grid.display_cursor_row();
        let sc = input_start_column(grid, dr);
        let ec = logical_line_end_col(grid, dr);
        let mut sel = TerminalSelection {
            anchor: (dr, sc),
            head: (dr, ec),
        };
        clamp_selection_to_input(grid, &mut sel);
        self.terminal_selection.insert(focused, Some(sel));
    }

    fn extend_terminal_selection_keyboard(&mut self, delta_row: i32, delta_col: i32) {
        let Some(focused) = self.focused_pane_id() else {
            return;
        };
        let Some(term) = self.terminals.get(&focused) else {
            return;
        };
        let grid = &term.grid;
        let n = grid.display_line_count();
        let cols = grid.cols;
        if n == 0 || cols == 0 {
            return;
        }
        let cur = (grid.display_cursor_row(), grid.cursor_col);
        match self.terminal_selection.get_mut(&focused) {
            Some(Some(s)) => {
                if s.collapsed() && s.head != cur {
                    s.anchor = cur;
                    s.head = cur;
                }
                s.head = sel_move_cell(s.head, delta_row, delta_col, n, cols);
                clamp_selection_to_input(grid, s);
            }
            _ => {
                let mut sel = TerminalSelection {
                    anchor: cur,
                    head: sel_move_cell(cur, delta_row, delta_col, n, cols),
                };
                clamp_selection_to_input(grid, &mut sel);
                self.terminal_selection.insert(focused, Some(sel));
            }
        }
    }

    fn extend_terminal_selection_line_start(&mut self) {
        let Some(focused) = self.focused_pane_id() else {
            return;
        };
        let Some(term) = self.terminals.get(&focused) else {
            return;
        };
        let grid = &term.grid;
        let n = grid.display_line_count();
        let cols = grid.cols;
        if n == 0 || cols == 0 {
            return;
        }
        let cur = (grid.display_cursor_row(), grid.cursor_col);
        let start_c = input_start_column(grid, cur.0);
        match self.terminal_selection.get_mut(&focused) {
            Some(Some(s)) => {
                if s.collapsed() && s.head != cur {
                    s.anchor = cur;
                    s.head = cur;
                }
                let start_c = input_start_column(grid, s.head.0);
                let c = if s.head.0 == grid.display_cursor_row() {
                    start_c
                } else {
                    0
                };
                s.head = (s.head.0, c);
                clamp_selection_to_input(grid, s);
            }
            _ => {
                let mut sel = TerminalSelection {
                    anchor: cur,
                    head: (cur.0, start_c),
                };
                clamp_selection_to_input(grid, &mut sel);
                self.terminal_selection.insert(focused, Some(sel));
            }
        }
    }

    fn extend_terminal_selection_line_end(&mut self) {
        let Some(focused) = self.focused_pane_id() else {
            return;
        };
        let Some(term) = self.terminals.get(&focused) else {
            return;
        };
        let grid = &term.grid;
        if grid.display_line_count() == 0 || grid.cols == 0 {
            return;
        }
        let cur = (grid.display_cursor_row(), grid.cursor_col);
        match self.terminal_selection.get_mut(&focused) {
            Some(Some(s)) => {
                if s.collapsed() && s.head != cur {
                    s.anchor = cur;
                    s.head = cur;
                }
                let end_col = logical_line_end_col(grid, s.head.0);
                s.head = (s.head.0, end_col);
                clamp_selection_to_input(grid, s);
            }
            _ => {
                let end_col = logical_line_end_col(grid, cur.0);
                let mut sel = TerminalSelection {
                    anchor: cur,
                    head: (cur.0, end_col),
                };
                clamp_selection_to_input(grid, &mut sel);
                self.terminal_selection.insert(focused, Some(sel));
            }
        }
    }

    fn focused_pane_id(&self) -> Option<PaneId> {
        self.workspace_manager.active().split_tree().focused_pane
    }

    fn save_session(&self) {
        use crate::session::{
            capture_split_layout, SessionState, TabState, WorkspaceState,
        };

        let workspaces: Vec<WorkspaceState> = self
            .workspace_manager
            .workspaces()
            .iter()
            .map(|ws| {
                let tabs: Vec<TabState> = ws
                    .tabs
                    .iter()
                    .map(|tab| {
                        let split_layout =
                            if let Some(ref root) = tab.split_tree.root {
                                capture_split_layout(root, &|_| {
                                    tab.cwd.clone()
                                })
                            } else {
                                crate::session::SplitLayoutState::Single {
                                    cwd: tab.cwd.clone(),
                                }
                            };
                        TabState {
                            cwd: tab.cwd.clone(),
                            split_layout,
                        }
                    })
                    .collect();
                WorkspaceState {
                    name: ws.name.clone(),
                    pinned: ws.pinned,
                    tabs,
                    active_tab_index: ws.active_tab_index,
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

    /// Get the ratio of a split node by its UUID.
    fn get_split_ratio(&self, split_id: Uuid) -> Option<f32> {
        for ws in self.workspace_manager.workspaces() {
            for tab in &ws.tabs {
                if let Some(root) = &tab.split_tree.root {
                    if let Some(r) = find_split_ratio(root, split_id) {
                        return Some(r);
                    }
                }
            }
        }
        None
    }

    /// Set the ratio of a split node by its UUID.
    fn set_split_ratio(&mut self, split_id: Uuid, ratio: f32) {
        for ws in self.workspace_manager.workspaces_mut() {
            for tab in &mut ws.tabs {
                if let Some(root) = &mut tab.split_tree.root {
                    set_split_ratio_recursive(root, split_id, ratio);
                }
            }
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
                        let t = ws
                            .tabs
                            .get(ws.active_tab_index)
                            .or_else(|| ws.tabs.first());
                        json!({
                            "id": ws.id.to_string(),
                            "name": ws.name,
                            "cwd": t.and_then(|x| x.cwd.clone()),
                            "git_branch": t.and_then(|x| x.git_branch.clone()),
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
                self.workspace_manager
                    .active_mut()
                    .active_tab_mut()
                    .split_tree = SplitTree::with_pane(pane_id);
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
                        .map(|w| w.all_pane_ids())
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
                let tab = ws.active_tab();
                let _ = reply.send(Response::success(
                    req_id,
                    json!({
                        "id": ws.id.to_string(),
                        "name": ws.name,
                        "cwd": tab.cwd,
                        "git_branch": tab.git_branch,
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
                let focused_tree = ws.split_tree();
                let surfaces: Vec<serde_json::Value> = ws
                    .tabs
                    .iter()
                    .flat_map(|tab| tab.split_tree.pane_ids())
                    .map(|pid| {
                        json!({
                            "id": pid.to_string(),
                            "focused": focused_tree.focused_pane == Some(pid),
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

fn find_split_ratio(node: &vibemux_mux::SplitNode, target_id: Uuid) -> Option<f32> {
    match node {
        vibemux_mux::SplitNode::Leaf { .. } => None,
        vibemux_mux::SplitNode::Split {
            id,
            ratio,
            first,
            second,
            ..
        } => {
            if *id == target_id {
                Some(*ratio)
            } else {
                find_split_ratio(first, target_id)
                    .or_else(|| find_split_ratio(second, target_id))
            }
        }
    }
}

fn set_split_ratio_recursive(
    node: &mut vibemux_mux::SplitNode,
    target_id: Uuid,
    new_ratio: f32,
) {
    match node {
        vibemux_mux::SplitNode::Leaf { .. } => {}
        vibemux_mux::SplitNode::Split {
            id,
            ratio,
            first,
            second,
            ..
        } => {
            if *id == target_id {
                *ratio = new_ratio;
            } else {
                set_split_ratio_recursive(first, target_id, new_ratio);
                set_split_ratio_recursive(second, target_id, new_ratio);
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

/// Normalize OS clipboard text before injecting into the shell.
fn sanitize_clipboard_for_shell(raw: &str) -> String {
    let s = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    let s = s.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\t' | '\n' => out.push(ch),
            c if !c.is_control() => out.push(c),
            _ => {}
        }
    }
    out
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

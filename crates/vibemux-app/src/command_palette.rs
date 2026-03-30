use crate::app::Message;
use crate::theme;
use iced::widget::operation::{self, RelativeOffset};
use iced::widget::{column, container, scrollable, text, text_input, Column, Id};
use iced::{Border, Color, Element, Fill, Length, Padding, Task, Theme};

pub fn scroll_id() -> Id {
    Id::from("command-palette-scroll")
}

#[derive(Debug, Clone)]
pub struct CommandEntry {
    pub label: String,
    pub shortcut: Option<String>,
    pub message: Message,
}

pub struct CommandPalette {
    pub visible: bool,
    pub query: String,
    commands: Vec<CommandEntry>,
    pub selected_index: usize,
    /// First visible row index in the list (for scroll position).
    list_scroll_anchor: usize,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            commands: Self::all_commands(),
            selected_index: 0,
            list_scroll_anchor: 0,
        }
    }

    /// ~8 rows visible in the fixed-height list (see `view` scroll height / row padding).
    const VISIBLE_ROWS: usize = 8;

    fn all_commands() -> Vec<CommandEntry> {
        vec![
            CommandEntry {
                label: "New Workspace".into(),
                shortcut: Some("Ctrl+Shift+N".into()),
                message: Message::CreateWorkspace,
            },
            CommandEntry {
                label: "Copy terminal selection".into(),
                shortcut: Some("Ctrl+Shift+C".into()),
                message: Message::CopyTerminalSelection,
            },
            CommandEntry {
                label: "Paste into terminal".into(),
                shortcut: Some("Ctrl+Shift+V".into()),
                message: Message::RequestClipboardPaste,
            },
            CommandEntry {
                label: "Select all terminal input".into(),
                shortcut: Some("Ctrl+A".into()),
                message: Message::SelectAllTerminalInput,
            },
            CommandEntry {
                label: "Cut terminal selection".into(),
                shortcut: Some("Ctrl+Shift+X".into()),
                message: Message::CutTerminalSelection,
            },
            CommandEntry {
                label: "Toggle find in terminal".into(),
                shortcut: Some("Ctrl+Shift+F".into()),
                message: Message::ToggleFindBar,
            },
            CommandEntry {
                label: "New Tab".into(),
                shortcut: Some("Ctrl+Shift+T".into()),
                message: Message::NewTab,
            },
            CommandEntry {
                label: "Close Tab".into(),
                shortcut: Some("Ctrl+F4".into()),
                message: Message::CloseActiveTab,
            },
            CommandEntry {
                label: "Next Tab".into(),
                shortcut: Some("Ctrl+PgDn".into()),
                message: Message::NextTab,
            },
            CommandEntry {
                label: "Previous Tab".into(),
                shortcut: Some("Ctrl+PgUp".into()),
                message: Message::PrevTab,
            },
            CommandEntry {
                label: "Split Right".into(),
                shortcut: Some("Ctrl+Shift+D".into()),
                message: Message::SplitRight,
            },
            CommandEntry {
                label: "Split Down".into(),
                shortcut: Some("Ctrl+Shift+E".into()),
                message: Message::SplitDown,
            },
            CommandEntry {
                label: "Close Pane".into(),
                shortcut: Some("Ctrl+Shift+Q".into()),
                message: Message::CloseFocusedPane,
            },
            CommandEntry {
                label: "Next Workspace".into(),
                shortcut: Some("Ctrl+Tab".into()),
                message: Message::NextWorkspace,
            },
            CommandEntry {
                label: "Previous Workspace".into(),
                shortcut: Some("Ctrl+Shift+Tab".into()),
                message: Message::PrevWorkspace,
            },
            CommandEntry {
                label: "Focus Next Pane".into(),
                shortcut: Some("Alt+Tab".into()),
                message: Message::FocusNextPane,
            },
            CommandEntry {
                label: "Toggle Notification Panel".into(),
                shortcut: Some("Ctrl+Shift+I".into()),
                message: Message::ToggleNotificationPanel,
            },
        ]
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if self.visible {
            self.query.clear();
            self.selected_index = 0;
            self.list_scroll_anchor = 0;
        }
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
        self.list_scroll_anchor = 0;
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.selected_index = 0;
        self.list_scroll_anchor = 0;
    }

    pub fn select_up(&mut self) {
        let filtered = self.filtered_commands();
        if !filtered.is_empty() && self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn select_down(&mut self) {
        let filtered = self.filtered_commands();
        if self.selected_index + 1 < filtered.len() {
            self.selected_index += 1;
        }
    }

    pub fn confirm(&mut self) -> Option<Message> {
        let filtered = self.filtered_commands();
        if self.selected_index < filtered.len() {
            let msg = filtered[self.selected_index].message.clone();
            self.close();
            Some(msg)
        } else {
            None
        }
    }

    fn filtered_commands(&self) -> Vec<&CommandEntry> {
        if self.query.is_empty() {
            return self.commands.iter().collect();
        }
        let q = self.query.to_lowercase();
        self.commands
            .iter()
            .filter(|cmd| fuzzy_match(&cmd.label, &q))
            .collect()
    }

    /// Updates scroll only when the selection moves outside the visible window, or when `force` is set
    /// (e.g. open palette or filter text changed).
    pub fn scroll_list_to_selection_task(&mut self, force: bool) -> Task<Message> {
        let n = self.filtered_commands().len();
        if n == 0 {
            return Task::none();
        }
        let vr = Self::VISIBLE_ROWS;
        let max_anchor = n.saturating_sub(vr);
        let mut anchor = self.list_scroll_anchor.min(max_anchor);
        if self.selected_index < anchor {
            anchor = self.selected_index;
        }
        if vr > 0 && n > vr && self.selected_index >= anchor + vr {
            anchor = self.selected_index + 1 - vr;
        }
        anchor = anchor.min(max_anchor);
        let changed = anchor != self.list_scroll_anchor;
        self.list_scroll_anchor = anchor;
        if !force && !changed {
            return Task::none();
        }
        let y = if max_anchor == 0 {
            0.0
        } else {
            (anchor as f32 / max_anchor as f32).clamp(0.0, 1.0)
        };
        operation::snap_to(scroll_id(), RelativeOffset { x: 0.0, y })
    }

    pub fn view(&self) -> Element<'_, Message> {
        let input = text_input("Type a command...", &self.query)
            .on_input(Message::CommandPaletteInput)
            .size(14)
            .padding(Padding::from([10.0, 14.0]))
            .width(Fill);

        let filtered = self.filtered_commands();
        let mut items = Column::new().spacing(1);

        for (i, cmd) in filtered.iter().enumerate() {
            let is_selected = i == self.selected_index;

            let label = text(&cmd.label)
                .size(13)
                .color(theme::FG_PRIMARY);

            let shortcut_text = if let Some(ref sc) = cmd.shortcut {
                text(sc).size(11).color(theme::FG_DIM)
            } else {
                text("").size(11)
            };

            let row_content = iced::widget::row![label, shortcut_text]
                .spacing(12)
                .align_y(iced::Alignment::Center)
                .width(Fill);

            let bg = if is_selected {
                theme::BG_SURFACE
            } else {
                Color::TRANSPARENT
            };

            let item = container(row_content)
                .padding(Padding::from([8.0, 14.0]))
                .width(Fill)
                .style(move |_t: &Theme| container::Style {
                    background: Some(bg.into()),
                    ..Default::default()
                });

            items = items.push(item);
        }

        let palette = column![
            input,
            scrollable(items)
                .id(scroll_id())
                .width(Fill)
                .height(Length::Fixed(300.0)),
        ]
        .width(Length::Fixed(500.0));

        let overlay = container(
            container(palette)
                .style(|_t: &Theme| container::Style {
                    background: Some(theme::BG_SIDEBAR.into()),
                    border: Border {
                        color: theme::BORDER,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..Default::default()
                })
                .padding(Padding::new(4.0)),
        )
        .width(Fill)
        .height(Fill)
        .padding(Padding::from([100.0, 0.0]))
        .center_x(Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(
                Color::from_rgba(0.0, 0.0, 0.0, 0.5).into(),
            ),
            ..Default::default()
        });

        overlay.into()
    }
}

fn fuzzy_match(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    let mut text_chars = text.chars();
    for qc in query.chars() {
        loop {
            match text_chars.next() {
                Some(tc) if tc == qc => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

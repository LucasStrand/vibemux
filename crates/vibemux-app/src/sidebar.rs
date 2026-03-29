use crate::app::Message;
use crate::theme;
use iced::widget::{button, column, container, progress_bar, row, scrollable, text, Column};
use iced::{Border, Color, Element, Fill, Length, Padding};
use vibemux_mux::WorkspaceManager;

pub fn view<'a>(manager: &'a WorkspaceManager) -> Element<'a, Message> {
    let active_idx = manager.active_index();

    let mut items = Column::new().spacing(2).padding(Padding::new(6.0));

    for (i, ws) in manager.workspaces().iter().enumerate() {
        let is_active = i == active_idx;

        let name_color = if is_active {
            theme::FG_PRIMARY
        } else {
            theme::FG_DIM
        };

        let name_text = text(&ws.name).size(13).color(name_color);

        let meta_str = if let Some(ref branch) = ws.metadata.git_branch {
            branch.clone()
        } else if let Some(ref cwd) = ws.metadata.cwd {
            cwd.rsplit(['/', '\\'])
                .next()
                .unwrap_or(cwd)
                .to_string()
        } else {
            String::new()
        };

        let meta_color = if ws.metadata.git_branch.is_some() {
            theme::ACCENT_GREEN
        } else {
            theme::FG_DIM
        };

        let mut content = column![
            name_text,
            text(meta_str).size(11).color(meta_color),
        ]
        .spacing(2);

        if is_active {
            for status in &ws.metadata.status_entries {
                let status_text = text(format!("{}: {}", status.key, status.value))
                    .size(10)
                    .color(theme::ACCENT);
                content = content.push(status_text);
            }

            if let Some(ref progress) = ws.metadata.progress {
                let label = progress
                    .label
                    .as_deref()
                    .unwrap_or("");
                let bar = column![
                    text(label).size(10).color(theme::FG_DIM),
                    progress_bar(0.0..=1.0, progress.value),
                ]
                .spacing(2);
                content = content.push(bar);
            }
        }

        if ws.has_unread {
            let badge = text(" *").size(13).color(theme::NOTIFICATION);
            let name_row = row![
                text(&ws.name).size(13).color(name_color),
                badge,
            ]
            .spacing(2);
            content = column![
                name_row,
                text(if let Some(ref branch) = ws.metadata.git_branch {
                    branch.clone()
                } else {
                    String::new()
                })
                .size(11)
                .color(meta_color),
            ]
            .spacing(2);
        }

        let ws_id = ws.id;
        let btn = button(
            container(content)
                .padding(Padding::from([8.0, 10.0]))
                .width(Fill),
        )
        .on_press(Message::SelectWorkspace(ws_id))
        .width(Fill)
        .style(move |_t: &iced::Theme, _status| {
            let bg = if is_active {
                theme::BG_SURFACE
            } else {
                Color::TRANSPARENT
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme::FG_PRIMARY,
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..Default::default()
            }
        });

        items = items.push(btn);
    }

    let new_ws_btn = button(
        container(text("+  New Workspace").size(12).color(theme::FG_DIM))
            .padding(Padding::from([8.0, 10.0]))
            .width(Fill),
    )
    .on_press(Message::CreateWorkspace)
    .width(Fill)
    .style(|_t: &iced::Theme, _status| button::Style {
        background: Some(Color::TRANSPARENT.into()),
        text_color: theme::FG_DIM,
        border: Border {
            radius: 6.0.into(),
            ..Border::default()
        },
        ..Default::default()
    });

    let header = container(
        text("VibeMux")
            .size(16)
            .color(theme::FG_PRIMARY)
            .font(iced::Font::with_name("Segoe UI")),
    )
    .padding(Padding::from([14.0, 12.0]));

    let sidebar_content = column![
        header,
        scrollable(items).height(Fill),
        container(new_ws_btn).padding(Padding::new(6.0)),
    ]
    .height(Fill);

    container(sidebar_content)
        .width(Length::Fixed(220.0))
        .height(Fill)
        .style(|_t: &iced::Theme| container::Style {
            background: Some(theme::BG_SIDEBAR.into()),
            ..Default::default()
        })
        .into()
}

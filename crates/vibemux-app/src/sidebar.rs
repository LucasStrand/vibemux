use crate::app::Message;
use crate::theme;
use iced::widget::{button, column, container, progress_bar, row, scrollable, text, Column};
use iced::{Border, Color, Element, Fill, Length, Padding};
use vibemux_mux::WorkspaceManager;

pub fn view<'a>(manager: &'a WorkspaceManager) -> Element<'a, Message> {
    let active_idx = manager.active_index();
    let multi = manager.workspaces().len() > 1;

    let mut items = Column::new().spacing(2).padding(Padding::from([4.0, 6.0]));

    for (i, ws) in manager.workspaces().iter().enumerate() {
        let is_active = i == active_idx;

        let name_color = if is_active {
            theme::FG_PRIMARY
        } else {
            theme::FG_DIM
        };

        let name_text = text(&ws.name).size(13).color(name_color);

        let shell_tab = ws
            .tabs
            .get(ws.active_tab_index)
            .or_else(|| ws.tabs.first());

        let meta_str = if let Some(t) = shell_tab {
            if let Some(ref branch) = t.git_branch {
                branch.clone()
            } else if let Some(ref cwd) = t.cwd {
                cwd.rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(cwd)
                    .to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let meta_color = if shell_tab.and_then(|t| t.git_branch.as_ref()).is_some() {
            theme::ACCENT_GREEN
        } else {
            theme::FG_DIM
        };

        let mut content = column![
            name_text,
            text(meta_str.clone()).size(11).color(meta_color),
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
                let label = progress.label.as_deref().unwrap_or("");
                let bar = column![
                    text(label).size(10).color(theme::FG_DIM),
                    progress_bar(0.0..=1.0, progress.value),
                ]
                .spacing(2);
                content = content.push(bar);
            }
        }

        if ws.has_unread {
            let badge = text(" \u{2022}").size(13).color(theme::NOTIFICATION);
            let name_row = row![
                text(&ws.name).size(13).color(name_color),
                badge,
            ]
            .spacing(2);
            content = column![
                name_row,
                text(meta_str.clone()).size(11).color(meta_color),
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
                    let label = progress.label.as_deref().unwrap_or("");
                    let bar = column![
                        text(label).size(10).color(theme::FG_DIM),
                        progress_bar(0.0..=1.0, progress.value),
                    ]
                    .spacing(2);
                    content = content.push(bar);
                }
            }
        }

        let ws_id = ws.id;

        let select_btn = button(content)
            .on_press(Message::SelectWorkspace(ws_id))
            .width(Fill)
            .style(|_t: &iced::Theme, _status| button::Style {
                background: Some(Color::TRANSPARENT.into()),
                text_color: theme::FG_PRIMARY,
                border: Border::default(),
                ..Default::default()
            });

        let card_body: Element<'a, Message> = if multi {
            let close_btn = button(text("\u{00D7}").size(13).color(theme::FG_DIM))
                .on_press(Message::CloseWorkspace(ws_id))
                .padding(Padding::from([4.0, 6.0]))
                .style(|_t: &iced::Theme, _status| button::Style {
                    background: Some(Color::TRANSPARENT.into()),
                    text_color: theme::FG_DIM,
                    border: Border::default(),
                    ..Default::default()
                });

            row![select_btn, close_btn]
                .spacing(0)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            select_btn.into()
        };

        let card = container(card_body)
            .padding(Padding::from([8.0, 10.0]))
            .width(Fill)
            .style(move |_t: &iced::Theme| container::Style {
                background: Some(
                    if is_active {
                        theme::BG_SURFACE
                    } else {
                        Color::TRANSPARENT
                    }
                    .into(),
                ),
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..Default::default()
            });

        items = items.push(card);
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

    let sidebar_content = column![
        scrollable(items).height(Fill),
        container(new_ws_btn).padding(Padding::from([4.0, 6.0])),
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

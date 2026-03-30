use crate::app::Message;
use crate::theme;
use iced::widget::{button, container, row, text};
use iced::{Border, Color, Element, Fill, Padding, Theme};
use vibemux_mux::Workspace;

pub fn view<'a>(ws: &'a Workspace) -> Element<'a, Message> {
    let mut tabs_row = row![].spacing(2).padding(Padding::from([4.0, 6.0]));

    for (i, tab) in ws.tabs.iter().enumerate() {
        let is_active = i == ws.active_tab_index;
        let tab_id = tab.id;
        let label = tab.label(i);

        let label_btn = button(
            container(text(label).size(12).color(if is_active {
                theme::FG_PRIMARY
            } else {
                theme::FG_DIM
            }))
            .padding(Padding::from([6.0, 8.0])),
        )
        .on_press(Message::SelectTab(tab_id))
        .style(move |_t: &Theme, _status| {
            let bg = if is_active {
                theme::BG_SURFACE
            } else {
                Color::TRANSPARENT
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme::FG_PRIMARY,
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..Default::default()
            }
        });

        if ws.tabs.len() > 1 {
            let close_btn = button(
                text("\u{00D7}")
                    .size(13)
                    .color(theme::FG_DIM),
            )
            .on_press(Message::CloseTab(tab_id))
            .padding(Padding::from([4.0, 6.0]))
            .style(|_t: &Theme, _status| button::Style {
                background: Some(Color::TRANSPARENT.into()),
                text_color: theme::FG_DIM,
                border: Border::default(),
                ..Default::default()
            });

            tabs_row = tabs_row.push(
                container(
                    row![label_btn, close_btn].spacing(0),
                )
                .style(move |_t: &Theme| container::Style {
                    background: Some(
                        if is_active {
                            theme::BG_SURFACE
                        } else {
                            Color::TRANSPARENT
                        }
                        .into(),
                    ),
                    border: Border {
                        radius: 4.0.into(),
                        ..Border::default()
                    },
                    ..Default::default()
                }),
            );
        } else {
            tabs_row = tabs_row.push(label_btn);
        }
    }

    let new_tab = button(text("+").size(14).color(theme::FG_DIM))
        .on_press(Message::NewTab)
        .padding([4.0, 10.0])
        .style(|_t: &Theme, _status| button::Style {
            background: Some(Color::TRANSPARENT.into()),
            text_color: theme::FG_DIM,
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..Default::default()
        });

    tabs_row = tabs_row.push(new_tab);

    container(tabs_row)
        .width(Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(theme::BG_SIDEBAR.into()),
            ..Default::default()
        })
        .into()
}

use crate::app::Message;
use crate::theme;
use iced::widget::operation::{self, RelativeOffset};
use iced::widget::{button, container, row, scrollable, text, Id};
use iced::{Border, Color, Element, Fill, Length, Padding, Task, Theme};
use vibemux_mux::Workspace;

const TAB_LABEL_MAX_CHARS: usize = 48;

pub fn tabs_scroll_id() -> Id {
    Id::from("shell-tabs-scroll")
}

/// Scroll the horizontal tab strip so the active tab is in view.
pub fn snap_active_tab_scroll_task(ws: &Workspace) -> Task<Message> {
    let n = ws.tabs.len();
    if n <= 1 {
        return Task::none();
    }
    let i = ws.active_tab_index.min(n - 1);
    let x = if n <= 1 {
        0.0
    } else {
        (i as f32 / (n - 1) as f32).clamp(0.0, 1.0)
    };
    operation::snap_to(
        tabs_scroll_id(),
        RelativeOffset { x, y: 0.0 },
    )
}

fn truncate_label(s: &str) -> String {
    if s.chars().count() <= TAB_LABEL_MAX_CHARS {
        s.to_string()
    } else {
        let t: String = s.chars().take(TAB_LABEL_MAX_CHARS - 1).collect();
        format!("{t}\u{2026}")
    }
}

/// Match × / + hit area and cap height with tab label buttons (padding [6, 8]).
const TAB_ICON_BTN_PAD: Padding = Padding {
    top: 6.0,
    right: 8.0,
    bottom: 6.0,
    left: 8.0,
};

pub fn view<'a>(ws: &'a Workspace) -> Element<'a, Message> {
    let mut tabs_row = row![]
        .spacing(2)
        .padding(Padding::from([4.0, 6.0]))
        .align_y(iced::Alignment::Center);

    for (i, tab) in ws.tabs.iter().enumerate() {
        let is_active = i == ws.active_tab_index;
        let tab_id = tab.id;
        let label = truncate_label(&tab.label(i));

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
            let close_btn = button(text("\u{00D7}").size(14).color(theme::FG_DIM))
                .on_press(Message::CloseTab(tab_id))
                .padding(TAB_ICON_BTN_PAD)
                .style(|_t: &Theme, _status| button::Style {
                    background: Some(Color::TRANSPARENT.into()),
                    text_color: theme::FG_DIM,
                    border: Border::default(),
                    ..Default::default()
                });

            tabs_row = tabs_row.push(
                container(
                    row![label_btn, close_btn]
                        .spacing(0)
                        .align_y(iced::Alignment::Center),
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
                        ..Default::default()
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
        .padding(TAB_ICON_BTN_PAD)
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

    let tabs_scroll = scrollable(tabs_row)
        .id(tabs_scroll_id())
        .horizontal()
        .width(Fill)
        .height(Length::Shrink);

    container(tabs_scroll)
        .width(Fill)
        .style(|_t: &Theme| container::Style {
            background: Some(theme::BG_SIDEBAR.into()),
            ..Default::default()
        })
        .into()
}

use crate::app::Message;
use crate::notifications::NotificationManager;
use crate::theme;
use iced::widget::{column, container, scrollable, text, Column};
use iced::{Border, Element, Fill, Length, Padding, Theme};

pub fn view(manager: &NotificationManager) -> Element<'_, Message> {
    let mut col = Column::new().spacing(8).padding(Padding::from([10.0, 12.0]));

    let items: Vec<_> = manager.iter_chronological().collect();
    if items.is_empty() {
        col = col.push(
            text(
                "No notifications yet. Apps can send them via terminal OSC 9 \
                 or iTerm-style OSC 777; the panel lists them here and may show a Windows toast.",
            )
            .size(12)
            .color(theme::FG_DIM),
        );
    } else {
        for n in items {
            let title = text(&n.title)
                .size(13)
                .color(theme::FG_PRIMARY);
            let body = text(&n.body).size(12).color(theme::FG_DIM);
            let mut block = column![title, body].spacing(4);
            if let Some(ref st) = n.subtitle {
                block = block.push(
                    text(st).size(11).color(theme::ACCENT_GREEN),
                );
            }
            col = col.push(
                container(block)
                    .padding(Padding::from([8.0, 10.0]))
                    .width(Fill)
                    .style(|_t: &Theme| container::Style {
                        background: Some(theme::BG_SURFACE.into()),
                        border: Border {
                            radius: 6.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            );
        }
    }

    let body = scrollable(col).width(Fill).height(Fill);

    container(
        column![
            text("Notifications")
                .size(14)
                .color(theme::FG_PRIMARY),
            body,
        ]
        .spacing(8)
        .padding(Padding::from([12.0, 10.0])),
    )
    .width(Length::Fixed(280.0))
    .height(Fill)
    .style(|_t: &Theme| container::Style {
        background: Some(theme::BG_SIDEBAR.into()),
        border: Border {
            color: theme::BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

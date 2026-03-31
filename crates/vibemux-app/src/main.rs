mod app;
mod box_drawing;
mod command_palette;
mod find_bar;
mod git_info;
mod notification_panel;
mod notifications;
mod pty_stream;
mod resize_layout;
mod session;
mod sidebar;
mod term_selection;
mod split_view;
mod tab_bar;
mod term_view;
mod theme;

use app::VibeMux;

fn main() -> iced::Result {
    env_logger::init();

    iced::application(VibeMux::new, VibeMux::update, VibeMux::view)
        .title(VibeMux::title)
        .theme(VibeMux::theme)
        .subscription(VibeMux::subscription)
        .window(
            iced::window::Settings {
                size: iced::Size::new(1200.0, 800.0),
                min_size: Some(iced::Size::new(480.0, 320.0)),
                ..Default::default()
            },
        )
        .antialiasing(true)
        .run()
}

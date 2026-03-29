mod app;
mod command_palette;
mod find_bar;
mod git_info;
mod notifications;
mod pty_stream;
mod session;
mod sidebar;
mod split_view;
mod term_view;
mod theme;

use app::VibeMux;

fn main() -> iced::Result {
    env_logger::init();

    iced::application(VibeMux::new, VibeMux::update, VibeMux::view)
        .title(VibeMux::title)
        .theme(VibeMux::theme)
        .subscription(VibeMux::subscription)
        .window_size((1200.0, 800.0))
        .antialiasing(true)
        .run()
}

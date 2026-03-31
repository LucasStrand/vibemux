use crate::app::Message;
use crate::theme;
use iced::widget::{button, container, row, text, text_input};
use iced::{Border, Element, Fill, Length, Padding, Theme};

pub struct FindBar {
    pub visible: bool,
    pub query: String,
    pub match_count: usize,
    pub current_match: usize,
    pub matches: Vec<(usize, usize)>,
}

impl FindBar {
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            match_count: 0,
            current_match: 0,
            matches: Vec::new(),
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            self.query.clear();
            self.match_count = 0;
            self.current_match = 0;
            self.matches.clear();
        }
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    pub fn next_match(&mut self) {
        if self.match_count > 0 {
            self.current_match = (self.current_match + 1) % self.match_count;
        }
    }

    pub fn prev_match(&mut self) {
        if self.match_count > 0 {
            self.current_match = if self.current_match == 0 {
                self.match_count - 1
            } else {
                self.current_match - 1
            };
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let input = text_input("Find in terminal...", &self.query)
            .on_input(Message::FindBarInput)
            .on_submit(Message::FindBarNext)
            .size(13)
            .padding(Padding::from([6.0, 10.0]))
            .width(Length::Fixed(300.0));

        let count_text = if self.match_count > 0 {
            text(format!("{}/{}", self.current_match + 1, self.match_count))
                .size(12)
                .color(theme::FG_DIM)
        } else if !self.query.is_empty() {
            text("No results").size(12).color(theme::FG_DIM)
        } else {
            text("").size(12)
        };

        let prev_btn = button(text("\u{2191}").size(12))
            .on_press(Message::FindBarPrev)
            .padding(Padding::from([4.0, 8.0]))
            .style(|_t: &Theme, _s| button::Style {
                background: Some(theme::BG_SURFACE.into()),
                text_color: theme::FG_PRIMARY,
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            });

        let next_btn = button(text("\u{2193}").size(12))
            .on_press(Message::FindBarNext)
            .padding(Padding::from([4.0, 8.0]))
            .style(|_t: &Theme, _s| button::Style {
                background: Some(theme::BG_SURFACE.into()),
                text_color: theme::FG_PRIMARY,
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            });

        let bar = row![input, prev_btn, next_btn, count_text]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .padding(Padding::from([6.0, 12.0]));

        container(bar)
            .width(Fill)
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
}

pub fn search_grid(
    grid: &vibemux_term::grid::TerminalGrid,
    query: &str,
) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return vec![];
    }

    let query_lower = query.to_lowercase();
    let mut matches = vec![];

    for row_idx in 0..grid.display_line_count() {
        let Some(row) = grid.display_line_cells(row_idx) else {
            continue;
        };
        let line: String = row.iter().map(|c| c.c).collect();
        let line_lower = line.to_lowercase();
        let mut start = 0;
        while let Some(pos) = line_lower[start..].find(&query_lower) {
            matches.push((row_idx, start + pos));
            start += pos + 1;
        }
    }

    matches
}

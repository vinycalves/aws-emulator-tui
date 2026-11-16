//! Colors and shared styles.

use ratatui::style::{Color, Modifier, Style};

pub const BG: Color = Color::Rgb(14, 16, 20);
pub const PANEL: Color = Color::Rgb(22, 25, 31);
pub const PANEL_ALT: Color = Color::Rgb(28, 32, 40);
pub const BORDER: Color = Color::Rgb(52, 58, 70);
pub const BORDER_FOCUS: Color = Color::Rgb(255, 153, 0);
pub const TEXT: Color = Color::Rgb(222, 226, 232);
pub const MUTED: Color = Color::Rgb(132, 140, 152);
pub const DIM: Color = Color::Rgb(92, 99, 110);
pub const ORANGE: Color = Color::Rgb(255, 153, 0);
pub const GREEN: Color = Color::Rgb(88, 196, 124);
pub const RED: Color = Color::Rgb(238, 96, 96);
pub const YELLOW: Color = Color::Rgb(232, 190, 84);
pub const BLUE: Color = Color::Rgb(108, 170, 255);
pub const PURPLE: Color = Color::Rgb(176, 140, 255);

pub fn base() -> Style {
    Style::default().fg(TEXT).bg(BG)
}

pub fn panel() -> Style {
    Style::default().fg(TEXT).bg(PANEL)
}

pub fn muted() -> Style {
    Style::default().fg(MUTED)
}

pub fn title() -> Style {
    Style::default().fg(ORANGE).add_modifier(Modifier::BOLD)
}

pub fn selected() -> Style {
    Style::default()
        .fg(Color::Rgb(20, 22, 26))
        .bg(ORANGE)
        .add_modifier(Modifier::BOLD)
}

pub fn key() -> Style {
    Style::default().fg(YELLOW)
}

pub fn ok() -> Style {
    Style::default().fg(GREEN)
}

pub fn err() -> Style {
    Style::default().fg(RED)
}

use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;

use super::model::RowTone;

pub(crate) const BORDER_SET: border::Set = border::ROUNDED;

const SCREEN_BACKGROUND: Color = Color::Rgb(10, 14, 18);
const PANEL_BACKGROUND: Color = Color::Rgb(13, 18, 24);
const MUTED_FOREGROUND: Color = Color::Rgb(126, 141, 156);
const DEFAULT_FOREGROUND: Color = Color::Rgb(220, 226, 232);
const ACCENT: Color = Color::Rgb(93, 214, 187);
const GOOD: Color = Color::Rgb(118, 213, 137);
const WARNING: Color = Color::Rgb(231, 193, 95);
const DANGER: Color = Color::Rgb(234, 116, 116);
const SELECTED_FOREGROUND: Color = Color::Rgb(5, 9, 12);

pub(crate) fn screen() -> Style {
    Style::default()
        .fg(DEFAULT_FOREGROUND)
        .bg(SCREEN_BACKGROUND)
}

pub(crate) fn panel() -> Style {
    Style::default().fg(DEFAULT_FOREGROUND).bg(PANEL_BACKGROUND)
}

pub(crate) fn panel_border() -> Style {
    Style::default().fg(Color::Rgb(47, 60, 72))
}

pub(crate) fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub(crate) fn tab() -> Style {
    Style::default().fg(MUTED_FOREGROUND)
}

pub(crate) fn selected_tab() -> Style {
    Style::default()
        .fg(SELECTED_FOREGROUND)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn footer() -> Style {
    Style::default().fg(MUTED_FOREGROUND).bg(SCREEN_BACKGROUND)
}

pub(crate) fn row(tone: RowTone) -> Style {
    match tone {
        RowTone::Default => Style::default().fg(DEFAULT_FOREGROUND),
        RowTone::Section => Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        RowTone::Muted => Style::default().fg(MUTED_FOREGROUND),
        RowTone::Good => Style::default().fg(GOOD),
        RowTone::Warning => Style::default().fg(WARNING),
        RowTone::Danger => Style::default().fg(DANGER),
        RowTone::Accent => Style::default().fg(ACCENT),
    }
}

pub(crate) fn selected_row() -> Style {
    Style::default()
        .fg(SELECTED_FOREGROUND)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub(crate) fn hovered_row(tone: RowTone) -> Style {
    row(tone).add_modifier(Modifier::UNDERLINED)
}

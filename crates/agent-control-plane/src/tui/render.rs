use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs};

use super::geometry::{FOOTER_HEIGHT, TAB_BAR_HEIGHT};
use super::model::{PanelModel, RenderRow};
use super::state::TuiState;
use super::tabs::TuiTab;
use super::theme;

pub(crate) fn render(frame: &mut ratatui::Frame<'_>, app: &mut TuiState) {
    frame.render_widget(Block::default().style(theme::screen()), frame.area());
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(TAB_BAR_HEIGHT),
            Constraint::Min(0),
            Constraint::Length(FOOTER_HEIGHT),
        ])
        .split(frame.area());
    app.remember_geometry(chunks[0], chunks[1], chunks[2]);

    frame.render_widget(tabs(app), chunks[0]);
    frame.render_widget(panel(app.panel_model()), chunks[1]);
    frame.render_widget(footer(app), chunks[2]);
}

fn tabs(app: &TuiState) -> Tabs<'static> {
    let titles = TuiTab::ALL.iter().map(|tab| Line::from(tab.title()));
    Tabs::new(titles)
        .select(app.tab_index())
        .style(theme::tab())
        .highlight_style(theme::selected_tab())
        .block(shell_block("looper"))
}

fn panel(model: PanelModel) -> List<'static> {
    List::new(model.rows.into_iter().map(row_item).collect::<Vec<_>>())
        .block(shell_block(model.title))
        .style(theme::panel())
}

fn row_item(row: RenderRow) -> ListItem<'static> {
    let prefix = if row.selected {
        "> "
    } else if row.hovered {
        "* "
    } else {
        "  "
    };
    let style = if row.selected {
        theme::selected_row()
    } else if row.hovered {
        theme::hovered_row(row.tone)
    } else {
        theme::row(row.tone)
    };
    ListItem::new(format!("{prefix}{}", row.text)).style(style)
}

fn footer(app: &TuiState) -> Paragraph<'static> {
    Paragraph::new(app.footer_text())
        .style(theme::footer())
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(theme::panel_border()),
        )
}

fn shell_block(title: &'static str) -> Block<'static> {
    Block::default()
        .title(Line::styled(format!(" {title} "), theme::title()))
        .borders(Borders::ALL)
        .border_set(theme::BORDER_SET)
        .border_style(theme::panel_border())
        .style(theme::panel())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_rows_are_prefixed_for_scanability() {
        let row = row_item(RenderRow::selectable(true, false, "thread"));

        assert_eq!(row.height(), 1);
    }
}

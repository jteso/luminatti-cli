use super::super::{App, Focus};
use crate::diff_view::SELECTION_BACKGROUND;
use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

pub(super) fn accent_style() -> Style {
    Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD)
}

pub(super) fn selected_row_style() -> Style {
    accent_style().bg(SELECTION_BACKGROUND)
}

pub(super) fn rounded_block<'a>() -> Block<'a> {
    Block::default().border_type(BorderType::Rounded)
}

fn scrollbar_position(content_length: usize, viewport_length: usize, offset: usize) -> usize {
    let max_offset = content_length.saturating_sub(viewport_length);
    if max_offset == 0 {
        return 0;
    }
    offset
        .min(max_offset)
        .saturating_mul(content_length.saturating_sub(1))
        / max_offset
}

pub(super) fn render_vertical_scrollbar(
    frame: &mut ratatui::Frame,
    area: Rect,
    content_length: usize,
    viewport_length: usize,
    offset: usize,
    visible: bool,
) {
    if !visible
        || content_length <= viewport_length
        || viewport_length == 0
        || area.width == 0
        || area.height <= 2
    {
        return;
    }
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .track_style(Style::default().fg(Color::DarkGray))
        .thumb_symbol("┃")
        .thumb_style(accent_style());
    let mut state = ScrollbarState::new(content_length)
        .position(scrollbar_position(content_length, viewport_length, offset))
        .viewport_content_length(viewport_length);
    frame.render_stateful_widget(
        scrollbar,
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut state,
    );
}

pub(super) fn render_horizontal_scrollbar(
    frame: &mut ratatui::Frame,
    area: Rect,
    content_length: usize,
    viewport_length: usize,
    offset: usize,
    visible: bool,
) {
    if !visible || content_length <= viewport_length || viewport_length == 0 || area.width == 0 {
        return;
    }
    let scrollbar = Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("─"))
        .track_style(Style::default().fg(Color::DarkGray))
        .thumb_symbol("━")
        .thumb_style(accent_style());
    let mut state = ScrollbarState::new(content_length)
        .position(scrollbar_position(content_length, viewport_length, offset))
        .viewport_content_length(viewport_length);
    frame.render_stateful_widget(scrollbar, area, &mut state);
}

pub(super) fn single_panel_block(
    panel: &'static str,
    title: &'static str,
    focused: bool,
) -> Block<'static> {
    let muted = Style::default().fg(Color::DarkGray);
    rounded_block()
        .borders(Borders::ALL)
        .border_style(if focused { accent_style() } else { muted })
        .title(Line::from(vec![
            Span::styled(
                format!(" {panel} "),
                if focused { accent_style() } else { muted },
            ),
            Span::styled(format!("{title} "), accent_style()),
        ]))
}

pub(super) fn panel_block(
    panel: &'static str,
    first: &'static str,
    first_active: bool,
    second: &'static str,
    second_active: bool,
    focused: bool,
) -> Block<'static> {
    let panel_style = if focused {
        accent_style()
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let muted_style = Style::default().fg(Color::DarkGray);
    rounded_block()
        .borders(Borders::ALL)
        .border_style(if focused { accent_style() } else { muted_style })
        .title(Line::from(vec![
            Span::styled(format!(" {panel} "), panel_style),
            Span::styled(
                first,
                if first_active {
                    accent_style()
                } else {
                    muted_style
                },
            ),
            Span::styled(" - ", muted_style),
            Span::styled(
                format!("{second} "),
                if second_active {
                    accent_style()
                } else {
                    muted_style
                },
            ),
        ]))
}

pub(super) fn right_panel_block(
    app: &App,
    changes_active: bool,
    comments_active: bool,
) -> Block<'static> {
    let path = app.active_path().unwrap_or("No changed file");
    let mut block = panel_block(
        "[2]",
        "Changes",
        changes_active,
        "Comments",
        comments_active,
        !app.show_help && app.focus == Focus::Right,
    );
    if app.final_view && changes_active {
        block = block.title_bottom(Line::from(Span::styled(
            " Final · [h] show diff ",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if let Some(title) = right_panel_path_title(path, comments_active) {
        block.title(title)
    } else {
        block
    }
}

fn right_panel_path_title(path: &str, comments_active: bool) -> Option<Line<'static>> {
    (!comments_active).then(|| path_title(path).alignment(Alignment::Right))
}

fn path_title(path: &str) -> Line<'static> {
    let (directory, filename) = path
        .rsplit_once('/')
        .map_or((String::new(), path.to_owned()), |(directory, filename)| {
            (format!("{directory}/"), filename.to_owned())
        });

    Line::from(vec![
        Span::styled(
            format!(" {directory}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(format!("{filename} "), Style::default().fg(Color::Gray)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_title_mutes_the_directory_and_softens_the_filename() {
        let line = path_title("packages/business/src/NetsuiteConnector.ts");

        assert_eq!(line.spans[0].content, " packages/business/src/");
        assert_eq!(line.spans[0].style.fg, Some(Color::DarkGray));
        assert_eq!(line.spans[1].content, "NetsuiteConnector.ts ");
        assert_eq!(line.spans[1].style.fg, Some(Color::Gray));
    }

    #[test]
    fn comments_tab_hides_the_file_path_title() {
        assert!(right_panel_path_title("src/main.rs", true).is_none());
        assert!(right_panel_path_title("src/main.rs", false).is_some());
    }

    #[test]
    fn scrollbar_position_reaches_both_ends_of_overflowing_content() {
        assert_eq!(scrollbar_position(10, 4, 0), 0);
        assert_eq!(scrollbar_position(10, 4, 6), 9);
        assert_eq!(scrollbar_position(4, 4, 0), 0);
    }
}

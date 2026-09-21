use super::super::{App, Focus};
use crate::diff_view::SELECTION_BACKGROUND;
use ratatui::{
    layout::{Alignment, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(super) fn accent_style() -> Style {
    Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD)
}

pub(super) fn selected_row_style() -> Style {
    accent_style().bg(SELECTION_BACKGROUND)
}

/// Selection background without recoloring the row's own foreground.
pub(super) fn selected_row_background() -> Style {
    Style::default().bg(SELECTION_BACKGROUND)
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
    width: u16,
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
    let counts = changed_line_counts(&app.diff_rows);
    if let Some(title) = right_panel_path_title(path, counts, comments_active, width) {
        block.title(title)
    } else {
        block
    }
}

fn right_panel_path_title(
    path: &str,
    counts: (usize, usize),
    comments_active: bool,
    panel_width: u16,
) -> Option<Line<'static>> {
    (!comments_active).then(|| {
        let tabs_width = " [2] Changes - Comments ".width();
        let title_width = (panel_width.saturating_sub(2) as usize)
            .saturating_sub(tabs_width)
            .saturating_sub(1);
        path_title(path, counts, title_width).alignment(Alignment::Right)
    })
}

fn changed_line_counts(rows: &[crate::diff::DiffRow]) -> (usize, usize) {
    let additions = rows
        .iter()
        .filter(|row| row.new_line.is_some() && row.new_changed)
        .count();
    let deletions = rows
        .iter()
        .filter(|row| row.old_line.is_some() && (row.old_changed || row.new_line.is_none()))
        .count();
    (additions, deletions)
}

fn path_title(
    path: &str,
    (additions, deletions): (usize, usize),
    max_width: usize,
) -> Line<'static> {
    let addition = format!(" [+{additions}]");
    let deletion = format!(" [-{deletions}]");
    let path_width = max_width
        .saturating_sub(addition.width())
        .saturating_sub(deletion.width())
        .saturating_sub(2);
    let path = abbreviate_path(path, path_width);
    let (directory, filename) = path
        .rsplit_once('/')
        .map_or((String::new(), path.clone()), |(directory, filename)| {
            (format!("{directory}/"), filename.to_owned())
        });

    Line::from(vec![
        Span::styled(
            format!(" {directory}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(filename, Style::default().fg(Color::Gray)),
        Span::styled(addition, Style::default().fg(Color::LightGreen)),
        Span::styled(deletion, Style::default().fg(Color::LightRed)),
        Span::raw(" "),
    ])
}

fn abbreviate_path(path: &str, max_width: usize) -> String {
    if path.width() <= max_width {
        return path.to_owned();
    }

    let components = path.split('/').collect::<Vec<_>>();
    for (leading, trailing_directories) in [(2, 2), (1, 2), (1, 1), (0, 1), (0, 0)] {
        if let Some(candidate) = abbreviated_components(&components, leading, trailing_directories)
            && candidate.width() <= max_width
        {
            return candidate;
        }
    }

    truncate_path_end(components.last().copied().unwrap_or(path), max_width)
}

fn abbreviated_components(
    components: &[&str],
    leading: usize,
    trailing_directories: usize,
) -> Option<String> {
    let suffix_length = trailing_directories + 1;
    (leading + suffix_length < components.len()).then(|| {
        let mut visible = components[..leading].to_vec();
        visible.push("...");
        visible.extend_from_slice(&components[components.len() - suffix_length..]);
        visible.join("/")
    })
}

fn truncate_path_end(path: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if path.width() <= max_width {
        return path.to_owned();
    }

    let suffix_width = max_width.saturating_sub(1);
    let mut width = 0;
    let mut start = path.len();
    for (index, character) in path.char_indices().rev() {
        let character_width = character.width().unwrap_or(0);
        if width + character_width > suffix_width {
            break;
        }
        width += character_width;
        start = index;
    }
    format!("…{}", &path[start..])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_title_mutes_the_directory_and_softens_the_filename() {
        let line = path_title("packages/business/src/NetsuiteConnector.ts", (15, 11), 80);

        assert_eq!(line.spans[0].content, " packages/business/src/");
        assert_eq!(line.spans[0].style.fg, Some(Color::DarkGray));
        assert_eq!(line.spans[1].content, "NetsuiteConnector.ts");
        assert_eq!(line.spans[1].style.fg, Some(Color::Gray));
        assert_eq!(line.spans[2].content, " [+15]");
        assert_eq!(line.spans[2].style.fg, Some(Color::LightGreen));
        assert_eq!(line.spans[3].content, " [-11]");
        assert_eq!(line.spans[3].style.fg, Some(Color::LightRed));
    }

    #[test]
    fn comments_tab_hides_the_file_path_title() {
        assert!(right_panel_path_title("src/main.rs", (1, 4), true, 80).is_none());
        assert!(right_panel_path_title("src/main.rs", (1, 4), false, 80).is_some());
    }

    #[test]
    fn long_paths_keep_their_outer_directories_and_filename() {
        let path = "org/team/packages/business/generated/client/src/NetsuiteConnector.ts";
        let expected = "org/team/.../client/src/NetsuiteConnector.ts";

        assert_eq!(abbreviate_path(path, expected.width()), expected);
    }

    #[test]
    fn changed_line_counts_include_both_sides_of_replacements() {
        let rows =
            crate::diff::line_diff_document("same\nremoved\n", "same\nreplacement\nadditional\n")
                .rows;

        assert_eq!(changed_line_counts(&rows), (2, 1));
    }

    #[test]
    fn scrollbar_position_reaches_both_ends_of_overflowing_content() {
        assert_eq!(scrollbar_position(10, 4, 0), 0);
        assert_eq!(scrollbar_position(10, 4, 6), 9);
        assert_eq!(scrollbar_position(4, 4, 0), 0);
    }
}

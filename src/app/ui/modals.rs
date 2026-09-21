use super::super::{App, Input, InputKind, cursor_blink_visible};
use super::widgets::{
    accent_style, render_vertical_scrollbar, rounded_block, selected_row_background,
};
use crate::search::ranked_matches;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Borders, Clear, List, ListItem, Paragraph},
};

fn help_binding(key: &'static str, description: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:>16}  "), Style::default().fg(Color::Cyan)),
        Span::styled(description, Style::default().fg(Color::Gray)),
    ])
}

fn help_heading(title: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("                  "),
        Span::styled(title, Style::default().fg(Color::Magenta)),
    ])
}

fn modal_actions(actions: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (index, (key, label)) in actions.iter().enumerate() {
        spans.push(Span::styled(format!("[{key}]"), accent_style()));
        spans.push(Span::styled(
            format!(
                " {label}{}",
                if index + 1 == actions.len() {
                    " "
                } else {
                    "   "
                }
            ),
            Style::default().fg(Color::Gray),
        ));
    }
    Line::from(spans).alignment(Alignment::Right)
}

pub(super) fn draw_help(frame: &mut ratatui::Frame, area: Rect) {
    let width = 84.min(area.width.saturating_sub(2)).max(1);
    let height = 38.min(area.height.saturating_sub(1)).max(1);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let lines = vec![
        help_heading("GLOBAL"),
        help_binding("1 / 2 / 3", "focus panel; repeat to maximize / restore"),
        help_binding("Tab", "cycle tabs within the focused panel"),
        help_binding(">", "increase panel width"),
        help_binding("<", "reduce panel width"),
        help_binding("}", "jump to next file"),
        help_binding("{", "jump to previous file"),
        help_binding("/", "find a changed file"),
        help_binding("r", "refresh"),
        help_binding("q", "quit"),
        Line::default(),
        help_heading("FILES"),
        help_binding("↑ / ↓", "move selection"),
        help_binding("Enter", "open file or fold directory"),
        Line::default(),
        help_heading("CHANGES"),
        help_binding("↑ / ↓", "move through changed lines"),
        help_binding("← / →", "scroll code horizontally"),
        help_binding("]", "jump to next hunk"),
        help_binding("[", "jump to previous hunk"),
        help_binding("s", "show split view"),
        help_binding("u", "show unified view"),
        help_binding("h", "toggle final version in unified view"),
        help_binding("i", "show or hide common lines"),
        help_binding("c", "add review comment"),
        help_binding("Esc", "clear line selection"),
        Line::default(),
        help_heading("COMMENTS"),
        help_binding("y", "copy selected comment JSON"),
        help_binding("d", "remove comment"),
        help_binding("D", "remove all comments"),
        Line::default(),
        help_heading("FILTERS"),
        help_binding("a / x", "add / remove filter"),
        help_binding("↑ / ↓", "move selection"),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            rounded_block()
                .borders(Borders::ALL)
                .border_style(accent_style())
                .title(Line::from(Span::styled(" Keybindings ", accent_style())))
                .title_bottom(modal_actions(&[("ESC", "Close")])),
        ),
        popup,
    );
}
pub(super) fn draw_input(frame: &mut ratatui::Frame, input: &Input, area: Rect) {
    let popup = centered_popup(area, area.width.saturating_mul(3) / 4, 5);
    frame.render_widget(Clear, popup);
    let (title, save_label) = match input.kind {
        InputKind::Comment => (" Add Comment... ", "Save comment"),
        InputKind::Filter => (" Exclude Glob... ", "Save filter"),
        InputKind::FileSearch => unreachable!("file search has its own dialog"),
    };
    let mut spans = vec![Span::raw(" "), Span::raw(input.value.clone())];
    if cursor_blink_visible() {
        spans.push(Span::styled(
            " ",
            Style::default()
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(
            rounded_block()
                .borders(Borders::ALL)
                .border_style(accent_style())
                .title(Line::from(Span::styled(title, accent_style())))
                .title_bottom(modal_actions(&[("ENTER", save_label), ("ESC", "Cancel")])),
        ),
        popup,
    );
}

pub(super) fn draw_delete_all_confirmation(frame: &mut ratatui::Frame, area: Rect) {
    let popup = centered_popup(area, 52, 5);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::default(),
            Line::from("Are you sure you want to delete all comments?"),
        ])
        .alignment(Alignment::Center)
        .block(
            rounded_block()
                .borders(Borders::ALL)
                .border_style(accent_style())
                .title(Line::from(Span::styled(
                    " Confirmation Required ",
                    accent_style(),
                )))
                .title_bottom(modal_actions(&[("ENTER", "Confirm"), ("ESC", "Close")])),
        ),
        popup,
    );
}

/// Builds a path line with the matched characters accented.
fn file_path_line(path: &str, positions: &[usize]) -> Line<'static> {
    let matched = positions
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut spans = vec![Span::raw("  ")];
    for (char_index, character) in path.chars().enumerate() {
        let style = if matched.contains(&char_index) {
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(character.to_string(), style));
    }
    Line::from(spans)
}

fn centered_popup(area: Rect, requested_width: u16, requested_height: u16) -> Rect {
    let width = requested_width.min(area.width.saturating_sub(2)).max(1);
    let height = requested_height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub(super) fn draw_file_search(frame: &mut ratatui::Frame, app: &App, input: &Input, area: Rect) {
    let match_count = ranked_matches(&app.files, &input.value).len();
    let height = (match_count.min(11) as u16).saturating_add(3).max(5);
    let popup = centered_popup(area, area.width.saturating_mul(3) / 4, height);
    frame.render_widget(Clear, popup);

    let block = rounded_block()
        .borders(Borders::ALL)
        .border_style(accent_style())
        .title(Line::from(Span::styled(" Open File... ", accent_style())))
        .title_bottom(modal_actions(&[("ENTER", "Open"), ("ESC", "Close")]));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.height == 0 {
        return;
    }
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    let mut prompt = vec![Span::raw(" "), Span::raw(input.value.clone())];
    if cursor_blink_visible() {
        prompt.push(Span::styled(
            " ",
            Style::default()
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(prompt)), sections[0]);
    let matches = ranked_matches(&app.files, &input.value);
    let selected = (!matches.is_empty()).then(|| input.selected.min(matches.len() - 1));
    let items = if matches.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No matching files",
            Style::default().fg(Color::DarkGray),
        )))]
    } else {
        matches
            .iter()
            .map(|(index, positions)| {
                ListItem::new(file_path_line(&app.files[*index].path, positions))
            })
            .collect::<Vec<_>>()
    };
    let mut state = ratatui::widgets::ListState::default();
    state.select(selected);
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(selected_row_background())
            .highlight_symbol("›"),
        sections[1],
        &mut state,
    );
    render_vertical_scrollbar(
        frame,
        popup,
        match_count,
        sections[1].height as usize,
        state.offset(),
        app.scrollbars_visible(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::preview_app;
    use crate::git::FileItem;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn keybindings_overlay_renders_as_a_grouped_command_palette() {
        let backend = TestBackend::new(100, 45);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| draw_help(frame, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let contents = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(contents.contains("Keybindings"));
        assert!(contents.contains("                  GLOBAL"));
        assert!(contents.contains("                  FILES"));
        assert!(contents.contains("                  CHANGES"));
        assert!(contents.contains("                  COMMENTS"));
        assert!(contents.contains("                  FILTERS"));
        assert!(!contents.contains("GLOBAL ─"));
        assert!(!contents.contains("FILES ─"));
        assert!(!contents.contains("CHANGES ─"));
        assert!(!contents.contains("COMMENTS ─"));
        assert!(!contents.contains("FILTERS ─"));
        assert!(contents.contains(">  increase panel width"));
        assert!(contents.contains("<  reduce panel width"));
        assert!(contents.contains("}  jump to next file"));
        assert!(contents.contains("{  jump to previous file"));
        assert!(contents.contains("]  jump to next hunk"));
        assert!(contents.contains("[  jump to previous hunk"));
        assert!(!contents.contains("[ / ]"));
        assert!(contents.contains("s  show split view"));
        assert!(contents.contains("u  show unified view"));
        assert!(contents.contains("h  toggle final version in unified view"));
        assert!(contents.contains("Esc  clear line selection"));
        assert!(contents.contains("d  remove comment"));
        assert!(contents.contains("D  remove all comments"));
        assert!(!contents.contains("f / l"));
        assert!(!contents.contains("? / Esc"));
        assert!(!contents.contains("cycle changes / comments"));
        assert!(!contents.contains("cycle filters / ignored"));
        assert!(!contents.contains("[1] FILES"));
        assert!(!contents.contains("[2] CHANGES"));
        assert!(!contents.contains("[2] COMMENTS"));
        assert!(!contents.contains("[3] FILTERS / IGNORED"));

        let popup = Rect::new(8, 3, 84, 38);
        assert_eq!(buffer[(popup.x, popup.y)].fg, Color::Magenta);
        let description_column = popup.x + 1 + 18;
        assert_eq!(buffer[(description_column, popup.y + 1)].symbol(), "G");
        assert_eq!(buffer[(description_column, popup.y + 1)].fg, Color::Magenta);
        assert_eq!(buffer[(description_column, popup.y + 2)].symbol(), "f");
        let bottom_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y + popup.height - 1)].symbol())
            .collect::<String>();
        assert!(bottom_border.contains("[ESC] Close"));
        assert!(bottom_border.ends_with(" [ESC] Close ╯"));
        let esc_column = popup.x + popup.width - 13;
        assert_eq!(
            buffer[(esc_column, popup.y + popup.height - 1)].fg,
            Color::Magenta
        );
    }

    #[test]
    fn delete_all_confirmation_shows_confirm_and_close_actions() {
        let backend = TestBackend::new(60, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| draw_delete_all_confirmation(frame, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let contents = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(contents.contains("Confirmation Required"));
        assert!(contents.contains("Are you sure you want to delete all comments?"));

        let popup = centered_popup(buffer.area, 52, 5);
        assert_eq!(buffer[(popup.x, popup.y)].fg, Color::Magenta);
        assert_eq!(buffer[(popup.x + 2, popup.y)].fg, Color::Magenta);
        let bottom_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y + popup.height - 1)].symbol())
            .collect::<String>();
        assert!(bottom_border.ends_with(" [ENTER] Confirm   [ESC] Close ╯"));
    }

    #[test]
    fn text_entry_modals_use_consistent_titles_and_bottom_actions() {
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        let comment = Input {
            kind: InputKind::Comment,
            value: "Looks good".into(),
            selected: 0,
        };

        terminal
            .draw(|frame| draw_input(frame, &comment, frame.area()))
            .unwrap();

        let popup = centered_popup(terminal.backend().buffer().area, 60, 5);
        let buffer = terminal.backend().buffer();
        let top_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y)].symbol())
            .collect::<String>();
        let bottom_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y + popup.height - 1)].symbol())
            .collect::<String>();
        assert!(top_border.contains("Add Comment..."));
        assert!(bottom_border.ends_with(" [ENTER] Save comment   [ESC] Cancel ╯"));
        assert_eq!(buffer[(popup.x, popup.y)].fg, Color::Magenta);

        let filter = Input {
            kind: InputKind::Filter,
            value: "target/**".into(),
            selected: 0,
        };
        terminal
            .draw(|frame| draw_input(frame, &filter, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let top_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y)].symbol())
            .collect::<String>();
        let bottom_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y + popup.height - 1)].symbol())
            .collect::<String>();
        assert!(top_border.contains("Exclude Glob..."));
        assert!(bottom_border.ends_with(" [ENTER] Save filter   [ESC] Cancel ╯"));
    }

    #[test]
    fn file_search_renders_title_and_actions_on_opposite_borders() {
        let mut app = preview_app("before\n", "after\n");
        app.files = vec![FileItem {
            path: "src/main.rs".into(),
            status: " M".into(),
        }];
        let input = Input {
            kind: InputKind::FileSearch,
            value: String::new(),
            selected: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();

        terminal
            .draw(|frame| draw_file_search(frame, &app, &input, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let popup = Rect::new(12, 7, 75, 5);
        let top_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y)].symbol())
            .collect::<String>();
        let bottom_border = (popup.x..popup.x + popup.width)
            .map(|x| buffer[(x, popup.y + popup.height - 1)].symbol())
            .collect::<String>();
        assert!(top_border.contains("Open File..."));
        assert!(!top_border.contains("Enter opens"));
        assert!(bottom_border.ends_with(" [ENTER] Open   [ESC] Close ╯"));
    }

    #[test]
    fn file_search_prompt_omits_slash_prefix() {
        let mut app = preview_app("before\n", "after\n");
        app.files = vec![FileItem {
            path: "src/main.rs".into(),
            status: " M".into(),
        }];
        let input = Input {
            kind: InputKind::FileSearch,
            value: "main".into(),
            selected: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();

        terminal
            .draw(|frame| draw_file_search(frame, &app, &input, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let prompt = (0..buffer.area.width)
            .map(|x| buffer[(x, 8)].symbol())
            .collect::<String>();
        assert!(prompt.contains(" main"));
        assert!(!prompt.contains("/ main"));
        let matches = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .any(|line| line.contains("src/main.rs"));
        assert!(matches);
    }

    #[test]
    fn file_search_highlights_matched_characters_in_paths() {
        let mut app = preview_app("before\n", "after\n");
        app.files = vec![FileItem {
            path: "src/main.rs".into(),
            status: " M".into(),
        }];
        let input = Input {
            kind: InputKind::FileSearch,
            value: "sr".into(),
            selected: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();

        terminal
            .draw(|frame| draw_file_search(frame, &app, &input, frame.area()))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut found = false;
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                if cell.symbol() == "s"
                    && cell.fg == Color::Magenta
                    && cell.modifier.contains(Modifier::BOLD)
                {
                    found = true;
                }
            }
        }
        assert!(found);
    }
}

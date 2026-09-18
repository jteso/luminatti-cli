use super::super::{App, FilterTab, Focus};
use super::widgets::{
    accent_style, panel_block, render_vertical_scrollbar, right_panel_block, selected_row_style,
    single_panel_block,
};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem},
};

pub(super) fn draw_files(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let tree_rows = app.file_tree_rows();
    let items: Vec<_> = tree_rows
        .iter()
        .map(|row| {
            let name = row.path.rsplit('/').next().unwrap_or(&row.path);
            let indent = "  ".repeat(row.depth);
            if let Some(file_index) = row.file_index {
                let file = &app.files[file_index];
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{indent}  ")),
                    Span::styled(
                        format!("{} ", file.status.trim()),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(name.to_owned()),
                ]))
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{indent}{} ", if row.expanded { "▼" } else { "▶" }),
                        accent_style(),
                    ),
                    Span::styled(
                        name.to_owned(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]))
            }
        })
        .collect();
    let list = List::new(items)
        .block(single_panel_block(
            "[1]",
            "Files",
            !app.show_help && app.focus == Focus::Files,
        ))
        .highlight_style(selected_row_style());
    let mut state = ratatui::widgets::ListState::default();
    state.select((!tree_rows.is_empty()).then_some(app.selected_file));
    frame.render_stateful_widget(list, area, &mut state);
    render_vertical_scrollbar(
        frame,
        area,
        tree_rows.len(),
        area.height.saturating_sub(2) as usize,
        state.offset(),
        app.scrollbars_visible(),
    );
}

pub(super) fn draw_filters(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let (items, selected) = match app.filter_tab {
        FilterTab::Filters => (
            app.filters
                .patterns
                .iter()
                .map(|pattern| ListItem::new(format!("  {pattern}")))
                .collect::<Vec<_>>(),
            (!app.filters.patterns.is_empty()).then_some(app.selected_filter),
        ),
        FilterTab::Ignored => (
            app.ignored_files
                .iter()
                .map(|file| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{} ", file.status.trim()),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(file.path.clone()),
                    ]))
                })
                .collect::<Vec<_>>(),
            (!app.ignored_files.is_empty()).then_some(app.selected_ignored),
        ),
    };
    let list = List::new(items)
        .block(panel_block(
            "[3]",
            "Filters",
            app.filter_tab == FilterTab::Filters,
            "Ignored",
            app.filter_tab == FilterTab::Ignored,
            !app.show_help && app.focus == Focus::Filters,
        ))
        .highlight_style(selected_row_style())
        .highlight_symbol("›");
    let mut state = ratatui::widgets::ListState::default();
    state.select(selected);
    frame.render_stateful_widget(list, area, &mut state);
    render_vertical_scrollbar(
        frame,
        area,
        match app.filter_tab {
            FilterTab::Filters => app.filters.patterns.len(),
            FilterTab::Ignored => app.ignored_files.len(),
        },
        area.height.saturating_sub(2) as usize,
        state.offset(),
        app.scrollbars_visible(),
    );
}

pub(super) fn draw_comments(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let comments = app.all_comments();
    let items: Vec<_> = comments
        .iter()
        .map(|c| {
            let anchor = c
                .new_line
                .map(|l| format!("new:{l}"))
                .or_else(|| c.old_line.map(|l| format!("old:{l}")))
                .unwrap_or_else(|| "hunk".into());
            ListItem::new(vec![
                Line::from(Span::styled(
                    format!("{}  {}", c.file_path, anchor),
                    accent_style(),
                )),
                Line::from(c.summary.clone()),
                Line::from(Span::styled(
                    format!(
                        "{} · {}",
                        c.source,
                        c.author.clone().unwrap_or_else(|| "anonymous".into())
                    ),
                    Style::default().fg(Color::DarkGray),
                )),
            ])
        })
        .collect();
    let list = List::new(items)
        .block(right_panel_block(app, false, true))
        .highlight_style(selected_row_style());
    let mut state = ratatui::widgets::ListState::default();
    state.select((!comments.is_empty()).then_some(app.selected_comment));
    frame.render_stateful_widget(list, area, &mut state);
    render_vertical_scrollbar(
        frame,
        area,
        comments.len(),
        area.height.saturating_sub(2).saturating_div(3).max(1) as usize,
        state.offset(),
        app.scrollbars_visible(),
    );
}

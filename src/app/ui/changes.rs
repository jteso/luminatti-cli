//! Diff panel drawing. Only the visible slice of the diff is materialized
//! into styled lines; geometry comes from the cached render state.
use super::super::{App, navigation::clamped_diff_scroll};
use super::widgets::{
    render_horizontal_scrollbar, render_vertical_scrollbar, right_panel_block, rounded_block,
};
use crate::diff_view::{
    DiffMode, RenderedDiffLine, pad_selected_line, side_line, source_prefix, split_separator_line,
    unified_occurrence_line,
};
use ratatui::{
    layout::{Constraint, Direction, Margin, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Borders, Paragraph},
};

/// The styled lines of the visible slice, per display mode.
enum VisibleContent {
    Unified(Vec<Line<'static>>),
    Split {
        old: Vec<Line<'static>>,
        new: Vec<Line<'static>>,
    },
}

pub(super) fn draw_diff(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let outer = right_panel_block(app, true, false);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let diff_area = inner;
    let state = app.render_state();
    if state.lines.is_empty() {
        let message = if app.pending_diff.is_some() && app.diff_signature.is_none() {
            "Loading diff…".to_owned()
        } else if app.final_view {
            "Final version is empty (file empty or deleted).".to_owned()
        } else if !app.diff_has_syntactic_changes && !app.diff_language.is_empty() {
            format!("No syntactic changes ({})", app.diff_language)
        } else {
            "No changed lines to display.".to_owned()
        };
        frame.render_widget(
            Paragraph::new(message).style(Style::default().fg(Color::DarkGray)),
            diff_area,
        );
        return;
    }
    let content_length = state.lines.len();
    let scroll = clamped_diff_scroll(app.diff_scroll, content_length, diff_area.height);
    let visible_end = content_length.min(scroll + diff_area.height as usize);
    let visible = &state.lines[scroll..visible_end];
    let rows = app.active_rows();

    let split_columns = (app.diff_mode == DiffMode::SideBySide).then(|| split_columns(diff_area));
    let horizontal_viewport = match (&split_columns, app.diff_mode) {
        (Some(columns), _) => columns[0].width.min(columns[1].width.saturating_sub(1)) as usize,
        (None, _) => diff_area.width as usize,
    };
    let content = match app.diff_mode {
        DiffMode::Unified => VisibleContent::Unified(
            visible
                .iter()
                .filter_map(|entry| match entry {
                    RenderedDiffLine::Row { row, occurrence } => unified_occurrence_line(
                        *row,
                        &rows[*row],
                        *occurrence,
                        app.selected_row,
                        app.diff_horizontal_scroll as usize + horizontal_viewport,
                    ),
                    RenderedDiffLine::Separator => None,
                })
                .collect(),
        ),
        DiffMode::SideBySide => {
            let mut old = Vec::with_capacity(visible.len());
            let mut new = Vec::with_capacity(visible.len());
            for line in visible {
                match line {
                    RenderedDiffLine::Row { row, .. } => {
                        let index = *row;
                        let row = &rows[index];
                        old.push(side_line(
                            index,
                            row.old_line,
                            source_prefix(
                                &row.old_text,
                                app.diff_horizontal_scroll as usize + horizontal_viewport,
                            ),
                            &row.old_spans,
                            row.old_changed,
                            true,
                            app.selected_row,
                        ));
                        new.push(side_line(
                            index,
                            row.new_line,
                            source_prefix(
                                &row.new_text,
                                app.diff_horizontal_scroll as usize + horizontal_viewport,
                            ),
                            &row.new_spans,
                            row.new_changed,
                            false,
                            app.selected_row,
                        ));
                    }
                    RenderedDiffLine::Separator => {
                        old.push(split_separator_line());
                        new.push(split_separator_line());
                    }
                }
            }
            VisibleContent::Split { old, new }
        }
    };

    let horizontal_content = state.content_width;
    let horizontal_scroll = app.diff_horizontal_scroll.min(
        horizontal_content
            .saturating_sub(horizontal_viewport)
            .min(u16::MAX as usize) as u16,
    );
    // Only the visible portion of the selection background needs padding.
    let padded_width =
        (horizontal_scroll as usize + horizontal_viewport).min(u16::MAX as usize) as u16;
    match content {
        VisibleContent::Unified(lines) => {
            let lines = pad_lines(lines, padded_width);
            frame.render_widget(
                Paragraph::new(lines).scroll((0, horizontal_scroll)),
                diff_area,
            );
        }
        VisibleContent::Split { old, new } => {
            let columns = split_columns.expect("split mode computes columns");
            frame.render_widget(
                Paragraph::new(pad_lines(old, padded_width)).scroll((0, horizontal_scroll)),
                columns[0],
            );
            frame.render_widget(
                Paragraph::new(pad_lines(new, padded_width))
                    .scroll((0, horizontal_scroll))
                    .block(
                        rounded_block()
                            .borders(Borders::LEFT)
                            .border_style(Style::default().fg(Color::DarkGray)),
                    ),
                columns[1],
            );
        }
    }
    render_vertical_scrollbar(
        frame,
        area,
        content_length,
        diff_area.height as usize,
        scroll,
        app.scrollbars_visible(),
    );
    render_horizontal_scrollbar(
        frame,
        area.inner(Margin {
            vertical: 0,
            horizontal: 1,
        }),
        horizontal_content,
        horizontal_viewport,
        horizontal_scroll as usize,
        app.scrollbars_visible(),
    );
}

fn split_columns(area: Rect) -> [Rect; 2] {
    let columns = ratatui::layout::Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    [columns[0], columns[1]]
}

fn pad_lines(lines: Vec<Line<'static>>, padded_width: u16) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| pad_selected_line(line, padded_width))
        .collect()
}

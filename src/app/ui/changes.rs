//! Diff panel drawing. Only the visible slice of the diff is materialized
//! into styled lines; geometry comes from the cached render state.
use super::super::{App, navigation::clamped_diff_scroll};
use super::widgets::{
    render_horizontal_scrollbar, render_vertical_scrollbar, right_panel_block, rounded_block,
};
use crate::diff_view::{
    DiffMode, RenderedDiffLine, missing_line, pad_line_background, side_line, source_prefix,
    split_separator_line, unified_occurrence_line, unified_separator_line,
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
        old: Vec<SplitLine>,
        new: Vec<SplitLine>,
    },
}

enum SplitLine {
    Content(Line<'static>),
    Missing,
}

pub(super) fn draw_diff(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let outer = right_panel_block(app, true, false, area.width);
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
            "No changes to display".to_owned()
        };
        let line = Rect {
            x: diff_area.x,
            y: diff_area.y + diff_area.height.saturating_sub(1) / 2,
            width: diff_area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(message)
                .alignment(ratatui::layout::Alignment::Center)
                .style(Style::default().fg(Color::DarkGray)),
            line,
        );
        return;
    }
    let content_length = state.lines.len();
    let scroll = clamped_diff_scroll(app.diff_scroll, content_length, diff_area.height);
    let visible_end = content_length.min(scroll + diff_area.height as usize);
    let visible = &state.lines[scroll..visible_end];
    let rows = app.active_rows();
    let selected_row = app.diff_selection_active.then_some(app.selected_row);

    let split_columns = (app.diff_mode == DiffMode::SideBySide && !app.final_view)
        .then(|| split_columns(diff_area));
    let horizontal_viewport = match (&split_columns, app.diff_mode) {
        (Some(columns), _) => columns[0].width.min(columns[1].width.saturating_sub(1)) as usize,
        (None, _) => diff_area.width as usize,
    };
    let content = if app.final_view {
        VisibleContent::Unified(
            visible
                .iter()
                .filter_map(|entry| match entry {
                    RenderedDiffLine::Row { row, .. } => {
                        let index = *row;
                        let row = &rows[index];
                        Some(side_line(
                            index,
                            row.new_line,
                            source_prefix(
                                &row.new_text,
                                app.diff_horizontal_scroll as usize + horizontal_viewport,
                            ),
                            &row.new_spans,
                            false,
                            false,
                            selected_row,
                        ))
                    }
                    RenderedDiffLine::Separator => None,
                })
                .collect(),
        )
    } else {
        match app.diff_mode {
            DiffMode::Unified => VisibleContent::Unified(
                visible
                    .iter()
                    .filter_map(|entry| match entry {
                        RenderedDiffLine::Row { row, occurrence } => unified_occurrence_line(
                            *row,
                            &rows[*row],
                            *occurrence,
                            selected_row,
                            app.diff_horizontal_scroll as usize + horizontal_viewport,
                        ),
                        RenderedDiffLine::Separator => Some(unified_separator_line()),
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
                            old.push(row.old_line.map_or(SplitLine::Missing, |_| {
                                SplitLine::Content(side_line(
                                    index,
                                    row.old_line,
                                    source_prefix(
                                        &row.old_text,
                                        app.diff_horizontal_scroll as usize + horizontal_viewport,
                                    ),
                                    &row.old_spans,
                                    row.old_changed,
                                    true,
                                    selected_row,
                                ))
                            }));
                            new.push(row.new_line.map_or(SplitLine::Missing, |_| {
                                SplitLine::Content(side_line(
                                    index,
                                    row.new_line,
                                    source_prefix(
                                        &row.new_text,
                                        app.diff_horizontal_scroll as usize + horizontal_viewport,
                                    ),
                                    &row.new_spans,
                                    row.new_changed,
                                    false,
                                    selected_row,
                                ))
                            }));
                        }
                        RenderedDiffLine::Separator => {
                            old.push(SplitLine::Content(split_separator_line()));
                            new.push(SplitLine::Content(split_separator_line()));
                        }
                    }
                }
                VisibleContent::Split { old, new }
            }
        }
    };

    let horizontal_content = state.content_width;
    let horizontal_scroll = app.diff_horizontal_scroll.min(
        horizontal_content
            .saturating_sub(horizontal_viewport)
            .min(u16::MAX as usize) as u16,
    );
    match content {
        VisibleContent::Unified(lines) => {
            let lines = pad_lines(lines, background_width(horizontal_scroll, diff_area.width));
            frame.render_widget(
                Paragraph::new(lines).scroll((0, horizontal_scroll)),
                diff_area,
            );
        }
        VisibleContent::Split { old, new } => {
            let columns = split_columns.expect("split mode computes columns");
            let old_background_width = background_width(horizontal_scroll, columns[0].width);
            let new_background_width =
                background_width(horizontal_scroll, columns[1].width.saturating_sub(1));
            frame.render_widget(
                Paragraph::new(fill_split_lines(old, old_background_width))
                    .scroll((0, horizontal_scroll)),
                columns[0],
            );
            frame.render_widget(
                Paragraph::new(fill_split_lines(new, new_background_width))
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
        .map(|line| pad_line_background(line, padded_width))
        .collect()
}

fn fill_split_lines(lines: Vec<SplitLine>, width: u16) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| match line {
            SplitLine::Content(line) => pad_line_background(line, width),
            SplitLine::Missing => missing_line(width),
        })
        .collect()
}

/// Paint row backgrounds through the viewport, including the scrolled-off prefix.
fn background_width(horizontal_scroll: u16, viewport_width: u16) -> u16 {
    (horizontal_scroll as usize + viewport_width as usize).min(u16::MAX as usize) as u16
}

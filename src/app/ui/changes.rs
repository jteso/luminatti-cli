use super::super::{App, navigation::clamped_diff_scroll};
use super::widgets::{
    render_horizontal_scrollbar, render_vertical_scrollbar, right_panel_block, rounded_block,
};
use crate::diff_view::{
    DiffMode, RenderedDiffLine, pad_selected_line, side_line, split_separator_line, unified_lines,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Borders, Paragraph},
};

pub(super) fn draw_diff(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let outer = right_panel_block(app, true, false);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    let diff_area = inner;
    let visible = app.displayed_indices();
    if visible.is_empty() {
        let message = if app.final_view {
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
    let (content_length, scroll, horizontal_content, horizontal_viewport, horizontal_scroll) =
        if app.diff_mode == DiffMode::Unified {
            let mut lines: Vec<_> = visible
                .iter()
                .flat_map(|index| {
                    unified_lines(*index, &app.active_rows()[*index], app.selected_row)
                })
                .collect();
            let horizontal_content = lines.iter().map(Line::width).max().unwrap_or(0);
            let horizontal_viewport = diff_area.width as usize;
            let horizontal_scroll = app.diff_horizontal_scroll.min(
                horizontal_content
                    .saturating_sub(horizontal_viewport)
                    .min(u16::MAX as usize) as u16,
            );
            let content_area = diff_area;
            let padded_width = horizontal_content
                .max(horizontal_viewport)
                .min(u16::MAX as usize) as u16;
            lines = lines
                .into_iter()
                .map(|line| pad_selected_line(line, padded_width))
                .collect();
            let scroll = clamped_diff_scroll(app.diff_scroll, lines.len(), content_area.height);
            let content_length = lines.len();
            frame.render_widget(
                Paragraph::new(lines).scroll((scroll, horizontal_scroll)),
                content_area,
            );
            (
                content_length,
                scroll as usize,
                horizontal_content,
                horizontal_viewport,
                horizontal_scroll as usize,
            )
        } else {
            let rendered = app.rendered_lines();
            let initial_columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(diff_area);
            let mut old: Vec<_> = rendered
                .iter()
                .map(|line| match line {
                    RenderedDiffLine::Row(index) => {
                        let row = &app.diff_rows[*index];
                        side_line(
                            *index,
                            row.old_line,
                            &row.old_text,
                            &row.old_spans,
                            row.old_changed,
                            true,
                            app.selected_row,
                        )
                    }
                    RenderedDiffLine::Separator => split_separator_line(),
                })
                .collect();
            let mut new: Vec<_> = rendered
                .iter()
                .map(|line| match line {
                    RenderedDiffLine::Row(index) => {
                        let row = &app.diff_rows[*index];
                        side_line(
                            *index,
                            row.new_line,
                            &row.new_text,
                            &row.new_spans,
                            row.new_changed,
                            false,
                            app.selected_row,
                        )
                    }
                    RenderedDiffLine::Separator => split_separator_line(),
                })
                .collect();
            let horizontal_content = old.iter().chain(&new).map(Line::width).max().unwrap_or(0);
            let horizontal_viewport = initial_columns[0]
                .width
                .min(initial_columns[1].width.saturating_sub(1))
                as usize;
            let horizontal_scroll = app.diff_horizontal_scroll.min(
                horizontal_content
                    .saturating_sub(horizontal_viewport)
                    .min(u16::MAX as usize) as u16,
            );
            let content_area = diff_area;
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(content_area);
            let padded_width = horizontal_content
                .max(horizontal_viewport)
                .min(u16::MAX as usize) as u16;
            old = old
                .into_iter()
                .map(|line| pad_selected_line(line, padded_width))
                .collect();
            new = new
                .into_iter()
                .map(|line| pad_selected_line(line, padded_width))
                .collect();
            let scroll = clamped_diff_scroll(app.diff_scroll, rendered.len(), content_area.height);
            frame.render_widget(
                Paragraph::new(old).scroll((scroll, horizontal_scroll)),
                columns[0],
            );
            frame.render_widget(
                Paragraph::new(new)
                    .scroll((scroll, horizontal_scroll))
                    .block(
                        rounded_block()
                            .borders(Borders::LEFT)
                            .border_style(Style::default().fg(Color::DarkGray)),
                    ),
                columns[1],
            );
            (
                rendered.len(),
                scroll as usize,
                horizontal_content,
                horizontal_viewport,
                horizontal_scroll as usize,
            )
        };
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
        horizontal_scroll,
        app.scrollbars_visible(),
    );
}

//! Shared geometry for drawing, scrolling, and mouse hit testing.
use super::{App, Focus};
use crate::diff_view::{
    DiffMode, RenderedDiffLine, side_line, split_separator_line, unified_lines,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub(super) fn diff_content_width(app: &App) -> usize {
    if app.diff_mode == DiffMode::Unified {
        return app
            .displayed_indices()
            .iter()
            .flat_map(|index| unified_lines(*index, &app.active_rows()[*index], app.selected_row))
            .map(|line| line.width())
            .max()
            .unwrap_or(0);
    }

    app.rendered_lines()
        .iter()
        .map(|line| match line {
            RenderedDiffLine::Row(index) => {
                let row = &app.diff_rows[*index];
                let old = side_line(
                    *index,
                    row.old_line,
                    &row.old_text,
                    &row.old_spans,
                    row.old_changed,
                    true,
                    app.selected_row,
                )
                .width();
                let new = side_line(
                    *index,
                    row.new_line,
                    &row.new_text,
                    &row.new_spans,
                    row.new_changed,
                    false,
                    app.selected_row,
                )
                .width();
                old.max(new)
            }
            RenderedDiffLine::Separator => split_separator_line().width(),
        })
        .max()
        .unwrap_or(0)
}

pub(super) fn diff_inner_width(app: &App, terminal_width: u16) -> u16 {
    let panel_width = match app.maximized_panel {
        Some(Focus::Right) => terminal_width,
        Some(_) => 0,
        None => terminal_width.saturating_sub(constrained_divider(app.divider, terminal_width)),
    };
    panel_width.saturating_sub(2)
}

pub(super) fn diff_horizontal_viewport_width(app: &App, terminal_width: u16) -> u16 {
    let inner_width = diff_inner_width(app, terminal_width);
    if app.diff_mode == DiffMode::Unified {
        return inner_width;
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(Rect::new(0, 0, inner_width, 1));
    columns[0].width.min(columns[1].width.saturating_sub(1))
}

pub(super) fn max_diff_horizontal_scroll(app: &App, terminal_width: u16) -> u16 {
    diff_content_width(app)
        .saturating_sub(diff_horizontal_viewport_width(app, terminal_width) as usize)
        .min(u16::MAX as usize) as u16
}

fn diff_viewport_height(terminal_height: u16) -> u16 {
    terminal_height.saturating_sub(3)
}

pub(super) fn active_diff_viewport_height(
    app: &App,
    terminal_width: u16,
    terminal_height: u16,
) -> u16 {
    diff_viewport_height(terminal_height).saturating_sub(u16::from(
        diff_content_width(app) > diff_horizontal_viewport_width(app, terminal_width) as usize,
    ))
}

pub(super) fn constrained_divider(requested: u16, terminal_width: u16) -> u16 {
    // Terminal multiplexers can briefly report a 0×0 size during startup or a
    // resize. Keep layout arithmetic total so the UI never panics there.
    if terminal_width < 52 {
        return terminal_width.saturating_div(2).max(1);
    }
    requested.clamp(24, terminal_width - 28)
}

pub(super) fn resize_focused_panel(
    divider: &mut u16,
    focus: Focus,
    wider: bool,
    terminal_width: u16,
) {
    const STEP: u16 = 4;
    let focused_on_left = focus != Focus::Right;
    let grow_left = focused_on_left == wider;
    let requested = if grow_left {
        divider.saturating_add(STEP)
    } else {
        divider.saturating_sub(STEP)
    };
    *divider = constrained_divider(requested, terminal_width);
}

pub(super) fn left_panel_areas(area: Rect) -> [Rect; 2] {
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(area);
    [panels[0], panels[1]]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_take_twenty_percent_of_the_left_column() {
        let panels = left_panel_areas(Rect::new(0, 0, 40, 100));
        assert_eq!(panels[0].height, 80);
        assert_eq!(panels[1].height, 20);
        assert_eq!(panels[1].y, 80);
    }

    #[test]
    fn resize_shortcuts_change_the_focused_panel_width() {
        let mut divider = 34;

        resize_focused_panel(&mut divider, Focus::Files, true, 100);
        assert_eq!(divider, 38);
        resize_focused_panel(&mut divider, Focus::Filters, false, 100);
        assert_eq!(divider, 34);

        resize_focused_panel(&mut divider, Focus::Right, true, 100);
        assert_eq!(divider, 30);
        resize_focused_panel(&mut divider, Focus::Right, false, 100);
        assert_eq!(divider, 34);
    }

    #[test]
    fn resize_shortcuts_preserve_minimum_panel_widths() {
        let mut divider = 24;
        resize_focused_panel(&mut divider, Focus::Files, false, 100);
        assert_eq!(divider, 24);

        divider = 72;
        resize_focused_panel(&mut divider, Focus::Right, false, 100);
        assert_eq!(divider, 72);
    }
}

//! Mouse hit testing, pane resizing, and wheel scrolling.
use super::{
    App, FilterTab, Focus, RightTab,
    layout::{active_diff_viewport_height, constrained_divider, left_panel_areas},
    navigation::{clamped_diff_scroll, move_diff_horizontally},
};
use anyhow::Result;
use crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;

pub(super) fn handle_mouse(
    app: &mut App,
    mouse: crossterm::event::MouseEvent,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<()> {
    if app.show_help || app.input.is_some() || app.confirmation.is_some() {
        return Ok(());
    }
    if matches!(
        mouse.kind,
        MouseEventKind::ScrollDown
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
    ) {
        app.reveal_scrollbars();
    }
    if let Some(panel) = app.maximized_panel {
        return handle_maximized_mouse(app, panel, mouse, terminal_width, terminal_height);
    }
    let divider = constrained_divider(app.divider, terminal_width);
    let left_panels = left_panel_areas(Rect::new(0, 0, divider, terminal_height.saturating_sub(1)));
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) if mouse.column.abs_diff(divider) <= 1 => {
            app.dragging_divider = true
        }
        MouseEventKind::Drag(MouseButton::Left) if app.dragging_divider => {
            app.divider = constrained_divider(mouse.column, terminal_width)
        }
        MouseEventKind::Up(MouseButton::Left) => app.dragging_divider = false,
        MouseEventKind::Down(MouseButton::Left) => {
            if mouse.row == 0 {
                if mouse.column < divider {
                    app.focus_panel(Focus::Files);
                } else {
                    app.focus_panel(Focus::Right);
                    let panel_column = mouse.column.saturating_sub(divider);
                    if panel_column < 16 {
                        app.right_tab = RightTab::Diff;
                    } else if panel_column < 27 {
                        app.right_tab = RightTab::Comments;
                    }
                }
            } else if mouse.column < divider {
                if mouse.row < left_panels[1].y {
                    app.focus_panel(Focus::Files);
                    let index =
                        mouse.row.saturating_sub(left_panels[0].y.saturating_add(1)) as usize;
                    let tree_rows = app.file_tree_rows();
                    app.selected_file = index.min(tree_rows.len().saturating_sub(1));
                    if tree_rows
                        .get(app.selected_file)
                        .and_then(|row| row.file_index)
                        .is_some()
                    {
                        app.rebuild_diff()?;
                    }
                } else {
                    app.focus_panel(Focus::Filters);
                    if mouse.row == left_panels[1].y {
                        let panel_column = mouse.column.saturating_sub(left_panels[1].x);
                        if panel_column < 16 {
                            app.filter_tab = FilterTab::Filters;
                        } else if panel_column < 27 {
                            app.filter_tab = FilterTab::Ignored;
                        }
                        return Ok(());
                    }
                    let index =
                        mouse.row.saturating_sub(left_panels[1].y.saturating_add(1)) as usize;
                    if app.filter_tab == FilterTab::Filters {
                        app.selected_filter =
                            index.min(app.filters.patterns.len().saturating_sub(1));
                    } else {
                        app.selected_ignored = index.min(app.ignored_files.len().saturating_sub(1));
                    }
                }
            } else {
                app.focus_panel(Focus::Right);
                if app.right_tab == RightTab::Diff {
                    if let Some(index) =
                        clicked_row_index(app, mouse.row, terminal_width, terminal_height)
                    {
                        app.selected_row = index;
                        app.diff_selection_active = true;
                    }
                } else {
                    let index = mouse.row.saturating_sub(1) as usize;
                    app.selected_comment = index.min(app.all_comments().len().saturating_sub(1));
                }
            }
        }
        MouseEventKind::ScrollDown
            if app.right_tab == RightTab::Diff && mouse.column >= divider =>
        {
            app.diff_scroll = clamped_diff_scroll(
                app.diff_scroll.saturating_add(3),
                app.rendered_line_count(),
                active_diff_viewport_height(app, terminal_width, terminal_height),
            );
        }
        MouseEventKind::ScrollUp if app.right_tab == RightTab::Diff && mouse.column >= divider => {
            app.diff_scroll = app.diff_scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollRight
            if app.right_tab == RightTab::Diff && mouse.column >= divider =>
        {
            move_diff_horizontally(app, true, terminal_width);
        }
        MouseEventKind::ScrollLeft
            if app.right_tab == RightTab::Diff && mouse.column >= divider =>
        {
            move_diff_horizontally(app, false, terminal_width);
        }
        _ => {}
    }
    Ok(())
}

fn handle_maximized_mouse(
    app: &mut App,
    panel: Focus,
    mouse: crossterm::event::MouseEvent,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<()> {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => match panel {
            Focus::Files if mouse.row > 0 => {
                let index = mouse.row.saturating_sub(1) as usize;
                let tree_rows = app.file_tree_rows();
                app.selected_file = index.min(tree_rows.len().saturating_sub(1));
                if tree_rows
                    .get(app.selected_file)
                    .and_then(|row| row.file_index)
                    .is_some()
                {
                    app.rebuild_diff()?;
                }
            }
            Focus::Filters if mouse.row == 0 => {
                if mouse.column < 16 {
                    app.filter_tab = FilterTab::Filters;
                } else if mouse.column < 27 {
                    app.filter_tab = FilterTab::Ignored;
                }
            }
            Focus::Filters => {
                let index = mouse.row.saturating_sub(1) as usize;
                if app.filter_tab == FilterTab::Filters {
                    app.selected_filter = index.min(app.filters.patterns.len().saturating_sub(1));
                } else {
                    app.selected_ignored = index.min(app.ignored_files.len().saturating_sub(1));
                }
            }
            Focus::Right if mouse.row == 0 => {
                if mouse.column < 16 {
                    app.right_tab = RightTab::Diff;
                } else if mouse.column < 27 {
                    app.right_tab = RightTab::Comments;
                }
            }
            Focus::Right if app.right_tab == RightTab::Diff => {
                if let Some(index) =
                    clicked_row_index(app, mouse.row, terminal_width, terminal_height)
                {
                    app.selected_row = index;
                    app.diff_selection_active = true;
                }
            }
            Focus::Right => {
                let index = mouse.row.saturating_sub(1) as usize;
                app.selected_comment = index.min(app.all_comments().len().saturating_sub(1));
            }
            Focus::Files => {}
        },
        MouseEventKind::ScrollDown if panel == Focus::Right && app.right_tab == RightTab::Diff => {
            app.diff_scroll = clamped_diff_scroll(
                app.diff_scroll.saturating_add(3),
                app.rendered_line_count(),
                active_diff_viewport_height(app, terminal_width, terminal_height),
            );
        }
        MouseEventKind::ScrollUp if panel == Focus::Right && app.right_tab == RightTab::Diff => {
            app.diff_scroll = app.diff_scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollRight if panel == Focus::Right && app.right_tab == RightTab::Diff => {
            move_diff_horizontally(app, true, terminal_width);
        }
        MouseEventKind::ScrollLeft if panel == Focus::Right && app.right_tab == RightTab::Diff => {
            move_diff_horizontally(app, false, terminal_width);
        }
        _ => {}
    }
    Ok(())
}

/// Row index of the rendered diff line under the mouse pointer, if any.
fn clicked_row_index(
    app: &App,
    row: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> Option<usize> {
    let state = app.render_state();
    let scroll = clamped_diff_scroll(
        app.diff_scroll,
        state.lines.len(),
        active_diff_viewport_height(app, terminal_width, terminal_height),
    );
    state
        .lines
        .get(scroll + row.saturating_sub(1) as usize)
        .and_then(|line| line.row_index())
}

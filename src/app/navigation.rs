//! Selection movement and scroll bounds independent of input devices.
use super::diff::selected_line_position;
use super::{App, Focus, RightTab, layout::max_diff_horizontal_scroll};
use crate::{
    diff::{DiffRow, adjacent_changed_row},
    file_tree::adjacent_file_index,
};
use anyhow::Result;

pub(super) fn max_diff_scroll(content_height: usize, viewport_height: u16) -> usize {
    content_height.saturating_sub(viewport_height as usize)
}

pub(super) fn clamped_diff_scroll(
    scroll: usize,
    content_height: usize,
    viewport_height: u16,
) -> usize {
    scroll.min(max_diff_scroll(content_height, viewport_height))
}

pub(super) fn move_diff_horizontally(app: &mut App, right: bool, terminal_width: u16) {
    const STEP: u16 = 4;
    let max_scroll = max_diff_horizontal_scroll(app, terminal_width);
    let current = app.diff_horizontal_scroll.min(max_scroll);
    if right {
        app.diff_horizontal_scroll = current.saturating_add(STEP).min(max_scroll);
    } else {
        app.diff_horizontal_scroll = current.saturating_sub(STEP);
    }
}

pub(super) fn nearest_source_row(rows: &[DiffRow], line: Option<u32>) -> usize {
    rows.iter()
        .enumerate()
        .min_by_key(|(_, row)| {
            row.new_line
                .map_or(u32::MAX, |n| n.abs_diff(line.unwrap_or(1)))
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

pub(super) fn move_diff_selection(app: &mut App, down: bool, viewport_height: u16) {
    let next_index = {
        let state = app.render_state();
        if state.indices.is_empty() {
            None
        } else {
            let current = state
                .indices
                .iter()
                .position(|index| *index == app.selected_row)
                .unwrap_or(0);
            let next = if down {
                (current + 1).min(state.indices.len() - 1)
            } else {
                current.saturating_sub(1)
            };
            Some(state.indices[next])
        }
    };
    let Some(next_index) = next_index else {
        return;
    };
    app.selected_row = next_index;
    app.diff_selection_active = true;
    scroll_diff_selection_into_view(app, viewport_height);
}

pub(super) fn scroll_diff_selection_into_view(app: &mut App, viewport_height: u16) {
    let (content_length, rendered_start, rendered_end) = {
        let state = app.render_state();
        let rendered_start = selected_line_position(&state.lines, app.selected_row);
        let rendered_end = state
            .lines
            .iter()
            .rposition(|line| line.row_index() == Some(app.selected_row))
            .unwrap_or(rendered_start);
        (state.lines.len(), rendered_start, rendered_end)
    };
    let mut scroll = clamped_diff_scroll(app.diff_scroll, content_length, viewport_height);
    if rendered_start < scroll {
        scroll = rendered_start;
    } else if rendered_end >= scroll + viewport_height as usize {
        scroll = rendered_end
            .saturating_add(1)
            .saturating_sub(viewport_height as usize);
    }
    app.diff_scroll = clamped_diff_scroll(scroll, content_length, viewport_height);
}

pub(super) fn move_change_selection(
    app: &mut App,
    forward: bool,
    viewport_height: u16,
) -> Result<()> {
    app.right_tab = RightTab::Diff;
    app.focus_panel(Focus::Right);

    // Change navigation returns to the diff so deletions have a visible target.
    if app.final_view {
        app.toggle_final_view(viewport_height);
    }

    if let Some(target) = adjacent_changed_row(&app.diff_rows, app.selected_row, forward) {
        app.selected_row = target;
        app.diff_selection_active = true;
        scroll_diff_selection_into_view(app, viewport_height);
        return Ok(());
    }

    let current_file = app.active_file_index();
    if let Some(file_index) = adjacent_file_index(&app.files, current_file, forward) {
        app.select_file(file_index)?;
        app.select_last_change = !forward;
    } else {
        app.message = if forward {
            "already at the last changed file"
        } else {
            "already at the first changed file"
        }
        .into();
    }
    Ok(())
}

pub(super) fn move_file_selection(app: &mut App, forward: bool) -> Result<()> {
    let current_file = app.active_file_index();
    if let Some(file_index) = adjacent_file_index(&app.files, current_file, forward) {
        app.select_file(file_index)?;
    } else {
        app.message = if forward {
            "already at the last changed file"
        } else {
            "already at the first changed file"
        }
        .into();
    }
    Ok(())
}

pub(super) fn toggle_unchanged(app: &mut App) {
    app.show_unchanged = !app.show_unchanged;
    let selected_hidden =
        !app.show_unchanged && !app.render_state().indices.contains(&app.selected_row);
    if selected_hidden {
        app.selected_row = app
            .diff_rows
            .iter()
            .position(DiffRow::is_changed)
            .unwrap_or(0);
    }
    app.diff_scroll = 0;
    app.message = if app.show_unchanged {
        "showing unchanged code"
    } else {
        "hiding unchanged code"
    }
    .into();
}

pub(super) fn move_selection(current: &mut usize, len: usize, down: bool) {
    if len == 0 {
        return;
    }
    *current = if down {
        (*current + 1).min(len - 1)
    } else {
        current.saturating_sub(1)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diff_scroll_stays_zero_until_content_exceeds_the_viewport() {
        assert_eq!(max_diff_scroll(22, 40), 0);
        assert_eq!(clamped_diff_scroll(12, 22, 40), 0);
        assert_eq!(max_diff_scroll(60, 40), 20);
        assert_eq!(clamped_diff_scroll(30, 60, 40), 20);
        assert_eq!(max_diff_scroll(100_000, 40), 99_960);
    }
}

//! Selection movement and scroll bounds independent of input devices.
use super::{App, Focus, RightTab, layout::max_diff_horizontal_scroll};
use crate::{
    diff::{DiffRow, adjacent_changed_row, changed_row_indices},
    file_tree::adjacent_file_index,
};
use anyhow::Result;

pub(super) fn max_diff_scroll(content_height: usize, viewport_height: u16) -> u16 {
    content_height
        .saturating_sub(viewport_height as usize)
        .min(u16::MAX as usize) as u16
}

pub(super) fn clamped_diff_scroll(scroll: u16, content_height: usize, viewport_height: u16) -> u16 {
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
    let visible = app.displayed_indices();
    if visible.is_empty() {
        return;
    }
    let current = visible
        .iter()
        .position(|index| *index == app.selected_row)
        .unwrap_or(0);
    let next = if down {
        (current + 1).min(visible.len() - 1)
    } else {
        current.saturating_sub(1)
    };
    app.selected_row = visible[next];
    scroll_diff_selection_into_view(app, viewport_height);
}

pub(super) fn scroll_diff_selection_into_view(app: &mut App, viewport_height: u16) {
    let rendered = app.rendered_lines();
    let rendered_start = rendered
        .iter()
        .position(|line| line.row_index() == Some(app.selected_row))
        .unwrap_or(0);
    let rendered_end = rendered
        .iter()
        .rposition(|line| line.row_index() == Some(app.selected_row))
        .unwrap_or(rendered_start);
    let mut scroll = clamped_diff_scroll(app.diff_scroll, rendered.len(), viewport_height) as usize;
    if rendered_start < scroll {
        scroll = rendered_start;
    } else if rendered_end >= scroll + viewport_height as usize {
        scroll = rendered_end
            .saturating_add(1)
            .saturating_sub(viewport_height as usize);
    }
    app.diff_scroll = clamped_diff_scroll(
        scroll.min(u16::MAX as usize) as u16,
        rendered.len(),
        viewport_height,
    );
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
        scroll_diff_selection_into_view(app, viewport_height);
        return Ok(());
    }

    let current_file = app.active_file_index();
    if let Some(file_index) = adjacent_file_index(&app.files, current_file, forward) {
        app.select_file(file_index)?;
        if !forward && let Some(last_change) = changed_row_indices(&app.diff_rows).last().copied() {
            app.selected_row = last_change;
            scroll_diff_selection_into_view(app, viewport_height);
        }
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
    let displayed = app.displayed_indices();
    if !app.show_unchanged && !displayed.contains(&app.selected_row) {
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
    }
}

//! Read-only rendering, grouped by panel and shared widgets.
use super::{
    App, ConfirmationKind, Focus, InputKind, RightTab,
    layout::{constrained_divider, left_panel_areas},
};
use ratatui::layout::{Constraint, Direction, Layout};
mod changes;
mod footer;
mod modals;
mod panels;
mod widgets;
use changes::draw_diff;
use footer::draw_footer;
use modals::{draw_delete_all_confirmation, draw_file_search, draw_help, draw_input};
use panels::{draw_comments, draw_files, draw_filters};
#[cfg(test)]
mod tests;

pub(super) fn draw(frame: &mut ratatui::Frame, app: &App) {
    let area = frame.area();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .split(area);
    match app.maximized_panel {
        Some(Focus::Files) => draw_files(frame, app, vertical[0]),
        Some(Focus::Filters) => draw_filters(frame, app, vertical[0]),
        Some(Focus::Right) => draw_right(frame, app, vertical[0]),
        None => {
            let width = constrained_divider(app.divider, vertical[0].width);
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(width), Constraint::Min(28)])
                .split(vertical[0]);
            draw_left(frame, app, panes[0]);
            draw_right(frame, app, panes[1]);
        }
    }
    draw_footer(frame, app, vertical[1]);
    if let Some(input) = &app.input {
        if input.kind == InputKind::FileSearch {
            draw_file_search(frame, app, input, area);
        } else {
            draw_input(frame, input, area);
        }
    }
    if app.confirmation == Some(ConfirmationKind::DeleteAllComments) {
        draw_delete_all_confirmation(frame, area);
    }
    if app.show_help {
        draw_help(frame, area);
    }
}

fn draw_left(frame: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let panels = left_panel_areas(area);
    draw_files(frame, app, panels[0]);
    draw_filters(frame, app, panels[1]);
}

fn draw_right(frame: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    match app.right_tab {
        RightTab::Diff => draw_diff(frame, app, area),
        RightTab::Comments => draw_comments(frame, app, area),
    }
}

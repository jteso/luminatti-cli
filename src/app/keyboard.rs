//! Keyboard dispatch, modal text entry, and panel switching.
use super::{
    App, ConfirmationKind, FilterTab, Focus, Input, InputKind, RightTab,
    layout::{active_diff_viewport_height, resize_focused_panel},
    navigation::{
        move_change_selection, move_diff_horizontally, move_diff_selection, move_file_selection,
        move_selection, toggle_unchanged,
    },
};
use crate::{diff_view::DiffMode, search::fuzzy_file_indices};
use anyhow::Result;
use crossterm::event::KeyCode;

fn handle_file_search_key(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc => app.input = None,
        KeyCode::Enter => {
            let file_index = app.input.as_ref().and_then(|input| {
                fuzzy_file_indices(&app.files, &input.value)
                    .get(input.selected)
                    .copied()
            });
            if let Some(file_index) = file_index {
                app.input = None;
                app.select_file(file_index)?;
            }
        }
        KeyCode::Down | KeyCode::Up => {
            app.reveal_scrollbars();
            let query = app
                .input
                .as_ref()
                .map(|input| input.value.as_str())
                .unwrap_or_default();
            let match_count = fuzzy_file_indices(&app.files, query).len();
            if let Some(input) = &mut app.input {
                move_selection(&mut input.selected, match_count, code == KeyCode::Down);
            }
        }
        KeyCode::Backspace => {
            if let Some(input) = &mut app.input {
                input.value.pop();
                input.selected = 0;
            }
        }
        KeyCode::Char(character) => {
            if let Some(input) = &mut app.input {
                input.value.push(character);
                input.selected = 0;
            }
        }
        _ => {}
    }
    Ok(())
}

fn activate_numbered_panel(panel: u8, focus: &mut Focus, maximized_panel: &mut Option<Focus>) {
    let target = match panel {
        1 => Focus::Files,
        2 => Focus::Right,
        3 => Focus::Filters,
        _ => return,
    };
    if *focus != target {
        *focus = target;
        *maximized_panel = None;
    } else if *maximized_panel == Some(target) {
        *maximized_panel = None;
    } else {
        *maximized_panel = Some(target);
    }
}

fn cycle_focused_panel_tab(focus: Focus, right_tab: &mut RightTab, filter_tab: &mut FilterTab) {
    match focus {
        Focus::Right => {
            *right_tab = match *right_tab {
                RightTab::Diff => RightTab::Comments,
                RightTab::Comments => RightTab::Diff,
            }
        }
        Focus::Filters => {
            *filter_tab = match *filter_tab {
                FilterTab::Filters => FilterTab::Ignored,
                FilterTab::Ignored => FilterTab::Filters,
            }
        }
        Focus::Files => {}
    }
}

pub(super) fn handle_key(
    app: &mut App,
    code: KeyCode,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<bool> {
    if app.show_help {
        if matches!(code, KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')) {
            app.show_help = false;
        }
        return Ok(false);
    }
    if app.confirmation == Some(ConfirmationKind::DeleteAllComments) {
        match code {
            KeyCode::Enter => {
                app.confirmation = None;
                app.delete_all_comments()?;
            }
            KeyCode::Esc => {
                app.confirmation = None;
                app.message = "delete cancelled".into();
            }
            _ => {}
        }
        return Ok(false);
    }
    if app
        .input
        .as_ref()
        .is_some_and(|input| input.kind == InputKind::FileSearch)
    {
        handle_file_search_key(app, code)?;
        return Ok(false);
    }
    if let Some(input) = &mut app.input {
        match code {
            KeyCode::Esc => app.input = None,
            KeyCode::Enter => {
                let input = app.input.take().expect("input exists");
                if input.value.trim().is_empty() {
                    return Ok(false);
                }
                match input.kind {
                    InputKind::Comment => app.add_comment(input.value)?,
                    InputKind::Filter => app.add_filter(input.value)?,
                    InputKind::FileSearch => unreachable!("file search handled above"),
                };
            }
            KeyCode::Backspace => {
                input.value.pop();
            }
            KeyCode::Char(c) => input.value.push(c),
            _ => {}
        }
        return Ok(false);
    }
    match code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Esc if app.right_tab == RightTab::Diff => app.diff_selection_active = false,
        KeyCode::Char('?') => app.show_help = true,
        KeyCode::Char('/') => {
            app.input = Some(Input {
                kind: InputKind::FileSearch,
                value: String::new(),
                selected: 0,
            })
        }
        KeyCode::Tab => cycle_focused_panel_tab(app.focus, &mut app.right_tab, &mut app.filter_tab),
        KeyCode::Char('h')
            if app.right_tab == RightTab::Diff && app.diff_mode == DiffMode::Unified =>
        {
            let viewport = active_diff_viewport_height(app, terminal_width, terminal_height);
            app.toggle_final_view(viewport);
        }
        KeyCode::Char('h') => app.focus_panel(Focus::Files),
        KeyCode::Char('l') => app.focus_panel(Focus::Right),
        KeyCode::Char('<') => {
            resize_focused_panel(&mut app.divider, app.focus, false, terminal_width)
        }
        KeyCode::Char('>') => {
            resize_focused_panel(&mut app.divider, app.focus, true, terminal_width)
        }
        KeyCode::Char('[') => {
            app.reveal_scrollbars();
            let viewport = active_diff_viewport_height(app, terminal_width, terminal_height);
            move_change_selection(app, false, viewport)?
        }
        KeyCode::Char(']') => {
            app.reveal_scrollbars();
            let viewport = active_diff_viewport_height(app, terminal_width, terminal_height);
            move_change_selection(app, true, viewport)?
        }
        KeyCode::Char('{') => move_file_selection(app, false)?,
        KeyCode::Char('}') => move_file_selection(app, true)?,
        KeyCode::Char('1') => activate_numbered_panel(1, &mut app.focus, &mut app.maximized_panel),
        KeyCode::Char('2') => activate_numbered_panel(2, &mut app.focus, &mut app.maximized_panel),
        KeyCode::Char('3') => activate_numbered_panel(3, &mut app.focus, &mut app.maximized_panel),
        KeyCode::Char('f') => app.focus_panel(Focus::Files),
        KeyCode::Char('d') if app.focus == Focus::Right && app.right_tab == RightTab::Comments => {
            app.delete_selected_comment()?;
        }
        KeyCode::Char('D') if app.focus == Focus::Right && app.right_tab == RightTab::Comments => {
            app.confirmation = Some(ConfirmationKind::DeleteAllComments);
        }
        KeyCode::Char('u') => {
            app.set_diff_mode(DiffMode::Unified)?;
        }
        KeyCode::Char('s') => {
            app.set_diff_mode(DiffMode::SideBySide)?;
        }
        KeyCode::Char('i') if app.right_tab == RightTab::Diff && !app.final_view => {
            toggle_unchanged(app)
        }
        KeyCode::Char('r') => {
            app.refresh()?;
            app.message = "refreshed".into();
        }
        KeyCode::Char('c') if app.right_tab == RightTab::Diff => {
            app.input = Some(Input {
                kind: InputKind::Comment,
                value: String::new(),
                selected: 0,
            })
        }
        KeyCode::Char('a')
            if app.focus == Focus::Filters && app.filter_tab == FilterTab::Filters =>
        {
            app.input = Some(Input {
                kind: InputKind::Filter,
                value: String::new(),
                selected: 0,
            })
        }
        KeyCode::Char('x')
            if app.focus == Focus::Filters && app.filter_tab == FilterTab::Filters =>
        {
            if app.selected_filter < app.filters.patterns.len() {
                app.filters.patterns.remove(app.selected_filter);
                app.save_filters()?;
                app.refresh()?;
            }
        }
        KeyCode::Char('y') if app.right_tab == RightTab::Comments => app.copy_comment()?,
        KeyCode::Down | KeyCode::Up => {
            let down = code == KeyCode::Down;
            app.reveal_scrollbars();
            match (app.focus, app.right_tab) {
                (Focus::Files, _) => {
                    let tree_len = app.file_tree_rows().len();
                    move_selection(&mut app.selected_file, tree_len, down)
                }
                (Focus::Filters, _) => {
                    if app.filter_tab == FilterTab::Filters {
                        move_selection(&mut app.selected_filter, app.filters.patterns.len(), down)
                    } else {
                        move_selection(&mut app.selected_ignored, app.ignored_files.len(), down)
                    }
                }
                (Focus::Right, RightTab::Diff) => {
                    let viewport =
                        active_diff_viewport_height(app, terminal_width, terminal_height);
                    move_diff_selection(app, down, viewport);
                }
                (Focus::Right, RightTab::Comments) => {
                    let comment_count =
                        app.local_comments.comments.len() + app.agent_comments.len();
                    move_selection(&mut app.selected_comment, comment_count, down)
                }
            }
        }
        KeyCode::Left | KeyCode::Right
            if app.focus == Focus::Right && app.right_tab == RightTab::Diff =>
        {
            app.reveal_scrollbars();
            move_diff_horizontally(app, code == KeyCode::Right, terminal_width)
        }
        KeyCode::Enter if app.focus == Focus::Files => {
            if app.active_file_index().is_some() {
                app.rebuild_diff()?;
                app.focus_panel(Focus::Right);
            } else {
                app.toggle_selected_directory();
            }
        }
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_panel_numbers_focus_maximize_and_restore_without_changing_tabs() {
        let mut focus = Focus::Files;
        let mut maximized_panel = None;
        let right_tab = RightTab::Comments;
        let filter_tab = FilterTab::Ignored;

        activate_numbered_panel(3, &mut focus, &mut maximized_panel);
        assert_eq!(focus, Focus::Filters);
        assert_eq!(maximized_panel, None);
        assert_eq!(filter_tab, FilterTab::Ignored);
        activate_numbered_panel(3, &mut focus, &mut maximized_panel);
        assert_eq!(maximized_panel, Some(Focus::Filters));
        activate_numbered_panel(3, &mut focus, &mut maximized_panel);
        assert_eq!(maximized_panel, None);

        activate_numbered_panel(2, &mut focus, &mut maximized_panel);
        assert_eq!(focus, Focus::Right);
        assert_eq!(right_tab, RightTab::Comments);
        activate_numbered_panel(2, &mut focus, &mut maximized_panel);
        assert_eq!(maximized_panel, Some(Focus::Right));
        activate_numbered_panel(2, &mut focus, &mut maximized_panel);
        assert_eq!(maximized_panel, None);

        activate_numbered_panel(1, &mut focus, &mut maximized_panel);
        assert_eq!(focus, Focus::Files);
        assert_eq!(maximized_panel, None);
    }

    #[test]
    fn tab_cycles_only_within_the_focused_panel() {
        let mut right_tab = RightTab::Diff;
        let mut filter_tab = FilterTab::Filters;

        cycle_focused_panel_tab(Focus::Right, &mut right_tab, &mut filter_tab);
        assert_eq!(right_tab, RightTab::Comments);
        assert_eq!(filter_tab, FilterTab::Filters);

        cycle_focused_panel_tab(Focus::Filters, &mut right_tab, &mut filter_tab);
        assert_eq!(right_tab, RightTab::Comments);
        assert_eq!(filter_tab, FilterTab::Ignored);

        cycle_focused_panel_tab(Focus::Files, &mut right_tab, &mut filter_tab);
        assert_eq!(right_tab, RightTab::Comments);
        assert_eq!(filter_tab, FilterTab::Ignored);
    }
}

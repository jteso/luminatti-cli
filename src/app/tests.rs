use super::*;
use super::{
    keyboard::handle_key,
    mouse::handle_mouse,
    test_support::{preview_app, test_diff_rows},
};
use crate::{
    diff::{adjacent_changed_row, changed_row_indices, visible_diff_indices},
    diff_view::{RenderedDiffLine, rendered_diff_lines},
};
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

#[test]
fn arrow_keys_move_only_the_focused_list_and_stop_at_its_boundaries() {
    let cases = [
        (Focus::Files, FilterTab::Filters),
        (Focus::Filters, FilterTab::Filters),
        (Focus::Filters, FilterTab::Ignored),
        (Focus::Right, FilterTab::Filters),
    ];
    for (focus, filter_tab) in cases {
        let mut app = preview_app("old\n", "new\n");
        app.focus = focus;
        app.filter_tab = filter_tab;
        app.right_tab = RightTab::Comments;
        app.files = ["a.rs", "b.rs"]
            .into_iter()
            .map(|path| FileItem {
                path: path.into(),
                status: " M".into(),
            })
            .collect();
        app.ignored_files = app.files.clone();
        app.filters.patterns = vec!["first/**".into(), "second/**".into()];
        app.local_comments = serde_json::from_str(
            r#"{"comments":[{"filePath":"a.rs","summary":"first"},{"filePath":"b.rs","summary":"second"}]}"#,
        ).unwrap();

        let active_index = match (focus, filter_tab) {
            (Focus::Files, _) => 0,
            (Focus::Filters, FilterTab::Filters) => 1,
            (Focus::Filters, FilterTab::Ignored) => 2,
            (Focus::Right, _) => 3,
        };
        for (key, selected) in [
            (KeyCode::Up, 0),
            (KeyCode::Down, 1),
            (KeyCode::Down, 1),
            (KeyCode::Up, 0),
        ] {
            assert!(!handle_key(&mut app, key, 100, 20).unwrap());
            let mut expected = [0; 4];
            expected[active_index] = selected;
            assert_eq!(
                [
                    app.selected_file,
                    app.selected_filter,
                    app.selected_ignored,
                    app.selected_comment
                ],
                expected,
                "{focus:?} / {filter_tab:?} / {key:?}"
            );
        }
    }
}

#[test]
fn final_toggle_shows_complete_source_and_restores_deleted_selection() {
    let mut app = preview_app("first\nremoved\nlast\n", "first\nlast\n");
    app.selected_row = app
        .diff_rows
        .iter()
        .position(|row| row.new_line.is_none())
        .unwrap();
    let deleted = app.selected_row;
    handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
    assert!(app.final_view);
    assert_eq!(app.focus, Focus::Right);
    assert_eq!(app.displayed_indices(), vec![0, 1]);
    assert_eq!(
        app.active_rows()
            .iter()
            .map(|row| row.new_text.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "last"]
    );
    assert_eq!(app.selected_row, 1);
    handle_key(&mut app, KeyCode::Char('i'), 100, 20).unwrap();
    assert!(!app.show_unchanged);
    handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
    assert!(!app.final_view);
    assert_eq!(app.selected_row, deleted);
    assert!(!app.show_unchanged);
}

#[test]
fn final_toggle_is_available_when_another_panel_is_focused() {
    let mut app = preview_app("old\n", "new\n");

    app.focus = Focus::Files;
    handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
    assert!(app.final_view);
    assert_eq!(app.focus, Focus::Files);

    app.focus = Focus::Filters;
    handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
    assert!(!app.final_view);
    assert_eq!(app.focus, Focus::Filters);
}

#[test]
fn final_navigation_and_mouse_use_physical_source_lines() {
    let mut app = preview_app("first\nold\nlast\n", "first\nnew\nlast\n");
    app.show_unchanged = true;
    handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
    handle_key(&mut app, KeyCode::Down, 100, 20).unwrap();
    assert_eq!(app.active_rows()[app.selected_row].new_text, "new");
    handle_mouse(
        &mut app,
        crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 50,
            row: 3,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        100,
        20,
    )
    .unwrap();
    assert_eq!(app.active_rows()[app.selected_row].new_text, "last");
    handle_key(&mut app, KeyCode::Char('h'), 100, 20).unwrap();
    assert_eq!(app.active_rows()[app.selected_row].new_line, Some(3));
    assert!(app.show_unchanged);
}

#[test]
fn scrollbars_are_hidden_until_scroll_activity_then_expire() {
    let mut app = preview_app("before\n", "after\n");

    assert!(!app.scrollbars_visible());
    app.reveal_scrollbars();
    assert!(app.scrollbars_visible());

    app.scrollbar_visible_until = Some(Instant::now() - Duration::from_millis(1));
    assert!(!app.scrollbars_visible());
}

#[test]
fn line_diff_aligns_replacements_and_tracks_line_numbers() {
    let rows = test_diff_rows("a\nb\n", "a\nc\n");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].old_line, Some(2));
    assert_eq!(rows[1].new_line, Some(2));
    assert_eq!(rows[1].old_text, "b");
    assert_eq!(rows[1].new_text, "c");
    assert!(rows[1].old_changed);
    assert!(rows[1].new_changed);
}

#[test]
fn bracket_navigation_visits_each_changed_row_including_adjacent_changes() {
    let rows = test_diff_rows(
        "same\nold one\nold two\nbetween\nold three\nend\n",
        "same\nnew one\nnew two\nbetween\nnew three\nend\n",
    );

    assert_eq!(changed_row_indices(&rows), vec![1, 2, 4]);
    assert_eq!(adjacent_changed_row(&rows, 1, true), Some(2));
    assert_eq!(adjacent_changed_row(&rows, 2, true), Some(4));
    assert_eq!(adjacent_changed_row(&rows, 4, true), None);
    assert_eq!(adjacent_changed_row(&rows, 4, false), Some(2));
    assert_eq!(adjacent_changed_row(&rows, 2, false), Some(1));
    assert_eq!(adjacent_changed_row(&rows, 1, false), None);
    assert_eq!(adjacent_changed_row(&rows, 3, true), Some(4));
    assert_eq!(adjacent_changed_row(&rows, 3, false), Some(2));
}

#[test]
fn same_code_filter_can_hide_or_include_unchanged_rows() {
    let rows = test_diff_rows("same\nold\n", "same\nnew\n");
    assert_eq!(visible_diff_indices(&rows, false), vec![1]);
    assert_eq!(visible_diff_indices(&rows, true), vec![0, 1]);
    assert_eq!(
        rendered_diff_lines(&rows, false, DiffMode::Unified),
        vec![RenderedDiffLine::Row(1), RenderedDiffLine::Row(1)]
    );
}

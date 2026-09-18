use super::super::{Focus, keyboard::handle_key, test_support::preview_app};
use super::widgets::panel_block;
use super::*;
use crate::diff::line_diff_document;
use crate::diff_view::DiffMode;
use crossterm::event::KeyCode;
use ratatui::{
    Terminal,
    backend::TestBackend,
    style::{Color, Modifier},
};
use std::path::PathBuf;

#[test]
fn final_view_renders_neutral_numbered_source_and_handles_empty_files() {
    let mut app = preview_app("old\n", "new\n");
    app.toggle_final_view(5);
    let mut terminal = Terminal::new(TestBackend::new(60, 7)).unwrap();
    terminal
        .draw(|frame| draw_diff(frame, &app, frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let source: String = (1..20).map(|x| buffer[(x, 1)].symbol()).collect();
    assert!(source.starts_with("     1 new"));
    assert_eq!(buffer[(8, 1)].fg, Color::Gray);
    assert!(!buffer[(8, 1)].modifier.contains(Modifier::CROSSED_OUT));
    let mut empty = preview_app("removed\n", "");
    empty.toggle_final_view(5);
    assert!(empty.rendered_lines().is_empty());
    terminal
        .draw(|frame| draw_diff(frame, &empty, frame.area()))
        .unwrap();
    empty.toggle_final_view(5);
    assert!(!empty.rendered_lines().is_empty());
}

#[test]
fn panel_tabs_render_in_the_top_border_with_purple_accents() {
    let backend = TestBackend::new(32, 3);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            frame.render_widget(
                panel_block("[1]", "Files", true, "Filters", false, true),
                frame.area(),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let top_border: String = (0..buffer.area.width)
        .map(|x| buffer[(x, 0)].symbol())
        .collect();
    assert!(top_border.starts_with("╭ [1] Files - Filters "));
    assert_eq!(buffer[(0, 0)].fg, Color::Magenta);
    assert_eq!(buffer[(2, 0)].fg, Color::Magenta);
    assert_eq!(buffer[(6, 0)].fg, Color::Magenta);
    assert_eq!(buffer[(14, 0)].fg, Color::DarkGray);
}

#[test]
fn footer_keeps_status_left_and_permanent_shortcuts_right() {
    let mut app = preview_app("before\n", "after\n");
    app.repo = PathBuf::from("/tmp/luminatti-cli");
    app.remote.branch = "main".into();
    let mut terminal = Terminal::new(TestBackend::new(100, 1)).unwrap();

    terminal
        .draw(|frame| draw_footer(frame, &app, frame.area()))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let footer = (0..buffer.area.width)
        .map(|x| buffer[(x, 0)].symbol())
        .collect::<String>();
    assert!(footer.starts_with(" luminatti-cli · main"));
    assert!(footer.ends_with("[/] Search  [?] Help  [q] Quit "));
    assert!(!footer.contains("find file"));
    assert_eq!(buffer[(69, 0)].fg, Color::Yellow);
}

#[test]
fn keybindings_overlay_removes_focus_from_background_panels() {
    let mut app = preview_app("before\n", "after\n");
    app.focus = Focus::Files;
    app.show_help = true;
    let mut terminal = Terminal::new(TestBackend::new(120, 45)).unwrap();

    terminal.draw(|frame| draw(frame, &app)).unwrap();

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].fg, Color::DarkGray);
    assert_eq!(buffer[(18, 3)].fg, Color::Magenta);
    assert_eq!(app.focus, Focus::Files);
}

#[test]
fn horizontal_scroll_moves_both_split_columns_together() {
    let before = "shared-prefix-old-abcdefghijklmnopqrstuvwxyz\n";
    let after = "shared-prefix-new-abcdefghijklmnopqrstuvwxyz\n";
    let mut app = preview_app(before, after);
    app.diff_mode = DiffMode::SideBySide;
    app.diff_rows = line_diff_document(before, after).rows;
    app.maximized_panel = Some(Focus::Right);

    handle_key(&mut app, KeyCode::Right, 50, 8).unwrap();
    assert_eq!(app.diff_horizontal_scroll, 4);
    handle_key(&mut app, KeyCode::Left, 50, 8).unwrap();
    assert_eq!(app.diff_horizontal_scroll, 0);

    app.diff_horizontal_scroll = 6;
    app.reveal_scrollbars();
    let mut terminal = Terminal::new(TestBackend::new(50, 8)).unwrap();
    terminal
        .draw(|frame| draw_diff(frame, &app, frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();

    assert_eq!(buffer[(1, 1)].symbol(), "s");
    assert_eq!(buffer[(26, 1)].symbol(), "s");
    assert!((1..49).any(|x| buffer[(x, 7)].symbol() == "━"));
    assert_eq!(buffer[(49, 1)].fg, Color::Magenta);
}

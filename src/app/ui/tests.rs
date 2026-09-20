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
use std::time::Instant;

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
    app.diff_rows = line_diff_document(before, after).rows.into();
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

#[test]
#[ignore = "performance probe: cargo test --release large_diff_stays_viewport_sized -- --ignored --nocapture"]
fn large_diff_stays_viewport_sized() {
    let mut before = String::with_capacity(8_000_000);
    for index in 0..100_000 {
        before.push_str(&format!(
            "line {index}: some fairly representative source statement with tokens;\n"
        ));
    }
    let after = before
        .replace("line 500: ", "line 500: edited ")
        .replace("line 90_000: ", "line 90_000: edited ");
    let mut app = preview_app(&before, &after);
    app.diff_rows = line_diff_document(&before, &after).rows.into();
    app.diff_mode = DiffMode::SideBySide;
    app.show_unchanged = true;
    assert_eq!(app.rendered_line_count(), 100_000);

    let mut terminal = Terminal::new(TestBackend::new(220, 50)).unwrap();
    let mut frames = 0;
    let started = Instant::now();
    for _ in 0..300 {
        app.diff_scroll = app.diff_scroll.saturating_add(3);
        terminal
            .draw(|frame| draw_diff(frame, &app, frame.area()))
            .unwrap();
        frames += 1;
    }
    println!(
        "{frames} full redraws of a 100k-line diff: {:?} ({:.3} ms/frame)",
        started.elapsed(),
        started.elapsed().as_secs_f64() * 1000.0 / frames as f64
    );

    let started = Instant::now();
    for _ in 0..100 {
        let viewport = super::super::layout::active_diff_viewport_height(&app, 220, 50);
        super::super::navigation::move_diff_selection(&mut app, true, viewport);
    }
    println!(
        "100 selection moves on a 100k-line diff: {:?}",
        started.elapsed()
    );
}

#[test]
fn unified_rows_render_each_physical_line_exactly_once() {
    let mut app = preview_app("shared\nold line\n", "shared\nnew line\n");
    app.show_unchanged = true;
    let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
    terminal
        .draw(|frame| draw_diff(frame, &app, frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rendered: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect();
    let deletions = rendered
        .iter()
        .filter(|line| line.contains('-') && line.contains("old line"))
        .count();
    let insertions = rendered
        .iter()
        .filter(|line| line.contains('+') && line.contains("new line"))
        .count();
    assert_eq!(deletions, 1, "deletion rendered {deletions} times");
    assert_eq!(insertions, 1, "insertion rendered {insertions} times");
}

#[test]
fn unified_occurrence_selects_the_right_physical_line() {
    use crate::diff_view::unified_occurrence_line;
    let rows = crate::diff::line_diff_document("old\n", "new\n").rows;
    let deletion = unified_occurrence_line(0, &rows[0], 0, usize::MAX, usize::MAX).unwrap();
    let insertion = unified_occurrence_line(0, &rows[0], 1, usize::MAX, usize::MAX).unwrap();
    assert_eq!(deletion.spans[0].content, "-      ");
    assert_eq!(insertion.spans[0].content, "+    1 ");
    assert!(unified_occurrence_line(0, &rows[0], 2, usize::MAX, usize::MAX).is_none());
}
#[test]
fn very_long_source_lines_do_not_block_drawing() {
    let mut app = preview_app("", "");
    app.diff_rows = line_diff_document("", &"x".repeat(4_000_000)).rows.into();
    app.render_state();
    let backend = ratatui::backend::TestBackend::new(100, 25);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    let start = std::time::Instant::now();
    terminal
        .draw(|frame| changes::draw_diff(frame, &app, frame.area()))
        .unwrap();
    let elapsed = start.elapsed();
    println!("4 MB line redraw: {elapsed:?}");
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "drawing one visible line took {elapsed:?}"
    );
}

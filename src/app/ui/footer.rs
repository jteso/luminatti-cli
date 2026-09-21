use super::super::App;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

const SYNC_STATUS_COLOR: Color = Color::Rgb(202, 170, 92);

pub(super) fn draw_footer(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let area = area.inner(Margin {
        vertical: 0,
        horizontal: 1,
    });
    let status = remote_status_line(app);
    let shortcuts = permanent_shortcut_line();
    let shortcuts_width = (shortcuts.width() as u16).min(area.width.saturating_sub(1));
    let pieces = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(shortcuts_width)])
        .split(area);
    let mut status_with_message = status.clone();
    if !app.message.is_empty() {
        status_with_message.spans.push(Span::styled(
            format!("  ·  {}", app.message),
            Style::default().fg(Color::DarkGray),
        ));
    }
    let status = if status_with_message.width() <= pieces[0].width as usize {
        status_with_message
    } else {
        status
    };
    frame.render_widget(Paragraph::new(status), pieces[0]);
    frame.render_widget(
        Paragraph::new(shortcuts).alignment(Alignment::Right),
        pieces[1],
    );
}

fn push_shortcut_display(
    spans: &mut Vec<Span<'static>>,
    display: impl Into<String>,
    label: impl Into<String>,
) {
    if !spans.is_empty() {
        spans.push(Span::raw("  "));
    }
    spans.push(Span::styled(
        display.into(),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(
        format!(" {}", label.into()),
        Style::default().fg(Color::Gray),
    ));
}

fn push_shortcut(spans: &mut Vec<Span<'static>>, key: &'static str, label: impl Into<String>) {
    push_shortcut_display(spans, format!("[{key}]"), label);
}

fn permanent_shortcut_line() -> Line<'static> {
    let mut spans = vec![];
    push_shortcut(&mut spans, "/", "Search");
    push_shortcut(&mut spans, "?", "Help");
    push_shortcut(&mut spans, "q", "Quit");
    Line::from(spans)
}

fn remote_status_line(app: &App) -> Line<'static> {
    let project = app
        .repo
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("git");
    let white = Style::default().fg(Color::White);
    let sync_status = Style::default().fg(SYNC_STATUS_COLOR);
    let mut spans = vec![
        Span::styled(project.to_owned(), white.add_modifier(Modifier::BOLD)),
        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        Span::styled(app.remote.branch.clone(), white),
    ];
    match (app.remote.behind, app.remote.ahead) {
        (Some(behind), Some(ahead)) => {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("↓", sync_status));
            spans.push(Span::styled(behind.to_string(), sync_status));
            spans.push(Span::raw(" "));
            spans.push(Span::styled("↑", sync_status));
            spans.push(Span::styled(ahead.to_string(), sync_status));
        }
        _ => {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("◆", sync_status));
            spans.push(Span::styled(" no upstream", white));
        }
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_support::preview_app;

    #[test]
    fn sync_arrows_and_counts_use_muted_yellow() {
        let mut app = preview_app("before\n", "after\n");
        app.remote.behind = Some(0);
        app.remote.ahead = Some(1);

        let line = remote_status_line(&app);
        let muted_yellow = Some(Color::Rgb(202, 170, 92));

        for span in [
            &line.spans[4],
            &line.spans[5],
            &line.spans[7],
            &line.spans[8],
        ] {
            assert_eq!(span.style.fg, muted_yellow);
            assert!(!span.style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn permanent_shortcut_tokens_are_white() {
        let line = permanent_shortcut_line();

        for span in [&line.spans[0], &line.spans[3], &line.spans[6]] {
            assert_eq!(span.style.fg, Some(Color::White));
        }
    }
}

//! Terminal session and event loop.
use super::{App, keyboard::handle_key, mouse::handle_mouse, ui::draw};
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute, queue,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{io, time::Duration};

pub(super) fn run_tui(mut app: App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    queue!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = (|| -> Result<()> {
        let mut redraw = true;
        let mut scrollbars_visible = app.scrollbars_visible();
        loop {
            match app.poll_refresh() {
                Ok(changed) => redraw |= changed,
                Err(error) => {
                    app.message = error.to_string();
                    redraw = true;
                }
            }
            let was_loading = app.pending_diff.is_some();
            if app.poll_diff() {
                let size = terminal.size()?;
                let viewport =
                    super::layout::active_diff_viewport_height(&app, size.width, size.height);
                super::navigation::scroll_diff_selection_into_view(&mut app, viewport);
                redraw = true;
            }
            redraw |= was_loading && app.pending_diff.is_none();
            let visible = app.scrollbars_visible();
            redraw |= visible != scrollbars_visible;
            scrollbars_visible = visible;
            if redraw {
                terminal.draw(|frame| draw(frame, &app))?;
                redraw = false;
            }
            let poll_interval = if app.pending_diff.is_some() || app.refresh_pending {
                16
            } else {
                80
            };
            if event::poll(Duration::from_millis(poll_interval))? {
                redraw = true;
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        let size = terminal.size()?;
                        if handle_key(&mut app, key.code, size.width, size.height)? {
                            break;
                        }
                    }
                    Event::Mouse(mouse) => {
                        let size = terminal.size()?;
                        handle_mouse(&mut app, mouse, size.width, size.height)?
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
            if app.input.is_none()
                && app.last_refresh.elapsed() >= Duration::from_millis(450)
                && let Err(error) = app.refresh()
            {
                app.message = error.to_string();
                redraw = true;
            }
        }
        Ok(())
    })();
    let settings_result = if result.is_ok() {
        app.save_settings()
    } else {
        Ok(())
    };
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result?;
    settings_result
}

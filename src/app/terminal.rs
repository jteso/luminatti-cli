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
        loop {
            terminal.draw(|frame| draw(frame, &app))?;
            if event::poll(Duration::from_millis(80))? {
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

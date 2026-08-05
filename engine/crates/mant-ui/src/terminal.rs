//! Crossterm lifecycle boundary that always restores the host terminal.

use std::{io, panic, time::Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use mant_ast::QueryBundle;
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{App, UpdateOutcome};

/// Run the interactive frontend until the user requests exit.
///
/// # Errors
///
/// Returns terminal setup, event, drawing, or restoration errors.
pub fn run(bundle: &QueryBundle) -> io::Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    let mut guard = TerminalGuard { active: true };
    // Install the restoration guard before either terminal command can fail.
    // Otherwise an unsupported mouse/alternate-screen sequence could leave the
    // caller in raw mode without ever entering the event loop.
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(bundle);

    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| -> io::Result<()> {
        let mut redraw = true;
        while !app.should_quit() {
            let now = Instant::now();
            redraw |= app.tick(now).needs_redraw();
            if redraw {
                terminal.draw(|frame| app.draw(frame))?;
                redraw = false;
            }
            let Some(timeout) = app.next_wakeup(Instant::now()) else {
                redraw |= route_event(&mut app, &event::read()?).needs_redraw();
                continue;
            };
            if !event::poll(timeout)? {
                continue;
            }
            redraw |= route_event(&mut app, &event::read()?).needs_redraw();
        }
        Ok(())
    }));

    let restore_result = guard.restore();
    match result {
        Ok(run_result) => run_result.and(restore_result),
        Err(payload) => {
            let _ = restore_result;
            panic::resume_unwind(payload);
        }
    }
}

fn route_event(app: &mut App, event: &Event) -> UpdateOutcome {
    match event {
        Event::Key(key) if key.is_press() => app.handle_key(*key),
        Event::Mouse(mouse) => app.handle_mouse(*mouse),
        Event::Resize(_, _) => UpdateOutcome::Redraw,
        Event::FocusGained | Event::FocusLost | Event::Paste(_) | Event::Key(_) => {
            UpdateOutcome::Unchanged
        }
    }
}

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn restore(&mut self) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        }
    }
}

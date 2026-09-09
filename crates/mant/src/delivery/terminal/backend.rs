//! Ratatui delegates cursor ownership to the same session ledger as setup.
use std::{
    cell::RefCell,
    io::{self, Write},
    rc::Rc,
};

use crossterm::{cursor, event, execute, terminal};
use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
};

use crate::delivery::terminal_lease::{TerminalLease, TerminalOps, TerminalResource};

pub(super) type SharedLease<O> = Rc<RefCell<TerminalLease<O>>>;

/// All calls occur on the TUI thread. A reentrant cleanup must return an IO
/// error instead of panicking while another operation is already unwinding.
fn with_lease<O: TerminalOps>(
    lease: &SharedLease<O>,
    operation: impl FnOnce(&mut TerminalLease<O>) -> io::Result<()>,
) -> io::Result<()> {
    let mut lease = lease
        .try_borrow_mut()
        .map_err(|_| io::Error::other("terminal restoration is already in progress"))?;
    operation(&mut lease)
}

pub(super) fn restore<O: TerminalOps>(lease: &SharedLease<O>) -> io::Result<()> {
    with_lease(lease, TerminalLease::restore)
}

/// The order remains raw mode, alternate screen, then mouse capture. Cursor
/// acquisition happens only when Ratatui actually requests it during drawing.
pub(super) fn acquire_tui<O: TerminalOps>(lease: &SharedLease<O>) -> io::Result<()> {
    for resource in [
        TerminalResource::RawMode,
        TerminalResource::AlternateScreen,
        TerminalResource::MouseCapture,
    ] {
        with_lease(lease, |lease| lease.acquire(resource))?;
    }
    Ok(())
}

pub(super) struct CrosstermTerminalOps {
    output: io::Stdout,
}

impl CrosstermTerminalOps {
    pub(super) fn new() -> Self {
        Self {
            output: io::stdout(),
        }
    }
}

impl TerminalOps for CrosstermTerminalOps {
    fn acquire(&mut self, resource: TerminalResource) -> io::Result<()> {
        match resource {
            TerminalResource::RawMode => terminal::enable_raw_mode(),
            TerminalResource::AlternateScreen => {
                execute!(self.output, terminal::EnterAlternateScreen)
            }
            TerminalResource::MouseCapture => execute!(self.output, event::EnableMouseCapture),
            TerminalResource::HiddenCursor => execute!(self.output, cursor::Hide),
        }
    }

    fn release(&mut self, resource: TerminalResource) -> io::Result<()> {
        match resource {
            TerminalResource::RawMode => terminal::disable_raw_mode(),
            TerminalResource::AlternateScreen => {
                execute!(self.output, terminal::LeaveAlternateScreen)
            }
            TerminalResource::MouseCapture => execute!(self.output, event::DisableMouseCapture),
            TerminalResource::HiddenCursor => execute!(self.output, cursor::Show),
        }
    }
}

/// Drawing and geometry are unchanged Crossterm operations. Only cursor mode
/// transitions pass through the shared ledger, including Ratatui's Drop path.
pub(super) struct LeasedBackend<W: Write, O: TerminalOps> {
    inner: CrosstermBackend<W>,
    lease: SharedLease<O>,
}

impl<W: Write, O: TerminalOps> LeasedBackend<W, O> {
    pub(super) fn new(output: W, lease: SharedLease<O>) -> Self {
        Self {
            inner: CrosstermBackend::new(output),
            lease,
        }
    }
}

impl<W: Write, O: TerminalOps> Backend for LeasedBackend<W, O> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn append_lines(&mut self, count: u16) -> io::Result<()> {
        self.inner.append_lines(count)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        with_lease(&self.lease, |lease| {
            if lease.has_pending(TerminalResource::HiddenCursor) {
                Ok(())
            } else {
                lease.acquire(TerminalResource::HiddenCursor)
            }
        })
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        with_lease(&self.lease, |lease| {
            lease.release_one(TerminalResource::HiddenCursor)
        })
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, region: ClearType) -> io::Result<()> {
        self.inner.clear_region(region)
    }

    fn size(&self) -> io::Result<Size> {
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.inner)
    }
}

#[cfg(test)]
mod tests;

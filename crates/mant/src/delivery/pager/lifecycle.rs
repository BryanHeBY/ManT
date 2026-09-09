//! Process-owned pager modes. Every path locks stdout before the mode ledger.

use std::{
    io::{self, Write},
    panic::{self, AssertUnwindSafe, PanicHookInfo},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use parking_lot::Mutex;

use crate::delivery::terminal_lease::{TerminalLease, TerminalOps, TerminalResource};

use super::native::error::MinusError;

/// The mutex is private: neither a driver nor a signal monitor can invert the
/// stdout -> ledger lock order. `Option` lets Drop destroy the inner lease while
/// stdout is still locked, including its final best-effort release attempts.
pub(super) struct PagerTerminal<O: TerminalOps = PagerOperations> {
    lease: Mutex<Option<TerminalLease<O>>>,
    exited: Arc<AtomicBool>,
}

impl<O: TerminalOps> PagerTerminal<O> {
    pub(super) fn new(operations: O) -> Self {
        Self {
            lease: Mutex::new(Some(TerminalLease::new(operations))),
            exited: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(super) fn exited(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.exited)
    }

    pub(super) fn setup(&self) -> io::Result<()> {
        let _output = io::stdout().lock();
        self.ensure_running()?;
        let mut owner = self.lease.lock();
        let lease = owner
            .as_mut()
            .ok_or_else(|| io::Error::other("pager terminal is closed"))?;
        for resource in [
            TerminalResource::AlternateScreen,
            TerminalResource::RawMode,
            TerminalResource::MouseCapture,
            TerminalResource::HiddenCursor,
        ] {
            if let Err(error) = lease.acquire(resource) {
                // Retain setup as the primary error, but undo every attempted
                // mode before propagating it. Failed releases remain owned.
                self.exited.store(true, Ordering::SeqCst);
                let _ = lease.restore();
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) fn restore(&self) -> io::Result<()> {
        self.restore_then(|| Ok(()))
    }

    /// The signal monitor must retain stdout through default termination, so
    /// the reactor cannot redraw after leaving the alternate screen.
    pub(super) fn restore_then(&self, after: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
        self.exited.store(true, Ordering::SeqCst);
        let _output = io::stdout().lock();
        let restored = {
            let mut owner = self.lease.lock();
            owner.as_mut().map_or(Ok(()), TerminalLease::restore)
        };
        let result = after();
        restored.and(result)
    }

    pub(super) fn ensure_running(&self) -> io::Result<()> {
        ensure_running(&self.exited)
    }

    /// Search temporarily shows the cursor, but cannot take new ownership
    /// after cancellation or cleanup. A failed Show/Hide remains in the ledger.
    pub(super) fn cursor_visible(&self, visible: bool) -> io::Result<()> {
        let _output = io::stdout().lock();
        self.ensure_running()?;
        let mut owner = self.lease.lock();
        let lease = owner
            .as_mut()
            .ok_or_else(|| io::Error::other("pager terminal is closed"))?;
        if visible {
            lease.release_one(TerminalResource::HiddenCursor)
        } else if lease.has_pending(TerminalResource::HiddenCursor) {
            Ok(())
        } else {
            lease.acquire(TerminalResource::HiddenCursor)
        }
    }

    pub(super) fn output<W: Write>(&self, output: W) -> PagerOutput<W> {
        PagerOutput {
            inner: output,
            exited: self.exited(),
        }
    }

    fn restore_for_panic(&self) {
        self.exited.store(true, Ordering::SeqCst);
        let _output = io::stdout().lock();
        // A panic may come from an acquisition/release while this thread holds
        // the ledger. Do not deadlock (or cause a second panic) in its hook;
        // the enclosing unwind boundary retries after that guard is released.
        if let Some(mut owner) = self.lease.try_lock()
            && let Some(lease) = owner.as_mut()
        {
            let _ = lease.restore();
        }
    }
}

impl<O: TerminalOps + Send + 'static> PagerTerminal<O> {
    /// Only the actual interactive path installs a hook. Catch locally so the
    /// previous process hook is restored before resuming an unwind; `set_hook`
    /// must not run while the current thread is already panicking.
    pub(super) fn interactive(
        self: &Arc<Self>,
        run: impl FnOnce() -> Result<(), MinusError>,
    ) -> Result<(), MinusError> {
        let terminal = Arc::clone(self);
        with_panic_cleanup(
            move || terminal.restore_for_panic(),
            || {
                let result = run();
                let cleanup = self.restore().map_err(MinusError::TerminalLifecycle);
                result.and(cleanup)
            },
        )
    }
}

fn ensure_running(exited: &AtomicBool) -> io::Result<()> {
    if exited.load(Ordering::SeqCst) {
        // Interrupted would make Write::write_all retry forever. A stopped
        // session must fail its pending draw, not acknowledge discarded bytes.
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "pager session has stopped",
        ))
    } else {
        Ok(())
    }
}

/// The stop check and physical write share stdout's lock with restoration.
/// This closes the check/write race: a peer cannot Hide or redraw after the
/// panic hook has restored modes. Direct-output writes do not use this wrapper.
pub(super) struct PagerOutput<W> {
    inner: W,
    exited: Arc<AtomicBool>,
}

impl<W: Write> Write for PagerOutput<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let _output = io::stdout().lock();
        ensure_running(&self.exited)?;
        self.inner.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        let _output = io::stdout().lock();
        ensure_running(&self.exited)?;
        self.inner.flush()
    }
}

impl<O: TerminalOps> Drop for PagerTerminal<O> {
    fn drop(&mut self) {
        let _output = io::stdout().lock();
        drop(self.lease.get_mut().take());
    }
}

type PanicHook = Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send + 'static>;

fn with_panic_cleanup<T>(cleanup: impl Fn() + Send + Sync + 'static, run: impl FnOnce() -> T) -> T {
    let cleanup = Arc::new(cleanup);
    let panic_cleanup = Arc::clone(&cleanup);
    let previous: Arc<PanicHook> = Arc::new(panic::take_hook());
    let chained = Arc::clone(&previous);
    panic::set_hook(Box::new(move |info| {
        panic_cleanup();
        chained(info);
    }));
    let result = panic::catch_unwind(AssertUnwindSafe(run));
    if result.is_err() {
        // Retry if the hook ran while an operation still held the ledger.
        // Unwinding has now released that guard.
        cleanup();
    }
    // Drop the installed hook before unwrapping the original one. Normally
    // this restores the exact original Box, without accumulating wrappers.
    drop(panic::take_hook());
    let previous =
        Arc::try_unwrap(previous).unwrap_or_else(|shared| Box::new(move |info| shared(info)));
    panic::set_hook(previous);
    match result {
        Ok(value) => value,
        Err(payload) => panic::resume_unwind(payload),
    }
}

pub(super) struct PagerOperations;

impl TerminalOps for PagerOperations {
    fn acquire(&mut self, resource: TerminalResource) -> io::Result<()> {
        let mut output = io::stdout();
        match resource {
            TerminalResource::AlternateScreen => {
                crossterm::execute!(output, crossterm::terminal::EnterAlternateScreen)
            }
            TerminalResource::RawMode => crossterm::terminal::enable_raw_mode(),
            TerminalResource::MouseCapture => {
                crossterm::execute!(output, crossterm::event::EnableMouseCapture)
            }
            TerminalResource::HiddenCursor => crossterm::execute!(output, crossterm::cursor::Hide),
        }
    }

    fn release(&mut self, resource: TerminalResource) -> io::Result<()> {
        let mut output = io::stdout();
        match resource {
            TerminalResource::AlternateScreen => {
                crossterm::execute!(output, crossterm::terminal::LeaveAlternateScreen)
            }
            TerminalResource::RawMode => crossterm::terminal::disable_raw_mode(),
            TerminalResource::MouseCapture => {
                crossterm::execute!(output, crossterm::event::DisableMouseCapture)
            }
            TerminalResource::HiddenCursor => crossterm::execute!(output, crossterm::cursor::Show),
        }
    }
}

#[cfg(test)]
mod tests;

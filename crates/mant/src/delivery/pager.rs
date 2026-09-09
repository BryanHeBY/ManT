//! Static terminal paging for process-owned text output.

use std::io;

// The upstream API cannot replace physical rows. Keep the pinned static/search
// implementation private and packaged, rather than shipping a Cargo patch that
// silently disappears when mant is installed from crates.io.
#[path = "pager/vendor/lib.rs"]
#[rustfmt::skip]
#[allow(dead_code, unused_imports, missing_docs, clippy::all, clippy::pedantic, clippy::nursery)]
mod native;
mod lifecycle;
mod search_overlay;
mod sgr;
use minus::{Pager, hooks::Hook};
use native as minus;

/// Display text through a less-like pager when it exceeds the terminal height.
///
/// `minus` prints directly when the content fits. The caller remains
/// responsible for invoking this only when both process streams are terminals;
/// redirected and protocol output must bypass this presentation boundary.
///
/// # Errors
///
/// Returns terminal setup, rendering, input, or restoration errors.
pub(crate) fn page_text(text: String, prompt: &str) -> io::Result<()> {
    let pager = Pager::new();
    pager.set_text(text).map_err(pager_error)?;
    pager.set_prompt(prompt).map_err(pager_error)?;

    // minus defaults to exiting the entire process after `q`. ManT owns the
    // process lifecycle, so replace that reserved callback and let page_all
    // restore the terminal before returning normally.
    pager
        .remove_hook(Hook::PostPagerExit, 1)
        .map_err(pager_error)?;
    pager
        .add_hook(Hook::PostPagerExit, 1, Box::new(|_| {}))
        .map_err(pager_error)?;
    run(pager)
}

#[cfg(not(unix))]
fn run(pager: Pager) -> io::Result<()> {
    let terminal = std::sync::Arc::new(lifecycle::PagerTerminal::new(lifecycle::PagerOperations));
    minus::page_all(pager, &terminal).map_err(pager_error)
}

#[cfg(unix)]
fn run(pager: Pager) -> io::Result<()> {
    use crate::delivery::signals::TerminationSignals;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;

    // minus has no public cancellation operation. Defer signals until setup
    // finishes, restore from an ordinary thread, then apply the default signal
    // action. Its alternate-screen command precedes raw-mode setup, so that
    // command alone is not a safe cleanup boundary.
    let termination = TerminationSignals::install()?;
    let terminal = Arc::new(lifecycle::PagerTerminal::new(lifecycle::PagerOperations));
    let ready = Arc::new(AtomicBool::new(false));
    let initialized = Arc::clone(&ready);
    pager
        .add_hook(
            Hook::PostPagerStart,
            0,
            Box::new(move |_| {
                initialized.store(true, Ordering::Release);
            }),
        )
        .map_err(pager_error)?;
    std::thread::scope(|scope| {
        let (done, finished) = mpsc::channel::<()>();
        let monitored_terminal = Arc::clone(&terminal);
        let monitor = scope.spawn(move || -> io::Result<()> {
            loop {
                if ready.load(Ordering::Acquire)
                    && let Some(signal) = termination.take()
                {
                    // Hold stdout through termination: pager drawing cannot
                    // race another write after we leave the alternate screen.
                    return monitored_terminal.restore_then(|| termination.terminate(signal));
                }
                match finished.recv_timeout(Duration::from_millis(20)) {
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    _ => {
                        // No-overflow output never starts the interactive UI.
                        return termination.take().map_or(Ok(()), |signal| {
                            monitored_terminal.restore_then(|| termination.terminate(signal))
                        });
                    }
                }
            }
        });
        let result = minus::page_all(pager, &terminal).map_err(pager_error);
        drop(done);
        let cleanup = monitor
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        result.and(cleanup)
    })
}

fn pager_error(error: minus::MinusError) -> io::Error {
    io::Error::other(error)
}

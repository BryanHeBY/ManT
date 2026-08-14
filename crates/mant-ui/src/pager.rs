//! Static terminal paging for process-owned text output.

use std::io;

use minus::{Pager, hooks::Hook};

/// Display text through a less-like pager when it exceeds the terminal height.
///
/// `minus` prints directly when the content fits. The caller remains
/// responsible for invoking this only when both process streams are terminals;
/// redirected and protocol output must bypass this presentation boundary.
///
/// # Errors
///
/// Returns terminal setup, rendering, input, or restoration errors.
pub fn page_text(text: String, prompt: &str) -> io::Result<()> {
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
    minus::page_all(pager).map_err(pager_error)
}

fn pager_error(error: minus::MinusError) -> io::Error {
    io::Error::other(error)
}

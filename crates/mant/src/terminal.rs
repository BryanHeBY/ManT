//! Operating-system terminal preparation at the native process boundary.

/// Prepare a terminal stdout stream to interpret ANSI escape sequences.
///
/// Redirected streams deliberately remain untouched: explicit color output may
/// still be consumed by another ANSI-aware process, while automatic color mode
/// treats them as non-terminal output before consulting this result.
pub(crate) fn prepare_ansi_output(output_is_terminal: bool) -> bool {
    output_is_terminal && platform::prepare_ansi_output()
}

#[cfg(not(windows))]
mod platform {
    pub(super) const fn prepare_ansi_output() -> bool {
        true
    }
}

#[cfg(windows)]
mod platform {
    use crossterm_winapi::{ConsoleMode, Handle};

    // https://learn.microsoft.com/windows/console/setconsolemode
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

    pub(super) fn prepare_ansi_output() -> bool {
        enable_virtual_terminal_processing().is_ok()
            || std::env::var("TERM").is_ok_and(|term| term != "dumb")
    }

    fn enable_virtual_terminal_processing() -> std::io::Result<()> {
        let mode = ConsoleMode::from(Handle::current_out_handle()?);
        let current = mode.mode()?;
        if current & ENABLE_VIRTUAL_TERMINAL_PROCESSING == 0 {
            mode.set_mode(current | ENABLE_VIRTUAL_TERMINAL_PROCESSING)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::prepare_ansi_output;

    #[test]
    fn redirected_output_is_never_prepared_for_automatic_color() {
        assert!(!prepare_ansi_output(false));
    }
}

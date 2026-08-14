//! Operating-system terminal preparation at the native process boundary.

use crate::arguments::ColorMode;

/// Prepare a terminal stdout stream to interpret ANSI escape sequences.
///
/// Redirected streams deliberately remain untouched: explicit color output may
/// still be consumed by another ANSI-aware process, while automatic color mode
/// treats them as non-terminal output before consulting this result.
pub(crate) fn prepare_ansi_output(output_is_terminal: bool) -> bool {
    output_is_terminal && platform::prepare_ansi_output()
}

/// Resolve one human-facing stream against the shared colour policy.
pub(crate) fn color_enabled(
    mode: ColorMode,
    stream_is_terminal: bool,
    ansi_supported: bool,
) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => automatic_color_enabled(stream_is_terminal, ansi_supported),
    }
}

fn automatic_color_enabled(stream_is_terminal: bool, ansi_supported: bool) -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if environment_flag("CLICOLOR_FORCE").is_some_and(|enabled| enabled) {
        return true;
    }
    if std::env::var("TERM").ok().as_deref() == Some("dumb")
        || environment_flag("CLICOLOR").is_some_and(|enabled| !enabled)
    {
        return false;
    }
    stream_is_terminal && ansi_supported
}

fn environment_flag(name: &str) -> Option<bool> {
    std::env::var_os(name).map(|value| !value.is_empty() && value != "0")
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
        enable_virtual_terminal_processing(Handle::current_out_handle).is_ok()
            || std::env::var("TERM").is_ok_and(|term| term != "dumb")
    }

    fn enable_virtual_terminal_processing(
        handle: fn() -> std::io::Result<Handle>,
    ) -> std::io::Result<()> {
        let mode = ConsoleMode::from(handle()?);
        let current = mode.mode()?;
        if current & ENABLE_VIRTUAL_TERMINAL_PROCESSING == 0 {
            mode.set_mode(current | ENABLE_VIRTUAL_TERMINAL_PROCESSING)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{color_enabled, prepare_ansi_output};
    use crate::arguments::ColorMode;

    #[test]
    fn redirected_output_is_never_prepared_for_automatic_color() {
        assert!(!prepare_ansi_output(false));
    }

    #[test]
    fn explicit_colour_modes_override_stream_detection() {
        assert!(color_enabled(ColorMode::Always, false, false));
        assert!(!color_enabled(ColorMode::Never, true, true));
    }
}

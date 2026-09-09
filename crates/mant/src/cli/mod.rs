//! CLI parsing and injectable stream execution; no process terminal ownership.
use crate::{
    arguments::{self, ColorMode},
    error::{report_argument_error, report_failure},
    host::CliHost,
    output_policy::{TerminalCapabilities, resolve_process_presentation},
    presentation,
};
use std::io::{Read, Write};

mod execute;
mod query;
pub(crate) use execute::run_command;

pub(crate) fn run_with_host(
    arguments: &[String],
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
) -> u8 {
    let diagnostics_color = arguments::requested_color(arguments) == ColorMode::Always;
    let mut command = match arguments::parse(arguments) {
        Ok(command) => command,
        Err(error) => return report_argument_error(&error, diagnostics),
    };

    if let Err(error) = resolve_process_presentation(&mut command, TerminalCapabilities::detached())
    {
        return report_failure(&error, diagnostics, diagnostics_color);
    }

    run_command(
        command,
        input,
        output,
        diagnostics,
        host,
        diagnostics_color,
        presentation::OutputTarget::Stream,
    )
}

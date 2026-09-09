#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod application;
mod arguments;
mod cli;
#[cfg(feature = "tui")]
mod clipboard;
mod delivery;
mod doctor;
mod error;
#[cfg(feature = "tui")]
mod external;
mod host;
mod json_boundary;
#[cfg(feature = "mcp")]
mod mcp;
mod output_policy;
mod presentation;
mod request_input;
mod schema_output;
mod terminal;

use cli::run_with_host;
use host::SystemHost;
use std::io::{Read, Write};

mod process;
pub use process::run_process;

/// Exact stdio protocol exposed to external process clients.
pub use mant_protocol::CLI_PROTOCOL_VERSION;

/// Run one CLI invocation using explicit streams and return its exit status.
///
/// Keeping the process streams injectable makes malformed protocol requests
/// testable without consulting host manual sources or a tldr client.
pub fn run(
    arguments: &[String],
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> u8 {
    run_with_host(
        arguments,
        input,
        output,
        diagnostics,
        &SystemHost::default(),
    )
}

#[cfg(test)]
mod tests;

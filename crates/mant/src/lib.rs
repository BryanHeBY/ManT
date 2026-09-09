#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

mod application;
mod arguments;
mod cli;
mod clipboard;
mod delivery;
mod doctor;
mod error;
mod external;
mod host;
mod json_boundary;
mod mcp;
mod output_policy;
mod presentation;
mod request_input;
mod schema_output;
mod terminal;

use std::io::{self, IsTerminal, Read, Write};

use application::request_for_navigation;
use arguments::{Command, DisplayMode, OutputOptions, QuerySource};
use cli::{run_command, run_with_host};
use clipboard::SystemClipboard;
use error::{Failure, report_failure, report_process_argument_error};
use external::open_uri as open_external_uri;
use host::{CliHost, SystemHost};
use mant_engine::LoadPolicy;
use mant_protocol::{CatalogQuery, QueryView};
use output_policy::{TerminalCapabilities, TerminalKind, resolve_process_presentation};

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

/// Run one native-process invocation, including the long-lived MCP mode.
///
/// The conventional CLI keeps injectable streams through [`run`], while MCP
/// owns operating-system stdio because the protocol reserves it exclusively
/// for newline-delimited JSON-RPC messages.
pub async fn run_process(arguments: &[String]) -> u8 {
    let requested_color = arguments::requested_color(arguments);
    let mut command = match arguments::parse_process(arguments) {
        Ok(command) => command,
        Err(error) => return report_process_argument_error(&error),
    };

    if matches!(command, Command::Mcp) {
        return mcp::run_stdio().await;
    }
    let host = SystemHost::default();
    let mut diagnostics = anstream::AutoStream::new(io::stderr(), requested_color.into()).lock();
    let output_terminal = io::stdout().is_terminal();
    let input_terminal = io::stdin().is_terminal();
    let terminal_kind = if std::env::var("TERM").ok().as_deref() == Some("dumb") {
        TerminalKind::Dumb
    } else {
        TerminalKind::Capable
    };
    let output_ansi_supported = terminal::prepare_ansi_output(output_terminal);
    let terminal = TerminalCapabilities {
        input: input_terminal,
        output: output_terminal,
        color: terminal::color_enabled(requested_color, output_terminal, output_ansi_supported),
        kind: terminal_kind,
    };

    let display = match resolve_process_presentation(&mut command, terminal) {
        Ok(display) => display,
        Err(error) => return report_failure(&error, &mut diagnostics, true),
    };
    if display == DisplayMode::Tui {
        return run_interactive(command, &mut diagnostics, &host, true);
    }
    if display == DisplayMode::Pager {
        return run_paged(command, &mut diagnostics, &host, true);
    }

    run_command(
        command,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &mut diagnostics,
        &host,
        true,
        if terminal.output {
            presentation::OutputTarget::Terminal
        } else {
            presentation::OutputTarget::Stream
        },
    )
}

/// Buffer one successful human-readable result before lending the terminal to the pager.
/// The exit status and stderr stay owned by the command even if the user quits paging.
fn run_paged(
    command: Command,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
    diagnostics_color: bool,
) -> u8 {
    let mut output = Vec::new();
    let status = run_command(
        command,
        &mut io::empty(),
        &mut output,
        diagnostics,
        host,
        diagnostics_color,
        presentation::OutputTarget::Terminal,
    );
    if output.is_empty() {
        return status;
    }
    let rendered = match String::from_utf8(output) {
        Ok(rendered) => rendered,
        Err(error) => {
            return report_failure(&Failure::operational(error), diagnostics, diagnostics_color);
        }
    };
    match delivery::pager::page_text(rendered, "mant") {
        Ok(()) => status,
        Err(error) => report_failure(&Failure::operational(error), diagnostics, diagnostics_color),
    }
}

/// Load one full query and hand the normalized document directly to Ratatui.
fn run_interactive(
    command: Command,
    diagnostics: &mut dyn Write,
    host: &dyn CliHost,
    diagnostics_color: bool,
) -> u8 {
    let Command::Query {
        source,
        presentation:
            OutputOptions {
                display: DisplayMode::Tui,
                ..
            },
        policy,
        ..
    } = command
    else {
        return report_failure(
            &Failure::usage("interactive mode requires a document query"),
            diagnostics,
            diagnostics_color,
        );
    };
    let (query, scope_documents) = match source {
        QuerySource::Arguments(request) => {
            if !matches!(request.view, QueryView::Full {}) {
                return report_failure(
                    &Failure::usage("interactive mode requires the complete document view"),
                    diagnostics,
                    diagnostics_color,
                );
            }
            let query = match application::read_full(&request, policy, host) {
                Ok(query) => query,
                Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
            };
            (query.clone(), vec![query])
        }
        QuerySource::ScopeArguments { scope, view: None } => {
            if policy != LoadPolicy::Combined {
                return report_failure(
                    &Failure::usage("--manual and --tldr do not apply to document scopes"),
                    diagnostics,
                    diagnostics_color,
                );
            }
            let loaded = match host.resolve_scope(&scope) {
                Ok(loaded) => loaded,
                Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
            };
            let Some(query) = loaded.documents().first().cloned() else {
                return report_failure(
                    &Failure::operational("document scope resolved no readable documents"),
                    diagnostics,
                    diagnostics_color,
                );
            };
            (query, loaded.into_parts().1)
        }
        QuerySource::ScopeArguments { view: Some(_), .. } => {
            return report_failure(
                &Failure::usage("interactive mode does not accept --search or --explain"),
                diagnostics,
                diagnostics_color,
            );
        }
        QuerySource::StdinJson | QuerySource::InputStdin { .. } => {
            return report_failure(
                &Failure::usage("interactive mode requires a registered document selector"),
                diagnostics,
                diagnostics_color,
            );
        }
    };
    let catalog = match host.discover(&CatalogQuery::default()) {
        Ok(catalog) => catalog,
        Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
    };
    let mut clipboard = SystemClipboard::default();
    match delivery::terminal::run_with_catalog_and_scope_and_copy(
        &query,
        catalog,
        &scope_documents,
        |catalog_query| host.discover(catalog_query).map_err(Failure::into_message),
        |target| {
            let (request, policy) = request_for_navigation(target);
            application::read_full(&request, policy, host).map_err(Failure::into_message)
        },
        open_external_uri,
        |request| clipboard.copy(request),
    ) {
        Ok(()) => 0,
        Err(error) => report_failure(&Failure::operational(error), diagnostics, diagnostics_color),
    }
}

#[cfg(test)]
mod tests;

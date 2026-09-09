//! Native process policy, application dispatch and terminal delivery.

#[cfg(any(feature = "tui", feature = "pager"))]
use crate::delivery;
#[cfg(feature = "tui")]
use crate::{application, clipboard, external};
use crate::{arguments, error, host, output_policy, presentation, terminal};
#[cfg(any(feature = "tui", feature = "pager"))]
use std::io::Write;
use std::io::{self, IsTerminal};
#[cfg(feature = "tui")]
use std::sync::Arc;

use crate::cli::run_command;
#[cfg(feature = "tui")]
use application::request_for_navigation;
use arguments::{Command, DisplayMode};
#[cfg(feature = "tui")]
use arguments::{OutputOptions, QuerySource};
#[cfg(feature = "tui")]
use clipboard::SystemClipboard;
use error::{Failure, report_failure, report_process_argument_error};
#[cfg(feature = "tui")]
use external::open_uri as open_external_uri;
#[cfg(any(feature = "tui", feature = "pager"))]
use host::CliHost;
use host::SystemHost;
#[cfg(feature = "tui")]
use mant_engine::LoadPolicy;
#[cfg(feature = "tui")]
use mant_protocol::{CatalogQuery, QueryView};
use output_policy::{TerminalCapabilities, TerminalKind, resolve_process_presentation};

/// Run one native-process invocation, including the long-lived MCP mode.
///
/// The conventional CLI keeps injectable streams through [`crate::run`], while MCP
/// owns operating-system stdio because the protocol reserves it exclusively
/// for newline-delimited JSON-RPC messages.
/// An async runtime is created only for an explicitly selected MCP session;
/// conventional CLI execution does not require a caller-provided runtime.
#[must_use]
pub fn run_process(arguments: &[String]) -> u8 {
    let requested_color = arguments::requested_color(arguments);
    let mut command = match arguments::parse_process(arguments) {
        Ok(command) => command,
        Err(error) => return report_process_argument_error(&error),
    };

    if matches!(command, Command::Mcp) {
        return run_mcp();
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
        #[cfg(feature = "tui")]
        return run_interactive(command, &mut diagnostics, &host, true);
        #[cfg(not(feature = "tui"))]
        return report_failure(
            &Failure::usage("TUI is unavailable in this build"),
            &mut diagnostics,
            true,
        );
    }
    if display == DisplayMode::Pager {
        #[cfg(feature = "pager")]
        return run_paged(command, &mut diagnostics, &host, true);
        #[cfg(not(feature = "pager"))]
        return report_failure(
            &Failure::usage("pager is unavailable in this build"),
            &mut diagnostics,
            true,
        );
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
#[cfg(feature = "pager")]
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
#[cfg(feature = "tui")]
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
    let scope_documents = match source {
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
            vec![Arc::new(query)]
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
            loaded.into_parts().1.into_iter().map(Arc::new).collect()
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
    let Some(query) = scope_documents.first().cloned() else {
        return report_failure(
            &Failure::operational("document scope resolved no readable documents"),
            diagnostics,
            diagnostics_color,
        );
    };
    let catalog = match host.discover(&CatalogQuery::default()) {
        Ok(catalog) => catalog,
        Err(error) => return report_failure(&error, diagnostics, diagnostics_color),
    };
    let mut clipboard = SystemClipboard::default();
    let mut discover =
        |catalog_query: &CatalogQuery| host.discover(catalog_query).map_err(Failure::into_message);
    let mut open = |target: &mant_protocol::DocumentOpenTarget| {
        let (request, policy) = request_for_navigation(target);
        application::read_full(&request, policy, host).map_err(Failure::into_message)
    };
    let mut external = open_external_uri;
    let mut copy = |request| clipboard.copy(request);
    match delivery::terminal::run_reader(
        mant_ui::ReaderOptions {
            current: query,
            catalog,
            scope: scope_documents,
        },
        &mut mant_ui::ReaderServices {
            discover_documents: Some(&mut discover),
            open_document: Some(&mut open),
            open_external: Some(&mut external),
            copy_to_clipboard: Some(&mut copy),
        },
    ) {
        Ok(()) => 0,
        Err(error) => report_failure(&Failure::operational(error), diagnostics, diagnostics_color),
    }
}

#[cfg(feature = "mcp")]
fn run_mcp() -> u8 {
    let runtime = match mcp_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            return report_failure(
                &Failure::operational(error),
                &mut io::stderr().lock(),
                false,
            );
        }
    };
    runtime.block_on(crate::mcp::run_stdio())
}

#[cfg(feature = "mcp")]
fn mcp_runtime() -> io::Result<tokio::runtime::Runtime> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(io::Error::other(
            "the native MCP entry point must run outside an existing async runtime",
        ));
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

#[cfg(all(test, feature = "mcp"))]
mod tests {
    #[tokio::test]
    async fn nested_process_runtime_is_rejected_before_stdio_acquisition() {
        let error = super::mcp_runtime().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside an existing async runtime")
        );
    }
}

#[cfg(not(feature = "mcp"))]
fn run_mcp() -> u8 {
    report_failure(
        &Failure::usage("MCP is unavailable in this build"),
        &mut io::stderr().lock(),
        false,
    )
}

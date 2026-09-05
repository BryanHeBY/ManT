//! Pure process output decisions, separate from terminal detection and rendering.

use crate::arguments::{CatalogPaging, ColorMode, Command, QueryFormat, QueryPresentation};
use crate::error::Failure;

/// Terminal capabilities consulted only by the OS process entry point.
///
/// The injectable [`crate::run`] boundary intentionally remains deterministic and
/// treats `Auto` as text output without automatic terminal styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalCapabilities {
    pub(crate) input: bool,
    pub(crate) output: bool,
    pub(crate) color: bool,
    pub(crate) kind: TerminalKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalKind {
    Capable,
    Dumb,
}

pub(crate) fn should_page_catalog(command: &Command, terminal: TerminalCapabilities) -> bool {
    terminal.input
        && terminal.output
        && terminal.kind == TerminalKind::Capable
        && matches!(
            command,
            Command::Catalog {
                format: QueryFormat::Text,
                paging: CatalogPaging::Auto,
                ..
            }
        )
}

/// Resolve terminal-sensitive defaults without coupling argument parsing to
/// operating-system streams.
pub(crate) fn resolve_process_presentation(
    command: &mut Command,
    terminal: TerminalCapabilities,
) -> Result<(), Failure> {
    if let Command::Doctor { color, .. } = command {
        if *color == ColorMode::Auto {
            *color = if terminal.output && terminal.color {
                ColorMode::Always
            } else {
                ColorMode::Never
            };
        }
        return Ok(());
    }
    let Command::Query { presentation, .. } = command else {
        return Ok(());
    };
    match *presentation {
        QueryPresentation::Auto(_)
            if terminal.input && terminal.output && terminal.kind == TerminalKind::Capable =>
        {
            *presentation = QueryPresentation::Interactive;
        }
        QueryPresentation::Auto(color) => {
            *presentation = QueryPresentation::Output {
                format: QueryFormat::Text,
                color: match color {
                    ColorMode::Auto if terminal.output && terminal.color => ColorMode::Always,
                    ColorMode::Auto => ColorMode::Never,
                    explicit => explicit,
                },
            };
        }
        QueryPresentation::Interactive
            if !terminal.input || !terminal.output || terminal.kind == TerminalKind::Dumb =>
        {
            return Err(Failure::usage(
                "interactive view requires a capable input and output terminal; omit --ui or select --format",
            ));
        }
        QueryPresentation::Tldr(ColorMode::Auto) => {
            *presentation = QueryPresentation::Tldr(if terminal.output && terminal.color {
                ColorMode::Always
            } else {
                ColorMode::Never
            });
        }
        QueryPresentation::Output {
            format,
            color: ColorMode::Auto,
        } => {
            *presentation = QueryPresentation::Output {
                format,
                color: if terminal.output && terminal.color {
                    ColorMode::Always
                } else {
                    ColorMode::Never
                },
            };
        }
        QueryPresentation::Interactive
        | QueryPresentation::Output {
            color: ColorMode::Always | ColorMode::Never,
            ..
        }
        | QueryPresentation::Tldr(ColorMode::Always | ColorMode::Never) => {}
    }
    Ok(())
}

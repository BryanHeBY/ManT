//! Maps engine failures onto the CLI's two stable exit-status classes.

use std::io::Write;

use anstyle::{AnsiColor, Style};
use mant_engine::{ProjectionError, QueryError, QueryExecutionError, SearchError};

const ERROR_STYLE: Style = AnsiColor::Red.on_default().bold();
const WARNING_STYLE: Style = AnsiColor::Yellow.on_default().bold();
const ADVICE_STYLE: Style = AnsiColor::Cyan.on_default().bold();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureKind {
    Usage,
    Operational,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Failure {
    kind: FailureKind,
    message: String,
}

impl Failure {
    pub(super) fn usage(message: impl std::fmt::Display) -> Self {
        Self {
            kind: FailureKind::Usage,
            message: message.to_string(),
        }
    }

    pub(super) fn operational(message: impl std::fmt::Display) -> Self {
        Self {
            kind: FailureKind::Operational,
            message: message.to_string(),
        }
    }

    pub(super) fn into_message(self) -> String {
        self.message
    }

    #[cfg(test)]
    pub(super) fn message(&self) -> &str {
        &self.message
    }
}

pub(super) fn query_failure(error: QueryError) -> Failure {
    match error {
        QueryError::EmptyName
        | QueryError::InvalidManualSection
        | QueryError::TldrManualSection { .. }
        | QueryError::InvalidSource
        | QueryError::ConflictingSourceSelectors
        | QueryError::EmptyMarkdownPath
        | QueryError::UnsupportedInputFormat { .. }
        | QueryError::EmptySelection
        | QueryError::EmptySelector
        | QueryError::EmptyEntry
        | QueryError::InvalidSearch(_) => Failure::usage(error),
        QueryError::Markdown { .. }
        | QueryError::EmptyMarkdown { .. }
        | QueryError::Registry { .. }
        | QueryError::Manual(_)
        | QueryError::ManualWithTldr { .. }
        | QueryError::TldrNotFound { .. }
        | QueryError::Tldr { .. }
        | QueryError::NoReadableContent { .. } => Failure::operational(error),
    }
}

fn projection_failure(error: ProjectionError) -> Failure {
    match error {
        ProjectionError::MissingContent { .. } => Failure::operational(error),
        ProjectionError::UnknownSelector { document, selector } => Failure::usage(format!(
            "document '{document}' has no outline node '{selector}'\nhint: run `mant {document} --outline=entries --format json` for available selectors and diagnostics"
        )),
        ProjectionError::SelectorFoundOnlyInText {
            document,
            selector,
            location,
            line,
        } => Failure::usage(format!(
            "document '{document}' has no semantic entry '{selector}'\nnote: that text appears in outline node {} ({}) at line {line}\nhint: use --search to inspect the matching document text",
            location.path(),
            location.title()
        )),
        ProjectionError::ExplanationRequiresEntry { document, selector } => {
            Failure::usage(format!(
                "document '{document}' outline node '{selector}' is not a semantic entry\nhint: use --node to read sections"
            ))
        }
        ProjectionError::EmptySelection
        | ProjectionError::EmptySelector
        | ProjectionError::AmbiguousSelector { .. } => Failure::usage(error),
    }
}

pub(super) fn query_execution_failure(error: QueryExecutionError) -> Failure {
    match error {
        QueryExecutionError::Query(error) => query_failure(error),
        QueryExecutionError::Projection(error) => projection_failure(error),
        QueryExecutionError::Search(error) => search_failure(error),
    }
}

fn search_failure(error: SearchError) -> Failure {
    Failure::usage(error)
}

pub(super) fn report_failure(error: &Failure, diagnostics: &mut dyn Write, color: bool) -> u8 {
    let mut lines = error.message.split('\n');
    let first = lines.next().unwrap_or_default();
    if color {
        let _ = writeln!(diagnostics, "{ERROR_STYLE}mant:{ERROR_STYLE:#} {first}");
    } else {
        let _ = writeln!(diagnostics, "mant: {first}");
    }
    for line in lines {
        let _ = write_diagnostic_line(diagnostics, line, color);
    }
    if error.kind == FailureKind::Usage {
        if color {
            let _ = writeln!(
                diagnostics,
                "{ADVICE_STYLE}Try{ADVICE_STYLE:#} 'mant --help' for more information."
            );
        } else {
            let _ = writeln!(diagnostics, "Try 'mant --help' for more information.");
        }
        2
    } else {
        1
    }
}

fn write_diagnostic_line(
    diagnostics: &mut dyn Write,
    line: &str,
    color: bool,
) -> std::io::Result<()> {
    if !color {
        return writeln!(diagnostics, "{line}");
    }
    for (label, style) in [
        ("warning:", WARNING_STYLE),
        ("hint:", ADVICE_STYLE),
        ("help:", ADVICE_STYLE),
        ("note:", ADVICE_STYLE),
    ] {
        if let Some(message) = line.strip_prefix(label) {
            return writeln!(diagnostics, "{style}{label}{style:#}{message}");
        }
    }
    writeln!(diagnostics, "{line}")
}

/// Preserve clap's actionable usage and suggestion text on the injected stream.
pub(super) fn report_argument_error(error: &clap::Error, diagnostics: &mut dyn Write) -> u8 {
    let rendered = error.to_string();
    let _ = diagnostics.write_all(rendered.as_bytes());
    if !rendered.ends_with('\n') {
        let _ = diagnostics.write_all(b"\n");
    }
    2
}

/// Let clap choose the native stdout/stderr stream and apply its configured
/// terminal color policy. Help and version are successful display results;
/// every other parser diagnostic retains the conventional usage status.
pub(super) fn report_process_argument_error(error: &clap::Error) -> u8 {
    let status = u8::try_from(error.exit_code()).unwrap_or(2);
    let _ = error.print();
    status
}

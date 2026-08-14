//! Maps engine failures onto the CLI's two stable exit-status classes.

use std::io::Write;

use mant_engine::{ProjectionError, QueryError, QueryExecutionError, SearchError};

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
        ProjectionError::EmptySelection
        | ProjectionError::EmptySelector
        | ProjectionError::UnknownSelector { .. }
        | ProjectionError::AmbiguousSelector { .. }
        | ProjectionError::ExplanationRequiresEntry { .. } => Failure::usage(error),
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

pub(super) fn report_failure(error: &Failure, diagnostics: &mut dyn Write) -> u8 {
    let _ = writeln!(diagnostics, "mant: {}", error.message);
    if error.kind == FailureKind::Usage {
        let _ = writeln!(diagnostics, "Try 'mant --help' for more information.");
        2
    } else {
        1
    }
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

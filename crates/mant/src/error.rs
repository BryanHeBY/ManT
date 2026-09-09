//! Maps engine failures onto the CLI's two stable exit-status classes.

use std::io::Write;

use anstyle::{AnsiColor, Style};
use mant_engine::{
    LoadError, ProjectionError, QueryError, QueryExecutionError, QueryValidationError,
    ScopeQueryError, SearchError,
};
use mant_protocol::sanitize_terminal_text;

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
            message: sanitized_message(message),
        }
    }

    pub(super) fn operational(message: impl std::fmt::Display) -> Self {
        Self {
            kind: FailureKind::Operational,
            message: sanitized_message(message),
        }
    }

    /// Construct an intentional multi-line usage diagnostic from independently
    /// sanitized lines. Dynamic data may not create a new terminal line.
    pub(super) fn usage_lines<I, T>(first: impl std::fmt::Display, lines: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: std::fmt::Display,
    {
        Self::with_lines(FailureKind::Usage, first, lines)
    }

    /// Construct an intentional multi-line operational diagnostic from
    /// independently sanitized lines.
    fn operational_lines<I, T>(first: impl std::fmt::Display, lines: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: std::fmt::Display,
    {
        Self::with_lines(FailureKind::Operational, first, lines)
    }

    fn with_lines<I, T>(kind: FailureKind, first: impl std::fmt::Display, lines: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: std::fmt::Display,
    {
        let mut message = sanitized_message(first);
        for line in lines {
            message.push('\n');
            message.push_str(&sanitized_message(line));
        }
        Self { kind, message }
    }

    pub(super) fn into_message(self) -> String {
        self.message
    }

    #[cfg(test)]
    pub(super) fn message(&self) -> &str {
        &self.message
    }
}

fn sanitized_message(message: impl std::fmt::Display) -> String {
    let message = message.to_string();
    sanitize_terminal_text(&message).into_owned()
}

pub(super) fn query_failure(error: QueryError) -> Failure {
    match error {
        QueryError::Load(error) => load_failure(error),
        QueryError::QueryValidation(error) => query_validation_failure(error),
    }
}

fn load_failure(error: LoadError) -> Failure {
    match error {
        LoadError::EmptyName
        | LoadError::InvalidManualSection
        | LoadError::TldrManualSection { .. }
        | LoadError::InvalidSource
        | LoadError::ConflictingSourceSelectors
        | LoadError::EmptyMarkdownPath
        | LoadError::UnsupportedInputFormat { .. }
        | LoadError::InvalidSelector { .. } => Failure::usage(error),
        LoadError::ManualWithTldr { error, topic } => Failure::operational_lines(
            error,
            [format!(
                "hint: a tldr entry is available; run `mant {topic} --tldr`"
            )],
        ),
        LoadError::Markdown { .. }
        | LoadError::EmptyMarkdown { .. }
        | LoadError::Registry { .. }
        | LoadError::Manual(_)
        | LoadError::TldrNotFound { .. }
        | LoadError::Tldr { .. }
        | LoadError::NoReadableContent { .. } => Failure::operational(error),
    }
}

fn query_validation_failure(error: QueryValidationError) -> Failure {
    match error {
        QueryValidationError::EmptySelection
        | QueryValidationError::TooManySelections { .. }
        | QueryValidationError::EmptySelector
        | QueryValidationError::InvalidContentSelector
        | QueryValidationError::InvalidReferenceProjection(_)
        | QueryValidationError::InvalidEntryKinds
        | QueryValidationError::EmptyEntry
        | QueryValidationError::InvalidViewSelector { .. } => Failure::usage(error),
        QueryValidationError::InvalidSearch(error) => search_failure(&error),
        QueryValidationError::InvalidExplanation(mant_engine::ExplanationError::MissingContent) => {
            Failure::operational("explanation requires readable content")
        }
        QueryValidationError::InvalidExplanation(error) => Failure::usage(error),
    }
}

fn projection_failure(error: ProjectionError) -> Failure {
    match error {
        ProjectionError::MissingContent { .. } => Failure::operational(error),
        ProjectionError::UnknownSelector { document, selector } => Failure::usage_lines(
            format!("document '{document}' has no outline node '{selector}'"),
            [format!(
                "hint: run `mant {document} --outline --outline-entries all --format json` for available selectors and diagnostics"
            )],
        ),
        ProjectionError::EmptySelection
        | ProjectionError::EmptySelector
        | ProjectionError::InvalidSelector
        | ProjectionError::InvalidReferenceProjection(_)
        | ProjectionError::TooManySelections { .. }
        | ProjectionError::AmbiguousSelector { .. } => Failure::usage(error),
    }
}

pub(super) fn query_execution_failure(error: QueryExecutionError) -> Failure {
    match error {
        QueryExecutionError::Query(error) => query_failure(error),
        QueryExecutionError::Projection(error) => projection_failure(error),
        QueryExecutionError::Search(error) => search_failure(&error),
    }
}

pub(super) fn scope_query_failure(error: ScopeQueryError) -> Failure {
    match error {
        ScopeQueryError::NoResolvedDocuments { .. } | ScopeQueryError::InvalidLoadedScope(_) => {
            Failure::operational(error)
        }
        ScopeQueryError::EmptyScope
        | ScopeQueryError::TooManyDocuments
        | ScopeQueryError::DepthLimit
        | ScopeQueryError::DocumentLimit
        | ScopeQueryError::TraversalLimitsRequireLinks
        | ScopeQueryError::DocumentSelector(_)
        | ScopeQueryError::EntrySelector(_) => Failure::usage(error),
        ScopeQueryError::Search(error) => search_failure(&error),
        ScopeQueryError::Explanation(error) => Failure::usage(error),
    }
}

fn search_failure(error: &SearchError) -> Failure {
    let message = error.to_string();
    let mut lines = message.lines();
    Failure::usage_lines(lines.next().unwrap_or_default(), lines)
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

#[cfg(test)]
mod tests {
    use mant_engine::SearchError;

    use super::{Failure, report_failure, search_failure};

    #[test]
    fn loading_and_query_validation_errors_keep_their_exit_categories() {
        use mant_engine::{LoadError, QueryError, QueryValidationError};
        for (error, expected_status, expected_message) in [
            (
                QueryError::Load(LoadError::EmptyName),
                2,
                "name must not be empty",
            ),
            (
                QueryError::Load(LoadError::InvalidSelector {
                    field: "document source",
                    error: mant_protocol::ScopeTextError::ControlCharacter,
                }),
                2,
                "document source must not contain control characters",
            ),
            (
                QueryError::QueryValidation(QueryValidationError::EmptyEntry),
                2,
                "semantic entry must not be empty",
            ),
            (
                QueryError::QueryValidation(QueryValidationError::InvalidExplanation(
                    mant_engine::ExplanationError::MissingContent,
                )),
                1,
                "explanation requires readable content",
            ),
            (
                QueryError::Load(LoadError::Markdown {
                    path: "demo.md".into(),
                    detail: "permission denied".into(),
                }),
                1,
                "could not load Markdown document 'demo.md': permission denied",
            ),
        ] {
            let failure = super::query_failure(error);
            assert_eq!(failure.message(), expected_message);
            assert_eq!(
                report_failure(&failure, &mut Vec::new(), false),
                expected_status
            );
        }
    }

    #[test]
    fn failure_messages_mask_dynamic_terminal_controls() {
        let error = Failure::operational("bad\u{1b}[2J\nnext\rline");
        assert_eq!(error.message(), "bad�[2J�next�line");

        let error = Failure::usage_lines("first\u{1b}[31m", ["hint: next\tline"]);
        let mut diagnostics = Vec::new();
        assert_eq!(report_failure(&error, &mut diagnostics, false), 2);
        assert_eq!(
            String::from_utf8(diagnostics).expect("diagnostics UTF-8"),
            "mant: first�[31m\nhint: next�line\nTry 'mant --help' for more information.\n"
        );

        let error =
            Failure::operational_lines("could not load topic\nforged", ["hint: retry\nforged"]);
        let mut diagnostics = Vec::new();
        assert_eq!(report_failure(&error, &mut diagnostics, false), 1);
        assert_eq!(
            String::from_utf8(diagnostics).expect("diagnostics UTF-8"),
            "mant: could not load topic�forged\nhint: retry�forged\n"
        );
    }

    #[test]
    fn regex_diagnostics_keep_trusted_line_structure() {
        let error = search_failure(&SearchError::InvalidPattern(
            "regex parse error:\n    (a\n    ^\nerror: unclosed group\u{1b}[31m".to_owned(),
        ));
        let mut diagnostics = Vec::new();

        assert_eq!(report_failure(&error, &mut diagnostics, false), 2);
        assert_eq!(
            String::from_utf8(diagnostics).expect("diagnostics UTF-8"),
            "mant: invalid search pattern: regex parse error:\n    (a\n    ^\nerror: unclosed group�[31m\nTry 'mant --help' for more information.\n"
        );
    }
}

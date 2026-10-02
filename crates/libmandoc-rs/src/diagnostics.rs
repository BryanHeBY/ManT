//! Converts libmandoc's textual findings into stable structured diagnostics.

/// Severity assigned by libmandoc's validation diagnostics.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticLevel {
    /// A construct is valid roff but unsupported by libmandoc.
    Unsupported,
    /// The source contains an error that may make output incomplete.
    Error,
    /// The source is recoverable but suspicious or non-portable.
    Warning,
    /// The source violates a style recommendation without changing meaning.
    Style,
}

/// Stable machine-readable classification for known safety findings.
///
/// Ordinary native findings have no stable code. The pinned native input
/// budget finding is classified without exposing upstream numeric values.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCode {
    /// Descendants beyond the owned syntax-tree depth limit were omitted.
    SyntaxTreeDepthLimit,
    /// Content beyond the native equation-tree depth limit was omitted.
    EquationTreeDepthLimit,
    /// A nested escape suffix was omitted by the native parser depth guard.
    EscapeDepthLimit,
    /// Native input execution reached a macro, expansion or replay budget.
    /// Safe partial content can include rejected syntax as literal words.
    InputProcessingLimit,
}

// Pinned mandoc_msg.c's MANDOCERR_ROFFLOOP. read.c, roff.c, eqn.c and
// mdoc_macro.c stop execution, omit a suffix or retain unexecuted literal
// syntax at this limit. Match the complete Error finding, never its severity
// or an arbitrary mention of an infinite loop.
const INPUT_PROCESSING_LIMIT_MESSAGE: &str = "input stack limit exceeded, infinite loop?";

pub(crate) const SYNTAX_TREE_DEPTH_MESSAGE: &str =
    "owned syntax tree exceeded the 256-level copy limit; deeper descendants were omitted";
pub(crate) const EQUATION_TREE_DEPTH_MESSAGE: &str =
    "equation tree exceeded the 256-level copy limit; deeper equation content was omitted";
pub(crate) const ESCAPE_DEPTH_MESSAGE: &str =
    "native escape argument exceeded the 256-level nesting limit; the remaining suffix was omitted";

/// Optional source location extracted from a libmandoc diagnostic prefix.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    /// One-based source line.
    pub line: u32,
    /// One-based source column.
    pub column: u32,
}

/// One non-fatal finding emitted while parsing a manual source.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    /// Stable wrapper classification, independent of the human-readable text.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub code: Option<DiagnosticCode>,
    /// Severity classified from libmandoc's diagnostic marker.
    pub level: DiagnosticLevel,
    /// Human-readable finding with the location prefix removed.
    pub message: String,
    /// Source position when libmandoc supplied a parseable prefix.
    pub location: Option<SourceLocation>,
}

impl Diagnostic {
    /// Return the stable classification for a wrapper-generated finding.
    ///
    /// Ordinary native messages return `None`; the pinned input-processing
    /// budget finding has a wrapper classification.
    #[must_use]
    pub fn code(&self) -> Option<DiagnosticCode> {
        self.code
    }
}

pub(crate) fn report_diagnostics(output: &str, escape_truncated: bool) -> Vec<Diagnostic> {
    let mut findings = parse_diagnostics(output);
    if escape_truncated {
        findings.push(Diagnostic {
            code: Some(DiagnosticCode::EscapeDepthLimit),
            level: DiagnosticLevel::Error,
            message: ESCAPE_DEPTH_MESSAGE.to_owned(),
            location: None,
        });
    }
    findings
}

pub(crate) fn parse_diagnostics(output: &str) -> Vec<Diagnostic> {
    output.lines().filter_map(parse_diagnostic).collect()
}

fn parse_diagnostic(line: &str) -> Option<Diagnostic> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (level, marker) = [
        (DiagnosticLevel::Unsupported, ": UNSUPP: "),
        (DiagnosticLevel::Error, ": ERROR: "),
        (DiagnosticLevel::Error, ": BADARG: "),
        (DiagnosticLevel::Error, ": SYSERR: "),
        (DiagnosticLevel::Warning, ": WARNING: "),
        (DiagnosticLevel::Style, ": STYLE: "),
    ]
    .into_iter()
    .find(|(_, marker)| line.contains(marker))
    .unwrap_or((DiagnosticLevel::Warning, ": "));
    let (prefix, message) = line.split_once(marker).unwrap_or(("", line));
    Some(Diagnostic {
        code: (level == DiagnosticLevel::Error && message == INPUT_PROCESSING_LIMIT_MESSAGE)
            .then_some(DiagnosticCode::InputProcessingLimit),
        level,
        message: message.to_owned(),
        location: source_location(prefix),
    })
}

fn source_location(prefix: &str) -> Option<SourceLocation> {
    let mut fields = prefix.rsplitn(3, ':');
    let column = fields.next()?.trim().parse().ok()?;
    let line = fields.next()?.trim().parse().ok()?;
    Some(SourceLocation { line, column })
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticCode, DiagnosticLevel, SourceLocation, parse_diagnostics};

    #[test]
    fn only_the_complete_native_error_identifies_an_input_processing_limit() {
        let diagnostics = parse_diagnostics(
            "mant: page.1:9:625: ERROR: input stack limit exceeded, infinite loop?\n\
             mant: page.1:10:1: WARNING: input stack limit exceeded, infinite loop?\n\
             mant: page.1:11:1: ERROR: input stack limit exceeded, infinite loop? extra\n\
             mant: page.1:12:1: ERROR: another error\n",
        );
        assert_eq!(
            diagnostics[0].code(),
            Some(DiagnosticCode::InputProcessingLimit)
        );
        assert_eq!(
            diagnostics[0].location,
            Some(SourceLocation {
                line: 9,
                column: 625
            })
        );
        assert!(
            diagnostics[1..]
                .iter()
                .all(|finding| finding.code().is_none())
        );
    }

    #[test]
    fn preserves_each_finding_and_classifies_known_levels() {
        let diagnostics = parse_diagnostics(
            "mant: page.1:8:2: UNSUPP: unsupported roff request: ab\n\
             mant: page.1:9:1: WARNING: skipping paragraph macro\n",
        );

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].level, DiagnosticLevel::Unsupported);
        assert_eq!(diagnostics[0].code(), None);
        assert_eq!(diagnostics[0].message, "unsupported roff request: ab");
        assert_eq!(
            diagnostics[0].location,
            Some(SourceLocation { line: 8, column: 2 })
        );
        assert_eq!(diagnostics[1].level, DiagnosticLevel::Warning);
    }
}

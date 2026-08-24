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

/// Stable machine-readable classification for wrapper-generated findings.
///
/// Native libmandoc findings do not expose a stable code and therefore leave
/// [`Diagnostic::code`] unset.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticCode {
    /// Descendants beyond the owned syntax-tree depth limit were omitted.
    SyntaxTreeDepthLimit,
    /// Content beyond the native equation-tree depth limit was omitted.
    EquationTreeDepthLimit,
}

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
    /// Severity classified from libmandoc's diagnostic marker.
    pub level: DiagnosticLevel,
    /// Stable wrapper-generated classification, when one is available.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub code: Option<DiagnosticCode>,
    /// Human-readable finding with the location prefix removed.
    pub message: String,
    /// Source position when libmandoc supplied a parseable prefix.
    pub location: Option<SourceLocation>,
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
        level,
        code: None,
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
    use super::{DiagnosticLevel, SourceLocation, parse_diagnostics};

    #[test]
    fn preserves_each_finding_and_classifies_known_levels() {
        let diagnostics = parse_diagnostics(
            "mant: page.1:8:2: UNSUPP: unsupported roff request: ab\n\
             mant: page.1:9:1: WARNING: skipping paragraph macro\n",
        );

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].level, DiagnosticLevel::Unsupported);
        assert_eq!(diagnostics[0].code, None);
        assert_eq!(diagnostics[0].message, "unsupported roff request: ab");
        assert_eq!(
            diagnostics[0].location,
            Some(SourceLocation { line: 8, column: 2 })
        );
        assert_eq!(diagnostics[1].level, DiagnosticLevel::Warning);
    }
}

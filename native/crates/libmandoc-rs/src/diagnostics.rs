//! Converts libmandoc's textual findings into stable structured diagnostics.

/// Severity assigned by libmandoc's validation diagnostics.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticLevel {
    Unsupported,
    Error,
    Warning,
    Style,
}

/// Optional source location extracted from a libmandoc diagnostic prefix.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLocation {
    pub line: u32,
    pub column: u32,
}

/// One non-fatal finding emitted while parsing a manual source.
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
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
        assert_eq!(diagnostics[0].message, "unsupported roff request: ab");
        assert_eq!(
            diagnostics[0].location,
            Some(SourceLocation { line: 8, column: 2 })
        );
        assert_eq!(diagnostics[1].level, DiagnosticLevel::Warning);
    }
}

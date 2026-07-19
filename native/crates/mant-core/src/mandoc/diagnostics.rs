//! Converts libmandoc's captured text findings into stable diagnostics.

use mant_ast::{Diagnostic, DiagnosticLevel};

pub(super) fn parse_diagnostics(output: &str) -> Vec<Diagnostic> {
    output
        .lines()
        .filter_map(|line| {
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
            let message = line
                .split_once(marker)
                .map_or(line, |(_, message)| message)
                .to_owned();
            Some(Diagnostic {
                level,
                code: None,
                message,
                source: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use mant_ast::DiagnosticLevel;

    use super::parse_diagnostics;

    #[test]
    fn preserves_each_finding_and_classifies_known_levels() {
        let diagnostics = parse_diagnostics(
            "mant: page.1:8:2: UNSUPP: unsupported roff request: ab\n\
             mant: page.1:9:1: WARNING: skipping paragraph macro\n",
        );

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].level, DiagnosticLevel::Unsupported);
        assert_eq!(diagnostics[0].message, "unsupported roff request: ab");
        assert_eq!(diagnostics[1].level, DiagnosticLevel::Warning);
    }
}

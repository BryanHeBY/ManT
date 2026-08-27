//! Adapts the engine's parser-neutral diagnostics into `ManT`'s document contract.

use mant_ir::{Diagnostic, DiagnosticLevel};

use super::syntax::{Diagnostic as MandocDiagnostic, DiagnosticLevel as MandocDiagnosticLevel};

pub(super) fn lower_diagnostics(input: &[MandocDiagnostic]) -> Vec<Diagnostic> {
    input
        .iter()
        .map(|diagnostic| Diagnostic {
            level: match diagnostic.level {
                MandocDiagnosticLevel::Unsupported => DiagnosticLevel::Unsupported,
                MandocDiagnosticLevel::Error => DiagnosticLevel::Error,
                MandocDiagnosticLevel::Warning => DiagnosticLevel::Warning,
                MandocDiagnosticLevel::Style => DiagnosticLevel::Style,
            },
            code: truncation_code(&diagnostic.message).map(str::to_owned),
            message: diagnostic.message.clone(),
            source: diagnostic.location.map(|location| mant_ir::SourceSpan {
                byte_range: None,
                line: location.line,
                column: location.column,
                end_line: None,
                end_column: None,
            }),
        })
        .collect()
}

fn truncation_code(message: &str) -> Option<&'static str> {
    match message {
        "owned syntax tree exceeded the 256-level copy limit; deeper descendants were omitted" => {
            Some("manual.syntax-depth-truncated")
        }
        "equation tree exceeded the 256-level copy limit; deeper equation content was omitted" => {
            Some("manual.equation-depth-truncated")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use mant_ir::DiagnosticLevel;

    use super::{
        super::syntax::{Diagnostic as MandocDiagnostic, DiagnosticLevel as MandocDiagnosticLevel},
        lower_diagnostics,
    };

    #[test]
    fn preserves_each_finding_and_classifies_known_levels() {
        let diagnostics = lower_diagnostics(&[
            MandocDiagnostic {
                level: MandocDiagnosticLevel::Unsupported,
                message: "unsupported roff request: ab".into(),
                location: None,
            },
            MandocDiagnostic {
                level: MandocDiagnosticLevel::Warning,
                message: "skipping paragraph macro".into(),
                location: None,
            },
            MandocDiagnostic {
                level: MandocDiagnosticLevel::Warning,
                message: "owned syntax tree exceeded the 256-level copy limit; deeper descendants were omitted".into(),
                location: None,
            },
        ]);

        assert_eq!(diagnostics.len(), 3);
        assert_eq!(diagnostics[0].level, DiagnosticLevel::Unsupported);
        assert_eq!(diagnostics[0].message, "unsupported roff request: ab");
        assert_eq!(diagnostics[1].level, DiagnosticLevel::Warning);
        assert_eq!(
            diagnostics[2].code.as_deref(),
            Some("manual.syntax-depth-truncated")
        );
    }
}

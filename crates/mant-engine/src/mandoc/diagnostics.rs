//! Adapts libmandoc-rs diagnostics into `ManT`'s document contract.

use mant_ir::{Diagnostic, DiagnosticLevel};

use libmandoc_rs::{
    Diagnostic as MandocDiagnostic, DiagnosticCode as MandocDiagnosticCode,
    DiagnosticLevel as MandocDiagnosticLevel,
};

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
            code: diagnostic.code().map(|code| {
                match code {
                    MandocDiagnosticCode::SyntaxTreeDepthLimit => "manual.syntax-depth-truncated",
                    MandocDiagnosticCode::EquationTreeDepthLimit => {
                        "manual.equation-depth-truncated"
                    }
                }
                .to_owned()
            }),
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

#[cfg(test)]
mod tests {
    use libmandoc_rs::{Diagnostic as MandocDiagnostic, DiagnosticLevel as MandocDiagnosticLevel};
    use mant_ir::DiagnosticLevel;

    use super::lower_diagnostics;

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

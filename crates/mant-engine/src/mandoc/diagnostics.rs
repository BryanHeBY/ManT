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

use super::{LoweringContext, MAX_INLINE_EQUATION_NORMALIZATIONS, Node, SourceSpan, source_span};

impl LoweringContext<'_> {
    pub(super) fn warn_unhandled_structural_parts(&self, node: &Node) {
        let macro_name = node.macro_name.as_deref().unwrap_or("unknown");
        self.diagnostics.borrow_mut().push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("manual.unhandled-structural-parts".to_owned()),
            message: format!(
                "structural macro '{macro_name}' contains parts without a complete lowering policy"
            ),
            source: source_span(node),
        });
    }

    pub(super) fn warn_unhandled_table_text_block(&self, node: &Node) {
        self.diagnostics.borrow_mut().push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("manual.unhandled-table-text-block".to_owned()),
            message: "tbl text block contains semantic roff that could not be retained".to_owned(),
            source: source_span(node),
        });
    }

    pub(super) fn warn_unhandled_table_text_block_line(&self, line: u32) {
        self.diagnostics.borrow_mut().push(Diagnostic {
            level: DiagnosticLevel::Warning,
            code: Some("manual.unhandled-table-text-block".to_owned()),
            message: "tbl inline semantics could not be reconstructed completely; complete native cell text or source spelling was retained".to_owned(),
            source: Some(SourceSpan {
                byte_range: None,
                line,
                column: 1,
                end_line: None,
                end_column: None,
            }),
        });
    }

    pub(super) fn warn_unexpanded_table_cell(&self, line: u32) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("manual.unexpanded-table-cell"))
        {
            return;
        }
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Unsupported,
            code: Some("manual.unexpanded-table-cell".to_owned()),
            message: "one or more tbl cells contain formatter strings that could not be expanded; their source spellings were preserved".to_owned(),
            source: Some(SourceSpan {
                byte_range: None,
                line,
                column: 1,
                end_line: None,
                end_column: None,
            }),
        });
    }

    pub(super) fn warn_inline_equation_budget(&self, line: u32) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("manual.inline-equation-budget"))
        {
            return;
        }
        diagnostics.push(Diagnostic {
            level: DiagnosticLevel::Unsupported,
            code: Some("manual.inline-equation-budget".to_owned()),
            message: format!(
                "more than {MAX_INLINE_EQUATION_NORMALIZATIONS} distinct inline table equations; later source spellings were retained without normalization"
            ),
            source: Some(SourceSpan {
                byte_range: None,
                line,
                column: 1,
                end_line: None,
                end_column: None,
            }),
        });
    }

    pub(super) fn take_diagnostics(&self) -> Vec<Diagnostic> {
        self.diagnostics.take()
    }
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

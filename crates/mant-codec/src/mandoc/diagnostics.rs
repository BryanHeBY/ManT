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
            impact: match diagnostic.code() {
                Some(
                    MandocDiagnosticCode::SyntaxTreeDepthLimit
                    | MandocDiagnosticCode::EquationTreeDepthLimit
                    | MandocDiagnosticCode::EscapeDepthLimit
                    | MandocDiagnosticCode::InputProcessingLimit,
                ) => mant_ir::DiagnosticImpact::ContentCoverage,
                None => mant_ir::DiagnosticImpact::None,
            },
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
                    MandocDiagnosticCode::EscapeDepthLimit => "manual.escape-depth-truncated",
                    MandocDiagnosticCode::InputProcessingLimit => "manual.input-processing-limit",
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

use super::{LoweringContext, Node, source_span};

impl LoweringContext<'_> {
    pub(super) fn warn_unhandled_structural_parts(&self, node: &Node) {
        let macro_name = node.macro_name.as_deref().unwrap_or("unknown");
        self.diagnostics.borrow_mut().push(Diagnostic {
            impact: mant_ir::DiagnosticImpact::None,
            level: DiagnosticLevel::Warning,
            code: Some("manual.unhandled-structural-parts".to_owned()),
            message: format!(
                "structural macro '{macro_name}' contains parts without a complete lowering policy"
            ),
            source: source_span(node),
        });
    }

    pub(super) fn take_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = self.diagnostics.take();
        let table_budget = self.table_recovery_budget.borrow();
        if table_budget.exhaustion().is_some() || table_budget.refused_fragments() > 0 {
            diagnostics.push(Diagnostic {
                impact: mant_ir::DiagnosticImpact::None,
                level: DiagnosticLevel::Warning,
                code: Some("manual.table-recovery-budget".to_owned()),
                message: format!(
                    "optional table enrichment reached a bounded allowance (page limit: {:?}; oversized fragments: {}; attempts: {}); finalized native cell payloads were retained",
                    table_budget.exhaustion(),
                    table_budget.refused_fragments(),
                    table_budget.attempts(),
                ),
                source: None,
            });
        }
        if self.escape_coverage.truncated() {
            diagnostics.push(Diagnostic {
                impact: mant_ir::DiagnosticImpact::ContentCoverage,
                level: DiagnosticLevel::Unsupported,
                code: Some("manual.escape-scan-truncated".to_owned()),
                message: "escape argument scanning reached its bounded work limit; some source text could not be projected".to_owned(),
                source: None,
            });
        }
        diagnostics
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
                code: None,
                level: MandocDiagnosticLevel::Unsupported,
                message: "unsupported roff request: ab".into(),
                location: None,
            },
            MandocDiagnostic {
                code: None,
                level: MandocDiagnosticLevel::Warning,
                message: "skipping paragraph macro".into(),
                location: None,
            },
            MandocDiagnostic {
                code: Some(libmandoc_rs::DiagnosticCode::SyntaxTreeDepthLimit),
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

    #[test]
    fn optional_table_refusal_reports_once_without_reclassifying_existing_loss() {
        let context = crate::mandoc::LoweringContext::new(None, None);
        context.diagnostics.borrow_mut().push(mant_ir::Diagnostic {
            impact: mant_ir::DiagnosticImpact::ContentCoverage,
            level: DiagnosticLevel::Unsupported,
            code: Some("existing-loss".to_owned()),
            message: "already incomplete".to_owned(),
            source: None,
        });
        let mut budget = context.table_recovery_budget.borrow_mut();
        budget.reject_fragment();
        budget.reject_fragment();
        assert!(!budget.charge_scan(usize::MAX));
        drop(budget);
        let diagnostics = context.take_diagnostics();
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(
            diagnostics[0].impact,
            mant_ir::DiagnosticImpact::ContentCoverage
        );
        assert_eq!(
            diagnostics[1].code.as_deref(),
            Some("manual.table-recovery-budget")
        );
        assert_eq!(diagnostics[1].impact, mant_ir::DiagnosticImpact::None);
        assert!(diagnostics[1].source.is_none());
        assert!(diagnostics[1].message.contains("oversized fragments: 2"));
    }
}

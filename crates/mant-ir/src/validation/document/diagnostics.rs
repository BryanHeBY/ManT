//! Impact and source attachment for this validator's own findings.
use crate::{Diagnostic, DiagnosticLevel, SourceSpan};

// Internal classification of this validator's own findings. Consumers read
// Diagnostic::impact, never these codes or another producer's private list.
pub(super) fn is_semantic_completeness_diagnostic(code: &str) -> bool {
    matches!(
        code,
        "ir.empty-identity"
            | "ir.invalid-declaration-group"
            | "ir.invalid-entry-content"
            | "ir.invalid-entry-name-binding"
            | "ir.invalid-entry-alias-groups"
            | "ir.invalid-entry-alias-of"
            | "ir.cyclic-entry-alias"
            | "ir.invalid-identity"
            | "ir.identity-role-collision"
            | "ir.duplicate-identity"
            | "ir.empty-fragment-alias"
            | "ir.invalid-fragment-alias"
            | "ir.ambiguous-fragment-alias"
            | "ir.empty-semantic-document-reference"
            | "ir.invalid-semantic-document-reference"
            | "ir.empty-entry-value-domain"
            | "ir.duplicate-entry-value-kind"
            | "ir.invalid-entry-choices"
    )
}

pub(super) fn invariant(code: &str, message: String) -> Diagnostic {
    Diagnostic {
        impact: invariant_impact(code),
        level: DiagnosticLevel::Warning,
        code: Some(code.to_owned()),
        message,
        source: None,
    }
}

pub(in crate::validation) fn invariant_at(
    code: &str,
    message: String,
    source: SourceSpan,
) -> Diagnostic {
    Diagnostic {
        impact: invariant_impact(code),
        level: DiagnosticLevel::Warning,
        code: Some(code.to_owned()),
        message,
        source: Some(source),
    }
}

fn invariant_impact(code: &str) -> crate::DiagnosticImpact {
    if code == "ir.equation-projection-mismatch" {
        crate::DiagnosticImpact::ContentCoverage
    } else if is_semantic_completeness_diagnostic(code) {
        crate::DiagnosticImpact::SemanticCoverage
    } else {
        crate::DiagnosticImpact::None
    }
}
